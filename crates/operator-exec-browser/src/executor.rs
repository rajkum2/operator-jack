//! Browser step executor implementation.

use operator_core::types::{Step, StepType};
use serde_json::Value;

use tokio::time::{timeout, Duration};
use tracing::{debug, info};

use crate::client::{discover_chrome_ws_url, launch_chrome, BrowserClient};
use crate::error::BrowserError;

/// Executor for browser automation steps.
pub struct BrowserExecutor {
    /// Chrome process (if we launched it).
    chrome_process: Option<tokio::process::Child>,

    /// CDP client.
    client: Option<BrowserClient>,

    /// Allowed domains.
    allow_domains: Vec<String>,

    /// Chrome debugging port.
    port: u16,
}

impl BrowserExecutor {
    /// Create a new browser executor.
    pub fn new(allow_domains: Vec<String>) -> Self {
        Self {
            chrome_process: None,
            client: None,
            allow_domains,
            port: 9222, // Default Chrome debugging port
        }
    }

    /// Set the Chrome debugging port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Connect to Chrome (launch if needed).
    pub async fn connect(&mut self) -> Result<(), BrowserError> {
        // Try to connect to existing Chrome first
        match discover_chrome_ws_url(self.port).await {
            Ok(ws_url) => {
                info!("Connecting to existing Chrome at port {}", self.port);
                self.client = Some(BrowserClient::connect(ws_url).await?);
                return Ok(());
            }
            Err(e) => {
                debug!("Could not connect to existing Chrome: {}", e);
            }
        }

        // Launch Chrome
        info!("Launching new Chrome instance");
        self.chrome_process = Some(launch_chrome(self.port).await?);

        // Wait for Chrome to be ready
        let mut attempts = 0;
        loop {
            tokio::time::sleep(Duration::from_millis(200)).await;
            attempts += 1;

            match discover_chrome_ws_url(self.port).await {
                Ok(ws_url) => {
                    self.client = Some(BrowserClient::connect(ws_url).await?);
                    break;
                }
                Err(_) if attempts < 50 => continue,
                Err(e) => return Err(e),
            }
        }

        // Enable required CDP domains
        let client = self.client.as_ref().unwrap();
        client.enable_domain("Page").await?;
        client.enable_domain("Runtime").await?;
        client.enable_domain("DOM").await?;

        info!("Browser executor ready");
        Ok(())
    }

    /// Check if a URL is allowed based on domain allowlist.
    fn is_url_allowed(&self, url: &str) -> bool {
        if self.allow_domains.is_empty() {
            return true;
        }

        match url::Url::parse(url) {
            Ok(parsed) => {
                if let Some(domain) = parsed.domain() {
                    self.allow_domains.iter().any(|allowed| {
                        domain == allowed || domain.ends_with(&format!(".{}", allowed))
                    })
                } else {
                    false
                }
            }
            Err(_) => false,
        }
    }

