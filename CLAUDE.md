# Operator Jack — CLAUDE.md

> Instructions for AI assistants working on this project.
> Read `PROJECT_CONTEXT.md` first for state tracking. This file covers conventions and build instructions.
> The authoritative forward roadmap (M7-M15) is `MASTER_PLAN.md` at the repo root.

## Project Overview

macOS-first CLI tool for deterministic computer automation. Rust core + Swift helper for accessibility.

**Repo:** `/Users/kirankumaralugonda/operator-jack/`
**Binary:** `operator-jack` (installed to PATH or run from `target/debug/operator-jack`)
**Architecture:** Plan JSON → Validation → Policy Gate → Typed Step Executors → Audit Logs

## Quick Start

```bash
# Set PATH (Rust installed via brew, not rustup)
export PATH="/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin:$PATH"

# Build Rust workspace
cd /Users/kirankumaralugonda/operator-jack && cargo build

# Run Rust tests (163 tests)
cargo test

# Build Swift helper and run Swift tests (10 tests)
cd macos-helper && swift build && swift test

# Run the CLI
./target/debug/operator-jack doctor
./target/debug/operator-jack run --plan-file docs/examples/calculator-buttons.json --mode unsafe --yes
```

## Workspace Structure

```
operator-jack/
├── crates/
│   ├── operator-cli/        # CLI binary (clap), command dispatch
│   ├── operator-core/       # Types, validation, interpolation, redaction, policy, selectors
│   ├── operator-runtime/    # Execution engine, policy gates, retry/timeout, disambiguation, browser lane bridge (M12: browser_lane)
│   ├── operator-store/      # SQLite persistence (rusqlite + rusqlite_migration)
│   ├── operator-exec-system/# sys.* step handlers (file ops, exec, clipboard)
│   ├── operator-exec-browser/# browser.* CDP executor - tokio-tungstenite + reqwest (M5; wired into plans/agent/MCP in M12 via operator-runtime browser_lane)
│   ├── operator-ipc/        # NDJSON IPC client for Swift helper
│   ├── operator-planner/    # LLM planner (M4) - Kimi/OpenAI/Anthropic/Ollama providers
│   ├── operator-skills/     # Skill manifests (M6) - parse/validate/expand to plans
│   ├── operator-mcp/        # MCP server (M9) - JSON-RPC over stdio, tools → RunSession
│   └── operator-agent/      # Native agent loop (M10) - observe→propose→execute→re-observe on RunSession
├── macos-helper/            # Swift SPM package
│   ├── Sources/OperatorMacOSHelper/
│   │   ├── main.swift       # NDJSON server loop, handler registration
│   │   ├── IpcProtocol.swift # Request/response types, JSONValue enum
│   │   ├── MethodDispatcher.swift # Method routing, HelperError type
│   │   ├── AXUtilities.swift # Shared AX foundation (selectors, traversal, waits, disambiguation)
│   │   └── Methods/         # One file per handler (FocusApp, Find, Click, etc.)
│   └── Tests/OperatorMacOSHelperTests/ # Swift test target (10 tests)
├── skills/                  # Built-in skill manifests (M6): open-and-type, create-note, take-screenshot
├── docs/
│   ├── examples/            # Golden plan JSONs for acceptance testing
│   └── *.md                 # Architecture, security, selectors, permissions, roadmap
├── MASTER_PLAN.md          # Authoritative forward roadmap (M7-M15)
├── SPEC_FREEZE_V0.6.md     # M5 spec (browser executor / CDP)
├── SPEC_FREEZE_V0.5.md     # M6 spec (skills; written retroactively)
├── SPEC_FREEZE_V0.4.md     # M4 spec (LLM planner)
├── SPEC_FREEZE_V0.3.md     # M3 spec (UI executor v1)
├── SPEC_FREEZE_V0.2.md     # Base normative spec (M0-M3)
├── PROJECT_CONTEXT.md       # State tracker, version log, resume file
└── CLAUDE.md                # This file
```

