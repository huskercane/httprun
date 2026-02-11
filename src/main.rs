mod env;
mod error;
mod http;
mod js;
mod output;
mod parser;
mod variable;

use std::path::PathBuf;
use std::process;

use clap::Parser;

use crate::error::AppError;
use crate::variable::VariableStore;

#[derive(Parser, Debug)]
#[command(name = "httprun", about = "Run IntelliJ .http request files from the terminal")]
struct Cli {
    /// Path to the .http file
    file: PathBuf,

    /// Environment name to use (from http-client.env.json)
    #[arg(long)]
    env: Option<String>,

    /// Path to the environment file (default: ./http-client.env.json)
    #[arg(long, default_value = "http-client.env.json")]
    env_file: PathBuf,

    /// Run a single request by name
    #[arg(long)]
    name: Option<String>,

    /// Run a single request by 1-based index
    #[arg(long)]
    index: Option<usize>,

    /// Show full request/response details
    #[arg(short, long)]
    verbose: bool,

    /// Parse and display without executing
    #[arg(long)]
    dry_run: bool,
}

fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli) {
        output::print_error(&format!("{}", e));
        process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), AppError> {
    // Read and parse the .http file
    let content = std::fs::read_to_string(&cli.file).map_err(|e| {
        AppError::Io(std::io::Error::new(
            e.kind(),
            format!("{}: {}", cli.file.display(), e),
        ))
    })?;

    let parse_result = parser::parse_http_file(&content)?;
    let all_requests = parse_result.requests;

    if all_requests.is_empty() {
        output::print_error("No requests found in file");
        return Ok(());
    }

    // Load environment variables
    let env_vars = if let Some(env_name) = &cli.env {
        // Resolve env file relative to the .http file's directory
        let env_file = if cli.env_file.is_relative() {
            if let Some(parent) = cli.file.parent() {
                parent.join(&cli.env_file)
            } else {
                cli.env_file.clone()
            }
        } else {
            cli.env_file.clone()
        };

        env::load_environment(&env_file, env_name)?
    } else {
        std::collections::HashMap::new()
    };

    let mut var_store = VariableStore::new(env_vars);

    // Load in-place variables
    for (name, value) in &parse_result.in_place_vars {
        var_store.set_in_place(name.clone(), value.clone());
    }

    // Filter requests if --name or --index specified
    let requests: Vec<(usize, &parser::ParsedRequest)> = if let Some(name) = &cli.name {
        all_requests
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                r.name
                    .as_ref()
                    .is_some_and(|n| n.to_lowercase().contains(&name.to_lowercase()))
            })
            .collect()
    } else if let Some(index) = cli.index {
        if index == 0 || index > all_requests.len() {
            return Err(AppError::Parse {
                line: 0,
                message: format!(
                    "Index {} out of range (1-{})",
                    index,
                    all_requests.len()
                ),
            });
        }
        vec![(index - 1, &all_requests[index - 1])]
    } else {
        all_requests.iter().enumerate().collect()
    };

    if requests.is_empty() {
        output::print_error("No matching requests found");
        return Ok(());
    }

    // Dry run mode
    if cli.dry_run {
        println!(
            "Dry run: {} request(s) from {}",
            requests.len(),
            cli.file.display()
        );
        for (i, req) in &requests {
            let mut resolved = (*req).clone();
            // Try to substitute variables (best-effort for dry run)
            if let Ok(url) = var_store.substitute(&resolved.url) {
                resolved.url = ensure_http_scheme(&url);
            }
            output::print_dry_run_request(i + 1, &resolved);
        }
        return Ok(());
    }

    // Execute requests
    let mut passed_tests = 0usize;
    let mut failed_tests = 0usize;
    let mut error_count = 0usize;

    for (i, req) in &requests {
        // Clone and resolve variables
        let mut resolved = (*req).clone();
        let resolved_url = var_store.substitute(&resolved.url)?;
        resolved.url = ensure_http_scheme(&resolved_url);

        // Substitute variables in headers
        for header in &mut resolved.headers {
            header.value = var_store.substitute(&header.value)?;
        }

        // Substitute variables in body
        if let Some(body) = &resolved.body {
            resolved.body = Some(var_store.substitute(body)?);
        }

        output::print_request_header(i + 1, &resolved);

        if cli.verbose {
            output::print_verbose_request(&resolved);
        }

        // Execute HTTP request
        match http::execute_request(&resolved) {
            Ok(response) => {
                output::print_response_status(&response);

                if cli.verbose {
                    output::print_verbose_response(&response);
                }

                // Run response handler if present
                if let Some(handler) = &resolved.response_handler {
                    match js::execute_handler(handler, &response) {
                        Ok(result) => {
                            // Merge global variables
                            var_store.merge_globals(&result.global_vars);

                            // Print logs
                            if !result.log_output.is_empty() {
                                output::print_log_output(&result.log_output);
                            }

                            // Print test results
                            if !result.test_results.is_empty() {
                                output::print_test_results(&result.test_results);
                                for tr in &result.test_results {
                                    if tr.passed {
                                        passed_tests += 1;
                                    } else {
                                        failed_tests += 1;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            output::print_error(&format!("Handler error: {}", e));
                            error_count += 1;
                        }
                    }
                }
            }
            Err(e) => {
                output::print_error(&format!("{}", e));
                error_count += 1;
            }
        }
    }

    // Print summary
    output::print_summary(requests.len(), passed_tests, failed_tests, error_count);

    // Exit with failure if any tests failed or errors occurred
    if failed_tests > 0 || error_count > 0 {
        process::exit(1);
    }

    Ok(())
}

fn ensure_http_scheme(url: &str) -> String {
    let trimmed = url.trim();
    if has_url_scheme(trimmed) {
        trimmed.to_string()
    } else {
        format!("https://{}", trimmed)
    }
}

fn has_url_scheme(url: &str) -> bool {
    let Some(idx) = url.find("://") else {
        return false;
    };
    if idx == 0 {
        return false;
    }
    let scheme = &url[..idx];
    let mut chars = scheme.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    let mut has_plus_or_dash = false;
    let mut has_dot = false;
    for c in chars {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' => {}
            '+' | '-' => {
                has_plus_or_dash = true;
            }
            '.' => {
                has_dot = true;
            }
            _ => return false,
        }
    }
    // Heuristic: treat dotted, domain-like prefixes without + or - as missing schemes.
    !(has_dot && !has_plus_or_dash)
}

#[cfg(test)]
mod tests {
    use super::{ensure_http_scheme, has_url_scheme};

    #[test]
    fn has_url_scheme_accepts_valid_schemes() {
        assert!(has_url_scheme("http://example.com"));
        assert!(has_url_scheme("https://example.com"));
        assert!(has_url_scheme("ftp://example.com"));
        assert!(has_url_scheme("custom+v1.2-scheme://example.com"));
    }

    #[test]
    fn has_url_scheme_rejects_invalid_or_missing_schemes() {
        assert!(!has_url_scheme("://example.com"));
        assert!(!has_url_scheme("1http://example.com"));
        assert!(!has_url_scheme("http:/example.com"));
        assert!(!has_url_scheme("example.com/path"));
        assert!(!has_url_scheme("example.com://path"));
    }

    #[test]
    fn ensure_http_scheme_only_prepends_when_missing() {
        assert_eq!(
            ensure_http_scheme("https://example.com"),
            "https://example.com"
        );
        assert_eq!(
            ensure_http_scheme("ftp://example.com"),
            "ftp://example.com"
        );
        assert_eq!(
            ensure_http_scheme("example.com/path"),
            "https://example.com/path"
        );
        assert_eq!(
            ensure_http_scheme("  example.com  "),
            "https://example.com"
        );
    }
}
