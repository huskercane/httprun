use std::cell::RefCell;
use std::rc::Rc;

use boa_engine::{
    Context, JsNativeError, JsResult, JsValue, NativeFunction, js_string,
    object::ObjectInitializer, property::Attribute,
};

use crate::js::response::json_to_js;
use crate::js::runtime::TestResult;
use crate::variable::GlobalVars;

/// Shared state between Rust and JS for the `client` object.
#[derive(Debug, Default)]
pub struct JsSharedState {
    pub global_vars: GlobalVars,
    pub test_results: Vec<TestResult>,
    pub log_output: Vec<String>,
}

/// Build the `client` JS global object.
pub fn build_client_object(
    shared: Rc<RefCell<JsSharedState>>,
    context: &mut Context,
) -> JsResult<JsValue> {
    // Build client.global object
    let global_obj = build_global_object(Rc::clone(&shared), context)?;

    // client.test(name, fn)
    let shared_test = Rc::clone(&shared);
    // SAFETY: The closure captures only Rc<RefCell<...>> which is safe to use from JS callbacks.
    // We only use this within a single-threaded boa context.
    let test_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let name = args
                .get(0)
                .cloned()
                .unwrap_or(JsValue::undefined())
                .to_string(ctx)?
                .to_std_string_escaped();

            let callback = args.get(1).cloned().unwrap_or(JsValue::undefined());

            if let Some(cb) = callback.as_callable() {
                let pre_len = shared_test.borrow().test_results.len();

                match cb.call(&JsValue::undefined(), &[], ctx) {
                    Ok(_) => {
                        let state = shared_test.borrow();
                        let had_failure =
                            state.test_results.iter().skip(pre_len).any(|r| !r.passed);

                        if !had_failure {
                            drop(state);
                            shared_test.borrow_mut().test_results.push(TestResult {
                                name,
                                passed: true,
                                failure_message: None,
                            });
                        }
                    }
                    Err(e) => {
                        shared_test.borrow_mut().test_results.push(TestResult {
                            name,
                            passed: false,
                            failure_message: Some(format!("Exception: {e}")),
                        });
                    }
                }
            }

            Ok(JsValue::undefined())
        })
    };

    // client.assert(condition, message)
    let shared_assert = Rc::clone(&shared);
    // SAFETY: Same as above — single-threaded context with Rc<RefCell<...>>.
    let assert_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let condition = args
                .get(0)
                .cloned()
                .unwrap_or(JsValue::from(false))
                .to_boolean();

            let message = args
                .get(1)
                .cloned()
                .unwrap_or(JsValue::from(js_string!("Assertion failed")))
                .to_string(ctx)?
                .to_std_string_escaped();

            if !condition {
                shared_assert.borrow_mut().test_results.push(TestResult {
                    name: message.clone(),
                    passed: false,
                    failure_message: Some(message),
                });
            }

            Ok(JsValue::undefined())
        })
    };

    // client.log(...)
    let shared_log = Rc::clone(&shared);
    // SAFETY: Same as above.
    let log_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let mut parts = Vec::new();
            for arg in args.iter() {
                let s = arg.to_string(ctx)?.to_std_string_escaped();
                parts.push(s);
            }
            let line = parts.join(" ");
            shared_log.borrow_mut().log_output.push(line);
            Ok(JsValue::undefined())
        })
    };

    let client = ObjectInitializer::new(context)
        .property(js_string!("global"), global_obj, Attribute::READONLY)
        .function(test_fn, js_string!("test"), 2)
        .function(assert_fn, js_string!("assert"), 2)
        .function(log_fn, js_string!("log"), 1)
        .build();

    Ok(client.into())
}

fn build_global_object(
    shared: Rc<RefCell<JsSharedState>>,
    context: &mut Context,
) -> JsResult<JsValue> {
    let shared_set = Rc::clone(&shared);
    // SAFETY: Single-threaded boa context with Rc<RefCell<...>>.
    let set_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let name = args
                .get(0)
                .cloned()
                .unwrap_or(JsValue::undefined())
                .to_string(ctx)?
                .to_std_string_escaped();

            let value = args.get(1).cloned().unwrap_or(JsValue::undefined());
            let stored = js_to_json(&value, ctx)?;

            shared_set.borrow_mut().global_vars.insert(name, stored);

            Ok(JsValue::undefined())
        })
    };

    let shared_get = Rc::clone(&shared);
    // SAFETY: Single-threaded boa context with Rc<RefCell<...>>.
    let get_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let name = args
                .get(0)
                .cloned()
                .unwrap_or(JsValue::undefined())
                .to_string(ctx)?
                .to_std_string_escaped();

            let stored = shared_get.borrow().global_vars.get(&name).cloned();
            match stored {
                Some(v) => json_to_js(&v, ctx),
                None => Ok(JsValue::undefined()),
            }
        })
    };

    let global = ObjectInitializer::new(context)
        .function(set_fn, js_string!("set"), 2)
        .function(get_fn, js_string!("get"), 1)
        .build();

    Ok(global.into())
}

/// Convert a JS value into the JSON value stored for a global variable.
/// Storing the type (rather than a stringified form) is what lets
/// `client.global.get` round-trip types while `{{var}}` substitution stays plain text.
fn js_to_json(value: &JsValue, ctx: &mut Context) -> JsResult<serde_json::Value> {
    if value.is_undefined() || value.is_null() {
        return Ok(serde_json::Value::Null);
    }
    if value.is_boolean() {
        return Ok(serde_json::Value::Bool(value.to_boolean()));
    }
    if value.is_string() {
        return Ok(serde_json::Value::String(
            value.to_string(ctx)?.to_std_string_escaped(),
        ));
    }
    if value.is_number() {
        return Ok(number_to_json(value.to_number(ctx)?));
    }
    if value.is_bigint() {
        // No lossless JSON number for a bigint; keep the exact digits as text.
        return Ok(serde_json::Value::String(
            value.to_string(ctx)?.to_std_string_escaped(),
        ));
    }

    // Objects and arrays go through JSON.stringify so we inherit JS semantics
    // (undefined members dropped, `toJSON` honored) instead of hand-rolling them.
    match json_stringify(value, ctx)? {
        Some(text) => serde_json::from_str(&text).map_err(|e| {
            JsNativeError::typ()
                .with_message(format!("cannot store global variable: {e}"))
                .into()
        }),
        None => Ok(serde_json::Value::Null),
    }
}

/// Whole numbers keep integer form so `{{count}}` renders `12`, not `12.0`.
/// NaN/Infinity become null, matching `JSON.stringify`.
fn number_to_json(n: f64) -> serde_json::Value {
    if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
        serde_json::Value::from(n as i64)
    } else {
        serde_json::Number::from_f64(n).map_or(serde_json::Value::Null, serde_json::Value::Number)
    }
}

/// Call the engine's `JSON.stringify`; `None` when it yields `undefined`.
fn json_stringify(value: &JsValue, ctx: &mut Context) -> JsResult<Option<String>> {
    let json = ctx.global_object().get(js_string!("JSON"), ctx)?;
    let stringify = json
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("JSON global is not an object"))?
        .get(js_string!("stringify"), ctx)?;
    let stringify = stringify
        .as_callable()
        .ok_or_else(|| JsNativeError::typ().with_message("JSON.stringify is not callable"))?
        .clone();

    let result = stringify.call(&JsValue::undefined(), std::slice::from_ref(value), ctx)?;
    if result.is_undefined() {
        return Ok(None);
    }
    Ok(Some(result.to_string(ctx)?.to_std_string_escaped()))
}
