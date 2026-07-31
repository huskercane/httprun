mod curl;
mod env;
mod error;
mod http;
mod js;
mod output;
mod parser;
mod report;
mod variable;

use std::io::{BufWriter, IsTerminal, Write};
use std::path::PathBuf;
use std::process;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use clap::{Parser, ValueEnum};

use crate::error::AppError;
use crate::report::{
    HumanReporter, JsonReporter, JsonStyle, Reporter, RequestReport, RequestResult, RunMeta,
    Summary,
};
use crate::variable::VariableStore;

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum Format {
    /// Colored, streaming terminal output
    Human,
    /// A single JSON document describing the whole run
    Json,
    /// One JSON object per line, streamed as each request completes
    Ndjson,
}

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

    /// Show full request/response details (curl-style trace)
    #[arg(short, long)]
    verbose: bool,

    /// Suppress per-request output; print only failures and the summary
    #[arg(short, long, conflicts_with = "verbose")]
    quiet: bool,

    /// Output format
    #[arg(long, value_enum, default_value_t = Format::Human)]
    format: Format,

    /// Write the report to a file instead of stdout
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Do not redact credential headers in json/ndjson output
    #[arg(long)]
    include_secrets: bool,

    /// Parse and display without executing
    #[arg(long)]
    dry_run: bool,

    /// Print a copy-pasteable curl command for each request
    #[arg(long)]
    curl: bool,
}

fn main() {
    let cli = Cli::parse();

    match run(cli) {
        Ok(code) => process::exit(code),
        Err(e) => {
            output::eprint_error(&format!("{}", e));
            process::exit(1);
        }
    }
}

/// Build the output sink. A terminal keeps line buffering so output streams
/// live; anything else gets a large buffer, which collapses the ~50 write
/// syscalls a verbose request would otherwise make into a handful.
fn build_writer(cli: &Cli) -> Result<(Box<dyn Write>, bool), AppError> {
    match &cli.output {
        Some(path) => {
            let file = std::fs::File::create(path).map_err(|e| {
                AppError::Io(std::io::Error::new(
                    e.kind(),
                    format!("{}: {}", path.display(), e),
                ))
            })?;
            Ok((Box::new(BufWriter::new(file)), true))
        }
        None => {
            let stdout = std::io::stdout();
            if stdout.is_terminal() {
                Ok((Box::new(stdout), false))
            } else {
                Ok((Box::new(BufWriter::with_capacity(64 * 1024, stdout)), false))
            }
        }
    }
}

fn build_reporter(cli: &Cli) -> Result<Box<dyn Reporter>, AppError> {
    let (writer, to_file) = build_writer(cli)?;

    match cli.format {
        Format::Human => {
            // `colored` detects a tty on stdout; it cannot know we redirected
            // into a file, so suppress escape codes explicitly.
            if to_file {
                colored::control::set_override(false);
            }
            Ok(Box::new(HumanReporter::new(
                writer,
                cli.verbose,
                cli.curl,
                cli.quiet,
            )))
        }
        Format::Json | Format::Ndjson => {
            let style = if cli.format == Format::Json {
                JsonStyle::Document
            } else {
                JsonStyle::Ndjson
            };
            let meta = RunMeta {
                file: cli.file.display().to_string(),
                environment: cli.env.clone(),
                started_at_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0),
            };
            Ok(Box::new(JsonReporter::new(
                writer,
                style,
                meta,
                cli.curl,
                !cli.include_secrets,
            )))
        }
    }
}

fn run(cli: Cli) -> Result<i32, AppError> {
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
        output::eprint_error("No requests found in file");
        return Ok(0);
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
        output::eprint_error("No matching requests found");
        return Ok(0);
    }

    let mut reporter = build_reporter(&cli)?;

    let result = if cli.dry_run {
        dry_run(&cli, &requests, &var_store, reporter.as_mut())
    } else {
        execute(&requests, &mut var_store, reporter.as_mut())
    };

    // Flush unconditionally: `process::exit` skips destructors, so a buffered
    // writer that is only flushed on the happy path silently loses output.
    let flushed = reporter.flush();
    let summary = result?;
    flushed?;

    Ok(summary.exit_code())
}

fn dry_run(
    cli: &Cli,
    requests: &[(usize, &parser::ParsedRequest)],
    var_store: &VariableStore,
    reporter: &mut dyn Reporter,
) -> Result<Summary, AppError> {
    reporter.dry_run_started(requests.len(), &cli.file)?;

    for (i, req) in requests {
        let resolved = resolve_request_best_effort(req, var_store);
        reporter.dry_run_request(i + 1, &resolved)?;
    }

    let summary = Summary {
        total: requests.len(),
        ..Default::default()
    };
    reporter.finish(&summary)?;
    Ok(summary)
}

