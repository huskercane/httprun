use std::io::{self, Write};
use std::path::Path;

use super::{Reporter, RequestReport, RequestResult, Summary};
use crate::curl;
use crate::output;
use crate::parser::ParsedRequest;

/// Streaming, colored terminal output — the default.
pub struct HumanReporter<W: Write> {
    w: W,
    verbose: bool,
    curl: bool,
    quiet: bool,
}

impl<W: Write> HumanReporter<W> {
    pub fn new(w: W, verbose: bool, curl: bool, quiet: bool) -> Self {
        Self {
            w,
            verbose,
            curl,
            quiet,
        }
    }
}

impl<W: Write> Reporter for HumanReporter<W> {
    fn request_started(&mut self, index: usize, request: &ParsedRequest) -> io::Result<()> {
        if self.quiet {
            // Nothing until we know whether this request failed — quiet mode
            // prints the header lazily in `request_finished`.
            return Ok(());
        }

        output::print_request_header(&mut self.w, index, request)?;

        if self.curl {
            output::print_curl_command(&mut self.w, &curl::to_curl_command(request))?;
        }

        if self.verbose {
            output::print_trace_request(&mut self.w, request)?;
        }

        Ok(())
    }

    fn request_finished(&mut self, report: &RequestReport<'_>) -> io::Result<()> {
        let failed = report.result.is_failure();

        // Quiet mode stays silent on success but must still surface failures,
        // otherwise a CI run that fails tells you nothing about which request.
        if self.quiet {
            if !failed {
                return Ok(());
            }
            output::print_request_header(&mut self.w, report.index, report.request)?;
        }

        match &report.result {
            RequestResult::Completed {
                response,
                logs,
                tests,
                handler_error,
            } => {
                output::print_response_status(&mut self.w, response)?;

                if self.verbose {
                    output::print_trace_response(&mut self.w, response)?;
                }

                if !logs.is_empty() && !self.quiet {
                    output::print_log_output(&mut self.w, logs)?;
                }

                if !tests.is_empty() {
                    output::print_test_results(&mut self.w, tests, self.quiet)?;
                }

                if let Some(err) = handler_error {
                    output::print_error(&mut self.w, err)?;
                }
            }
            RequestResult::Failed(msg) => {
                output::print_error(&mut self.w, msg)?;
            }
        }

        Ok(())
    }

    fn dry_run_started(&mut self, count: usize, file: &Path) -> io::Result<()> {
        writeln!(
            self.w,
            "Dry run: {} request(s) from {}",
            count,
            file.display()
        )
    }

    fn dry_run_request(&mut self, index: usize, request: &ParsedRequest) -> io::Result<()> {
        output::print_dry_run_request(&mut self.w, index, request)?;
        if self.curl {
            output::print_curl_command(&mut self.w, &curl::to_curl_command(request))?;
        }
        Ok(())
    }

    fn finish(&mut self, summary: &Summary) -> io::Result<()> {
        output::print_summary(&mut self.w, summary)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.w.flush()
    }
}
