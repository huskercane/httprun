use colored::Colorize;

use crate::http::HttpResponse;
use crate::js::TestResult;
use crate::parser::ParsedRequest;

pub fn print_separator() {
    println!("{}", "─".repeat(60).dimmed());
}

pub fn print_request_header(index: usize, request: &ParsedRequest) {
    let name = request
        .name
        .as_deref()
        .unwrap_or("Unnamed request");
    println!(
        "\n{} {}",
        format!("[{}]", index).cyan().bold(),
        name.cyan().bold()
    );
    println!(
        "  {} {}",
        request.method.as_str().white().bold(),
        request.url.white()
    );
}

pub fn print_response_status(response: &HttpResponse) {
    let status_str = format!("{}", response.status);
    let colored_status = match response.status {
        200..=299 => status_str.green().bold(),
        300..=399 => status_str.yellow().bold(),
        _ => status_str.red().bold(),
    };
    println!(
        "  {} {} ({}ms)",
        "→".dimmed(),
        colored_status,
        response.elapsed_ms
    );
}

/// curl-style trace marker for connection / TLS / timing meta info.
fn meta(line: &str) {
    println!("  {} {}", "*".magenta().dimmed(), line.dimmed());
}

/// curl-style trace marker for bytes sent.
fn sent(line: &str) {
    println!("  {} {}", ">".cyan().dimmed(), line.dimmed());
}

/// curl-style trace marker for bytes received.
fn recv(line: &str) {
    println!("  {} {}", "<".green().dimmed(), line.dimmed());
}

pub fn print_trace_request(request: &ParsedRequest) {
    if let Some(host) = host_from_url(&request.url) {
        meta(&format!("Connected to {}", host));
    }

    let path = path_from_url(&request.url);
    sent(&format!("{} {} HTTP/1.1", request.method.as_str(), path));
    for h in &request.headers {
        sent(&format!("{}: {}", h.name, h.value));
    }
    if let Some(body) = &request.body
        && !body.is_empty()
    {
        sent("");
        for line in body.lines() {
            sent(line);
        }
    }
}

pub fn print_trace_response(response: &HttpResponse) {
    recv(&format!("{} {}", response.http_version, response.status));
    for (name, values) in &response.headers {
        for v in values {
            recv(&format!("{}: {}", name, v));
        }
    }
    if !response.body_raw.is_empty() {
        recv("");
        let display = if let Some(json) = &response.body_json {
            serde_json::to_string_pretty(json).unwrap_or_else(|_| response.body_raw.clone())
        } else {
            response.body_raw.clone()
        };
        let lines: Vec<&str> = display.lines().collect();
        let cap = 30;
        for line in lines.iter().take(cap) {
            recv(line);
        }
        if lines.len() > cap {
            meta(&format!("... ({} more lines)", lines.len() - cap));
        }
    }
    meta(&format!("Total: {}ms", response.elapsed_ms));
}

pub fn print_curl_command(cmd: &str) {
    for line in cmd.lines() {
        println!("  {} {}", "$".yellow().bold(), line.dimmed());
    }
}

fn host_from_url(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let host_part = after_scheme.split(['/', '?', '#']).next()?;
    let host = host_part.split('@').next_back()?;
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

fn path_from_url(url: &str) -> String {
    let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    // Fragments are not sent on the wire — strip before extracting the path.
    let no_fragment = after_scheme.split_once('#').map_or(after_scheme, |(p, _)| p);
    match no_fragment.find(['/', '?']) {
        Some(idx) => {
            let rest = &no_fragment[idx..];
            if rest.starts_with('/') {
                rest.to_string()
            } else {
                format!("/{}", rest)
            }
        }
        None => "/".to_string(),
    }
}

pub fn print_test_results(results: &[TestResult]) {
    for result in results {
        if result.passed {
            println!("  {} {}", "PASS".green().bold(), result.name);
        } else {
            let msg = result
                .failure_message
                .as_deref()
                .unwrap_or("Assertion failed");
            println!("  {} {} — {}", "FAIL".red().bold(), result.name, msg.red());
        }
    }
}

pub fn print_log_output(logs: &[String]) {
    for line in logs {
        println!("  {} {}", "LOG".blue().bold(), line);
    }
}

pub fn print_error(msg: &str) {
    eprintln!("  {} {}", "ERROR".red().bold(), msg.red());
}

pub fn print_summary(total: usize, passed: usize, failed: usize, errors: usize) {
    println!();
    print_separator();

    let summary = format!(
        "Requests: {}  |  Tests passed: {}  |  Tests failed: {}  |  Errors: {}",
        total, passed, failed, errors
    );

    if failed == 0 && errors == 0 {
        println!("{}", summary.green().bold());
    } else {
        println!("{}", summary.red().bold());
    }
}

#[cfg(test)]
mod tests {
    use super::{host_from_url, path_from_url};

    #[test]
    fn host_from_url_extracts_host() {
        assert_eq!(host_from_url("https://api.example.com/v1/x"), Some("api.example.com".into()));
        assert_eq!(host_from_url("http://localhost:8080/x?y=1"), Some("localhost:8080".into()));
        assert_eq!(host_from_url("https://user:pw@host.tld/x"), Some("host.tld".into()));
        assert_eq!(host_from_url("example.com/x"), Some("example.com".into()));
    }

    #[test]
    fn path_from_url_extracts_path() {
        assert_eq!(path_from_url("https://example.com/v1/users?a=1"), "/v1/users?a=1");
        assert_eq!(path_from_url("https://example.com"), "/");
        assert_eq!(path_from_url("https://example.com/"), "/");
    }

    #[test]
    fn path_from_url_preserves_query_without_explicit_path() {
        assert_eq!(path_from_url("https://example.com?foo=1"), "/?foo=1");
        assert_eq!(path_from_url("https://example.com?foo=1&bar=2"), "/?foo=1&bar=2");
    }

    #[test]
    fn path_from_url_strips_fragment() {
        // Fragments are client-side and not sent on the wire.
        assert_eq!(path_from_url("https://example.com#anchor"), "/");
        assert_eq!(path_from_url("https://example.com/page#section"), "/page");
        assert_eq!(path_from_url("https://example.com/page?q=1#section"), "/page?q=1");
    }
}

pub fn print_dry_run_request(index: usize, request: &ParsedRequest) {
    print_request_header(index, request);

    if !request.headers.is_empty() {
        for h in &request.headers {
            println!("    {}: {}", h.name, h.value);
        }
    }

    if let Some(body) = &request.body {
        println!();
        for line in body.lines() {
            println!("    {}", line);
        }
    }

    if request.response_handler.is_some() {
        println!("    {}", "(has response handler)".dimmed());
    }
}
