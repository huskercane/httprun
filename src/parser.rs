use regex::Regex;
use std::sync::LazyLock;

use crate::error::AppError;

static REQUEST_LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS)\s+(\S+)(?:\s+HTTP/[\d.]+)?$").unwrap()
});

static HEADER_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Za-z0-9\-]+)\s*:\s*(.+)$").unwrap());

static HANDLER_START_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^>\s*\{%\s*$").unwrap());

static HANDLER_END_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*%\}\s*$").unwrap());

static RESPONSE_HISTORY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^<>\s+").unwrap());

static IN_PLACE_VAR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^@(\S+)\s*=\s*(.+)$").unwrap());

#[derive(Debug, Clone, PartialEq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl HttpMethod {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "GET" => Some(Self::Get),
            "POST" => Some(Self::Post),
            "PUT" => Some(Self::Put),
            "PATCH" => Some(Self::Patch),
            "DELETE" => Some(Self::Delete),
            "HEAD" => Some(Self::Head),
            "OPTIONS" => Some(Self::Options),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
        }
    }
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct ParsedRequest {
    pub name: Option<String>,
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<Header>,
    pub body: Option<String>,
    pub response_handler: Option<String>,
    pub line_number: usize,
}

#[derive(Debug)]
enum ParserState {
    AwaitingRequest,
    ReadingHeaders,
    ReadingBody,
    ReadingHandler,
}

pub struct ParseResult {
    pub requests: Vec<ParsedRequest>,
    pub in_place_vars: Vec<(String, String)>,
}

