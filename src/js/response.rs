use std::collections::HashMap;

use boa_engine::{
    Context, JsResult, JsValue, NativeFunction,
    js_string,
    object::builtins::JsArray,
    object::ObjectInitializer,
    property::Attribute,
};

use crate::http::HttpResponse;

/// Build the `response` JS global object from an HttpResponse.
pub fn build_response_object(
    http_response: &HttpResponse,
    context: &mut Context,
) -> JsResult<JsValue> {
    let status = http_response.status as i32;

    // Build body value — either parsed JSON object or raw string
    let body_value = if let Some(json) = &http_response.body_json {
        json_to_js(json, context)?
    } else {
        JsValue::from(js_string!(http_response.body_raw.clone()))
    };

    // Build headers object with valueOf and valuesOf methods
    let headers_obj = build_headers_object(&http_response.headers, context)?;

    // Build contentType object
    let content_type_obj = if let Some(ct) = &http_response.content_type {
        let charset_val = match &ct.charset {
            Some(c) => JsValue::from(js_string!(c.clone())),
            None => JsValue::null(),
        };
        ObjectInitializer::new(context)
            .property(
                js_string!("mimeType"),
                js_string!(ct.mime_type.clone()),
                Attribute::READONLY,
            )
            .property(js_string!("charset"), charset_val, Attribute::READONLY)
            .build()
            .into()
    } else {
        JsValue::null()
    };

    let response = ObjectInitializer::new(context)
        .property(js_string!("status"), status, Attribute::READONLY)
        .property(js_string!("body"), body_value, Attribute::READONLY)
        .property(js_string!("headers"), headers_obj, Attribute::READONLY)
        .property(
            js_string!("contentType"),
            content_type_obj,
            Attribute::READONLY,
        )
        .build();

    Ok(response.into())
}

fn build_headers_object(
    headers: &HashMap<String, Vec<String>>,
    context: &mut Context,
) -> JsResult<JsValue> {
    let headers_for_value_of = headers.clone();
    let headers_for_values_of = headers.clone();

    let obj = ObjectInitializer::new(context)
        .function(
            NativeFunction::from_copy_closure_with_captures(
                |_this, args, captures, ctx| {
                    let name = args
                        .get(0)
                        .cloned()
                        .unwrap_or(JsValue::undefined())
                        .to_string(ctx)?;
                    let name_str = name.to_std_string_escaped().to_lowercase();

                    match captures.get(&name_str) {
                        Some(values) => {
                            if let Some(first) = values.first() {
                                Ok(JsValue::from(js_string!(first.clone())))
                            } else {
                                Ok(JsValue::null())
                            }
                        }
                        None => Ok(JsValue::null()),
                    }
                },
                headers_for_value_of,
            ),
            js_string!("valueOf"),
            1,
        )
        .function(
            NativeFunction::from_copy_closure_with_captures(
                |_this, args, captures, ctx| {
                    let name = args
                        .get(0)
                        .cloned()
                        .unwrap_or(JsValue::undefined())
                        .to_string(ctx)?;
                    let name_str = name.to_std_string_escaped().to_lowercase();

                    match captures.get(&name_str) {
                        Some(values) => {
                            let arr = JsArray::new(ctx);
                            for v in values {
                                arr.push(js_string!(v.clone()), ctx)?;
                            }
                            Ok(arr.into())
                        }
                        None => {
                            let arr = JsArray::new(ctx);
                            Ok(arr.into())
                        }
                    }
                },
                headers_for_values_of,
            ),
            js_string!("valuesOf"),
            1,
        )
        .build();

    Ok(obj.into())
}

/// Recursively convert serde_json::Value to boa JsValue.
pub fn json_to_js(value: &serde_json::Value, context: &mut Context) -> JsResult<JsValue> {
    match value {
        serde_json::Value::Null => Ok(JsValue::null()),
        serde_json::Value::Bool(b) => Ok(JsValue::from(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(JsValue::from(i as f64))
            } else if let Some(f) = n.as_f64() {
                Ok(JsValue::from(f))
            } else {
                Ok(JsValue::from(0.0))
            }
        }
        serde_json::Value::String(s) => Ok(JsValue::from(js_string!(s.clone()))),
        serde_json::Value::Array(arr) => {
            let js_arr = JsArray::new(context);
            for item in arr {
                let js_item = json_to_js(item, context)?;
                js_arr.push(js_item, context)?;
            }
            Ok(js_arr.into())
        }
        serde_json::Value::Object(map) => {
            let js_obj = boa_engine::JsObject::with_null_proto();
            for (key, val) in map {
                let js_val = json_to_js(val, context)?;
                js_obj.set(js_string!(key.clone()), js_val, false, context)?;
            }
            Ok(js_obj.into())
        }
    }
}
