//! Reporting port and adapters.
//!
//! The execution loop in `main` emits semantic events (a request started, a
//! request finished, the run finished) and knows nothing about rendering.
//! Each adapter decides what to do with them: `HumanReporter` streams a
//! colored terminal trace, `JsonReporter` accumulates a machine-readable
//! report for tools and CI.

use std::io;
use std::path::Path;

use crate::http::HttpResponse;
use crate::js::TestResult;
use crate::parser::ParsedRequest;

mod human;
mod json;
mod redact;

pub use human::HumanReporter;
pub use json::{JsonReporter, JsonStyle, RunMeta};

/// What happened to a single request.
pub enum RequestResult<'a> {
    /// A response came back. The response handler may still have failed —
    /// that is reported separately from a transport failure.
    Completed {
        response: &'a HttpResponse,
        logs: &'a [String],
        tests: &'a [TestResult],
        handler_error: Option<&'a str>,
    },
    /// No response — the request never completed.
    Failed(&'a str),
}

impl RequestResult<'_> {
    /// Whether this request should be treated as a failure for reporting
    /// purposes (drives what `--quiet` surfaces).
    pub fn is_failure(&self) -> bool {
        match self {
            Self::Completed {
                tests,
                handler_error,
                ..
            } => handler_error.is_some() || tests.iter().any(|t| !t.passed),
            Self::Failed(_) => true,
        }
    }
}

pub struct RequestReport<'a> {
    pub index: usize,
    pub request: &'a ParsedRequest,
    pub result: RequestResult<'a>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Summary {
    pub total: usize,
    pub tests_passed: usize,
    pub tests_failed: usize,
    pub errors: usize,
    pub duration_ms: u128,
}

impl Summary {
    pub fn is_success(&self) -> bool {
        self.tests_failed == 0 && self.errors == 0
    }

    /// Process exit code: 0 on success, 1 when anything failed.
    pub fn exit_code(&self) -> i32 {
        if self.is_success() { 0 } else { 1 }
    }
}

pub trait Reporter {
    /// Called before the request goes on the wire. Exists so humans get live
    /// feedback during the network wait; machine formats ignore it and emit
    /// everything in `request_finished`.
    fn request_started(&mut self, index: usize, request: &ParsedRequest) -> io::Result<()>;

    fn request_finished(&mut self, report: &RequestReport<'_>) -> io::Result<()>;

    fn dry_run_started(&mut self, _count: usize, _file: &Path) -> io::Result<()> {
        Ok(())
    }

    fn dry_run_request(&mut self, index: usize, request: &ParsedRequest) -> io::Result<()>;

    fn finish(&mut self, summary: &Summary) -> io::Result<()>;

    /// Must be called before the process exits. `process::exit` skips
    /// destructors, so a buffered writer would otherwise lose its contents.
    fn flush(&mut self) -> io::Result<()>;
}