## Key Conventions

### Naming
- Rust step types: snake_case (`ui.focus_app`)
- Swift IPC methods: camelCase (`ui.focusApp`)
- Translation handled in `operator-ipc/src/client.rs` → `translate_method_name()`
- IDs: ULIDs everywhere (plan_id, run_id, step_result_id, event_id, IPC message id)

### Execution Lanes
- **System lane** (`sys.*`): Rust-native, runs on worker thread with timeout
- **UI lane** (`ui.*`): Dispatched to Swift helper via NDJSON IPC. Includes `ui.scroll` (M13) — the first scroll verb; the Swift handler posts scroll-wheel CGEvents (params `app`, `dy`, `dx`, `clicks`, `to: "top"|"bottom"`).
- **Data/network** (`sys.list_dir`, `data.parse_csv`, `data.parse_json`, `http.request`): Rust-native on the system lane (M13). `sys.list_dir {path, pattern?}` (glob + metadata, name-sorted); `data.parse_csv {text|path, headers?}` (RFC-4180-ish — quoted fields, embedded commas, `""` escapes → row objects); `data.parse_json {text|path, pointer?}` (RFC-6901 JSON pointer); `http.request {url, method?, headers?, body?}` (blocking, 30s timeout, 1 MiB body cap; gated by the domain allow-list AND grants, like `sys.open_url`).
- **Browser lane** (`browser.*`): CDP executor (M5) now wired into plans, the agent loop, and MCP as of M12. The lane launches its OWN isolated Chrome via a per-run `BrowserHandle` (lazily spawned on the first browser step, torn down at run end) with a dedicated `--user-data-dir` — ephemeral per-run by default (removed on close) or a named persistent profile — never the user's real logged-in profile; it refuses to attach to a Chrome it didn't launch unless the escalating `browser.real_profile` capability is granted. Headless by default (headed available for human takeover). The sync engine talks to the async CDP client over std channels via `operator-runtime::browser_lane` (a dedicated OS thread running its own multi-threaded tokio runtime), so it is safe from the sync CLI and from within any other runtime (no nested-runtime panic).
- **Control lane** (`control.foreach`/`control.if`/`control.repeat_until`, `ui.human_takeover`): Rust-native, not dispatched to any helper. **Deterministic control flow** (M13): bodies carry sub-steps inside `params` (the flat plan schema is unchanged); conditions are `{left, op, right}` (ops `== != < <= > >= contains exists empty`, numeric or string compare); iteration caps are mandatory (100k hard ceiling; `repeat_until` requires `max_iterations`) and nesting is bounded at 16; variables `${var}` interpolate type-preserving, `save_as` binds a leaf's output, `foreach` binds the loop var + `__index`. The interpreter (`operator-runtime::control`) runs each leaf through `RunSession::execute_step` (same gate / `{{secret:*}}` resolution / redaction / audit; each iteration independently audited); `operator-jack run`/`exec` auto-detect control-flow plans and route them through the interpreter over a RunSession (flat plans keep the existing engine path). Control-flow plans are non-interactive for confirmations (they need `--yes` or a grant for safe-mode medium/high steps). **`ui.human_takeover`** (M11): an interactive run blocks for Enter; a non-interactive run pauses (checkpoint + approval) for out-of-band resume; the agent loop can emit it. Risk Low.

### Error Handling
- Rust: `thiserror` for library crates, `anyhow` for CLI
- Swift: `HelperError` struct with `code`, `message`, `retryable`, `details`
- Error codes: `APP_NOT_FOUND`, `APP_NOT_RUNNING`, `WINDOW_NOT_FOUND`, `ELEMENT_NOT_FOUND`, `ELEMENT_AMBIGUOUS`, `ELEMENT_NOT_ACTIONABLE`, `PERMISSION_DENIED`, `TIMEOUT`, `INPUT_BLOCKED`

