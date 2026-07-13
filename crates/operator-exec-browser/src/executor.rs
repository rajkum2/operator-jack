//! Browser step executor implementation.

use operator_core::types::{Step, StepType};
use serde_json::Value;

use tokio::time::{timeout, Duration};
use tracing::info;

use crate::client::{discover_chrome_ws_url, launch_chrome, BrowserClient};
use crate::error::BrowserError;

/// Encodes a string as a JavaScript string literal (double-quoted, fully
/// escaped) via JSON, whose string syntax is a valid subset of JS. This is the
/// safe way to embed a user/selector/text value into an injected script: it
/// escapes quotes, backslashes, and newlines, preventing the value from
/// breaking out of the literal or injecting arbitrary JS. The result includes
/// the surrounding quotes, so embed it bare (not inside your own quotes).
fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// Default auto-wait for a selector to appear before acting on it (ms).
const ELEMENT_WAIT_MS: u64 = 5_000;

/// Builds a JS expression that polls for `document.querySelector(<sel_literal>)`
/// to appear (up to `timeout_ms`), then resolves with `body(el)` — where `body`
/// is a JS expression referencing the bound element `el`. Rejects with
/// "Element not found" on timeout. This gives every selector-based action the
/// same auto-wait as Playwright, so a step run right after navigation (before
/// the element renders) polls instead of failing immediately.
fn wait_and_eval(sel_literal: &str, body: &str, timeout_ms: u64) -> String {
    format!(
        r#"
        new Promise((resolve, reject) => {{
            const sel = {sel_literal};
            const deadline = Date.now() + {timeout_ms};
            const tick = () => {{
                const el = document.querySelector(sel);
                if (el) {{
                    try {{ resolve(((el) => ({body}))(el)); }}
                    catch (e) {{ reject(e); }}
                }} else if (Date.now() > deadline) {{
                    reject(new Error('Element not found: ' + sel));
                }} else {{
                    setTimeout(tick, 100);
                }}
            }};
            tick();
        }})
        "#
    )
}

/// Profile-isolation options for the browser lane (M12). By default the lane
/// launches its OWN Chrome in an ephemeral, empty profile — never the user's
/// logged-in one — and refuses to attach to a foreign Chrome.
#[derive(Debug, Clone)]
pub struct BrowserLaunchOptions {
    /// Operator-owned profile dir. `None` = a fresh ephemeral dir per run
    /// (removed on close). A named dir keeps the profile across runs
    /// ("stay logged in to site X").
    pub user_data_dir: Option<std::path::PathBuf>,
    /// Run Chrome headless (default true). Headed is needed for human takeover.
    pub headless: bool,
    /// Allow attaching to / using a Chrome the lane did NOT launch — i.e. the
    /// user's real, possibly logged-in profile. Off by default; gated by the
    /// `browser.real_profile` grant (which escalates).
    pub allow_real_profile: bool,
}

impl Default for BrowserLaunchOptions {
    fn default() -> Self {
        Self {
            user_data_dir: None,
            headless: true,
            allow_real_profile: false,
        }
    }
}

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

    /// Profile isolation options (M12).
    options: BrowserLaunchOptions,

    /// The ephemeral profile dir we created (removed on close), if any.
    ephemeral_profile: Option<std::path::PathBuf>,
}

impl BrowserExecutor {
    /// Create a new browser executor.
    pub fn new(allow_domains: Vec<String>) -> Self {
        Self {
            chrome_process: None,
            client: None,
            allow_domains,
            port: 9222, // Default Chrome debugging port
            options: BrowserLaunchOptions::default(),
            ephemeral_profile: None,
        }
    }

    /// Set the Chrome debugging port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set profile-isolation options (M12).
    pub fn with_options(mut self, options: BrowserLaunchOptions) -> Self {
        self.options = options;
        self
    }

    /// Whether the CDP client is connected.
    pub fn is_connected(&self) -> bool {
        self.client.is_some()
    }

    /// Resolve the profile dir to launch with: a caller-named dir, or a fresh
    /// ephemeral dir under the app data dir (tracked for cleanup).
    fn resolve_profile_dir(&mut self) -> std::path::PathBuf {
        if let Some(ref dir) = self.options.user_data_dir {
            return dir.clone();
        }
        let base = dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("operator-jack")
            .join("browser-profiles");
        // Unique per run: port + process id (Date/rand are unavailable here).
        let dir = base.join(format!("ephemeral-{}-{}", self.port, std::process::id()));
        self.ephemeral_profile = Some(dir.clone());
        dir
    }

