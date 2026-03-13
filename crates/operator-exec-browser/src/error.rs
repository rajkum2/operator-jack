//! Error types for the browser executor.

use thiserror::Error;

/// Errors that can occur during browser automation.
#[derive(Debug, Error)]
pub enum BrowserError {
    #[error("Chrome not found. Is Chrome or Chromium installed?")]
    ChromeNotFound,

    #[error("Failed to connect to Chrome: {0}")]
    ConnectionError(String),

    #[error("Chrome DevTools Protocol error: {0}")]
    CdpError(String),

    #[error("Navigation failed: {0}")]
    NavigationError(String),

    #[error("Element not found: {0}")]
    ElementNotFound(String),

    #[error("Domain not allowed: {0}")]
    DomainNotAllowed(String),

    #[error("JavaScript execution failed: {0}")]
    JavaScriptError(String),

    #[error("Screenshot failed: {0}")]
    ScreenshotError(String),

    #[error("WebSocket error: {0}")]
    WebSocketError(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl BrowserError {
    /// Returns true if the error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            BrowserError::ConnectionError(_) | BrowserError::Timeout(_)
        )
    }
}
