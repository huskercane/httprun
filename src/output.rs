use std::io::{self, Write};

use colored::Colorize;

use crate::http::HttpResponse;
use crate::js::TestResult;
use crate::parser::ParsedRequest;
use crate::report::Summary;

pub fn print_separator(w: &mut dyn Write) -> io::Result<()> {
    writeln!(w, "{}", "─".repeat(60).dimmed())
}

pub fn print_request_header(
    w: &mut dyn Write,
    index: usize,
    request: &ParsedRequest,
) -> io::Result<()> {
    let name = request
        .name
        .as_deref()
        .unwrap_or("Unnamed request");
    writeln!(
        w,
        "\n{} {}",
        format!("[{}]", index).cyan().bold(),
        name.cyan().bold()
    )?;
    writeln!(
        w,
        "  {} {}",
        request.method.as_str().white().bold(),
        request.url.white()
    )
}

pub fn print_response_status(w: &mut dyn Write, response: &HttpResponse) -> io::Result<()> {
    let status_str = format!("{}", response.status);
    let colored_status = match response.status {
        200..=299 => status_str.green().bold(),
        300..=399 => status_str.yellow().bold(),
        _ => status_str.red().bold(),
    };
    writeln!(
        w,
        "  {} {} ({}ms)",
        "→".dimmed(),
        colored_status,
        response.elapsed_ms
    )
}

/// curl-style trace marker for connection / TLS / timing meta info.
fn meta(w: &mut dyn Write, line: &str) -> io::Result<()> {
    writeln!(w, "  {} {}", "*".magenta().dimmed(), line.dimmed())
}

/// curl-style trace marker for bytes sent.
fn sent(w: &mut dyn Write, line: &str) -> io::Result<()> {
    writeln!(w, "  {} {}", ">".cyan().dimmed(), line.dimmed())
}

/// curl-style trace marker for bytes received.
fn recv(w: &mut dyn Write, line: &str) -> io::Result<()> {
    writeln!(w, "  {} {}", "<".green().dimmed(), line.dimmed())
}

pub fn print_trace_request(w: &mut dyn Write, request: &ParsedRequest) -> io::Result<()> {
    if let Some(host) = host_from_url(&request.url) {
        meta(w, &format!("Connected to {}", host))?;
    }

    let path = path_from_url(&request.url);
    sent(w, &format!("{} {} HTTP/1.1", request.method.as_str(), path))?;
    for h in &request.headers {
        sent(w, &format!("{}: {}", h.name, h.value))?;
    }
    if let Some(body) = &request.body
        && !body.is_empty()
    {
        sent(w, "")?;
        for line in body.lines() {
            sent(w, line)?;
        }
    }
    Ok(())
}

pub fn print_trace_response(w: &mut dyn Write, response: &HttpResponse) -> io::Result<()> {
    recv(w, &format!("{} {}", response.http_version, response.status))?;
    for (name, values) in &response.headers {
        for v in values {
            recv(w, &format!("{}: {}", name, v))?;
        }
    }
    if !response.body_raw.is_empty() {
        recv(w, "")?;
        let display = if let Some(json) = &response.body_json {
            serde_json::to_string_pretty(json).unwrap_or_else(|_| response.body_raw.clone())
        } else {
            response.body_raw.clone()
        };
        let lines: Vec<&str> = display.lines().collect();
        let cap = 30;
        for line in lines.iter().take(cap) {
            recv(w, line)?;
        }
        if lines.len() > cap {
            meta(w, &format!("... ({} more lines)", lines.len() - cap))?;
        }
    }
    meta(w, &format!("Total: {}ms", response.elapsed_ms))
}

pub fn print_curl_command(w: &mut dyn Write, cmd: &str) -> io::Result<()> {
    for line in cmd.lines() {
        writeln!(w, "  {} {}", "$".yellow().bold(), line.dimmed())?;
    }
    Ok(())
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

/// Print test results. `only_failures` drops passing tests, which is what
/// `--quiet` needs: silence on success, detail on failure.
pub fn print_test_results(
    w: &mut dyn Write,
    results: &[TestResult],
    only_failures: bool,
) -> io::Result<()> {
    for result in results {
        if result.passed {
            if only_failures {
                continue;
            }
            writeln!(w, "  {} {}", "PASS".green().bold(), result.name)?;
        } else {
            let msg = result
                .failure_message
                .as_deref()
                .unwrap_or("Assertion failed");
            writeln!(
                w,
                "  {} {} — {}",
                "FAIL".red().bold(),
                result.name,
                msg.red()
            )?;
        }
    }
    Ok(())
}

pub fn print_log_output(w: &mut dyn Write, logs: &[String]) -> io::Result<()> {
    for line in logs {
        writeln!(w, "  {} {}", "LOG".blue().bold(), line)?;
    }
    Ok(())
}

pub fn print_error(w: &mut dyn Write, msg: &str) -> io::Result<()> {
    writeln!(w, "  {} {}", "ERROR".red().bold(), msg.red())
}

/// Fatal / out-of-band errors always go to stderr, so that stdout stays a
/// clean machine-readable stream under `--format json`.
pub fn eprint_error(msg: &str) {
    eprintln!("  {} {}", "ERROR".red().bold(), msg.red());
}

pub fn print_summary(w: &mut dyn Write, summary: &Summary) -> io::Result<()> {
    writeln!(w)?;
    print_separator(w)?;

    let line = format!(
        "Requests: {}  |  Tests passed: {}  |  Tests failed: {}  |  Errors: {}  |  {}ms",
        summary.total,
        summary.tests_passed,
        summary.tests_failed,
        summary.errors,
        summary.duration_ms
    );

    if summary.is_success() {
        writeln!(w, "{}", line.green().bold())
    } else {
        writeln!(w, "{}", line.red().bold())
    }
}

pub fn print_dry_run_request(
    w: &mut dyn Write,
    index: usize,
    request: &ParsedRequest,
) -> io::Result<()> {
    print_request_header(w, index, request)?;

    if !request.headers.is_empty() {
        for h in &request.headers {
            writeln!(w, "    {}: {}", h.name, h.value)?;
        }
    }

    if let Some(body) = &request.body {
        writeln!(w)?;
        for line in body.lines() {
            writeln!(w, "    {}", line)?;
        }
    }

    if request.response_handler.is_some() {
        writeln!(w, "    {}", "(has response handler)".dimmed())?;
    }
    Ok(())
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
