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
) -> Result<HandlerResult, AppError> {
    let mut context = Context::default();
    let shared_state = Rc::new(RefCell::new(JsSharedState::default()));

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
