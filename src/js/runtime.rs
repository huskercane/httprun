use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use boa_engine::{Context, Source, js_string, property::Attribute};

use crate::error::AppError;
use crate::http::HttpResponse;
use crate::js::client::{JsSharedState, build_client_object};
use crate::js::response::build_response_object;

#[derive(Debug, Clone)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub failure_message: Option<String>,
}

#[derive(Debug)]
pub struct HandlerResult {
    pub global_vars: HashMap<String, String>,
    pub test_results: Vec<TestResult>,
    pub log_output: Vec<String>,
}

pub fn execute_handler(
    script: &str,
    http_response: &HttpResponse,
    existing_globals: &HashMap<String, String>,
) -> Result<HandlerResult, AppError> {
    let mut context = Context::default();
    let shared_state = Rc::new(RefCell::new(JsSharedState {
        global_vars: existing_globals.clone(),
        ..Default::default()
    }));

    // Build and register `response` global
    let response_obj = build_response_object(http_response, &mut context)
        .map_err(|e| AppError::JavaScript(format!("Failed to build response object: {e}")))?;
    context
        .register_global_property(
            js_string!("response"),
            response_obj,
            Attribute::READONLY | Attribute::NON_ENUMERABLE,
        )
        .map_err(|e| AppError::JavaScript(format!("{e}")))?;

    // Build and register `client` global
    let client_obj = build_client_object(Rc::clone(&shared_state), &mut context)
        .map_err(|e| AppError::JavaScript(format!("Failed to build client object: {e}")))?;
    context
        .register_global_property(
            js_string!("client"),
            client_obj,
            Attribute::READONLY | Attribute::NON_ENUMERABLE,
        )
        .map_err(|e| AppError::JavaScript(format!("{e}")))?;

    // Execute the handler script
    context
        .eval(Source::from_bytes(script))
        .map_err(|e| AppError::JavaScript(format!("{e}")))?;

    let state = shared_state.borrow();
    Ok(HandlerResult {
        global_vars: state.global_vars.clone(),
        test_results: state.test_results.clone(),
        log_output: state.log_output.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{ContentType, HttpResponse};

    fn dummy_response() -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: HashMap::new(),
            body_raw: r#"{"totalElements": 12}"#.to_string(),
            body_json: serde_json::from_str(r#"{"totalElements": 12}"#).ok(),
            content_type: Some(ContentType {
                mime_type: "application/json".to_string(),
                charset: None,
            }),
            elapsed_ms: 0,
        }
    }

    #[test]
    fn global_vars_persist_across_handler_calls() {
        // First handler sets a global variable
        let script1 = r#"client.global.set("totalElements", response.body.totalElements);"#;
        let resp = dummy_response();
        let result1 = execute_handler(script1, &resp, &HashMap::new()).unwrap();
        assert_eq!(result1.global_vars.get("totalElements").unwrap(), "12");

        // Second handler reads the global variable set by the first
        let script2 = r#"
            client.test("Global persists", function() {
                var expected = client.global.get("totalElements");
                client.assert(expected === 12, "expected 12 but got " + expected);
            });
        "#;
        let result2 = execute_handler(script2, &resp, &result1.global_vars).unwrap();
        assert!(
            result2.test_results.iter().all(|r| r.passed),
            "test failed: {:?}",
            result2.test_results,
        );
    }

    #[test]
    fn global_get_returns_undefined_when_empty() {
        let script = r#"
            client.test("Missing global is undefined", function() {
                var val = client.global.get("nonexistent");
                client.assert(val === undefined, "expected undefined");
            });
        "#;
        let resp = dummy_response();
        let result = execute_handler(script, &resp, &HashMap::new()).unwrap();
        assert!(result.test_results.iter().all(|r| r.passed));
    }

    #[test]
    fn global_get_preserves_types() {
        let script = r#"
            client.global.set("num", 42);
            client.global.set("float", 3.14);
            client.global.set("str", "hello");
            client.global.set("t", true);
            client.global.set("f", false);

            client.test("Number preserved", function() {
                var v = client.global.get("num");
                client.assert(v === 42, "expected number 42, got " + typeof v + " " + v);
            });
            client.test("Float preserved", function() {
                var v = client.global.get("float");
                client.assert(v === 3.14, "expected 3.14, got " + typeof v + " " + v);
            });
            client.test("String preserved", function() {
                var v = client.global.get("str");
                client.assert(v === "hello", "expected string hello, got " + typeof v + " " + v);
            });
            client.test("Boolean true preserved", function() {
                var v = client.global.get("t");
                client.assert(v === true, "expected true, got " + typeof v + " " + v);
            });
            client.test("Boolean false preserved", function() {
                var v = client.global.get("f");
                client.assert(v === false, "expected false, got " + typeof v + " " + v);
            });
        "#;
        let resp = dummy_response();
        let result = execute_handler(script, &resp, &HashMap::new()).unwrap();
        assert!(
            result.test_results.iter().all(|r| r.passed),
            "test failed: {:?}",
            result.test_results,
        );
    }
}