    /// Execute a browser step.
    pub async fn execute_step(&self, step: &Step) -> Result<Value, BrowserError> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| BrowserError::ConnectionError("Not connected".to_string()))?;

        match step.step_type {
            StepType::BrowserNavigate => self.handle_navigate(client, &step.params).await,
            StepType::BrowserClick => self.handle_click(client, &step.params).await,
            StepType::BrowserType => self.handle_type(client, &step.params).await,
            StepType::BrowserGetText => self.handle_get_text(client, &step.params).await,
            StepType::BrowserGetAttribute => self.handle_get_attribute(client, &step.params).await,
            StepType::BrowserExecuteJs => self.handle_execute_js(client, &step.params).await,
            StepType::BrowserScreenshot => self.handle_screenshot(client, &step.params).await,
            StepType::BrowserWaitFor => self.handle_wait_for(client, &step.params).await,
            StepType::BrowserScroll => self.handle_scroll(client, &step.params).await,
            _ => Err(BrowserError::CdpError(format!(
                "Unsupported browser step type: {:?}",
                step.step_type
            ))),
        }
    }

    /// Handle browser.navigate step.
    async fn handle_navigate(&self, client: &BrowserClient, params: &Value) -> Result<Value, BrowserError> {
        let url = params
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'url' parameter".to_string()))?;

        if !self.is_url_allowed(url) {
            return Err(BrowserError::DomainNotAllowed(url.to_string()));
        }

        let frame_id = client.navigate(url).await?;

        // Wait for navigation to complete
        let wait_ms = params.get("wait_ms").and_then(|v| v.as_u64()).unwrap_or(1000);
        tokio::time::sleep(Duration::from_millis(wait_ms)).await;

        Ok(serde_json::json!({
            "frame_id": frame_id,
            "url": url
        }))
    }

    /// Handle browser.click step.
    async fn handle_click(&self, client: &BrowserClient, params: &Value) -> Result<Value, BrowserError> {
        let selector = params
            .get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'selector' parameter".to_string()))?;

        // Use JavaScript to find and click the element
        let script = format!(
            r#"
            (function() {{
                const el = document.querySelector('{}');
                if (!el) throw new Error('Element not found: {}');
                const rect = el.getBoundingClientRect();
                el.click();
                return {{
                    x: rect.left + rect.width / 2,
                    y: rect.top + rect.height / 2
                }};
            }})()
            "#,
            selector.replace("'", "\\'"),
            selector
        );

        let result = client.execute_js(script).await?;
        Ok(result)
    }

    /// Handle browser.type step.
    async fn handle_type(&self, client: &BrowserClient, params: &Value) -> Result<Value, BrowserError> {
        let selector = params
            .get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'selector' parameter".to_string()))?;

        let text = params
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'text' parameter".to_string()))?;

        // Use JavaScript to focus and set value
        let script = format!(
            r#"
            (function() {{
                const el = document.querySelector('{}');
                if (!el) throw new Error('Element not found: {}');
                el.focus();
                el.value = '{}';
                el.dispatchEvent(new Event('input', {{ bubbles: true }}));
                el.dispatchEvent(new Event('change', {{ bubbles: true }}));
                return true;
            }})()
            "#,
            selector.replace("'", "\\'"),
            selector,
            text.replace("'", "\\'")
        );

        client.execute_js(script).await?;
        Ok(serde_json::json!({ "typed": text.len() }))
    }

    /// Handle browser.get_text step.
    async fn handle_get_text(&self, client: &BrowserClient, params: &Value) -> Result<Value, BrowserError> {
        let selector = params
            .get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'selector' parameter".to_string()))?;

        let script = format!(
            r#"
            (function() {{
                const el = document.querySelector('{}');
                if (!el) throw new Error('Element not found: {}');
                return el.textContent || el.innerText || '';
            }})()
            "#,
            selector.replace("'", "\\'"),
            selector
        );

        let text = client.execute_js(script).await?;
        Ok(text)
    }

    /// Handle browser.get_attribute step.
    async fn handle_get_attribute(&self, client: &BrowserClient, params: &Value) -> Result<Value, BrowserError> {
        let selector = params
            .get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'selector' parameter".to_string()))?;

        let attribute = params
            .get("attribute")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'attribute' parameter".to_string()))?;

        let script = format!(
            r#"
            (function() {{
                const el = document.querySelector('{}');
                if (!el) throw new Error('Element not found: {}');
                return el.getAttribute('{}') || '';
            }})()
            "#,
            selector.replace("'", "\\'"),
            selector,
            attribute.replace("'", "\\'")
        );

        let value = client.execute_js(script).await?;
        Ok(value)
    }

    /// Handle browser.execute_js step.
    async fn handle_execute_js(&self, client: &BrowserClient, params: &Value) -> Result<Value, BrowserError> {
        let script = params
            .get("script")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'script' parameter".to_string()))?;

        let result = client.execute_js(script).await?;
        Ok(result)
    }

    /// Handle browser.screenshot step.
    async fn handle_screenshot(&self, client: &BrowserClient, params: &Value) -> Result<Value, BrowserError> {
        let full_page = params
            .get("full_page")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let image_data = client.screenshot(full_page).await?;

        // Encode to base64 for JSON response
        let base64_data = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &image_data);

        Ok(serde_json::json!({
            "format": "png",
            "full_page": full_page,
            "data": base64_data,
            "size_bytes": image_data.len()
        }))
    }

    /// Handle browser.wait_for step.
    async fn handle_wait_for(&self, client: &BrowserClient, params: &Value) -> Result<Value, BrowserError> {
        let selector = params
            .get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'selector' parameter".to_string()))?;

        let timeout_ms = params
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(10000);

        let script = format!(
            r#"
            new Promise((resolve, reject) => {{
                const startTime = Date.now();
                const timeout = {};
                const selector = '{}';
                
                const check = () => {{
                    const el = document.querySelector(selector);
                    if (el) {{
                        resolve(true);
                    }} else if (Date.now() - startTime > timeout) {{
                        reject(new Error('Timeout waiting for: ' + selector));
                    }} else {{
                        setTimeout(check, 100);
                    }}
                }};
                check();
            }})
            "#,
            timeout_ms,
            selector.replace("'", "\\'")
        );

        match timeout(Duration::from_millis(timeout_ms + 1000), client.execute_js(script)).await {
            Ok(Ok(_result)) => Ok(serde_json::json!({ "found": true })),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(BrowserError::Timeout(format!(
                "Element not found within {}ms: {}",
                timeout_ms, selector
            ))),
        }
    }

    /// Handle browser.scroll step.
    async fn handle_scroll(&self, client: &BrowserClient, params: &Value) -> Result<Value, BrowserError> {
        let x = params.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let y = params.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);

        let script = format!(
            "window.scrollTo({}, {})",
            x, y
        );

        client.execute_js(script).await?;
        Ok(serde_json::json!({ "x": x, "y": y }))
    }

    /// Close the executor and cleanup.
    pub async fn close(mut self) {
        if let Some(client) = self.client.take() {
            client.close().await;
        }

        if let Some(mut process) = self.chrome_process.take() {
            let _ = process.kill().await;
        }

        info!("Browser executor closed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_url_allowed() {
        let executor = BrowserExecutor::new(vec!["example.com".to_string(), "github.com".to_string()]);

        assert!(executor.is_url_allowed("https://example.com"));
        assert!(executor.is_url_allowed("https://www.example.com"));
        assert!(executor.is_url_allowed("https://github.com/user/repo"));
        assert!(!executor.is_url_allowed("https://evil.com"));
    }

    #[test]
    fn test_empty_allowlist_allows_all() {
        let executor = BrowserExecutor::new(vec![]);
        assert!(executor.is_url_allowed("https://any-domain.com"));
    }
}
