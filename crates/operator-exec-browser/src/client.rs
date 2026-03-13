//! Browser client for Chrome DevTools Protocol.

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::{timeout, Duration};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, trace, warn};

use crate::cdp::{CdpRequest, CdpResponse, DebuggerTarget};
use crate::error::BrowserError;

/// A client for communicating with Chrome via CDP.
pub struct BrowserClient {
    /// WebSocket sender channel.
    sender: mpsc::UnboundedSender<Message>,

    /// Pending request handlers.
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<CdpResponse>>>>,

    /// Request ID counter.
    next_id: Arc<AtomicU64>,

    /// Chrome debugger URL (kept for debugging).
    #[allow(dead_code)]
    ws_url: String,
}

impl BrowserClient {
    /// Connect to Chrome at the given WebSocket URL.
    pub async fn connect(ws_url: impl Into<String>) -> Result<Self, BrowserError> {
        let ws_url = ws_url.into();
        info!("Connecting to Chrome at: {}", ws_url);

        let (ws_stream, _) = connect_async(&ws_url).await.map_err(|e| {
            BrowserError::ConnectionError(format!("WebSocket connection failed: {}", e))
        })?;

        info!("WebSocket connection established");

        let (mut write, mut read) = ws_stream.split();

        // Channel for sending messages to WebSocket
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

        // Pending requests map
        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<CdpResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_clone = pending.clone();

        // Spawn task to send messages to WebSocket
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if let Err(e) = write.send(msg).await {
                    error!("Failed to send WebSocket message: {}", e);
                    break;
                }
            }
            debug!("WebSocket sender task ended");
        });

        // Spawn task to receive messages from WebSocket
        tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        trace!("Received: {}", text);
                        match serde_json::from_str::<CdpResponse>(&text) {
                            Ok(response) => {
                                if let Some(id) = response.id {
                                    // This is a response to a request
                                    let mut pending = pending_clone.lock().await;
                                    if let Some(sender) = pending.remove(&id) {
                                        let _ = sender.send(response);
                                    }
                                } else {
                                    // This is an event
                                    debug!("CDP event: {:?}", response.method);
                                }
                            }
                            Err(e) => {
                                warn!("Failed to parse CDP response: {}", e);
                            }
                        }
                    }
                    Ok(Message::Close(_)) => {
                        debug!("WebSocket closed");
                        break;
                    }
                    Err(e) => {
                        error!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
            debug!("WebSocket receiver task ended");
        });

        Ok(Self {
            sender: tx,
            pending,
            next_id: Arc::new(AtomicU64::new(1)),
            ws_url,
        })
    }

    /// Send a CDP request and wait for response.
    pub async fn send(
        &self,
        method: impl Into<String>,
        params: Option<Value>,
    ) -> Result<CdpResponse, BrowserError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let request = CdpRequest::new(id, method, params);

        let (tx, rx) = oneshot::channel();

        // Register pending request
        {
            let mut pending = self.pending.lock().await;
            pending.insert(id, tx);
        }

        // Send the request
        let msg = Message::Text(serde_json::to_string(&request)?);
        self.sender.send(msg).map_err(|_| {
            BrowserError::ConnectionError("Failed to send message".to_string())
        })?;

        debug!("Sent CDP request {}: {}", id, request.method);

        // Wait for response with timeout
        let response = timeout(Duration::from_secs(30), rx)
            .await
            .map_err(|_| BrowserError::Timeout(format!("Request {} timed out", id)))?;

        match response {
            Ok(resp) => {
                if let Some(ref error) = resp.error {
                    return Err(BrowserError::CdpError(format!(
                        "{}: {}",
                        error.code, error.message
                    )));
                }
                Ok(resp)
            }
            Err(_) => Err(BrowserError::ConnectionError(
                "Response channel closed".to_string(),
            )),
        }
    }

    /// Enable a CDP domain.
    pub async fn enable_domain(&self, domain: impl Into<String>) -> Result<(), BrowserError> {
        let method = format!("{}.enable", domain.into());
        let _ = self.send(method, None).await?;
        Ok(())
    }

    /// Navigate to a URL.
    pub async fn navigate(&self, url: impl Into<String>) -> Result<String, BrowserError> {
        let url = url.into();
        info!("Navigating to: {}", url);

        let params = serde_json::json!({
            "url": url
        });

        let response = self.send("Page.navigate", Some(params)).await?;

        if let Some(result) = response.result {
            if let Some(frame_id) = result.get("frameId").and_then(|v| v.as_str()) {
                return Ok(frame_id.to_string());
            }
        }

        Err(BrowserError::NavigationError(
            "Navigation response missing frameId".to_string(),
        ))
    }

    /// Execute JavaScript in the page.
    pub async fn execute_js(&self, expression: impl Into<String>) -> Result<Value, BrowserError> {
        let expression = expression.into();
        debug!("Executing JS: {}", expression);

        let params = serde_json::json!({
            "expression": expression,
            "returnByValue": true,
            "awaitPromise": true
        });

        let response = self.send("Runtime.evaluate", Some(params)).await?;

        if let Some(result) = response.result {
            if let Some(exception) = result.get("exceptionDetails") {
                let msg = exception
                    .get("exception")
                    .and_then(|e| e.get("description"))
                    .and_then(|d| d.as_str())
                    .unwrap_or("JavaScript execution failed");
                return Err(BrowserError::JavaScriptError(msg.to_string()));
            }

            if let Some(value) = result.get("result").and_then(|r| r.get("value")) {
                return Ok(value.clone());
            }
        }

        Ok(serde_json::Value::Null)
    }

    /// Take a screenshot.
    pub async fn screenshot(&self, full_page: bool) -> Result<Vec<u8>, BrowserError> {
        debug!("Taking screenshot (full_page={})", full_page);

        let mut params = serde_json::json!({
            "fromSurface": true
        });

        if full_page {
            params["captureBeyondViewport"] = serde_json::json!(true);
        }

        let response = self.send("Page.captureScreenshot", Some(params)).await?;

        if let Some(result) = response.result {
            if let Some(data) = result.get("data").and_then(|d| d.as_str()) {
                return base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data)
                    .map_err(|e| {
                        BrowserError::ScreenshotError(format!("Failed to decode base64: {}", e))
                    });
            }
        }

        Err(BrowserError::ScreenshotError(
            "Screenshot response missing data".to_string(),
        ))
    }

    /// Get the current page URL.
    pub async fn get_url(&self) -> Result<String, BrowserError> {
        let result = self.execute_js("window.location.href").await?;
        result
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| BrowserError::CdpError("Failed to get URL".to_string()))
    }

    /// Close the connection.
    pub async fn close(self) {
        let _ = self.sender.send(Message::Close(None));
        debug!("BrowserClient closed");
    }
}

