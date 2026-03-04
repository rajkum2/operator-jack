#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("Helper not found: {0}")]
    HelperNotFound(String),
    #[error("Helper spawn failed: {0}")]
    SpawnFailed(String),
    #[error("Helper protocol mismatch: expected {expected}, got {got}")]
    ProtocolMismatch { expected: String, got: String },
    #[error("Helper crashed: {0}")]
    HelperCrashed(String),
    #[error("IPC timeout after {0}ms")]
    Timeout(u64),
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Helper returned error: {code}: {message}")]
    HelperError {
        code: String,
        message: String,
        details: Option<serde_json::Value>,
    },
}
