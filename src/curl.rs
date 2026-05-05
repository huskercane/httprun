use crate::parser::{HttpMethod, ParsedRequest};

/// Render a request as a copy-pasteable `curl` command.
///
/// Expects the request's URL, header values, and body to already have
/// `{{var}}` references substituted (see `resolve_request` in `main.rs`).
/// Output is multi-line with backslash continuations for readability.
pub fn to_curl_command(req: &ParsedRequest) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!("curl {}", shell_quote(&req.url)));

    match req.method {
        HttpMethod::Get => {}
        HttpMethod::Head => parts.push("-I".to_string()),
        _ => parts.push(format!("-X {}", req.method.as_str())),
    }

    for h in &req.headers {
        parts.push(format!(
            "-H {}",
            shell_quote(&format!("{}: {}", h.name, h.value))
        ));
    }

    if let Some(body) = &req.body
        && !body.is_empty()
    {
        parts.push(format!("--data-raw {}", shell_quote(body)));
    }

    parts.join(" \\\n  ")
}

/// Single-quote a string for POSIX shells. Embedded `'` becomes `'\''`.
fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Header;

    fn req(method: HttpMethod, url: &str) -> ParsedRequest {
        ParsedRequest {
            name: None,
            method,
            url: url.to_string(),
            headers: Vec::new(),
            body: None,
            response_handler: None,
            line_number: 0,
        }
    }

    #[test]
    fn get_no_body_no_headers() {
        let r = req(HttpMethod::Get, "https://example.com/api");
        assert_eq!(to_curl_command(&r), "curl 'https://example.com/api'");
    }

    #[test]
    fn post_with_json_body_and_headers() {
        let mut r = req(HttpMethod::Post, "https://api.example.com/users");
        r.headers.push(Header {
            name: "Content-Type".to_string(),
            value: "application/json".to_string(),
        });
        r.headers.push(Header {
            name: "Authorization".to_string(),
            value: "Bearer abc123".to_string(),
        });
        r.body = Some(r#"{"name":"alice"}"#.to_string());

        let cmd = to_curl_command(&r);
        assert!(cmd.contains("curl 'https://api.example.com/users'"));
        assert!(cmd.contains("-X POST"));
        assert!(cmd.contains("-H 'Content-Type: application/json'"));
        assert!(cmd.contains("-H 'Authorization: Bearer abc123'"));
        assert!(cmd.contains(r#"--data-raw '{"name":"alice"}'"#));
    }

    #[test]
    fn head_uses_capital_i_flag() {
        let r = req(HttpMethod::Head, "https://example.com");
        let cmd = to_curl_command(&r);
        assert!(cmd.contains("-I"));
        assert!(!cmd.contains("-X HEAD"));
    }

    #[test]
    fn put_patch_delete_options_use_x_flag() {
        for m in [
            HttpMethod::Put,
            HttpMethod::Patch,
            HttpMethod::Delete,
            HttpMethod::Options,
        ] {
            let cmd = to_curl_command(&req(m.clone(), "https://example.com"));
            assert!(cmd.contains(&format!("-X {}", m.as_str())));
        }
    }

    #[test]
    fn single_quote_in_value_is_escaped() {
        let mut r = req(HttpMethod::Get, "https://example.com");
        r.headers.push(Header {
            name: "X-Note".to_string(),
            value: "it's fine".to_string(),
        });
        let cmd = to_curl_command(&r);
        assert!(cmd.contains(r#"-H 'X-Note: it'\''s fine'"#));
    }

    #[test]
    fn empty_body_is_omitted() {
        let mut r = req(HttpMethod::Post, "https://example.com");
        r.body = Some(String::new());
        let cmd = to_curl_command(&r);
        assert!(!cmd.contains("--data-raw"));
    }

    #[test]
    fn url_with_special_chars_is_quoted() {
        let r = req(
            HttpMethod::Get,
            "https://example.com/search?q=hello world&x=$y",
        );
        let cmd = to_curl_command(&r);
        assert!(cmd.contains("'https://example.com/search?q=hello world&x=$y'"));
    }

    #[test]
    fn multiline_format_uses_backslash_continuations() {
        let mut r = req(HttpMethod::Post, "https://example.com");
        r.headers.push(Header {
            name: "X-A".to_string(),
            value: "1".to_string(),
        });
        r.body = Some("body".to_string());
        let cmd = to_curl_command(&r);
        assert!(cmd.contains(" \\\n  "));
    }
}