### Policy & Risk
- Two modes: `safe` (confirm medium+high risk) and `unsafe` (allow all)
- Risk levels: Low (read-only), Medium (state changes), High (filesystem/exec)
- Redaction applied before logging/storage (Section 18 of spec)

### Swift Helper Patterns
- All handlers have signature: `([String: JSONValue]) throws -> [String: JSONValue]`
- Handlers registered in `main.swift` via `dispatcher.register("ui.methodName", handler: handleMethodName)`
- Common extraction: `extractApp(from:)`, `extractImplicitWaitMs(from:)`, `SelectorParams.parse(from:)`
- Selector matching: DFS traversal via `findElements(root:selector:maxDepth:maxResults:)`
- Implicit waits: `implicitWaitForElement()` polls at 200ms up to `waitMs` (default 2000)
- Disambiguation: `disambiguate()` returns single match or throws `ELEMENT_AMBIGUOUS` with candidates
- Evidence: `gatherEvidence()` returns `_evidence` key on action handler outputs
- Element refs: `ElementRefStore.shared` caches AXUIElements with ULID keys

### Selector System
- Fields: `role`, `subrole`, `name`, `name_contains`, `description`, `description_contains`, `value`, `value_contains`, `identifier`, `path`, `index`, `max_depth`
- Window scoping: `window: { index: N }` or `window: { title_contains: "..." }`
- `any_of`: Array of alternative selectors, first returning exactly 1 match wins
- Suffix convention: `name` = exact match, `name_contains` = substring (case-insensitive)
- `path` is a real matcher: suffix match on the computed traversal path (implemented in M7; was rejected-when-alone in Phase B)

### Testing
- Rust: `cargo test` — unit tests in each crate's source files (163 tests)
- Swift: `cd macos-helper && swift test` — XCTest target at `Tests/OperatorMacOSHelperTests/` (10 tests; selector parsing + criteria). AX-dependent behavior is covered by the live acceptance harness.
- Acceptance: `make check-examples` validates every golden plan in `docs/examples/` and dry-run-expands every skill. **No example or skill ships without passing it.** Run a plan live via `operator-jack run --plan-file <path> --mode unsafe --yes`.

## Spec Files

- **MASTER_PLAN.md** — Authoritative forward roadmap (M7-M15). Read this for what's next.
- **SPEC_FREEZE_V0.9.md** — M9 delta: MCP server mode (`RunSession` ad-hoc gated single-step execution, `mcp serve`, 15-tool surface over JSON-RPC-over-stdio)
- **SPEC_FREEZE_V0.8.md** — M8 delta: safety envelope (task-scoped grants, run budgets + `Partial` status, always-escalate classifier, redaction hardening)
- **SPEC_FREEZE_V0.7.md** — M7 delta: observation API (`ui.snapshot`/`ui.screenshot`, element refs, Electron/Chromium AX enablement, `path` selector matching)
- **SPEC_FREEZE_V0.6.md** — M5 delta: browser lane (`browser.*` step types, CDP, tokio, Chrome process management, `allow_domains` for browser)
- **SPEC_FREEZE_V0.5.md** — M6 skills spec (written retroactively): manifest schema, parameters, expansion to plans
- **SPEC_FREEZE_V0.4.md** — M4 delta: LLM planner, `do` command, provider configuration
- **SPEC_FREEZE_V0.3.md** — Delta: new step types (list_windows, focus_window), window scoping, anyOf, element_ref, error taxonomy, evidence hooks, `operator-jack ui inspect`
- **SPEC_FREEZE_V0.2.md** — Base spec: all step types, validation rules, policy, IPC protocol, CLI commands
- Never edit frozen specs. Create new `SPEC_FREEZE_V0.x.md` for changes.

## Milestone Status

