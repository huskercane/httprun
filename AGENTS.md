# Repository Guidelines

## Project Structure & Module Organization
- `src/` contains the Rust crate source.
- `src/main.rs` is the CLI entry point.
- Core modules live at `src/env.rs`, `src/http.rs`, `src/parser.rs`, `src/variable.rs`, `src/output.rs`, and `src/error.rs`.
- JavaScript response handler support lives under `src/js/`.
- Build artifacts land in `target/` (do not edit or commit).

## Build, Test, and Development Commands
- `cargo build` builds a debug binary.
- `cargo build --release` builds the optimized release binary at `target/release/httprun`.
- `cargo run -- <file.http> [OPTIONS]` runs the CLI locally.
- `cargo check` performs a fast compile check without producing binaries.

## Coding Style & Naming Conventions
- Use standard Rust formatting (`cargo fmt`) when making changes.
- Prefer Rust 2024 idioms (crate `edition = "2024"` in `Cargo.toml`).
- Module and file names follow snake_case (e.g., `variable.rs`).
- Keep CLI flags and options consistent with existing Clap usage in `src/main.rs`.

## Testing Guidelines
- There are no dedicated automated tests in this repository today.
- If adding tests, use Rust’s built-in test framework (`#[test]`) and place unit tests near the module they cover (e.g., in `src/parser.rs`).
- Name tests with clear, behavior-focused phrases (e.g., `parses_request_names`).

## Commit & Pull Request Guidelines
- Commit messages in history are short, imperative, and lower-case (e.g., `add readme`, `upgrade rust version to latest`).
- Keep commits focused on a single change when possible.
- PRs should include a brief summary, a list of major changes, and example commands used to verify behavior (e.g., `cargo build --release`, `cargo run -- sample.http`).

## Configuration & Security Notes
- Runtime environment files like `http-client.env.json` and `http-client.private.env.json` are expected to live alongside `.http` files. Do not commit private env files.
- Rust toolchain requirement: Rust 1.93+ (see `Cargo.toml`).
