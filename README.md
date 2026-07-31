# httprun

A fast, standalone CLI tool for executing [IntelliJ HTTP Client](https://www.jetbrains.com/help/idea/http-client-in-product-code-editor.html) `.http` request files from the terminal. No IDE required.

Built with Rust for speed and portability. Supports environment variables, JavaScript response handlers, tests, and all standard HTTP methods.

## Installation

### Download pre-built binaries

Pre-built binaries are available for each [GitHub release](https://github.com/huskercane/httprun/releases/latest).

| Platform              | Download |
|-----------------------|----------|
| Linux (x86_64)       | [httprun-\<version\>-x86_64-unknown-linux-gnu.tar.gz](https://github.com/huskercane/httprun/releases/latest) |
| Linux (aarch64)      | [httprun-\<version\>-aarch64-unknown-linux-gnu.tar.gz](https://github.com/huskercane/httprun/releases/latest) |
| macOS (Intel)        | [httprun-\<version\>-x86_64-apple-darwin.tar.gz](https://github.com/huskercane/httprun/releases/latest) |
| macOS (Apple Silicon) | [httprun-\<version\>-aarch64-apple-darwin.tar.gz](https://github.com/huskercane/httprun/releases/latest) |
| Windows (x86_64)     | [httprun-\<version\>-x86_64-pc-windows-msvc.zip](https://github.com/huskercane/httprun/releases/latest) |

**Linux / macOS:**

```sh
# Download and extract (replace <version> and <target> accordingly)
curl -LO https://github.com/huskercane/httprun/releases/latest/download/httprun-<version>-<target>.tar.gz
tar xzf httprun-<version>-<target>.tar.gz
sudo mv httprun /usr/local/bin/
```

**Windows:**

Download the `.zip` from the releases page, extract it, and add `httprun.exe` to your PATH.

### Build from source

Requires [Rust](https://rustup.rs/) 1.93 or later.

```sh
git clone https://github.com/huskercane/httprun.git
cd httprun
cargo build --release
# Binary will be at target/release/httprun
```

## Usage

```
httprun <file.http> [OPTIONS]
```

### Options

| Flag | Description |
|------|-------------|
| `--env <name>` | Environment name to use (from `http-client.env.json`) |
| `--env-file <path>` | Path to the environment file (default: `http-client.env.json`) |
| `--name <name>` | Run a single request by name (case-insensitive substring match) |
| `--index <n>` | Run a single request by 1-based index |
| `-v`, `--verbose` | Show a curl-style trace of each request and response |
| `-q`, `--quiet` | Suppress per-request output; print only failures and the summary |
| `--format <fmt>` | Output format: `human` (default), `json`, `ndjson` |
| `-o`, `--output <path>` | Write the report to a file instead of stdout |
| `--include-secrets` | Do not redact credential headers in `json` / `ndjson` output |
| `--no-progress` | Disable the in-flight spinner (already off when stderr is not a terminal) |
| `--dry-run` | Parse and display requests without executing them |
| `--curl` | Print a copy-pasteable `curl` command for each request |

### Examples

```sh
# Run all requests in a file
httprun api.http

# Run with an environment
httprun api.http --env dev

# Run a specific request by name
httprun api.http --name "create user"

# Run the 2nd request only
httprun api.http --index 2

# Preview without executing
httprun api.http --env staging --dry-run

# curl-style trace (sent / received headers, body, timing)
httprun api.http --env dev -v

# Print copy-pasteable curl commands instead of running
httprun api.http --env dev --dry-run --curl

# Run and also show the equivalent curl command for each request
httprun api.http --env dev --curl

# Quiet: only failures and the summary (good for CI logs)
httprun api.http --env dev --quiet

# Machine-readable report for tooling
httprun api.http --env dev --format json --output report.json
```

## Machine-readable output

`--format json` emits a single JSON document describing the whole run;
`--format ndjson` emits one JSON object per line, flushed as each request
completes, so a consumer can tail the stream live. Both work with `--dry-run`.

```sh
# What failed?
httprun api.http --format json | jq '.requests[] | select(.tests[]? | .passed == false)'

# Status code of every request
httprun api.http --format json | jq -r '.requests[] | "\(.name): \(.response.status)"'

# Follow a long run as it happens
httprun api.http --format ndjson | jq -r 'select(.type=="request") | "\(.index) \(.response.status)"'
```

Document shape:

```json
{
  "schemaVersion": 1,
  "file": "api.http",
  "environment": "dev",
  "startedAtMs": 1730000000000,
  "summary": {
    "total": 2, "testsPassed": 3, "testsFailed": 0,
    "errors": 0, "durationMs": 412, "success": true
  },
  "requests": [
    {
      "index": 1,
      "name": "login",
      "method": "POST",
      "url": "https://api.example.com/auth/login",
      "headers": { "Authorization": ["<redacted>"] },
      "body": "{\"username\":\"alice\"}",
      "response": {
        "status": 200,
        "httpVersion": "HTTP/1.1",
        "headers": { "content-type": ["application/json"] },
        "body": { "token": "..." },
        "elapsedMs": 170
      },
      "tests": [{ "name": "login returns 200", "passed": true }],
      "logs": ["token acquired"]
    }
  ]
}
```

Notes:

- `response.body` is the **parsed JSON value** when the response is JSON, and the raw body as a string otherwise — so `jq .response.body` works either way.
- `schemaVersion` is bumped when a field is removed or changes meaning. Additive changes keep the same version.
- In `ndjson`, each request line carries `"type": "request"` and the final line carries `"type": "summary"`.
- `curl` is only present when `--curl` is passed.
- Optional fields (`name`, `body`, `curl`, `response`, `error`, `failureMessage`) are omitted when absent rather than emitted as `null`.

### Secret redaction

JSON reports land in files, CI logs and artifact stores, so **credential
header values are masked by default** — `Authorization`, `Cookie`,
`Set-Cookie`, `X-API-Key` and friends become `<redacted>` in both the
`headers` fields and any emitted `curl` command. Pass `--include-secrets` to
disable this.

Redaction covers headers, **not request bodies** — a login body containing a
password is still written verbatim, because redacting payloads would gut the
usefulness of the report. Terminal output is never redacted; it is already
scoped to whoever ran the command, and `-v` exists precisely to show what went
on the wire.

## Progress

While a request is in flight, httprun shows a spinner with a live elapsed
counter, so a slow endpoint is distinguishable from a hung one:

```
[1] fetch report
  GET https://api.example.com/reports/large
  ⠹ [1] fetch report 4.2s
```

The line erases itself when the response arrives. This matters most under
`--quiet`, which otherwise prints nothing until the run ends.

It draws on **stderr**, so `--format json`, `--output` and pipelines keep a
clean stdout — `httprun api.http --format json | jq` still shows the spinner
in your terminal while emitting parseable JSON. When stderr is not a terminal
(CI, redirected logs) nothing is drawn at all, so there is no escape-code
noise to strip. Use `--no-progress` to turn it off explicitly.

## Performance

Requests execute sequentially, so wall-clock time is dominated by network
round trips, not by printing. Two things still help on large runs:

- Output is written through a large buffer whenever the destination is not a
  terminal (a pipe, or `--output`), which collapses the many small writes a
  `-v` run would otherwise make into a handful of syscalls.
- If a verbose run feels slow interactively, the cost is usually your terminal
  emulator rendering and scrolling, not httprun. Compare
  `time httprun f.http -v` against `time httprun f.http -v > /dev/null`; if the
  gap is large, use `--quiet` or redirect to a file.

### Debug output

Two complementary flags for inspecting what gets sent on the wire:

- `-v` / `--verbose` — live trace using curl's conventions: `*` for connection / timing meta, `>` for sent bytes, `<` for received bytes. Useful when you want to see exactly what httprun sent and what came back.
- `--curl` — emits a `curl` command line that reproduces each request, with proper shell quoting. Useful when you want to copy-paste a request into a terminal, share it in a bug report, or hand it off to another tool. Combine with `--dry-run` to print without sending.

Example `-v` output:

```
[1] get example
  GET https://httpbin.org/get
  * Connected to httpbin.org
  > GET /get HTTP/1.1
  > Accept: application/json
  → 200 (170ms)
  < HTTP/1.1 200
  < content-type: application/json
  < 
  < {"args": {}, ...}
  * Total: 170ms
```

Example `--curl --dry-run` output:

```
[2] post example
  POST https://api.example.com/users
  $ curl 'https://api.example.com/users' \
  $   -X POST \
  $   -H 'Content-Type: application/json' \
  $   --data-raw '{"name":"alice"}'
```

## HTTP File Format

httprun supports the standard `.http` file format used by IntelliJ/JetBrains IDEs.

### Basic requests

```http
### Get all users
GET https://api.example.com/users

### Create a user
POST https://api.example.com/users
Content-Type: application/json

{
  "name": "Alice",
  "email": "alice@example.com"
}
```

Requests are separated by `###`. The text after `###` is the request name.

### Supported HTTP methods

`GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`, `OPTIONS`

## Variables

### Environment variables

Create an `http-client.env.json` file alongside your `.http` files:

```json
{
  "dev": {
    "host": "http://localhost:8080",
    "token": "dev-token-123"
  },
  "staging": {
    "host": "https://staging.example.com",
    "token": "staging-token-456"
  }
}
```

Reference variables using `{{variable}}` syntax:

```http
GET {{host}}/api/users
Authorization: Bearer {{token}}
```

### Private environment variables

Sensitive values (API keys, passwords) can be stored in `http-client.private.env.json`, which follows the same format and overrides the public file. Add this file to `.gitignore`.

### In-place variables

Define variables directly in your `.http` file:

```http
@baseUrl = https://api.example.com
@contentType = application/json

GET {{baseUrl}}/users
Content-Type: {{contentType}}
```

### Dynamic variables

| Variable | Description |
|----------|-------------|
| `{{$uuid}}` | Random UUID v4 |
| `{{$timestamp}}` | Unix timestamp (seconds) |
| `{{$randomInt}}` | Random integer (0-999) |

### Variable precedence

In-place variables > Global variables (set by response handlers) > Environment variables

## Response Handlers & Tests

Write JavaScript response handlers to validate responses and extract values:

```http
### Login and save token
POST {{host}}/auth/login
Content-Type: application/json

{
  "username": "admin",
  "password": "secret"
}

> {%
    client.test("Status is 200", function() {
        client.assert(response.status === 200, "Expected 200");
    });

    // Save token for subsequent requests
    client.global.set("authToken", response.body.token);

    client.log("Logged in, token:", response.body.token);
%}

### Use saved token
GET {{host}}/api/protected
Authorization: Bearer {{authToken}}

> {%
    client.test("Access granted", function() {
        client.assert(response.status === 200, "Expected 200");
    });
%}
```

### Response handler API

**`response` object:**

| Property | Description |
|----------|-------------|
| `response.status` | HTTP status code (number) |
| `response.body` | Parsed JSON object, or raw string if not JSON |
| `response.headers.valueOf(name)` | First value of a header |
| `response.headers.valuesOf(name)` | All values of a header (array) |
| `response.contentType.mimeType` | MIME type (e.g. `application/json`) |
| `response.contentType.charset` | Charset if present |

**`client` object:**

| Method | Description |
|--------|-------------|
| `client.test(name, fn)` | Define a named test |
| `client.assert(condition, message)` | Assert a condition (fails the enclosing test) |
| `client.log(...)` | Print log output |
| `client.global.set(name, value)` | Set a global variable for subsequent requests |
| `client.global.get(name)` | Get a global variable |

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | All requests succeeded, all tests passed |
| `1` | One or more tests failed or requests errored |

This makes httprun suitable for use in CI/CD pipelines.

## License

This project is open source. See the repository for license details.
