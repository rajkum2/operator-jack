//! Minimal, spec-compliant MCP server over stdio for Operator Jack (M9).
//!
//! Exposes Operator Jack's executors as MCP tools so any MCP client (Claude
//! Code/Desktop, etc.) can drive the **gated, audited** hands — "brain rented,
//! hands owned". Every `tools/call` runs through a [`RunSession`], so the same
//! policy gate (grants, escalation), secret resolution, redaction, and audit
//! that govern plan execution apply identically to model-driven actions, and
//! the whole session is one run in the store.
//!
//! This is a hand-rolled synchronous JSON-RPC-over-stdio implementation (MCP's
//! stdio transport is newline-delimited JSON-RPC 2.0), chosen because
//! `RunSession` is synchronous (rusqlite + child-process pipes) and a blocking
//! request/response loop is the natural fit. It implements `initialize`,
//! `tools/list`, `tools/call`, and `ping`. (Adopting the `rmcp` SDK later is
//! possible without changing the tool surface.)

use std::io::{BufRead, Write};

use operator_core::types::{Step, StepType};
use operator_core::validation::validate_step_params;
use operator_runtime::engine::{RunSession, SessionError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// MCP protocol version this server implements. We echo the client's requested
/// version when present (best-effort compat), else advertise this.
const PROTOCOL_VERSION: &str = "2025-06-18";

// ---------------------------------------------------------------------------
// JSON-RPC framing
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RpcRequest {
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct RpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Serialize)]
struct RpcError {
    code: i64,
    message: String,
}