fn execute(
    requests: &[(usize, &parser::ParsedRequest)],
    var_store: &mut VariableStore,
    reporter: &mut dyn Reporter,
) -> Result<Summary, AppError> {
    let started = Instant::now();
    let mut summary = Summary {
        total: requests.len(),
        ..Default::default()
    };

    for (i, req) in requests {
        let index = i + 1;
        let resolved = resolve_request(req, var_store)?;

        reporter.request_started(index, &resolved)?;

        match http::execute_request(&resolved) {
            Ok(response) => {
                let mut logs = Vec::new();
                let mut tests = Vec::new();
                let mut handler_error = None;

                if let Some(handler) = &resolved.response_handler {
                    match js::execute_handler(handler, &response, var_store.globals()) {
                        Ok(result) => {
                            var_store.merge_globals(&result.global_vars);
                            logs = result.log_output;
                            tests = result.test_results;
                            for tr in &tests {
                                if tr.passed {
                                    summary.tests_passed += 1;
                                } else {
                                    summary.tests_failed += 1;
                                }
                            }
                        }
                        Err(e) => {
                            handler_error = Some(format!("Handler error: {}", e));
                            summary.errors += 1;
                        }
                    }
                }

                reporter.request_finished(&RequestReport {
                    index,
                    request: &resolved,
                    result: RequestResult::Completed {
                        response: &response,
                        logs: &logs,
                        tests: &tests,
                        handler_error: handler_error.as_deref(),
                    },
                })?;
            }
            Err(e) => {
                summary.errors += 1;
                let message = e.to_string();
                reporter.request_finished(&RequestReport {
                    index,
                    request: &resolved,
                    result: RequestResult::Failed(&message),
                })?;
            }
        }
    }

    summary.duration_ms = started.elapsed().as_millis();
    reporter.finish(&summary)?;
    Ok(summary)
}

/// Substitute `{{var}}` references in a request's URL, headers, and body.
/// Errors if any referenced variable is undefined.
fn resolve_request(
    req: &parser::ParsedRequest,
    vars: &VariableStore,
) -> Result<parser::ParsedRequest, AppError> {
    let mut resolved = req.clone();
    resolved.url = ensure_http_scheme(&vars.substitute(&resolved.url)?);
    for header in &mut resolved.headers {
        header.value = vars.substitute(&header.value)?;
    }
    if let Some(body) = &resolved.body {
        resolved.body = Some(vars.substitute(body)?);
    }
    Ok(resolved)
}

/// Best-effort variant of `resolve_request` for `--dry-run`: leaves any
/// reference whose variable is undefined as the original `{{var}}` literal,
/// so users can still inspect requests that depend on globals set by earlier
/// response handlers. The URL's scheme is only normalized when substitution
/// fully succeeded — otherwise the original text is preserved verbatim.
fn resolve_request_best_effort(
    req: &parser::ParsedRequest,
    vars: &VariableStore,
) -> parser::ParsedRequest {
    let sub = |s: &str| vars.substitute(s).unwrap_or_else(|_| s.to_string());
    let mut resolved = req.clone();
    let url_substituted = sub(&resolved.url);
    // Only normalize the scheme when the URL is fully resolved — otherwise
    // we'd mangle literals like `{{host}}/users` into `https://{{host}}/users`.
    resolved.url = if url_substituted.contains("{{") {
        url_substituted
    } else {
        ensure_http_scheme(&url_substituted)
    };
    for header in &mut resolved.headers {
        header.value = sub(&header.value);
    }
    if let Some(body) = &resolved.body {
        resolved.body = Some(sub(body));
    }
    resolved
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
    use super::{ensure_http_scheme, has_url_scheme, resolve_request_best_effort};
    use crate::parser::{Header, HttpMethod, ParsedRequest};
    use crate::variable::VariableStore;
    use std::collections::HashMap;

    #[test]
    fn dry_run_resolution_keeps_undefined_vars_as_literals() {
        let store = VariableStore::new(HashMap::new());
        let req = ParsedRequest {
            name: None,
            method: HttpMethod::Get,
            url: "{{host}}/users".to_string(),
            headers: vec![Header {
                name: "Authorization".to_string(),
                value: "Bearer {{authToken}}".to_string(),
            }],
            body: None,
            response_handler: None,
            line_number: 1,
        };
        let resolved = resolve_request_best_effort(&req, &store);
        // URL stays exactly as written — no scheme injection when substitution failed.
        assert_eq!(resolved.url, "{{host}}/users");
        assert_eq!(resolved.headers[0].value, "Bearer {{authToken}}");
    }

    #[test]
    fn dry_run_resolution_normalizes_scheme_when_fully_resolved() {
        let mut env = HashMap::new();
        env.insert("host".to_string(), "api.example.com".to_string());
        let store = VariableStore::new(env);
        let req = ParsedRequest {
            name: None,
            method: HttpMethod::Get,
            url: "{{host}}/users".to_string(),
            headers: Vec::new(),
            body: None,
            response_handler: None,
            line_number: 1,
        };
        let resolved = resolve_request_best_effort(&req, &store);
        assert_eq!(resolved.url, "https://api.example.com/users");
    }

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
