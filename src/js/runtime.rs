use std::cell::RefCell;
use std::rc::Rc;

use boa_engine::{Context, Source, js_string, property::Attribute};

use crate::error::AppError;
use crate::http::HttpResponse;
use crate::js::client::{JsSharedState, build_client_object};
use crate::js::response::build_response_object;
use crate::variable::GlobalVars;

#[derive(Debug, Clone)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub failure_message: Option<String>,
}

#[derive(Debug)]
pub struct HandlerResult {
    pub global_vars: GlobalVars,
    pub test_results: Vec<TestResult>,
    pub log_output: Vec<String>,
}

pub fn execute_handler(
    script: &str,
    http_response: &HttpResponse,
    existing_globals: &GlobalVars,
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
    use crate::variable::VariableStore;
    use std::collections::HashMap;

    fn dummy_response() -> HttpResponse {
        HttpResponse {
            status: 200,
            http_version: "HTTP/1.1".to_string(),
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
        let result1 = execute_handler(script1, &resp, &GlobalVars::new()).unwrap();
        assert_eq!(
            result1.global_vars.get("totalElements").unwrap(),
            &serde_json::json!(12)
        );

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
        let resp = dummy_response();

        // Test String
        let script1 = r#"client.global.set("s", "12");"#;
        let result1 = execute_handler(script1, &resp, &HashMap::new()).unwrap();
        let script2 = r#"
            client.test("String preserved", function() {
                var v = client.global.get("s");
                client.assert(typeof v === "string", "expected string, got " + typeof v);
                client.assert(v === "12", "expected '12', got " + v);
            });
        "#;
        let result2 = execute_handler(script2, &resp, &result1.global_vars).unwrap();
        assert!(
            result2.test_results.iter().all(|r| r.passed),
            "String test failed: {:?}",
            result2.test_results
        );

        // Test Number
        let script3 = r#"client.global.set("n", 12);"#;
        let result3 = execute_handler(script3, &resp, &HashMap::new()).unwrap();
        let script4 = r#"
            client.test("Number preserved", function() {
                var v = client.global.get("n");
                client.assert(typeof v === "number", "expected number, got " + typeof v);
                client.assert(v === 12, "expected 12, got " + v);
            });
        "#;
        let result4 = execute_handler(script4, &resp, &result3.global_vars).unwrap();
        assert!(
            result4.test_results.iter().all(|r| r.passed),
            "Number test failed: {:?}",
            result4.test_results
        );

        // Test Boolean
        let script5 = r#"client.global.set("b", true);"#;
        let result5 = execute_handler(script5, &resp, &HashMap::new()).unwrap();
        let script6 = r#"
            client.test("Boolean preserved", function() {
                var v = client.global.get("b");
                client.assert(typeof v === "boolean", "expected boolean, got " + typeof v);
                client.assert(v === true, "expected true, got " + v);
            });
        "#;
        let result6 = execute_handler(script6, &resp, &result5.global_vars).unwrap();
        assert!(
            result6.test_results.iter().all(|r| r.passed),
            "Boolean test failed: {:?}",
            result6.test_results
        );
    }

    /// Substituting a global set from JS must paste the raw text, never a JSON-quoted form.
    #[test]
    fn globals_substitute_without_json_quoting() {
        let resp = dummy_response();
        let script = r#"
            client.global.set("activityTime", "1783554153.000000000");
            client.global.set("id", 10323);
            client.global.set("enabled", true);
        "#;
        let result = execute_handler(script, &resp, &GlobalVars::new()).unwrap();

        let mut store = VariableStore::new(HashMap::new());
        store.merge_globals(&result.global_vars);

        let url = store
            .substitute("/activities/{{id}}?activityTime={{activityTime}}&enabled={{enabled}}")
            .unwrap();
        assert_eq!(
            url,
            "/activities/10323?activityTime=1783554153.000000000&enabled=true"
        );
    }

    /// A response value read as a JS number must not gain a `.0` when substituted.
    #[test]
    fn whole_number_global_substitutes_as_integer() {
        let resp = dummy_response();
        let script = r#"client.global.set("total", response.body.totalElements);"#;
        let result = execute_handler(script, &resp, &GlobalVars::new()).unwrap();

        let mut store = VariableStore::new(HashMap::new());
        store.merge_globals(&result.global_vars);

        assert_eq!(store.substitute("count={{total}}").unwrap(), "count=12");
    }

    #[test]
    fn object_global_round_trips_and_substitutes_as_json() {
        let resp = dummy_response();
        let script = r#"client.global.set("filter", { name: "cpu", limit: 5 });"#;
        let result = execute_handler(script, &resp, &GlobalVars::new()).unwrap();

        let script2 = r#"
            client.test("Object preserved", function() {
                var v = client.global.get("filter");
                client.assert(v.name === "cpu", "expected cpu, got " + v.name);
                client.assert(v.limit === 5, "expected 5, got " + v.limit);
            });
        "#;
        let result2 = execute_handler(script2, &resp, &result.global_vars).unwrap();
        assert!(
            result2.test_results.iter().all(|r| r.passed),
            "Object test failed: {:?}",
            result2.test_results,
        );

        let mut store = VariableStore::new(HashMap::new());
        store.merge_globals(&result.global_vars);
        let body = store.substitute("{{filter}}").unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap(),
            serde_json::json!({ "name": "cpu", "limit": 5 }),
        );
    }
}