impl RpcResponse {
    fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }
    fn err(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Tool registry
// ---------------------------------------------------------------------------

/// An exposed MCP tool: its name, the step type it maps to, a description, and
/// the JSON Schema for its arguments (which become the step's params).
struct ToolDef {
    name: &'static str,
    step_type: StepType,
    description: &'static str,
    schema: fn() -> Value,
}

fn app_prop() -> Value {
    json!({ "type": "string", "description": "Target application name (e.g. \"Calculator\", \"Notes\")." })
}
fn selector_prop() -> Value {
    json!({
        "type": "object",
        "description": "Element selector. Use fields like role, name, name_contains, identifier, value, index, or an element ref from a prior snapshot via element_ref instead.",
    })
}

fn tools() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "snapshot",
            step_type: StepType::UiSnapshot,
            description: "Capture a compact accessibility snapshot of an app's UI with actionable element refs (e1, e2, …). Call this before clicking/typing to see the current UI.",
            schema: || json!({
                "type": "object",
                "properties": {
                    "app": app_prop(),
                    "interactive_only": {"type": "boolean", "description": "Only actionable/labeled elements (default true)."},
                    "max_depth": {"type": "integer"}
                },
                "required": ["app"]
            }),
        },
        ToolDef {
            name: "screenshot",
            step_type: StepType::UiScreenshot,
            description: "Capture a downscaled PNG of an app window or the screen; returns the file path (requires Screen Recording permission).",
            schema: || json!({"type": "object", "properties": {"app": app_prop(), "max_width": {"type": "integer"}}}),
        },
        ToolDef {
            name: "click",
            step_type: StepType::UiClick,
            description: "Click a UI element, identified by a selector or an element_ref from a prior snapshot.",
            schema: || json!({
                "type": "object",
                "properties": {"app": app_prop(), "selector": selector_prop(), "element_ref": {"type": "string"}},
                "required": ["app"]
            }),
        },
        ToolDef {
            name: "type_text",
            step_type: StepType::UiTypeText,
            description: "Type text into the focused element (or an element matched by selector). Refuses secure/password fields; use a {{secret:name}} reference for credentials.",
            schema: || json!({
                "type": "object",
                "properties": {"app": app_prop(), "text": {"type": "string"}, "selector": selector_prop()},
                "required": ["app", "text"]
            }),
        },
        ToolDef {
            name: "key_press",
            step_type: StepType::UiKeyPress,
            description: "Press a key with optional modifiers (e.g. key=\"Return\", or key=\"a\" modifiers=[\"command\"]).",
            schema: || json!({
                "type": "object",
                "properties": {"app": app_prop(), "key": {"type": "string"}, "modifiers": {"type": "array", "items": {"type": "string"}}},
                "required": ["app", "key"]
            }),
        },
        ToolDef {
            name: "set_value",
            step_type: StepType::UiSetValue,
            description: "Set a UI element's value directly (text fields). Refuses secure fields.",
            schema: || json!({
                "type": "object",
                "properties": {"app": app_prop(), "selector": selector_prop(), "element_ref": {"type": "string"}, "value": {"type": "string"}},
                "required": ["app", "value"]
            }),
        },
        ToolDef {
            name: "select_menu",
            step_type: StepType::UiSelectMenu,
            description: "Select an app menu item by path, e.g. menu_path=[\"File\", \"New Note\"].",
            schema: || json!({
                "type": "object",
                "properties": {"app": app_prop(), "menu_path": {"type": "array", "items": {"type": "string"}}},
                "required": ["app", "menu_path"]
            }),
        },
        ToolDef {
            name: "read_text",
            step_type: StepType::UiReadText,
            description: "Read the text/value of a UI element (by selector or element_ref).",
            schema: || json!({
                "type": "object",
                "properties": {"app": app_prop(), "selector": selector_prop(), "element_ref": {"type": "string"}},
                "required": ["app"]
            }),
        },
        ToolDef {
            name: "wait_for",
            step_type: StepType::UiWaitFor,
            description: "Wait until an element matching the selector appears (or a condition is met).",
            schema: || json!({
                "type": "object",
                "properties": {"app": app_prop(), "selector": selector_prop(), "timeout_ms": {"type": "integer"}},
                "required": ["app", "selector"]
            }),
        },
        ToolDef {
            name: "list_apps",
            step_type: StepType::UiListApps,
            description: "List running (regular) applications.",
            schema: || json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "focus_app",
            step_type: StepType::UiFocusApp,
            description: "Bring an application to the foreground.",
            schema: || json!({"type": "object", "properties": {"app": app_prop()}, "required": ["app"]}),
        },
        ToolDef {
            name: "open_app",
            step_type: StepType::SysOpenApp,
            description: "Launch (or activate) an application by name.",
            schema: || json!({"type": "object", "properties": {"app": app_prop()}, "required": ["app"]}),
        },
        ToolDef {
            name: "read_file",
            step_type: StepType::SysReadFile,
            description: "Read a text file (subject to the session's fs_read grant).",
            schema: || json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
        },
        ToolDef {
            name: "write_file",
            step_type: StepType::SysWriteFile,
            description: "Write a text file (subject to the session's fs_write grant).",
            schema: || json!({
                "type": "object",
                "properties": {"path": {"type": "string"}, "content": {"type": "string"}, "create_parent": {"type": "boolean"}},
                "required": ["path", "content"]
            }),
        },
        ToolDef {
            name: "exec",
            step_type: StepType::SysExec,
            description: "Run a command (no shell; subject to the session's exec grant and the escalation classifier).",
            schema: || json!({
                "type": "object",
                "properties": {"command": {"type": "string"}, "args": {"type": "array", "items": {"type": "string"}}},
                "required": ["command"]
            }),
        },
        // Browser lane (M12) — an isolated Chrome for web scraping/automation.
        ToolDef {
            name: "browser_navigate",
            step_type: StepType::BrowserNavigate,
            description: "Open a URL in an isolated Chrome and wait for it to load.",
            schema: || json!({
                "type": "object",
                "properties": {"url": {"type": "string"}},
                "required": ["url"]
            }),
        },
        ToolDef {
            name: "browser_snapshot",
            step_type: StepType::BrowserSnapshot,
            description: "List the current page's links/buttons/inputs with CSS selectors (the browser analogue of ui.snapshot).",
            schema: || json!({"type": "object", "properties": {"max_elements": {"type": "integer"}}}),
        },
        ToolDef {
            name: "browser_get_text",
            step_type: StepType::BrowserGetText,
            description: "Get the text of the first element matching a CSS selector.",
            schema: || json!({
                "type": "object",
                "properties": {"selector": {"type": "string"}},
                "required": ["selector"]
            }),
        },
        ToolDef {
            name: "browser_get_text_all",
            step_type: StepType::BrowserGetTextAll,
            description: "Get the text of ALL elements matching a CSS selector as a JSON array (scrape a list).",
            schema: || json!({
                "type": "object",
                "properties": {"selector": {"type": "string"}},
                "required": ["selector"]
            }),
        },
        ToolDef {
            name: "browser_get_attributes_all",
            step_type: StepType::BrowserGetAttributesAll,
            description: "Get one attribute (e.g. href) from ALL matching elements as a JSON array.",
            schema: || json!({
                "type": "object",
                "properties": {"selector": {"type": "string"}, "attribute": {"type": "string"}},
                "required": ["selector", "attribute"]
            }),
        },
        ToolDef {
            name: "browser_click",
            step_type: StepType::BrowserClick,
            description: "Click the element matching a CSS selector (e.g. a pagination link).",
            schema: || json!({
                "type": "object",
                "properties": {"selector": {"type": "string"}},
                "required": ["selector"]
            }),
        },
        ToolDef {
            name: "browser_type",
            step_type: StepType::BrowserType,
            description: "Type text into an input matching a CSS selector.",
            schema: || json!({
                "type": "object",
                "properties": {"selector": {"type": "string"}, "text": {"type": "string"}},
                "required": ["selector", "text"]
            }),
        },
        ToolDef {
            name: "browser_download",
            step_type: StepType::BrowserDownload,
            description: "Download a file (by URL or by clicking a selector) and return its path.",
            schema: || json!({
                "type": "object",
                "properties": {"url": {"type": "string"}, "selector": {"type": "string"}, "dir": {"type": "string"}}
            }),
        },
    ]
}