    /// Connect to Chrome. In the default (isolated) mode this launches our own
    /// Chrome in an operator-owned profile and REFUSES to attach to a foreign
    /// Chrome already on the port. Attaching to a pre-existing (user) Chrome
    /// requires `allow_real_profile` (the escalating `browser.real_profile`
    /// grant).
    pub async fn connect(&mut self) -> Result<(), BrowserError> {
        let existing = discover_chrome_ws_url(self.port).await;

        if self.options.allow_real_profile {
            // Explicitly permitted: attach to an existing Chrome if present.
            if let Ok(ws_url) = existing {
                info!(
                    "Attaching to existing Chrome at port {} (browser.real_profile granted)",
                    self.port
                );
                self.client = Some(BrowserClient::connect(ws_url).await?);
                self.enable_domains().await?;
                return Ok(());
            }
        } else if existing.is_ok() {
            // Isolated mode + a foreign Chrome is already on the port: refuse,
            // rather than silently ride the user's logged-in session.
            return Err(BrowserError::ConnectionError(format!(
                "a Chrome is already listening on port {} that Operator Jack did not launch; \
                 refusing to attach to a non-owned browser (grant `browser.real_profile` to \
                 override, or use a different port)",
                self.port
            )));
        }

        // Launch our own isolated Chrome.
        let profile_dir = self.resolve_profile_dir();
        info!("Launching isolated Chrome instance");
        self.chrome_process =
            Some(launch_chrome(self.port, &profile_dir, self.options.headless).await?);

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

        self.enable_domains().await?;
        info!("Browser executor ready");
        Ok(())
    }