| Milestone | Status | Key Deliverables |
|-----------|--------|-----------------|
| M0 | DONE | Scaffolding, types, store, CLI skeleton, stub executors |
| M1 | DONE | All 13 sys.* executors, 21 tests |
| M2 | DONE | Swift helper v1, NDJSON IPC, ping/accessibility/listApps |
| M3a | DONE | 9 UI handlers (focus, find, wait, click, type, read, keyPress), disambiguation, 85 tests |
| M3b | DONE | selectMenu, setValue, inspect, anyOf, element_ref, evidence hooks, 91 tests |
| M4 | DONE | LLM planner (natural language → typed steps; Kimi/OpenAI/Anthropic/Ollama) |
| M5 | DONE | Browser executor (CDP) — `browser` CLI subcommands; plan/agent/MCP-lane wiring landed in M12 |
| M6 | DONE | Skills system (manifest → plan expansion; skill list/validate/run) |
| Phase B | DONE | Stabilization (v0.7.1): audit fixes, Swift correctness, security fixes, docs |
| M7 | DONE | Perception: ui.snapshot (compact AX + refs), ui.screenshot, Electron AX enablement, path selectors, doctor screen-recording check |
| M8 | DONE | Safety envelope: task-scoped grants (fs/exec/domains), run budgets + Partial status, always-escalate classifier, secure-field refusal, redaction hardening, exit code 3, Keychain secret store (`{{secret:name}}` injection, `secret` CLI) |
| M9 | DONE | MCP server mode: RunSession (ad-hoc gated single-step exec), operator-mcp crate (JSON-RPC-over-stdio, 15 tools), `operator-jack mcp serve --grants`, session-as-run audit |
| M10 | DONE | Native agent loop: operator-agent crate (observe→propose one JSON action→execute via RunSession→re-observe), provider `chat()`, `operator-jack agent` CLI |
| M11 | DONE | Long-horizon & supervision (pause/resume, approvals, human_takeover, steer, logs --follow/prune) |
| M12 | DONE | Web autonomy: browser lane wired into plans/agent/MCP, profile isolation, extraction verbs |
| M13 | DONE | Data/net steps (list_dir, parse_csv/json, http.request), ui.scroll, deterministic control flow (foreach/if/repeat_until) |
| M14-M15 | NOT STARTED | New roadmap in MASTER_PLAN.md (M14=native macOS lane, M15=trust/residency) |

> M13 note: only `ui.scroll` of the planned actuation verbs landed — right-click/double-click/hover/drag, coordinate clicks/scroll, typed `set_value`, window management, and Open/Save dialog helpers are deferred (see MASTER_PLAN.md / PROJECT_CONTEXT.md).

## Important Rules

1. **Spec is normative.** Check the relevant SPEC_FREEZE (V0.2 base; V0.3 UI; V0.4 planner; V0.5 skills; V0.6 browser; V0.7 observation; V0.8 safety envelope; V0.9 MCP) before inventing behavior; check MASTER_PLAN.md for anything M10+.
2. **Async allowed (M5+).** tokio is now a dependency (`operator-cli`, `operator-exec-browser`) for the browser/CDP lane. The old "no async / no tokio" rule is obsolete.
3. **Every change must compile.** Run `cargo build && cargo test && cd macos-helper && swift build`.
4. **Update PROJECT_CONTEXT.md** after completing milestones.
5. **Redact before logging.** Apply redaction rules from spec Section 18.
6. **ULIDs everywhere.** For all IDs across the system.
7. **Two-mode policy.** Safe (confirm medium+high) / Unsafe (allow all). Not three-tier. M8 adds task-scoped grants + run budgets + an always-escalate classifier on top (see SPEC_FREEZE_V0.8.md).
8. **Raw ApplicationServices.** No AXSwift. Direct AXUIElement* API calls.

## Current Handler Registry (19 handlers)