pub fn parse_http_file(content: &str) -> Result<ParseResult, AppError> {
    let lines: Vec<&str> = content.lines().collect();
    let mut requests: Vec<ParsedRequest> = Vec::new();
    let mut in_place_vars: Vec<(String, String)> = Vec::new();
    let mut state = ParserState::AwaitingRequest;

    let mut current_name: Option<String> = None;
    let mut current_method: Option<HttpMethod> = None;
    let mut current_url: Option<String> = None;
    let mut current_headers: Vec<Header> = Vec::new();
    let mut current_body_lines: Vec<String> = Vec::new();
    let mut current_handler_lines: Vec<String> = Vec::new();
    let mut current_line_number: usize = 0;

    let finalize_request =
        |requests: &mut Vec<ParsedRequest>,
         name: &mut Option<String>,
         method: &mut Option<HttpMethod>,
         url: &mut Option<String>,
         headers: &mut Vec<Header>,
         body_lines: &mut Vec<String>,
         handler_lines: &mut Vec<String>,
         line_number: usize| {
            if let (Some(m), Some(u)) = (method.take(), url.take()) {
                let body_text = body_lines.join("\n");
                let body = if body_text.trim().is_empty() {
                    None
                } else {
                    Some(body_text.trim_end().to_string())
                };

                let handler_text = handler_lines.join("\n");
                let handler = if handler_text.trim().is_empty() {
                    None
                } else {
                    Some(handler_text)
                };

                requests.push(ParsedRequest {
                    name: name.take(),
                    method: m,
                    url: u,
                    headers: std::mem::take(headers),
                    body,
                    response_handler: handler,
                    line_number,
                });
            }
            body_lines.clear();
            handler_lines.clear();
        };

    for (i, line) in lines.iter().enumerate() {
        let line_num = i + 1;
        let trimmed = line.trim();

        match state {
            ParserState::AwaitingRequest => {
                // Skip empty lines and comments
                if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                    // Check for ### separator with optional name
                    if trimmed.starts_with("###") {
                        let after = trimmed[3..].trim();
                        if !after.is_empty() {
                            current_name = Some(after.to_string());
                        }
                    }
                    continue;
                }

                // Check for in-place variable definitions: @name = value
                if let Some(caps) = IN_PLACE_VAR_RE.captures(trimmed) {
                    let var_name = caps[1].to_string();
                    let var_value = caps[2].trim().to_string();
                    in_place_vars.push((var_name, var_value));
                    continue;
                }

                // Check for response history lines
                if RESPONSE_HISTORY_RE.is_match(trimmed) {
                    continue;
                }

                // Try to parse as request line
                if let Some(caps) = REQUEST_LINE_RE.captures(trimmed) {
                    let method = HttpMethod::from_str(&caps[1]).unwrap();
                    let url = caps[2].to_string();
                    current_method = Some(method);
                    current_url = Some(url);
                    current_line_number = line_num;
                    state = ParserState::ReadingHeaders;
                }
            }

            ParserState::ReadingHeaders => {
                // Blank line transitions to body
                if trimmed.is_empty() {
                    state = ParserState::ReadingBody;
                    continue;
                }

                // Handler start
                if HANDLER_START_RE.is_match(trimmed) {
                    state = ParserState::ReadingHandler;
                    continue;
                }

                // ### separator means end of this request (no body)
                if trimmed.starts_with("###") {
                    finalize_request(
                        &mut requests,
                        &mut current_name,
                        &mut current_method,
                        &mut current_url,
                        &mut current_headers,
                        &mut current_body_lines,
                        &mut current_handler_lines,
                        current_line_number,
                    );
                    let after = trimmed[3..].trim();
                    if !after.is_empty() {
                        current_name = Some(after.to_string());
                    }
                    state = ParserState::AwaitingRequest;
                    continue;
                }

                // Response history
                if RESPONSE_HISTORY_RE.is_match(trimmed) {
                    finalize_request(
                        &mut requests,
                        &mut current_name,
                        &mut current_method,
                        &mut current_url,
                        &mut current_headers,
                        &mut current_body_lines,
                        &mut current_handler_lines,
                        current_line_number,
                    );
                    state = ParserState::AwaitingRequest;
                    continue;
                }

                // Try to parse header
                if let Some(caps) = HEADER_LINE_RE.captures(trimmed) {
                    current_headers.push(Header {
                        name: caps[1].to_string(),
                        value: caps[2].trim().to_string(),
                    });
                }
            }

            ParserState::ReadingBody => {
                // Handler start
                if HANDLER_START_RE.is_match(trimmed) {
                    state = ParserState::ReadingHandler;
                    continue;
                }

                // ### separator
                if trimmed.starts_with("###") {
                    finalize_request(
                        &mut requests,
                        &mut current_name,
                        &mut current_method,
                        &mut current_url,
                        &mut current_headers,
                        &mut current_body_lines,
                        &mut current_handler_lines,
                        current_line_number,
                    );
                    let after = trimmed[3..].trim();
                    if !after.is_empty() {
                        current_name = Some(after.to_string());
                    }
                    state = ParserState::AwaitingRequest;
                    continue;
                }

                // Response history line — finalize current request
                if RESPONSE_HISTORY_RE.is_match(trimmed) {
                    finalize_request(
                        &mut requests,
                        &mut current_name,
                        &mut current_method,
                        &mut current_url,
                        &mut current_headers,
                        &mut current_body_lines,
                        &mut current_handler_lines,
                        current_line_number,
                    );
                    state = ParserState::AwaitingRequest;
                    continue;
                }

                current_body_lines.push(line.to_string());
            }

            ParserState::ReadingHandler => {
                if HANDLER_END_RE.is_match(trimmed) {
                    // End of handler — check what comes next
                    // Stay in a transitional state: finalize when we see ### or <> or next request
                    // For simplicity, finalize now and go to AwaitingRequest
                    finalize_request(
                        &mut requests,
                        &mut current_name,
                        &mut current_method,
                        &mut current_url,
                        &mut current_headers,
                        &mut current_body_lines,
                        &mut current_handler_lines,
                        current_line_number,
                    );
                    state = ParserState::AwaitingRequest;
                    continue;
                }

                current_handler_lines.push(line.to_string());
            }
        }
    }

    // Finalize any remaining request
    finalize_request(
        &mut requests,
        &mut current_name,
        &mut current_method,
        &mut current_url,
        &mut current_headers,
        &mut current_body_lines,
        &mut current_handler_lines,
        current_line_number,
    );

    Ok(ParseResult {
        requests,
        in_place_vars,
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_http_file, HttpMethod};

    #[test]
    fn parses_request_with_body_and_handler() {
        let content = r#"
@token = abc123

### create item
POST https://example.com/items
Content-Type: application/json
X-Trace: 123

{
  "name": "widget"
}

> {%
  client.test("status is 200", function() {
    client.assert(response.status === 200);
  });
%}
"#;

        let parsed = parse_http_file(content).expect("parse should succeed");
        assert_eq!(parsed.in_place_vars, vec![("token".to_string(), "abc123".to_string())]);
        assert_eq!(parsed.requests.len(), 1);

        let req = &parsed.requests[0];
        assert_eq!(req.name.as_deref(), Some("create item"));
        assert_eq!(req.method, HttpMethod::Post);
        assert_eq!(req.url, "https://example.com/items");
        assert_eq!(req.headers.len(), 2);
        assert_eq!(req.headers[0].name, "Content-Type");
        assert_eq!(req.headers[0].value, "application/json");
        assert_eq!(req.headers[1].name, "X-Trace");
        assert_eq!(req.headers[1].value, "123");
        assert_eq!(
            req.body.as_deref(),
            Some("{\n  \"name\": \"widget\"\n}")
        );
        let handler = req.response_handler.as_deref().expect("handler present");
        assert!(handler.contains("client.test(\"status is 200\""));
        assert!(handler.contains("client.assert(response.status === 200);"));
    }
}
