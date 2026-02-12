use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use boa_engine::{
    Context, JsResult, JsValue, NativeFunction,
    js_string,
    object::ObjectInitializer,
    property::Attribute,
};

use crate::js::runtime::TestResult;

/// Shared state between Rust and JS for the `client` object.
#[derive(Debug, Default)]
pub struct JsSharedState {
    pub global_vars: HashMap<String, String>,
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

            let callback = args
                .get(1)
                .cloned()
                .unwrap_or(JsValue::undefined());

            if let Some(cb) = callback.as_callable() {
                let pre_len = shared_test.borrow().test_results.len();

                match cb.call(&JsValue::undefined(), &[], ctx) {
                    Ok(_) => {
                        let state = shared_test.borrow();
                        let had_failure = state.test_results.iter().skip(pre_len).any(|r| !r.passed);

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

            let value = args
                .get(1)
                .cloned()
                .unwrap_or(JsValue::undefined());

            let value_str = if value.is_number() {
                let n = value.to_number(ctx)?;
                if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
                    format!("{}", n as i64)
                } else {
                    format!("{n}")
                }
            } else {
                value.to_string(ctx)?.to_std_string_escaped()
            };

            shared_set
                .borrow_mut()
                .global_vars
                .insert(name, value_str);

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

            let state = shared_get.borrow();
            match state.global_vars.get(&name) {
                Some(v) => {
                    // Try to return numbers as numbers to preserve type
                    // through set/get roundtrip (matches IntelliJ behavior)
                    if let Ok(n) = v.parse::<f64>() {
                        Ok(JsValue::from(n))
                    } else if v == "true" {
                        Ok(JsValue::from(true))
                    } else if v == "false" {
                        Ok(JsValue::from(false))
                    } else {
                        Ok(JsValue::from(js_string!(v.clone())))
                    }
                }
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
