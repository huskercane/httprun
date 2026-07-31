//! A one-line spinner shown while a request is in flight.
//!
//! Exists to answer "is it stuck or just slow?" — most visible under
//! `--quiet`, which otherwise prints nothing until the run ends.
//!
//! Two properties keep it out of the way:
//!
//! - It draws on **stderr**, so `--format json`, a pipe, or `--output` still
//!   produce a clean stdout stream.
//! - It only animates when stderr is a terminal. Redirected stderr (CI logs)
//!   spawns no thread at all, so there is nothing to strip from the log.
//!
//! It runs only for the duration of the blocking HTTP call, a window in which
//! the reporter writes nothing — so there is no output interleaving to
//! coordinate, and no need for the reporter to know it exists.

use std::io::{IsTerminal, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const INTERVAL: Duration = Duration::from_millis(100);

/// Cap the label so a long request name cannot wrap onto a second line —
/// `\r\x1b[2K` only erases the line the cursor sits on, and a wrapped draw
/// would leave residue behind.
const MAX_LABEL: usize = 60;

/// Clear the current line and return the cursor to column 0.
const CLEAR_LINE: &str = "\r\x1b[2K";

pub struct Spinner {
    /// `None` when inert (disabled, or stderr is not a terminal).
    state: Option<SpinnerThread>,
}

struct SpinnerThread {
    stop: Arc<(Mutex<bool>, Condvar)>,
    handle: Option<JoinHandle<()>>,
}

impl Spinner {
    /// Start animating `label` on stderr. Inert — and free — unless `enabled`
    /// and stderr is a terminal.
    pub fn start(label: &str, enabled: bool) -> Self {
        if !enabled || !std::io::stderr().is_terminal() {
            return Self { state: None };
        }

        let label = truncate(label, MAX_LABEL);
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let thread_stop = Arc::clone(&stop);

        let handle = thread::spawn(move || {
            let started = Instant::now();
            let mut frame = 0usize;
            let (lock, cvar) = &*thread_stop;
            let mut stopped = lock.lock().unwrap_or_else(|e| e.into_inner());

            while !*stopped {
                draw(&frame_text(frame, &label, started.elapsed()));
                frame = frame.wrapping_add(1);

                // Waiting on a condvar rather than sleeping is what keeps
                // `stop` from costing up to a full frame interval per
                // request — a plain sleep would add ~100ms × N to a run.
                let (guard, _) = cvar
                    .wait_timeout(stopped, INTERVAL)
                    .unwrap_or_else(|e| e.into_inner());
                stopped = guard;
            }

            draw("");
        });

        Self {
            state: Some(SpinnerThread {
                stop,
                handle: Some(handle),
            }),
        }
    }

    /// Erase the spinner and join its thread. Called automatically on drop,
    /// including while unwinding, so a panic cannot leave a stray frame or a
    /// half-drawn line on the terminal.
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        let Some(state) = &mut self.state else {
            return;
        };

        let (lock, cvar) = &*state.stop;
        {
            let mut stopped = lock.lock().unwrap_or_else(|e| e.into_inner());
            *stopped = true;
        }
        cvar.notify_all();

        if let Some(handle) = state.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Whether a spinner would animate, given the flag and the current stderr.
/// Kept separate so callers can skip building labels for nothing.
pub fn is_available(enabled: bool) -> bool {
    enabled && std::io::stderr().is_terminal()
}

fn draw(text: &str) {
    let mut err = std::io::stderr().lock();
    let _ = write!(err, "{}{}", CLEAR_LINE, text);
    let _ = err.flush();
}

fn frame_text(frame: usize, label: &str, elapsed: Duration) -> String {
    format!(
        "  {} {} {:.1}s",
        FRAMES[frame % FRAMES.len()],
        label,
        elapsed.as_secs_f64()
    )
}

/// Char-aware truncation — byte slicing would panic on a multi-byte name.
fn truncate(label: &str, max: usize) -> String {
    if label.chars().count() <= max {
        return label.to_string();
    }
    let kept: String = label.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_text_cycles_through_frames() {
        let a = frame_text(0, "login", Duration::from_millis(1500));
        let b = frame_text(1, "login", Duration::from_millis(1500));
        assert!(a.contains(FRAMES[0]));
        assert!(b.contains(FRAMES[1]));
        // Index wraps rather than panicking on a long-running request.
        let wrapped = frame_text(FRAMES.len(), "login", Duration::ZERO);
        assert!(wrapped.contains(FRAMES[0]));
    }

    #[test]
    fn frame_text_shows_elapsed_seconds() {
        let text = frame_text(0, "slow call", Duration::from_millis(2340));
        assert!(text.contains("2.3s"), "got: {text}");
        assert!(text.contains("slow call"));
    }

    #[test]
    fn truncate_leaves_short_labels_alone() {
        assert_eq!(truncate("login", 60), "login");
    }

    #[test]
    fn truncate_caps_long_labels() {
        let long = "a".repeat(100);
        let out = truncate(&long, 10);
        assert_eq!(out.chars().count(), 10);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn truncate_is_char_aware() {
        // Byte slicing here would panic mid-codepoint.
        let label = "日本語のリクエスト名前テスト";
        let out = truncate(label, 5);
        assert_eq!(out.chars().count(), 5);
        assert!(out.starts_with("日本語の"));
    }

    #[test]
    fn disabled_spinner_stops_immediately() {
        // Under `cargo test` stderr is not a terminal, so this also covers the
        // redirected case: no thread, so stopping cannot block on a join.
        let started = Instant::now();
        let spinner = Spinner::start("login", false);
        spinner.stop();
        assert!(
            started.elapsed() < INTERVAL,
            "inert spinner should not wait a frame interval"
        );
    }

    #[test]
    fn spinner_is_inert_when_stderr_is_not_a_terminal() {
        assert!(!is_available(true));
        assert!(!is_available(false));
    }
}
