use std::fmt;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// OperatorError  (domain-level, JSON-serializable)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub details: serde_json::Value,
}

impl fmt::Display for OperatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl OperatorError {
    // -- helpers ----------------------------------------------------------

    fn new(code: &str, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            retryable,
            details: serde_json::Value::Null,
        }
    }

    fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = details;
        self
    }

    // -- constructors per error code --------------------------------------

    pub fn validation_error(message: impl Into<String>) -> Self {
        Self::new("VALIDATION_ERROR", message, false)
    }

    pub fn unsupported_step_type(step_type: &str) -> Self {
        Self::new(
            "UNSUPPORTED_STEP_TYPE",
            format!("Unsupported step type: {step_type}"),
            false,
        )
        .with_details(serde_json::json!({ "step_type": step_type }))
    }

    pub fn interpolation_missing(variable: &str) -> Self {
        Self::new(
            "INTERPOLATION_MISSING",
            format!("Variable not found: {variable}"),
            false,
        )
        .with_details(serde_json::json!({ "variable": variable }))
    }

    pub fn interpolation_type_error(variable: &str, expected: &str) -> Self {
        Self::new(
            "INTERPOLATION_TYPE_ERROR",
            format!("Variable '{variable}' must be {expected} in this context"),
            false,
        )
        .with_details(serde_json::json!({ "variable": variable, "expected": expected }))
    }

    pub fn policy_denied(reason: impl Into<String>) -> Self {
        Self::new("POLICY_DENIED", reason, false)
    }

    pub fn policy_confirmation_required(message: impl Into<String>) -> Self {
        Self::new("POLICY_CONFIRMATION_REQUIRED", message, false)
    }

    pub fn ask_requires_interactive() -> Self {
        Self::new(
            "ASK_REQUIRES_INTERACTIVE",
            "on_fail=ask requires an interactive session",
            false,
        )
    }

    pub fn helper_not_found(helper: &str) -> Self {
        Self::new(
            "HELPER_NOT_FOUND",
            format!("Helper not found: {helper}"),
            false,
        )
        .with_details(serde_json::json!({ "helper": helper }))
    }

    pub fn helper_spawn_failed(helper: &str, reason: impl Into<String>) -> Self {
        Self::new(
            "HELPER_SPAWN_FAILED",
            format!("Failed to spawn helper '{helper}': {}", reason.into()),
            true,
        )
        .with_details(serde_json::json!({ "helper": helper }))
    }

    pub fn helper_protocol_mismatch(helper: &str, detail: impl Into<String>) -> Self {
        Self::new(
            "HELPER_PROTOCOL_MISMATCH",
            format!("Protocol mismatch with helper '{helper}': {}", detail.into()),
            false,
        )
        .with_details(serde_json::json!({ "helper": helper }))
    }

    pub fn helper_crashed(helper: &str, detail: impl Into<String>) -> Self {
        Self::new(
            "HELPER_CRASHED",
            format!("Helper '{helper}' crashed: {}", detail.into()),
            true,
        )
        .with_details(serde_json::json!({ "helper": helper }))
    }

    pub fn ipc_timeout(detail: impl Into<String>) -> Self {
        Self::new("IPC_TIMEOUT", detail, true)
    }

    pub fn ipc_invalid_response(detail: impl Into<String>) -> Self {
        Self::new("IPC_INVALID_RESPONSE", detail, false)
    }

    pub fn selector_not_found(detail: impl Into<String>) -> Self {
        Self::new("SELECTOR_NOT_FOUND", detail, true)
    }

    pub fn selector_ambiguous(detail: impl Into<String>) -> Self {
        Self::new("SELECTOR_AMBIGUOUS", detail, false)
    }

    pub fn exec_timeout(command: &str) -> Self {
        Self::new(
            "EXEC_TIMEOUT",
            format!("Command timed out: {command}"),
            true,
        )
        .with_details(serde_json::json!({ "command": command }))
    }

    pub fn exec_failed(command: &str, detail: impl Into<String>) -> Self {
        Self::new(
            "EXEC_FAILED",
            format!("Command failed: {}", detail.into()),
            false,
        )
        .with_details(serde_json::json!({ "command": command }))
    }

    pub fn stop_requested() -> Self {
        Self::new("STOP_REQUESTED", "Run stop requested by user", false)
    }

    pub fn internal_error(detail: impl Into<String>) -> Self {
        Self::new("INTERNAL_ERROR", detail, false)
    }
}

// ---------------------------------------------------------------------------
// CoreError  (Rust-level, thiserror-based)
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Interpolation error: {0}")]
    Interpolation(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Operator error: {0}")]
    Operator(OperatorError),
}