| IPC Method | Handler | File |
|-----------|---------|------|
| ui.ping | handlePing | Methods/Ping.swift |
| ui.checkAccessibilityPermission | handleCheckAccessibility | Methods/CheckAccessibility.swift |
| ui.listApps | handleListApps | Methods/ListApps.swift |
| ui.focusApp | handleFocusApp | Methods/FocusApp.swift |
| ui.listWindows | handleListWindows | Methods/ListWindows.swift |
| ui.focusWindow | handleFocusWindow | Methods/FocusWindow.swift |
| ui.find | handleFind | Methods/Find.swift |
| ui.waitFor | handleWaitFor | Methods/WaitFor.swift |
| ui.click | handleClick | Methods/Click.swift |
| ui.typeText | handleTypeText | Methods/TypeText.swift |
| ui.readText | handleReadText | Methods/ReadText.swift |
| ui.keyPress | handleKeyPress | Methods/KeyPress.swift |
| ui.scroll | handleScroll | Methods/Scroll.swift |
| ui.selectMenu | handleSelectMenu | Methods/SelectMenu.swift |
| ui.setValue | handleSetValue | Methods/SetValue.swift |
| ui.inspect | handleInspect | Methods/Inspect.swift |
| ui.snapshot | handleSnapshot | Methods/Snapshot.swift |
| ui.screenshot | handleScreenshot | Methods/Screenshot.swift |
| ui.checkScreenRecordingPermission | handleCheckScreenRecording | Methods/Screenshot.swift |

## CLI Commands

### Natural Language (M4)
```
operator-jack do "open Notes and type hello"      # Natural language execution
operator-jack do "..." --provider kimi            # Use specific LLM provider
operator-jack do "..." --show-plan                # Preview generated plan
operator-jack do "..." --save-plan plan.json      # Save generated plan
```

### Plan-based Execution
```
operator-jack doctor                    # Environment health check
operator-jack run --plan-file <path>    # Validate + execute a plan
operator-jack run 'json:{...}'         # Inline plan execution
operator-jack run --resume <run-id>     # Resume a paused run (approval required; skips completed steps)
operator-jack exec <plan-id>           # Execute a saved plan
operator-jack plan validate --plan-file <path>
operator-jack plan save --plan-file <path>
operator-jack logs [run-id] [--full]
operator-jack logs --follow [run-id]    # Tail a run's JSONL live (also: logs follow)
operator-jack logs prune [--older-than-days N] [--keep M]   # Delete old runs (cascades; also: logs --prune)
operator-jack stop
operator-jack ui inspect --app <name> [--depth N]
operator-jack init                      # Create config file
```

### Skills (M6)
```
operator-jack skill list                          # List discovered skills
operator-jack skill validate <skill-file>         # Validate a skill manifest
operator-jack skill run <name> --param key=value  # Expand skill to a plan and run it
```

### Browser (M5 CLI subcommands + M12 plan/agent/MCP lane)
```
operator-jack browser doctor                      # Check Chrome availability
operator-jack browser launch [--port 9222]        # Launch Chrome with remote debugging
operator-jack browser navigate <url> [--port N]   # Navigate to a URL via CDP
operator-jack browser screenshot --output <path> [--full-page] [--port N]
```
As of M12, `browser.*` steps also run inside plans, the agent loop, and MCP (through an isolated
per-run Chrome — see Execution Lanes), routed through policy/redaction/audit like every other lane.
Step types: the M5 verbs `browser.navigate`, `browser.click`, `browser.type`, `browser.get_text`,
`browser.get_attribute`, `browser.wait_for`, `browser.screenshot`, `browser.scroll`, `browser.execute_js`,
plus the M12 additions `browser.get_text_all` (querySelectorAll → array), `browser.get_attributes_all`
(attribute from all matches → array; href/src resolved to absolute), `browser.snapshot` (compact
interactive-element list with CSS-selector refs — the browser analogue of `ui.snapshot`), and
`browser.download` (`Page.setDownloadBehavior` + wait-for-complete → file path). Selectors on
get_text/get_attribute/click/type auto-wait up to 5s; navigation waits on `document.readyState`.
Examples: `docs/examples/browser-navigate.json`, `docs/examples/browser-scrape.json`.