    /// Enable the CDP domains the executor relies on.
    async fn enable_domains(&self) -> Result<(), BrowserError> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| BrowserError::ConnectionError("Not connected".to_string()))?;
        client.enable_domain("Page").await?;
        client.enable_domain("Runtime").await?;
        client.enable_domain("DOM").await?;
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
            StepType::BrowserGetTextAll => self.handle_get_text_all(client, &step.params).await,
            StepType::BrowserGetAttributesAll => {
                self.handle_get_attributes_all(client, &step.params).await
            }
            StepType::BrowserSnapshot => self.handle_snapshot(client, &step.params).await,
            StepType::BrowserDownload => self.handle_download(client, &step.params).await,
            _ => Err(BrowserError::CdpError(format!(
                "Unsupported browser step type: {:?}",
                step.step_type
            ))),
        }
    }

    /// Handle browser.navigate step.
    async fn handle_navigate(
        &self,
        client: &BrowserClient,
        params: &Value,
    ) -> Result<Value, BrowserError> {
        let url = params
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'url' parameter".to_string()))?;

        if !self.is_url_allowed(url) {
            return Err(BrowserError::DomainNotAllowed(url.to_string()));
        }

        let frame_id = client.navigate(url).await?;

        // Wait for the document to actually finish loading, rather than a blind
        // fixed sleep (M12): poll until readyState is "complete" and we've left
        // about:blank, up to `wait_ms` (default 15s). Best-effort — if the deadline
        // passes we proceed anyway so a slow/streaming page doesn't hard-fail.
        let wait_ms = params
            .get("wait_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(15_000);
        let poll_every = 150u64;
        let max_polls = (wait_ms / poll_every).max(1);
        // Give the navigation a moment to commit before the first check so we
        // don't read the previous document's "complete" state.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let mut loaded = false;
        for _ in 0..max_polls {
            let ready = client
                .execute_js(
                    "(document.readyState === 'complete' && location.href !== 'about:blank')"
                        .to_string(),
                )
                .await
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if ready {
                loaded = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(poll_every)).await;
        }

        Ok(serde_json::json!({
            "frame_id": frame_id,
            "url": url,
            "loaded": loaded
        }))
    }

    /// Handle browser.click step.
    async fn handle_click(
        &self,
        client: &BrowserClient,
        params: &Value,
    ) -> Result<Value, BrowserError> {
        let selector = params
            .get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'selector' parameter".to_string()))?;

        // Auto-wait for the element, then click it.
        let sel = js_string(selector);
        let body = "(() => { const r = el.getBoundingClientRect(); el.scrollIntoView({block:'center'}); el.click(); return { x: r.left + r.width/2, y: r.top + r.height/2 }; })()";
        let script = wait_and_eval(&sel, body, ELEMENT_WAIT_MS);

        let result = client.execute_js(script).await?;
        Ok(result)
    }

    /// Handle browser.type step.
    async fn handle_type(
        &self,
        client: &BrowserClient,
        params: &Value,
    ) -> Result<Value, BrowserError> {
        let selector = params
            .get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'selector' parameter".to_string()))?;

        let text = params
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'text' parameter".to_string()))?;

        // Auto-wait for the element, then focus and set its value.
        let sel = js_string(selector);
        let val = js_string(text);
        let body = format!(
            "(() => {{ el.focus(); el.value = {val}; el.dispatchEvent(new Event('input', {{bubbles:true}})); el.dispatchEvent(new Event('change', {{bubbles:true}})); return true; }})()"
        );
        let script = wait_and_eval(&sel, &body, ELEMENT_WAIT_MS);

        client.execute_js(script).await?;
        Ok(serde_json::json!({ "typed": text.len() }))
    }

    /// Handle browser.get_text step.
    async fn handle_get_text(
        &self,
        client: &BrowserClient,
        params: &Value,
    ) -> Result<Value, BrowserError> {
        let selector = params
            .get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'selector' parameter".to_string()))?;

        let sel = js_string(selector);
        let script = wait_and_eval(
            &sel,
            "el.textContent || el.innerText || ''",
            ELEMENT_WAIT_MS,
        );

        let text = client.execute_js(script).await?;
        Ok(text)
    }

    /// Handle browser.get_attribute step.
    async fn handle_get_attribute(
        &self,
        client: &BrowserClient,
        params: &Value,
    ) -> Result<Value, BrowserError> {
        let selector = params
            .get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'selector' parameter".to_string()))?;

        let attribute = params
            .get("attribute")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'attribute' parameter".to_string()))?;

        let sel = js_string(selector);
        let attr = js_string(attribute);
        let body = format!("el.getAttribute({attr}) || ''");
        let script = wait_and_eval(&sel, &body, ELEMENT_WAIT_MS);

        let value = client.execute_js(script).await?;
        Ok(value)
    }

    /// Handle browser.execute_js step.
    async fn handle_execute_js(
        &self,
        client: &BrowserClient,
        params: &Value,
    ) -> Result<Value, BrowserError> {
        let script = params
            .get("script")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'script' parameter".to_string()))?;

        let result = client.execute_js(script).await?;
        Ok(result)
    }

    /// Handle browser.screenshot step.
    async fn handle_screenshot(
        &self,
        client: &BrowserClient,
        params: &Value,
    ) -> Result<Value, BrowserError> {
        let full_page = params
            .get("full_page")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let image_data = client.screenshot(full_page).await?;

        // Write the PNG to a file and return its PATH — never base64 into the
        // store (M12): a screenshot could be large and would bloat the audit DB.
        let out_dir = params
            .get("output_dir")
            .and_then(|v| v.as_str())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                dirs::data_dir()
                    .unwrap_or_else(std::env::temp_dir)
                    .join("operator-jack")
                    .join("screenshots")
            });
        std::fs::create_dir_all(&out_dir)
            .map_err(|e| BrowserError::CdpError(format!("failed to create screenshot dir: {e}")))?;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = out_dir.join(format!("browser-{nanos}.png"));
        std::fs::write(&path, &image_data)
            .map_err(|e| BrowserError::CdpError(format!("failed to write screenshot: {e}")))?;

        Ok(serde_json::json!({
            "format": "png",
            "full_page": full_page,
            "path": path.to_string_lossy(),
            "size_bytes": image_data.len()
        }))
    }

    /// Handle browser.wait_for step.
    async fn handle_wait_for(
        &self,
        client: &BrowserClient,
        params: &Value,
    ) -> Result<Value, BrowserError> {
        let selector = params
            .get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'selector' parameter".to_string()))?;

        let timeout_ms = params
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(10000);

        let sel = js_string(selector);
        let script = format!(
            r#"
            new Promise((resolve, reject) => {{
                const startTime = Date.now();
                const timeout = {timeout_ms};
                const selector = {sel};

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
            "#
        );

        match timeout(
            Duration::from_millis(timeout_ms + 1000),
            client.execute_js(script),
        )
        .await
        {
            Ok(Ok(_result)) => Ok(serde_json::json!({ "found": true })),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(BrowserError::Timeout(format!(
                "Element not found within {}ms: {}",
                timeout_ms, selector
            ))),
        }
    }

    /// Handle browser.get_text_all: text of ALL elements matching the selector.
    async fn handle_get_text_all(
        &self,
        client: &BrowserClient,
        params: &Value,
    ) -> Result<Value, BrowserError> {
        let selector = params
            .get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'selector' parameter".to_string()))?;
        let sel = js_string(selector);
        let script = format!(
            "Array.from(document.querySelectorAll({sel})).map(el => (el.textContent || el.innerText || '').trim())"
        );
        let arr = client.execute_js(script).await?;
        let count = arr.as_array().map(|a| a.len()).unwrap_or(0);
        Ok(serde_json::json!({ "count": count, "items": arr }))
    }

    /// Handle browser.get_attributes_all: one attribute from ALL matching elements.
    async fn handle_get_attributes_all(
        &self,
        client: &BrowserClient,
        params: &Value,
    ) -> Result<Value, BrowserError> {
        let selector = params
            .get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'selector' parameter".to_string()))?;
        let attribute = params
            .get("attribute")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::CdpError("Missing 'attribute' parameter".to_string()))?;
        let sel = js_string(selector);
        let attr = js_string(attribute);
        // For href/src, prefer the resolved DOM property so callers get absolute URLs.
        let script = format!(
            "Array.from(document.querySelectorAll({sel})).map(el => {{ const p = el[{attr}]; return (p !== undefined && p !== null && typeof p !== 'object') ? String(p) : (el.getAttribute({attr}) || ''); }})"
        );
        let arr = client.execute_js(script).await?;
        let count = arr.as_array().map(|a| a.len()).unwrap_or(0);
        Ok(serde_json::json!({ "count": count, "items": arr }))
    }

    /// Handle browser.snapshot: a compact list of the page's interactive/labeled
    /// elements with `bref` ids (browser refs), the lane analogue of ui.snapshot.
    async fn handle_snapshot(
        &self,
        client: &BrowserClient,
        params: &Value,
    ) -> Result<Value, BrowserError> {
        let max = params
            .get("max_elements")
            .and_then(|v| v.as_u64())
            .unwrap_or(80);
        // Enumerate actionable/labeled elements; tag each with a stable-ish
        // nth-of-match selector the agent can act on via `selector`.
        let script = format!(
            r#"
            (() => {{
                const sels = ['a[href]','button','input','textarea','select','[role=button]','[onclick]','h1','h2','h3'];
                const seen = new Set();
                const out = [];
                for (const s of sels) {{
                    const nodes = document.querySelectorAll(s);
                    for (let i = 0; i < nodes.length; i++) {{
                        const el = nodes[i];
                        if (seen.has(el)) continue; seen.add(el);
                        const rect = el.getBoundingClientRect();
                        if (rect.width === 0 && rect.height === 0) continue;
                        const label = (el.getAttribute('aria-label') || el.value || el.textContent || '').trim().slice(0, 80);
                        out.push({{
                            tag: el.tagName.toLowerCase(),
                            type: el.getAttribute('type') || null,
                            text: label,
                            href: el.getAttribute('href') || null,
                            selector: s + ':nth-of-type(' + (i + 1) + ')'
                        }});
                        if (out.length >= {max}) return {{ title: document.title, url: location.href, elements: out }};
                    }}
                }}
                return {{ title: document.title, url: location.href, elements: out }};
            }})()
            "#
        );
        let snap = client.execute_js(script).await?;
        Ok(snap)
    }

    /// Handle browser.download: point Chrome's download dir at a target folder,
    /// then either navigate to `url` or click `selector`, and wait for a file to
    /// finish downloading. Returns the downloaded file path.
    async fn handle_download(
        &self,
        client: &BrowserClient,
        params: &Value,
    ) -> Result<Value, BrowserError> {
        let dir = params
            .get("dir")
            .and_then(|v| v.as_str())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                dirs::download_dir()
                    .unwrap_or_else(std::env::temp_dir)
                    .join("operator-jack")
            });
        std::fs::create_dir_all(&dir)
            .map_err(|e| BrowserError::CdpError(format!("failed to create download dir: {e}")))?;

        // Snapshot existing files so we can detect the new one.
        let before: std::collections::HashSet<std::path::PathBuf> = std::fs::read_dir(&dir)
            .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default();

        // Allow downloads to the target dir.
        client
            .send(
                "Page.setDownloadBehavior",
                Some(serde_json::json!({
                    "behavior": "allow",
                    "downloadPath": dir.to_string_lossy(),
                })),
            )
            .await?;

        // Trigger the download.
        if let Some(url) = params.get("url").and_then(|v| v.as_str()) {
            if !self.is_url_allowed(url) {
                return Err(BrowserError::DomainNotAllowed(url.to_string()));
            }
            // Anchor-click download keeps us on the page (navigation to a file
            // can abort the CDP page). Fall back to location for direct files.
            let u = js_string(url);
            let script = format!(
                "(() => {{ const a = document.createElement('a'); a.href = {u}; a.download = ''; document.body.appendChild(a); a.click(); a.remove(); return true; }})()"
            );
            client.execute_js(script).await?;
        } else if let Some(selector) = params.get("selector").and_then(|v| v.as_str()) {
            let sel = js_string(selector);
            let script = wait_and_eval(&sel, "(el.click(), true)", ELEMENT_WAIT_MS);
            client.execute_js(script).await?;
        } else {
            return Err(BrowserError::CdpError(
                "browser.download needs a 'url' or 'selector'".to_string(),
            ));
        }

        // Wait for a new, complete file (no .crdownload) to appear.
        let timeout_ms = params
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(30_000);
        let polls = (timeout_ms / 250).max(1);
        for _ in 0..polls {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for entry in rd.flatten() {
                    let path = entry.path();
                    if before.contains(&path) {
                        continue;
                    }
                    let is_partial = path.extension().map(|e| e == "crdownload").unwrap_or(false);
                    if !is_partial {
                        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                        return Ok(serde_json::json!({
                            "path": path.to_string_lossy(),
                            "size_bytes": size
                        }));
                    }
                }
            }
        }
        Err(BrowserError::Timeout(format!(
            "no download completed within {timeout_ms}ms in {}",
            dir.display()
        )))
    }

    /// Handle browser.scroll step.
    async fn handle_scroll(
        &self,
        client: &BrowserClient,
        params: &Value,
    ) -> Result<Value, BrowserError> {
        let x = params.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let y = params.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);

        let script = format!("window.scrollTo({}, {})", x, y);

        client.execute_js(script).await?;
        Ok(serde_json::json!({ "x": x, "y": y }))
    }

    /// Close the executor and cleanup: disconnect CDP, kill the Chrome child we
    /// launched, and remove the ephemeral profile dir (if any).
    pub async fn close(mut self) {
        if let Some(client) = self.client.take() {
            client.close().await;
        }

        if let Some(mut process) = self.chrome_process.take() {
            let _ = process.kill().await;
            let _ = process.wait().await;
        }

        if let Some(dir) = self.ephemeral_profile.take() {
            let _ = std::fs::remove_dir_all(&dir);
        }

        info!("Browser executor closed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_url_allowed() {
        let executor =
            BrowserExecutor::new(vec!["example.com".to_string(), "github.com".to_string()]);

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

    #[test]
    fn test_js_string_escapes_injection() {
        // Plain value → quoted literal.
        assert_eq!(js_string("#id"), "\"#id\"");
        // Single quote is preserved inside a double-quoted literal (no breakout).
        assert_eq!(js_string("a'b"), "\"a'b\"");
        // Double quote and backslash are escaped.
        assert_eq!(js_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(js_string("a\\b"), "\"a\\\\b\"");
        // A classic breakout attempt stays inside the literal.
        let malicious = "'); alert('xss'); ('";
        let lit = js_string(malicious);
        assert!(lit.starts_with('"') && lit.ends_with('"'));
        // Round-trips back to the exact original via JSON parsing (valid JS literal).
        let parsed: String = serde_json::from_str(&lit).unwrap();
        assert_eq!(parsed, malicious);
        // Newlines are escaped, not literal (which would break the JS line).
        assert!(!js_string("a\nb").contains('\n'));
    }
}