fn tool_list_json() -> Value {
    let list: Vec<Value> = tools()
        .iter()
        .map(|t| json!({ "name": t.name, "description": t.description, "inputSchema": (t.schema)() }))
        .collect();
    json!({ "tools": list })
}

// ---------------------------------------------------------------------------
// Server loop
// ---------------------------------------------------------------------------

/// Runs the MCP server over stdio until stdin closes, executing every tool call
/// through `session`. Diagnostics go to stderr; stdout carries only protocol
/// messages. Finalizes the session on EOF.
pub fn serve(mut session: RunSession) -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    tracing::info!(run_id = %session.run_id(), "MCP server ready on stdio");

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let req: RpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                write_msg(
                    &mut out,
                    &RpcResponse::err(Value::Null, -32700, format!("parse error: {e}")),
                )?;
                continue;
            }
        };

        // Notifications (no id) get no response.
        let Some(id) = req.id.clone() else {
            continue;
        };

        let resp = handle(&mut session, id, &req.method, &req.params);
        write_msg(&mut out, &resp)?;
    }

    let _ = session.finish();
    Ok(())
}

fn write_msg(out: &mut std::io::Stdout, resp: &RpcResponse) -> std::io::Result<()> {
    let mut lock = out.lock();
    serde_json::to_writer(&mut lock, resp)?;
    lock.write_all(b"\n")?;
    lock.flush()
}

fn handle(session: &mut RunSession, id: Value, method: &str, params: &Value) -> RpcResponse {
    match method {
        "initialize" => {
            // Echo the client's protocol version if it sent one.
            let proto = params
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or(PROTOCOL_VERSION)
                .to_string();
            RpcResponse::ok(
                id,
                json!({
                    "protocolVersion": proto,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "operator-jack", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
        }
        "ping" => RpcResponse::ok(id, json!({})),
        "tools/list" => RpcResponse::ok(id, tool_list_json()),
        "tools/call" => call_tool(session, id, params),
        other => RpcResponse::err(id, -32601, format!("method not found: {other}")),
    }
}

fn call_tool(session: &mut RunSession, id: Value, params: &Value) -> RpcResponse {
    let name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return RpcResponse::err(id, -32602, "missing tool name"),
    };
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let defs = tools();
    let Some(def) = defs.iter().find(|t| t.name == name) else {
        return RpcResponse::err(id, -32602, format!("unknown tool: {name}"));
    };

    // Validate arguments against the step type up front so bad calls get a clean
    // error rather than failing deep in the executor.
    if let Err(msg) = validate_step_params(&def.step_type, &arguments) {
        return RpcResponse::ok(
            id,
            tool_error(format!("invalid arguments for '{name}': {msg}")),
        );
    }

    let step = Step {
        id: ulid::Ulid::new().to_string(),
        step_type: def.step_type.clone(),
        params: arguments,
        timeout_ms: None,
        retries: None,
        retry_backoff_ms: None,
        on_fail: None,
    };

    match session.execute_step(&step) {
        Ok(output) => {
            let text = serde_json::to_string_pretty(&output).unwrap_or_else(|_| output.to_string());
            RpcResponse::ok(
                id,
                json!({ "content": [{ "type": "text", "text": text }], "isError": false }),
            )
        }
        // A denial or failure is reported as a tool error (isError=true) so the
        // model sees it and the session continues — not a JSON-RPC error.
        Err(SessionError::Denied(msg)) => {
            RpcResponse::ok(id, tool_error(format!("denied by policy: {msg}")))
        }
        // MCP has no pause/resume loop; surface escalation and takeover as tool
        // errors so the client's model adapts (or asks its human) instead.
        Err(SessionError::Escalation(msg)) => RpcResponse::ok(
            id,
            tool_error(format!(
                "escalation required (a human must approve this action): {msg}"
            )),
        ),
        Err(SessionError::Takeover(msg)) => {
            RpcResponse::ok(id, tool_error(format!("human takeover required: {msg}")))
        }
        Err(SessionError::Failed(msg)) => RpcResponse::ok(id, tool_error(msg)),
    }
}

fn tool_error(msg: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": msg.into() }], "isError": true })
}
