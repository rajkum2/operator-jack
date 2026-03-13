# Operator CLI Spec Freeze v0.6

Status: Frozen for implementation  
Version: `0.7.0`  
Date: `2026-03-14`  
Predecessor: Spec Freeze v0.5  

This document is an **additive delta** to Spec Freeze v0.5. Everything in v0.5 remains normative unless explicitly overridden below. This document covers M5 (Browser Executor / CDP).

---

## 1) Scope Delta (v0.6 additions)

New in v0.6:
- Chrome DevTools Protocol (CDP) support for web automation
- New `browser.*` step types for browser interaction
- Async runtime (tokio) for browser operations
- Chrome process management (launch, connect, cleanup)
- `allow_domains` enforcement for browser navigation
- CLI commands for browser operations

---

## 2) New Step Types (Browser Lane)

All browser steps use the "browser" execution lane.

### `browser.navigate`

Navigate to a URL.

**Params:**
- `url` (string, required): URL to navigate to
- `wait_ms` (integer, optional): Milliseconds to wait after navigation (default: 1000)

**Risk level:** Medium

**allow_domains check:** Yes - navigation to non-allowed domains fails

### `browser.click`

Click an element by CSS selector.

**Params:**
- `selector` (string, required): CSS selector for the element

**Risk level:** Medium

### `browser.type`

Type text into an input element.

**Params:**
- `selector` (string, required): CSS selector for the input element
- `text` (string, required): Text to type

**Risk level:** Medium

### `browser.get_text`

Get text content of an element.

**Params:**
- `selector` (string, required): CSS selector for the element

**Return:** String containing the element's text content

**Risk level:** Low

### `browser.get_attribute`

Get an attribute value of an element.

**Params:**
- `selector` (string, required): CSS selector for the element
- `attribute` (string, required): Attribute name

**Return:** String containing the attribute value

**Risk level:** Low

### `browser.execute_js`

Execute JavaScript in the page context.

**Params:**
- `script` (string, required): JavaScript code to execute

**Return:** Result of the JavaScript execution (JSON-serializable)

**Risk level:** High (arbitrary code execution)

### `browser.screenshot`

Take a screenshot of the page.

**Params:**
- `full_page` (boolean, optional): Capture full page or viewport only (default: false)

**Return:** Object with:
- `format`: "png"
- `full_page`: Boolean
- `data`: Base64-encoded PNG image
- `size_bytes`: Image size in bytes

**Risk level:** Low

### `browser.wait_for`

Wait for an element to appear in the DOM.

**Params:**
- `selector` (string, required): CSS selector to wait for
- `timeout_ms` (integer, optional): Maximum wait time (default: 10000)

**Return:** Object with `found: true`

**Risk level:** Low

### `browser.scroll`

Scroll the page to a position.

**Params:**
- `x` (number, optional): Horizontal scroll position (default: 0)
- `y` (number, optional): Vertical scroll position (default: 0)

**Return:** Object with `x` and `y` scroll positions

**Risk level:** Low

---

## 3) Architecture

### New Crate: `operator-exec-browser`

Async crate for browser automation:

```
crates/operator-exec-browser/
  src/
    lib.rs           # Public exports
    error.rs         # BrowserError type
    cdp.rs           # CDP protocol types
    client.rs        # WebSocket CDP client
    executor.rs      # BrowserExecutor for step handling
```

### Dependencies

- `tokio` - Async runtime
- `tokio-tungstenite` - WebSocket client
- `futures-util` - Async utilities
- `reqwest` - HTTP client for Chrome discovery
- `base64` - Screenshot encoding
- `url` - URL parsing for domain checks

### Chrome Connection

1. Check for existing Chrome at `http://127.0.0.1:9222`
2. If not running, launch Chrome with `--remote-debugging-port=9222`
3. Connect via WebSocket using CDP
4. Enable required domains: Page, Runtime, DOM

### Domain Allowlist

Browser navigation enforces `allow_domains` from the plan:

```rust
fn is_url_allowed(&self, url: &str) -> bool {
    if self.allow_domains.is_empty() {
        return true;
    }
    // Parse domain and check against allowlist
}
```

---

## 4) CLI Commands

### `operator-jack browser doctor`

Check if Chrome is available on the system.

### `operator-jack browser launch --port 9222`

Launch Chrome with remote debugging enabled.

### `operator-jack browser navigate <url> --port 9222`

Navigate to a URL in the connected Chrome instance.

### `operator-jack browser screenshot --output screenshot.png --full-page --port 9222`

Take a screenshot and save to file.

---

## 5) Example Plan

```json
{
  "schema_version": 1,
  "name": "Web automation example",
  "description": "Navigate and interact with a website",
  "mode": "safe",
  "allow_domains": ["example.com"],
  "steps": [
    {
      "id": "navigate",
      "type": "browser.navigate",
      "params": {
        "url": "https://example.com",
        "wait_ms": 2000
      }
    },
    {
      "id": "click_button",
      "type": "browser.click",
      "params": {
        "selector": "button#submit"
      }
    },
    {
      "id": "take_screenshot",
      "type": "browser.screenshot",
      "params": {
        "full_page": true
      }
    }
  ]
}
```

---

## 6) Security Considerations

### JavaScript Execution

`browser.execute_js` is classified as **High Risk** because it allows arbitrary code execution in the browser context.

### Domain Restrictions

The `allow_domains` plan field is enforced for:
- `browser.navigate` - blocks navigation to non-allowed domains
- Implicitly protects against navigating to malicious sites

### Chrome Isolation

- Chrome is launched with minimal flags
- Extensions are disabled
- Default to headless mode for automation

---

## 7) Error Handling

### BrowserError Types

| Error | Description | Retryable |
|-------|-------------|-----------|
| ChromeNotFound | Chrome not installed | No |
| ConnectionError | WebSocket connection failed | Yes |
| CdpError | CDP protocol error | No |
| NavigationError | Page navigation failed | Yes |
| ElementNotFound | Selector didn't match | No |
| DomainNotAllowed | URL not in allowlist | No |
| JavaScriptError | JS execution failed | No |
| Timeout | Operation timed out | Yes |

---

## 8) Testing

Browser executor tests:
- Mock CDP server for unit tests
- Integration tests with real Chrome (optional)
- Domain allowlist validation tests

---

*End of Spec Freeze v0.6*
