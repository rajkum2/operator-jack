# Operator CLI Spec Freeze v0.9

Status: Frozen for implementation
Version: `0.10.0`
Date: `2026-07-09`
Predecessor: Spec Freeze v0.8

Additive delta to v0.8. Everything prior remains normative. Covers **M9 — MCP
server mode**: exposing Operator Jack's gated executors as MCP tools so any MCP
client (Claude Code/Desktop, others) can drive the audited hands — "brain
rented, hands owned". This same tool surface is reused by M10's native loop.

---

## 1) RunSession (shared execution substrate)

`operator_runtime::engine::RunSession` executes steps **one at a time** (ad hoc),
as opposed to `Engine::execute_plan` (a fixed plan). It is the substrate for MCP
mode (M9) and the agent loop (M10).

- `RunSession::open(store, config, session_grants, session_budget)` — persists a
  synthetic session plan (so the run has a plan row recording the granted
  envelope), creates one run, and builds the policy gate from config + session
  grants.
- `execute_step(&Step) -> Result<Value, SessionError>` — runs one step through
  the **same** policy gate (grants, escalation, allow-lists), `{{secret:*}}`
  resolution, store-boundary redaction, and dispatch as plan execution. Returns
  the step output, or `SessionError::Denied` (policy) vs `SessionError::Failed`
  (execution). No retries — the driver decides.
- `finish() -> RunSummary` — marks the run terminal and disconnects the helper.

The session holds a **persistent helper connection**, so element refs minted by
`ui.snapshot` survive across calls (snapshot → ref → click across tool calls),
and a **single run**, so every step is audited under one session.

`execute_plan` is unchanged; both paths share the security-critical helpers.

---

## 2) `operator-jack mcp serve`

Serves the MCP protocol on **stdio** (newline-delimited JSON-RPC 2.0). Methods:
`initialize` (echoes the client's `protocolVersion`; advertises `tools`
capability + serverInfo), `tools/list`, `tools/call`, `ping`. Notifications
(no id, e.g. `notifications/initialized`) get no response. Diagnostics and the
grant preview go to **stderr**; stdout is protocol-only.

Implementation note: a minimal hand-rolled JSON-RPC-over-stdio server (the
`operator-mcp` crate), chosen because `RunSession` is synchronous; the `rmcp`
SDK may be adopted later without changing the tool surface.

### Session safety posture
stdio is the protocol channel, so there is no interactive terminal for
confirmation prompts. The session therefore auto-approves confirmations
(`yes_to_all`) and relies on the **grant envelope + escalation classifier +
secure-field refusal** (M8) for safety — never on per-step prompts. Grants are
supplied at serve time via `--grants <file>` (a JSON `Grants` object) or config;
the server prints the effective envelope ("this session may: …") to stderr
before serving, and warns loudly if the session is unrestricted.

---

## 3) Tool surface

Each tool maps to a step type; the tool's `arguments` become the step's params,
validated against the step type before execution. `tools/call` returns MCP
content (`{ "content": [{ "type": "text", "text": <output JSON> }], "isError" }`).
A policy denial or execution failure returns `isError: true` with the reason —
**not** a JSON-RPC error — so the model sees it and the session continues.

UI lane: `snapshot`, `screenshot`, `click`, `type_text`, `key_press`,
`set_value`, `select_menu`, `read_text`, `wait_for`, `list_apps`, `focus_app`.
System lane (grant-gated): `open_app`, `read_file`, `write_file`, `exec`.

`click`/`read_text`/`set_value` accept either a `selector` or an `element_ref`
from a prior `snapshot`. (Browser tools arrive with the M12 browser-lane wiring;
`list_dir` / `data.*` with M13.)

---

## 4) Acceptance

- An MCP client `initialize`s, `tools/list` returns the tool set, and
  `tools/call` executes end-to-end, each call audited under one session run.
- A `tools/call` outside the session's grant returns `isError: true` naming the
  grant, and the session continues serving. *(Verified live via scripted stdio:
  `write_file` inside a `fs_write` grant succeeded; outside was denied with no
  file created; 15 tools listed.)*
- Element refs from `snapshot` remain valid for a subsequent `click` in the same
  session (persistent helper).
