use std::collections::BTreeMap;
use std::io::{self, Write};

use serde::Serialize;

use super::redact;
use super::{Reporter, RequestReport, RequestResult, Summary};
use crate::curl;
use crate::http::HttpResponse;
use crate::js::TestResult;
use crate::parser::ParsedRequest;

/// Bump when a field is removed or changes meaning. Tools read this output,
/// so the shape is a public API — additive changes keep the same version.
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonStyle {
    /// One JSON document for the whole run, emitted on `finish`.
    Document,
    /// One JSON object per line, flushed as each request completes.
    Ndjson,
}

/// Run-level context that isn't derivable from the request stream.
pub struct RunMeta {
    pub file: String,
    pub environment: Option<String>,
    pub started_at_ms: u128,
}

pub struct JsonReporter<W: Write> {
    w: W,
    style: JsonStyle,
    meta: RunMeta,
    include_curl: bool,
    redact: bool,
    requests: Vec<RequestEntry>,
}

impl<W: Write> JsonReporter<W> {
    pub fn new(w: W, style: JsonStyle, meta: RunMeta, include_curl: bool, redact: bool) -> Self {
        Self {
            w,
            style,
            meta,
            include_curl,
            redact,
            requests: Vec::new(),
        }
    }

    fn request_entry(
        &self,
        index: usize,
        request: &ParsedRequest,
        result: Option<&RequestResult<'_>>,
    ) -> RequestEntry {
        let mut entry = RequestEntry {
            index,
            name: request.name.clone(),
            method: request.method.as_str().to_string(),
            url: request.url.clone(),
            headers: request_headers(request, self.redact),
            body: request.body.clone(),
            curl: self.curl_command(request),
            response: None,
            tests: Vec::new(),
            logs: Vec::new(),
            error: None,
        };

        match result {
            Some(RequestResult::Completed {
                response,
                logs,
                tests,
                handler_error,
            }) => {
                entry.response = Some(self.response_entry(response));
                entry.logs = logs.to_vec();
                entry.tests = tests.iter().map(TestEntry::from).collect();
                entry.error = handler_error.map(str::to_string);
            }
            Some(RequestResult::Failed(msg)) => {
                entry.error = Some((*msg).to_string());
            }
            None => {}
        }

        entry
    }

    fn response_entry(&self, response: &HttpResponse) -> ResponseEntry {
        let mut headers: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (name, values) in &response.headers {
            let masked = values
                .iter()
                .map(|v| redact::header_value(name, v, self.redact))
                .collect();
            headers.insert(name.clone(), masked);
        }

        ResponseEntry {
            status: response.status,
            http_version: response.http_version.clone(),
            headers,
            // Parsed JSON when the body is JSON, otherwise the raw text as a
            // JSON string. Exactly one field either way, so `jq .body` works.
            body: response
                .body_json
                .clone()
                .unwrap_or_else(|| serde_json::Value::String(response.body_raw.clone())),
            elapsed_ms: response.elapsed_ms,
        }
    }

    /// Built from a redacted view of the request, so the emitted command is
    /// consistent with the `headers` field rather than leaking around it.
    fn curl_command(&self, request: &ParsedRequest) -> Option<String> {
        if !self.include_curl {
            return None;
        }
        let mut view = request.clone();
        if self.redact {
            for header in &mut view.headers {
                header.value = redact::header_value(&header.name, &header.value, true);
            }
        }
        Some(curl::to_curl_command(&view))
    }

    fn write_ndjson_line(&mut self, value: &serde_json::Value) -> io::Result<()> {
        serde_json::to_writer(&mut self.w, value)?;
        writeln!(self.w)?;
        // One flush per request is free next to a network round trip, and it
        // is the whole point of ndjson: consumers can tail the stream.
        self.w.flush()
    }
}

impl<W: Write> Reporter for JsonReporter<W> {
    fn request_started(&mut self, _index: usize, _request: &ParsedRequest) -> io::Result<()> {
        Ok(())
    }

