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
| `-v`, `--verbose` | Show full request/response headers and body |
| `--dry-run` | Parse and display requests without executing them |

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

# Verbose output with full headers
httprun api.http --env dev -v
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
