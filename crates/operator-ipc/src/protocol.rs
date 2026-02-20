use serde::{Deserialize, Serialize};

/// A request sent from the CLI to the macOS helper process over NDJSON.
#[derive(Debug, Serialize)]
pub struct IpcRequest {
    pub id: String,
    pub method: String,
    pub params: serde_json::Value,
}

impl IpcRequest {
    /// Creates a new IPC request with a ULID-generated id.
    pub fn new(method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            id: ulid::Ulid::new().to_string(),
            method: method.into(),
            params,
        }
    }
}

/// A response received from the macOS helper process over NDJSON.
#[derive(Debug, Serialize, Deserialize)]
pub struct IpcResponse {
    pub id: String,
    pub ok: bool,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<IpcErrorPayload>,
}

/// The error payload inside an IPC response when `ok` is false.
#[derive(Debug, Serialize, Deserialize)]
pub struct IpcErrorPayload {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default)]
    pub details: serde_json::Value,
}
