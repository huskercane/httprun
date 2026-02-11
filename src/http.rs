use std::collections::HashMap;
use std::time::Instant;

use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::error::AppError;
use crate::parser::{HttpMethod, ParsedRequest};

#[derive(Debug, Clone)]
pub struct ContentType {
    pub mime_type: String,
    pub charset: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, Vec<String>>,
    pub body_raw: String,
    pub body_json: Option<serde_json::Value>,
    pub content_type: Option<ContentType>,
    pub elapsed_ms: u128,
}

pub fn execute_request(request: &ParsedRequest) -> Result<HttpResponse, AppError> {
    let client = Client::new();

    let method = match &request.method {
        HttpMethod::Get => reqwest::Method::GET,
        HttpMethod::Post => reqwest::Method::POST,
        HttpMethod::Put => reqwest::Method::PUT,
        HttpMethod::Patch => reqwest::Method::PATCH,
        HttpMethod::Delete => reqwest::Method::DELETE,
        HttpMethod::Head => reqwest::Method::HEAD,
        HttpMethod::Options => reqwest::Method::OPTIONS,
    };

    let mut header_map = HeaderMap::new();
    for h in &request.headers {
        let name = HeaderName::from_bytes(h.name.as_bytes())
            .map_err(|e| AppError::Parse {
                line: request.line_number,
                message: format!("Invalid header name '{}': {}", h.name, e),
            })?;
        let value = HeaderValue::from_str(&h.value)
            .map_err(|e| AppError::Parse {
                line: request.line_number,
                message: format!("Invalid header value '{}': {}", h.value, e),
            })?;
        header_map.insert(name, value);
    }

    let mut builder = client.request(method, &request.url).headers(header_map);

    if let Some(body) = &request.body {
        builder = builder.body(body.clone());
    }

    let start = Instant::now();
    let response = builder.send()?;
    let elapsed_ms = start.elapsed().as_millis();

    let status = response.status().as_u16();

    // Collect headers
    let mut headers: HashMap<String, Vec<String>> = HashMap::new();
    for (name, value) in response.headers() {
        let name_str = name.as_str().to_string();
        let value_str = value.to_str().unwrap_or("").to_string();
        headers.entry(name_str).or_default().push(value_str);
    }

    // Parse content type
    let content_type = headers.get("content-type").and_then(|vals| {
        vals.first().map(|ct| {
            let parts: Vec<&str> = ct.split(';').collect();
            let mime_type = parts[0].trim().to_string();
            let charset = parts.iter().find_map(|p| {
                let p = p.trim();
                if p.to_lowercase().starts_with("charset=") {
                    Some(p[8..].trim().to_string())
                } else {
                    None
                }
            });
            ContentType { mime_type, charset }
        })
    });

    let body_raw = response.text()?;

    // Try to parse as JSON
    let body_json = serde_json::from_str(&body_raw).ok();

    Ok(HttpResponse {
        status,
        headers,
        body_raw,
        body_json,
        content_type,
        elapsed_ms,
    })
}
