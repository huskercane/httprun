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

pub fn print_response_body(response: &HttpResponse) {
    if response.body_raw.is_empty() {
        return;
    }

    // Pretty-print JSON if available, otherwise raw
    let display = if let Some(json) = &response.body_json {
        serde_json::to_string_pretty(json).unwrap_or_else(|_| response.body_raw.clone())
    } else {
        response.body_raw.clone()
    };

    // Truncate long bodies
    let lines: Vec<&str> = display.lines().collect();
    if lines.len() > 30 {
        for line in &lines[..30] {
            println!("  {}", line.dimmed());
        }
        println!("  {}", format!("... ({} more lines)", lines.len() - 30).dimmed());
    } else {
        for line in &lines {
            println!("  {}", line.dimmed());
        }
    }
}

pub fn print_verbose_request(request: &ParsedRequest) {
    if !request.headers.is_empty() {
        println!("  {}", "Request Headers:".dimmed());
        for h in &request.headers {
            println!("    {}: {}", h.name.dimmed(), h.value.dimmed());
        }
    }
    if let Some(body) = &request.body {
        println!("  {}", "Request Body:".dimmed());
        for line in body.lines() {
            println!("    {}", line.dimmed());
        }
    }
}

pub fn print_verbose_response(response: &HttpResponse) {
    println!("  {}", "Response Headers:".dimmed());
    for (name, values) in &response.headers {
        for v in values {
            println!("    {}: {}", name.dimmed(), v.dimmed());
        }
    }
    println!("  {}", "Response Body:".dimmed());
    print_response_body(response);
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
