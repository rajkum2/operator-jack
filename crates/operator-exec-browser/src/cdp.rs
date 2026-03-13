//! Chrome DevTools Protocol types and client.

use serde::{Deserialize, Serialize};
use serde_json::Value;


/// A CDP request message.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CdpRequest {
    /// Message ID for correlation.
    pub id: u64,

    /// Method name (e.g., "Page.navigate").
    pub method: String,

    /// Method parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// A CDP response message.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CdpResponse {
    /// Message ID (matches the request).
    pub id: Option<u64>,

    /// Response result (if successful).
    pub result: Option<Value>,

    /// Error details (if failed).
    pub error: Option<CdpError>,

    /// Event name (for events).
    pub method: Option<String>,

    /// Event parameters (for events).
    pub params: Option<Value>,
}

/// CDP error details.
#[derive(Debug, Clone, Deserialize)]
pub struct CdpError {
    /// Error code.
    pub code: i64,

    /// Error message.
    pub message: String,

    /// Additional error data.
    pub data: Option<Value>,
}

/// Chrome DevTools Protocol version info.
#[derive(Debug, Clone, Deserialize)]
pub struct CdpVersion {
    #[serde(rename = "Browser")]
    pub browser: String,

    #[serde(rename = "Protocol-Version")]
    pub protocol_version: String,

    #[serde(rename = "User-Agent")]
    pub user_agent: String,

    #[serde(rename = "V8-Version")]
    pub v8_version: Option<String>,

    #[serde(rename = "WebKit-Version")]
    pub webkit_version: Option<String>,
}

/// A WebSocket debugger target.
#[derive(Debug, Clone, Deserialize)]
pub struct DebuggerTarget {
    /// Target ID.
    pub id: String,

    /// Target title.
    pub title: String,

    /// Target type (e.g., "page").
    #[serde(rename = "type")]
    pub target_type: String,

    /// WebSocket URL for connecting.
    #[serde(rename = "webSocketDebuggerUrl")]
    pub websocket_url: String,

    /// Target URL.
    pub url: String,
}

/// Page navigation result.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigateResult {
    /// Navigation frame ID.
    pub frame_id: String,

    /// Final loader ID.
    pub loader_id: Option<String>,

    /// Navigation error text (if failed).
    pub error_text: Option<String>
}

/// DOM node information.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInfo {
    pub node_id: i64,
    pub backend_node_id: Option<i64>,
    pub node_type: Option<i32>,
    pub node_name: Option<String>,
    pub local_name: Option<String>,
    pub node_value: Option<String>,
}

/// Remote object (JavaScript evaluation result).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteObject {
    pub object_id: Option<String>,
    pub object_type: String,
    pub subtype: Option<String>,
    pub value: Option<Value>,
    pub description: Option<String>,
}

/// Screenshot result.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenshotResult {
    /// Base64-encoded image data.
    pub data: String,
}

impl CdpRequest {
    /// Create a new CDP request.
    pub fn new(id: u64, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            id,
            method: method.into(),
            params,
        }
    }

    /// Create a Page.navigate request.
    pub fn navigate(id: u64, url: impl Into<String>) -> Self {
        let params = serde_json::json!({
            "url": url.into()
        });
        Self::new(id, "Page.navigate", Some(params))
    }

    /// Create a Runtime.evaluate request.
    pub fn evaluate(id: u64, expression: impl Into<String>) -> Self {
        let params = serde_json::json!({
            "expression": expression.into(),
            "returnByValue": true,
            "awaitPromise": true
        });
        Self::new(id, "Runtime.evaluate", Some(params))
    }

    /// Create a Page.captureScreenshot request.
    pub fn screenshot(id: u64, full_page: bool) -> Self {
        let params = if full_page {
            serde_json::json!({
                "fromSurface": true,
                "captureBeyondViewport": true
            })
        } else {
            serde_json::json!({
                "fromSurface": true
            })
        };
        Self::new(id, "Page.captureScreenshot", Some(params))
    }

    /// Create a DOM.querySelector request.
    pub fn query_selector(id: u64, node_id: i64, selector: impl Into<String>) -> Self {
        let params = serde_json::json!({
            "nodeId": node_id,
            "selector": selector.into()
        });
        Self::new(id, "DOM.querySelector", Some(params))
    }

    /// Create a DOM.getDocument request.
    pub fn get_document(id: u64) -> Self {
        let params = serde_json::json!({
            "depth": -1,
            "pierce": false
        });
        Self::new(id, "DOM.getDocument", Some(params))
    }

    /// Create an Input.dispatchMouseEvent request.
    pub fn mouse_click(id: u64, x: f64, y: f64) -> Self {
        let params = serde_json::json!({
            "type": "mousePressed",
            "x": x,
            "y": y,
            "button": "left",
            "clickCount": 1
        });
        Self::new(id, "Input.dispatchMouseEvent", Some(params))
    }

    /// Create an Input.dispatchMouseEvent (release) request.
    pub fn mouse_release(id: u64, x: f64, y: f64) -> Self {
        let params = serde_json::json!({
            "type": "mouseReleased",
            "x": x,
            "y": y,
            "button": "left",
            "clickCount": 1
        });
        Self::new(id, "Input.dispatchMouseEvent", Some(params))
    }

    /// Create an Input.dispatchKeyEvent request (char).
    pub fn key_char(id: u64, text: impl Into<String>) -> Self {
        let params = serde_json::json!({
            "type": "char",
            "text": text.into()
        });
        Self::new(id, "Input.dispatchKeyEvent", Some(params))
    }
}

impl CdpResponse {
    /// Check if this is an error response.
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    /// Get the error message if this is an error response.
    pub fn error_message(&self) -> Option<String> {
        self.error.as_ref().map(|e| e.message.clone())
    }

    /// Check if this is an event (not a response to a request).
    pub fn is_event(&self) -> bool {
        self.method.is_some()
    }
}