### Secrets (M8)
```
operator-jack secret set <name> [--value V]   # Store in keychain (value from stdin if omitted)
operator-jack secret list                      # List secret names (never values)
operator-jack secret rm <name>                 # Remove a secret
```
Reference a secret in any plan/skill param as `{{secret:name}}`; the engine
resolves it at execution time and never persists the literal value.

### MCP Server (M9)
```
operator-jack mcp serve [--grants <file>]   # Serve MCP over stdio (connect an MCP client)
```
Exposes 27 gated tools (snapshot, screenshot, click, type_text, key_press, set_value,
select_menu, read_text, wait_for, list_apps, focus_app, open_app, read_file, write_file,
exec, the M12 browser tools browser_navigate, browser_snapshot, browser_get_text,
browser_get_text_all, browser_get_attributes_all, browser_click, browser_type, browser_download,
plus the M13 data/network tools list_dir, parse_csv, parse_json, http_request)
over newline-delimited JSON-RPC 2.0. Each tool call runs as one `RunSession` step —
same policy gate, escalation classifier, `{{secret:*}}` resolution, redaction, and audit
as plan execution. The effective grant envelope is printed to STDERR before serving
(stdout is protocol-only); denied calls return `isError` and the session continues.

### Agent (M10)
```
operator-jack agent "<goal>" [--max-iters N] [--provider P] [--grants <file>]
operator-jack agent --resume <run-id>            # Resume a paused agent run (approval required)
```
Runs the native autonomous agent loop: observe (`ui.snapshot`) → the model proposes ONE
JSON action → execute it through a `RunSession` (same policy gate, grants, escalation
classifier, `{{secret:*}}` resolution, redaction, and audit as plan execution) → re-observe.
Repeats until the model emits `done`, `--max-iters` is hit, or the provider errors.
Action vocabulary: open_app, focus_app, snapshot, click, type_text, key_press, set_value,
select_menu, read_text, wait_for, read_file, write_file, exec, plus the M12 browser actions
browser_navigate, browser_snapshot, browser_get_text, browser_get_text_all, browser_get_attributes_all,
browser_click, browser_type, browser_wait_for, browser_screenshot, browser_download, plus the M13 data/network actions
list_dir, parse_csv, parse_json, http_request. Autonomous and non-interactive
(auto-approves confirmations; safety comes from the grant envelope + escalation classifier +
secure-field refusal). Prints the effective grant envelope first; the whole run is one
session-run in the store and every executed action is audited.

### Supervision (M11)
```
operator-jack approvals list [--all]         # List pending approvals (or all with --all)
operator-jack approvals approve <id>         # Approve a paused run's escalation/takeover
operator-jack approvals deny <id>            # Deny it
operator-jack steer <run-id> "<message>"     # Inject guidance into a running agent's next turn
```
When the always-escalate classifier flags an action (or a plan/agent hits `ui.human_takeover`),
the run checkpoints, opens a pending approval, posts a macOS notification, and ends `Paused`
(exit code 0) instead of hard-failing. Approve it, then `run --resume` / `agent --resume` the
run-id to continue (resume verifies the approval is granted and consumes a one-shot escalation
allowance). `logs prune` bounds store growth by deleting old runs and cascading to their
step_results/events/checkpoints/approvals.

## Common Flags

```
--mode safe|unsafe    # Execution mode (default: safe)
--yes                 # Auto-approve all policy gates
--dry-run             # Simulate without side effects
--json                # JSON output format
--helper-path <path>  # Custom helper binary path
--allow-apps <list>   # Comma-separated app allowlist
--allow-domains <list># Comma-separated domain allowlist
--interactive         # Force interactive prompts
--no-interactive      # Disable interactive prompts
-v / -q               # Verbose / quiet
```
