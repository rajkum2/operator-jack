//! Keychain-backed secret store and `{{secret:name}}` injection (M8, see
//! SPEC_FREEZE_V0.8.md §5).
//!
//! Secrets live in the macOS login Keychain under the `operator-jack` service.
//! A plan/skill param may reference one as `{{secret:name}}`; the engine resolves
//! it to the literal value **only at execution time**, just before dispatch, and
//! never stores the resolved value (the audited params keep the reference form).

use std::sync::LazyLock;

use regex::Regex;
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

/// Keychain service name under which all operator-jack secrets are stored.
const SERVICE: &str = "operator-jack";
/// Reserved account holding the newline-separated index of secret names, so the
/// CLI can list them (the Keychain has no simple "accounts for a service" query
/// through the passwords API).
const INDEX_ACCOUNT: &str = "__operator_jack_secret_index__";

/// Matches a `{{secret:NAME}}` reference. NAME is a conservative identifier.
static SECRET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{secret:([A-Za-z0-9_.\-]+)\}\}").unwrap());

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("secret '{0}' not found in the keychain (set it with `operator-jack secret set {0}`)")]
    NotFound(String),
    #[error("keychain error: {0}")]
    Keychain(String),
}

/// Stores (or updates) a secret in the keychain and records its name in the index.
pub fn set_secret(name: &str, value: &str) -> Result<(), SecretError> {
    set_generic_password(SERVICE, name, value.as_bytes())
        .map_err(|e| SecretError::Keychain(e.to_string()))?;
    add_to_index(name)
}

/// Retrieves a secret's value from the keychain.
pub fn get_secret(name: &str) -> Result<String, SecretError> {
    match get_generic_password(SERVICE, name) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|e| SecretError::Keychain(e.to_string())),
        Err(_) => Err(SecretError::NotFound(name.to_string())),
    }
}

/// Deletes a secret from the keychain and the index.
pub fn delete_secret(name: &str) -> Result<(), SecretError> {
    delete_generic_password(SERVICE, name).map_err(|_| SecretError::NotFound(name.to_string()))?;
    remove_from_index(name);
    Ok(())
}

/// Lists the names of stored secrets (never their values).
pub fn list_secrets() -> Vec<String> {
    read_index()
}

/// True if the JSON value contains any `{{secret:*}}` reference.
pub fn has_secret_refs(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(s) => SECRET_RE.is_match(s),
        serde_json::Value::Array(a) => a.iter().any(has_secret_refs),
        serde_json::Value::Object(m) => m.values().any(has_secret_refs),
        _ => false,
    }
}

/// Recursively resolves every `{{secret:name}}` reference in a JSON value to the
/// keychain value. Errors if any referenced secret is missing. Callers must use
/// the result ONLY for dispatch and must persist the pre-resolution value.
pub fn resolve_secrets(value: &serde_json::Value) -> Result<serde_json::Value, SecretError> {
    match value {
        serde_json::Value::String(s) => Ok(serde_json::Value::String(resolve_in_string(s)?)),
        serde_json::Value::Array(arr) => {
            let out: Result<Vec<_>, _> = arr.iter().map(resolve_secrets).collect();
            Ok(serde_json::Value::Array(out?))
        }
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                out.insert(k.clone(), resolve_secrets(v)?);
            }
            Ok(serde_json::Value::Object(out))
        }
        other => Ok(other.clone()),
    }
}

fn resolve_in_string(s: &str) -> Result<String, SecretError> {
    // Collect referenced names first so a missing one errors before substitution.
    let mut result = String::with_capacity(s.len());
    let mut last = 0;
    for caps in SECRET_RE.captures_iter(s) {
        let m = caps.get(0).unwrap();
        let name = &caps[1];
        let value = get_secret(name)?;
        result.push_str(&s[last..m.start()]);
        result.push_str(&value);
        last = m.end();
    }
    result.push_str(&s[last..]);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Name index (for `secret list`)
// ---------------------------------------------------------------------------

fn read_index() -> Vec<String> {
    match get_generic_password(SERVICE, INDEX_ACCOUNT) {
        Ok(bytes) => String::from_utf8_lossy(&bytes)
            .lines()
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn write_index(names: &[String]) {
    let joined = names.join("\n");
    let _ = set_generic_password(SERVICE, INDEX_ACCOUNT, joined.as_bytes());
}

fn add_to_index(name: &str) -> Result<(), SecretError> {
    let mut names = read_index();
    if !names.iter().any(|n| n == name) {
        names.push(name.to_string());
        names.sort();
        write_index(&names);
    }
    Ok(())
}

fn remove_from_index(name: &str) {
    let mut names = read_index();
    names.retain(|n| n != name);
    write_index(&names);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn has_secret_refs_detects_nested() {
        assert!(has_secret_refs(&json!({"a": {"b": "x {{secret:pw}} y"}})));
        assert!(has_secret_refs(&json!(["{{secret:token}}"])));
        assert!(!has_secret_refs(&json!({"a": "plain", "n": 3})));
    }

    #[test]
    fn resolve_errors_on_missing_secret() {
        // A name that (almost certainly) is not in the test keychain.
        let v = json!({"password": "{{secret:__nonexistent_operator_jack_test__}}"});
        let err = resolve_secrets(&v).unwrap_err();
        assert!(matches!(err, SecretError::NotFound(_)));
    }

    #[test]
    fn resolve_is_noop_without_refs() {
        let v = json!({"app": "Calculator", "n": 2, "b": true});
        assert_eq!(resolve_secrets(&v).unwrap(), v);
    }
}
