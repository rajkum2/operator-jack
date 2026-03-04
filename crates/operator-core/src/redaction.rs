use std::sync::LazyLock;

use regex::Regex;

/// Keys whose values should always be redacted (case-insensitive match).
const SENSITIVE_KEYS: &[&str] = &[
    "password",
    "token",
    "secret",
    "api_key",
    "authorization",
    "credential",
];

/// Compiled regex for ULID strings (exactly 26 chars of Crockford base32).
/// ULIDs must NOT be redacted — they are structural IDs.
static ULID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9A-HJKMNP-TV-Z]{26}$").unwrap());

/// Compiled regex for base64-like strings (> 40 chars, only base64 alphabet).
/// Threshold raised to 40 to avoid false positives on short identifiers like ULIDs.
static BASE64_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9+/=]{41,}$").unwrap());

/// Compiled regex for hex strings (>= 32 hex chars).
static HEX_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Fa-f0-9]{32,}$").unwrap());

/// Compiled regex for JWT-like tokens (three dot-separated base64url segments).
static JWT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$").unwrap());

const REDACTED: &str = "[REDACTED]";

/// Recursively walks a JSON value and redacts sensitive data.
///
/// - Object keys matching sensitive names have their values replaced.
/// - String values matching secret-like patterns (base64, hex, JWT) are
///   replaced, unless they look like file paths.
pub fn redact_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut result = serde_json::Map::new();
            for (key, val) in map {
                if is_sensitive_key(key) {
                    result.insert(key.clone(), serde_json::Value::String(REDACTED.to_string()));
                } else {
                    result.insert(key.clone(), redact_value(val));
                }
            }
            serde_json::Value::Object(result)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(redact_value).collect())
        }
        serde_json::Value::String(s) => {
            if should_redact_string(s) {
                serde_json::Value::String(REDACTED.to_string())
            } else {
                value.clone()
            }
        }
        _ => value.clone(),
    }
}

/// Checks whether a key is a known sensitive key (case-insensitive).
fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    SENSITIVE_KEYS.iter().any(|&k| lower == k)
}

/// Determines whether a string value looks like a secret and should be
/// redacted.
fn should_redact_string(s: &str) -> bool {
    // Do not redact values that look like file paths.
    if s.contains('/') || s.contains('\\') {
        return false;
    }

    // Do not redact ULID identifiers.
    if ULID_RE.is_match(s) {
        return false;
    }

    // Base64-like: > 20 chars of base64 alphabet
    if BASE64_RE.is_match(s) {
        return true;
    }

    // Hex string: >= 32 hex chars
    if HEX_RE.is_match(s) {
        return true;
    }

    // JWT-like: three dot-separated base64url segments
    if JWT_RE.is_match(s) {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_sensitive_key_redacted() {
        let input = json!({"password": "secret123", "username": "alice"});
        let result = redact_value(&input);
        assert_eq!(result["password"], json!("[REDACTED]"));
        assert_eq!(result["username"], json!("alice"));
    }

    #[test]
    fn test_sensitive_key_case_insensitive() {
        let input = json!({"Token": "abc", "API_KEY": "xyz"});
        let result = redact_value(&input);
        assert_eq!(result["Token"], json!("[REDACTED]"));
        assert_eq!(result["API_KEY"], json!("[REDACTED]"));
    }

    #[test]
    fn test_ulid_not_redacted() {
        // A valid ULID: 26 chars of Crockford base32
        let input = json!({"id": "01ARYZ6S41TSV4RRFFQ69G5FAV"});
        let result = redact_value(&input);
        assert_eq!(result["id"], json!("01ARYZ6S41TSV4RRFFQ69G5FAV"));
    }

    #[test]
    fn test_long_base64_redacted() {
        // 50 chars of base64 alphabet
        let long_b64 = "A".repeat(50);
        let input = json!({"data": long_b64});
        let result = redact_value(&input);
        assert_eq!(result["data"], json!("[REDACTED]"));
    }

    #[test]
    fn test_short_string_not_redacted() {
        let input = json!({"name": "TextEdit"});
        let result = redact_value(&input);
        assert_eq!(result["name"], json!("TextEdit"));
    }

    #[test]
    fn test_hex_string_redacted() {
        let hex = "a".repeat(32); // 32 hex chars
        let input = json!({"hash": hex});
        let result = redact_value(&input);
        assert_eq!(result["hash"], json!("[REDACTED]"));
    }

    #[test]
    fn test_jwt_redacted() {
        let input = json!({"token": "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abc123"});
        let result = redact_value(&input);
        assert_eq!(result["token"], json!("[REDACTED]"));
    }

    #[test]
    fn test_file_path_not_redacted() {
        let input = json!({"path": "/Users/alice/Documents/notes.txt"});
        let result = redact_value(&input);
        assert_eq!(result["path"], json!("/Users/alice/Documents/notes.txt"));
    }

    #[test]
    fn test_nested_redaction() {
        let input = json!({
            "step": {
                "params": {
                    "password": "hunter2",
                    "app": "Safari"
                }
            }
        });
        let result = redact_value(&input);
        assert_eq!(result["step"]["params"]["password"], json!("[REDACTED]"));
        assert_eq!(result["step"]["params"]["app"], json!("Safari"));
    }

    #[test]
    fn test_array_redaction() {
        let input = json!([{"password": "x"}, {"name": "y"}]);
        let result = redact_value(&input);
        assert_eq!(result[0]["password"], json!("[REDACTED]"));
        assert_eq!(result[1]["name"], json!("y"));
    }
}