    fn request_finished(&mut self, report: &RequestReport<'_>) -> io::Result<()> {
        let entry = self.request_entry(report.index, report.request, Some(&report.result));

        match self.style {
            JsonStyle::Document => {
                self.requests.push(entry);
                Ok(())
            }
            JsonStyle::Ndjson => {
                let value = tagged("request", &entry)?;
                self.write_ndjson_line(&value)
            }
        }
    }

    fn dry_run_request(&mut self, index: usize, request: &ParsedRequest) -> io::Result<()> {
        let entry = self.request_entry(index, request, None);

        match self.style {
            JsonStyle::Document => {
                self.requests.push(entry);
                Ok(())
            }
            JsonStyle::Ndjson => {
                let value = tagged("request", &entry)?;
                self.write_ndjson_line(&value)
            }
        }
    }

    fn finish(&mut self, summary: &Summary) -> io::Result<()> {
        let summary_entry = SummaryEntry::from(summary);

        match self.style {
            JsonStyle::Document => {
                let report = RunReport {
                    schema_version: SCHEMA_VERSION,
                    file: &self.meta.file,
                    environment: self.meta.environment.as_deref(),
                    started_at_ms: self.meta.started_at_ms,
                    summary: summary_entry,
                    requests: &self.requests,
                };
                serde_json::to_writer_pretty(&mut self.w, &report)?;
                writeln!(self.w)?;
                Ok(())
            }
            JsonStyle::Ndjson => {
                let mut value = tagged("summary", &summary_entry)?;
                if let Some(obj) = value.as_object_mut() {
                    obj.insert(
                        "schemaVersion".to_string(),
                        serde_json::Value::from(SCHEMA_VERSION),
                    );
                }
                self.write_ndjson_line(&value)
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.w.flush()
    }
}

/// Serialize `entry` to an object and stamp it with a `type` discriminator so
/// ndjson consumers can tell request lines from the summary line.
fn tagged<T: Serialize>(kind: &str, entry: &T) -> serde_json::Result<serde_json::Value> {
    let mut value = serde_json::to_value(entry)?;
    if let Some(obj) = value.as_object_mut() {
        obj.insert("type".to_string(), serde_json::Value::from(kind));
    }
    Ok(value)
}

fn request_headers(request: &ParsedRequest, redact_secrets: bool) -> BTreeMap<String, Vec<String>> {
    let mut headers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for h in &request.headers {
        headers
            .entry(h.name.clone())
            .or_default()
            .push(redact::header_value(&h.name, &h.value, redact_secrets));
    }
    headers
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RunReport<'a> {
    schema_version: u32,
    file: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    environment: Option<&'a str>,
    started_at_ms: u128,
    summary: SummaryEntry,
    requests: &'a [RequestEntry],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestEntry {
    index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    method: String,
    url: String,
    headers: BTreeMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    curl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response: Option<ResponseEntry>,
    tests: Vec<TestEntry>,
    logs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResponseEntry {
    status: u16,
    http_version: String,
    headers: BTreeMap<String, Vec<String>>,
    body: serde_json::Value,
    elapsed_ms: u128,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TestEntry {
    name: String,
    passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_message: Option<String>,
}

impl From<&TestResult> for TestEntry {
    fn from(t: &TestResult) -> Self {
        Self {
            name: t.name.clone(),
            passed: t.passed,
            failure_message: t.failure_message.clone(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SummaryEntry {
    total: usize,
    tests_passed: usize,
    tests_failed: usize,
    errors: usize,
    duration_ms: u128,
    success: bool,
}

impl From<&Summary> for SummaryEntry {
    fn from(s: &Summary) -> Self {
        Self {
            total: s.total,
            tests_passed: s.tests_passed,
            tests_failed: s.tests_failed,
            errors: s.errors,
            duration_ms: s.duration_ms,
            success: s.is_success(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{Header, HttpMethod};

    fn request_with_auth() -> ParsedRequest {
        ParsedRequest {
            name: Some("login".to_string()),
            method: HttpMethod::Post,
            url: "https://api.example.com/login".to_string(),
            headers: vec![
                Header {
                    name: "Authorization".to_string(),
                    value: "Bearer super-secret".to_string(),
                },
                Header {
                    name: "Content-Type".to_string(),
                    value: "application/json".to_string(),
                },
            ],
            body: Some("{\"user\":\"alice\"}".to_string()),
            response_handler: None,
            line_number: 1,
        }
    }

    fn reporter(redact: bool, include_curl: bool) -> JsonReporter<Vec<u8>> {
        JsonReporter::new(
            Vec::new(),
            JsonStyle::Document,
            RunMeta {
                file: "api.http".to_string(),
                environment: Some("dev".to_string()),
                started_at_ms: 0,
            },
            include_curl,
            redact,
        )
    }

    #[test]
    fn credential_headers_are_redacted_by_default() {
        let req = request_with_auth();
        let entry = reporter(true, false).request_entry(1, &req, None);

        assert_eq!(entry.headers["Authorization"], vec![redact::REDACTED]);
        // Non-credential headers survive intact.
        assert_eq!(entry.headers["Content-Type"], vec!["application/json"]);
    }

    #[test]
    fn include_secrets_keeps_header_values() {
        let req = request_with_auth();
        let entry = reporter(false, false).request_entry(1, &req, None);

        assert_eq!(entry.headers["Authorization"], vec!["Bearer super-secret"]);
    }

    #[test]
    fn emitted_curl_command_respects_redaction() {
        let req = request_with_auth();
        let entry = reporter(true, true).request_entry(1, &req, None);

        let cmd = entry.curl.expect("curl command requested");
        // The curl string must not leak around the redacted headers field.
        assert!(!cmd.contains("super-secret"), "curl leaked a secret: {cmd}");
        assert!(cmd.contains(redact::REDACTED));
    }

    #[test]
    fn curl_is_omitted_unless_requested() {
        let req = request_with_auth();
        let entry = reporter(true, false).request_entry(1, &req, None);
        assert!(entry.curl.is_none());
    }

    #[test]
    fn document_output_is_valid_json_with_schema_version() {
        let mut r = reporter(true, false);
        let req = request_with_auth();
        r.dry_run_request(1, &req).unwrap();
        r.finish(&Summary {
            total: 1,
            duration_ms: 5,
            ..Default::default()
        })
        .unwrap();

        let parsed: serde_json::Value = serde_json::from_slice(&r.w).unwrap();
        assert_eq!(parsed["schemaVersion"], 1);
        assert_eq!(parsed["file"], "api.http");
        assert_eq!(parsed["environment"], "dev");
        assert_eq!(parsed["summary"]["success"], true);
        assert_eq!(parsed["requests"][0]["method"], "POST");
    }

    #[test]
    fn ndjson_emits_one_object_per_line_plus_summary() {
        let mut r = JsonReporter::new(
            Vec::new(),
            JsonStyle::Ndjson,
            RunMeta {
                file: "api.http".to_string(),
                environment: None,
                started_at_ms: 0,
            },
            false,
            true,
        );
        let req = request_with_auth();
        r.dry_run_request(1, &req).unwrap();
        r.dry_run_request(2, &req).unwrap();
        r.finish(&Summary {
            total: 2,
            ..Default::default()
        })
        .unwrap();

        let text = String::from_utf8(r.w).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);

        for line in &lines {
            // Every line must parse standalone — that is the ndjson contract.
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        }
        let last: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(last["type"], "summary");
        assert_eq!(last["total"], 2);
    }

    #[test]
    fn failed_request_records_error_and_no_response() {
        let req = request_with_auth();
        let entry =
            reporter(true, false).request_entry(1, &req, Some(&RequestResult::Failed("timeout")));

        assert_eq!(entry.error.as_deref(), Some("timeout"));
        assert!(entry.response.is_none());
    }
}