/// Discover Chrome DevTools WebSocket URL.
pub async fn discover_chrome_ws_url(port: u16) -> Result<String, BrowserError> {
    let url = format!("http://127.0.0.1:{}/json/version", port);
    debug!("Checking Chrome at: {}", url);

    let client = reqwest::Client::new();
    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        return Err(BrowserError::ConnectionError(format!(
            "Chrome returned status: {}",
            response.status()
        )));
    }

    let version: crate::cdp::CdpVersion = response.json().await?;
    debug!("Found Chrome: {}", version.browser);

    // Get list of targets
    let targets_url = format!("http://127.0.0.1:{}/json/list", port);
    let targets: Vec<DebuggerTarget> = client
        .get(&targets_url)
        .send()
        .await?
        .json()
        .await?;

    // Find the first page target
    for target in targets {
        if target.target_type == "page" {
            return Ok(target.websocket_url);
        }
    }

    Err(BrowserError::ConnectionError(
        "No page target found".to_string(),
    ))
}

/// Launch Chrome with remote debugging enabled.
pub async fn launch_chrome(port: u16) -> Result<tokio::process::Child, BrowserError> {
    use std::process::Stdio;

    info!("Launching Chrome with remote debugging on port {}", port);

    let chrome_paths = vec![
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
    ];

    let chrome_path = chrome_paths
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
        .ok_or(BrowserError::ChromeNotFound)?;

    let mut cmd = tokio::process::Command::new(chrome_path);
    cmd.arg(format!("--remote-debugging-port={}", port))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--headless=new")
        .arg("--disable-gpu")
        .arg("--disable-extensions")
        .arg("about:blank")
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let child = cmd.spawn().map_err(|e| {
        BrowserError::ConnectionError(format!("Failed to launch Chrome: {}", e))
    })?;

    // Wait a moment for Chrome to start
    tokio::time::sleep(Duration::from_millis(500)).await;

    info!("Chrome launched successfully");
    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cdp_request_serialization() {
        let req = CdpRequest::navigate(1, "https://example.com");
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("Page.navigate"));
        assert!(json.contains("https://example.com"));
    }
}
