//! Credential redaction for machine-readable output.
//!
//! JSON reports land in files, CI logs and artifact stores, so header values
//! that carry credentials are masked by default. Terminal output is not
//! redacted — it is already scoped to the person who ran the command, and
//! `-v` exists precisely to show what went on the wire.

pub const REDACTED: &str = "<redacted>";

/// Header names whose values are credentials often enough to mask by default.
const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "authentication",
    "cookie",
    "set-cookie",
    "x-api-key",
    "api-key",
    "apikey",
    "x-auth-token",
    "x-access-token",
    "x-csrf-token",
    "x-session-token",
];

pub fn is_sensitive(header_name: &str) -> bool {
    let lower = header_name.to_ascii_lowercase();
    SENSITIVE_HEADERS.contains(&lower.as_str())
}

/// Mask `value` when `name` is a known credential header and redaction is on.
pub fn header_value(name: &str, value: &str, redact: bool) -> String {
    if redact && is_sensitive(name) {
        REDACTED.to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_match_is_case_insensitive() {
        assert!(is_sensitive("Authorization"));
        assert!(is_sensitive("AUTHORIZATION"));
        assert!(is_sensitive("x-api-key"));
        assert!(is_sensitive("Set-Cookie"));
    }

    #[test]
    fn ordinary_headers_are_not_sensitive() {
        assert!(!is_sensitive("Content-Type"));
        assert!(!is_sensitive("Accept"));
        // Guard against a prefix-match regression — this is not `authorization`.
        assert!(!is_sensitive("x-authorization-scheme"));
    }

    #[test]
    fn header_value_masks_only_when_redacting() {
        assert_eq!(header_value("Authorization", "Bearer abc", true), REDACTED);
        assert_eq!(
            header_value("Authorization", "Bearer abc", false),
            "Bearer abc"
        );
        assert_eq!(
            header_value("Content-Type", "application/json", true),
            "application/json"
        );
    }
}
