# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

httprun is a Rust CLI tool that executes IntelliJ HTTP Client `.http` request files from the terminal. It supports environment variables, JavaScript response handlers (via Boa engine), variable substitution, and all standard HTTP methods.

## Build & Development Commands

```bash
cargo build                          # Debug build
cargo build --release                # Release binary → target/release/httprun
cargo check                          # Fast compile check (no binary)
cargo test                           # Run all tests
cargo fmt                            # Format code
cargo run -- <file.http> [OPTIONS]   # Run locally (e.g. --env dev --verbose --dry-run)
```

Rust 1.93+ required (edition 2024).

## Architecture

**Execution flow:** Parse CLI args → read .http file → parse requests → load environment → build a reporter → for each request: substitute variables → execute HTTP → run JS handler (if present) → emit reporter events → exit 0/1.

**Key modules:**
- `src/main.rs` — CLI entry point (Clap), orchestrates the pipeline
- `src/report/` — reporting port (`Reporter` trait) and adapters: `human.rs` (colored streaming terminal output), `json.rs` (json / ndjson for tooling), `redact.rs` (credential masking for machine formats). The execution loop emits events and does no formatting itself; add a new output format here rather than branching in `main.rs`
- `src/parser.rs` — State-machine parser for `.http` files (states: AwaitingRequest, ReadingHeaders, ReadingBody, ReadingHandler)
- `src/http.rs` — Blocking reqwest HTTP executor
- `src/env.rs` — Loads `http-client.env.json` / `http-client.private.env.json`
- `src/variable.rs` — `{{var}}` substitution with dynamic vars (`$uuid`, `$timestamp`, `$randomInt`); precedence: in-place > global > environment
- `src/js/` — Boa JS engine integration: `runtime.rs` (executor), `client.rs` (`client.test/assert/log/global`), `response.rs` (response object exposed to handlers)
- `src/progress.rs` — In-flight spinner. Draws on stderr only, and only when stderr is a terminal, so stdout stays a clean machine-readable stream. Runs only around the blocking HTTP call, a window in which the reporter writes nothing — keep it that way, or the spinner and reporter will interleave
- `src/output.rs` — Terminal formatting primitives; every function writes into a caller-supplied `&mut dyn Write` (never `println!`) so output can be buffered or redirected to a file
- `src/error.rs` — Centralized `AppError` enum via thiserror

## Code Style

- Rust 2024 idioms, `cargo fmt` for formatting
- snake_case modules and files
- CLI flags consistent with existing Clap derive usage in `main.rs`
- Commit messages: short, imperative, lowercase (e.g. `add readme`, `fix parser bug`)
- Tests use `#[test]` near the module they cover, named with behavior-focused phrases
