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

# Run Rust tests (128 tests)
cargo test

# Build Swift helper and run Swift tests (9 tests)
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
│   ├── operator-runtime/    # Execution engine, policy gates, retry/timeout, disambiguation
│   ├── operator-store/      # SQLite persistence (rusqlite + rusqlite_migration)
│   ├── operator-exec-system/# sys.* step handlers (file ops, exec, clipboard)
│   ├── operator-exec-browser/# browser.* CDP executor (M5) - tokio-tungstenite + reqwest
│   ├── operator-ipc/        # NDJSON IPC client for Swift helper
│   ├── operator-planner/    # LLM planner (M4) - Kimi/OpenAI/Anthropic/Ollama providers
│   └── operator-skills/     # Skill manifests (M6) - parse/validate/expand to plans
├── macos-helper/            # Swift SPM package
│   ├── Sources/OperatorMacOSHelper/
│   │   ├── main.swift       # NDJSON server loop, handler registration
│   │   ├── IpcProtocol.swift # Request/response types, JSONValue enum
│   │   ├── MethodDispatcher.swift # Method routing, HelperError type
│   │   ├── AXUtilities.swift # Shared AX foundation (selectors, traversal, waits, disambiguation)
│   │   └── Methods/         # One file per handler (FocusApp, Find, Click, etc.)
│   └── Tests/OperatorMacOSHelperTests/ # Swift test target (9 tests)
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
- **UI lane** (`ui.*`): Dispatched to Swift helper via NDJSON IPC
- **Browser lane** (`browser.*`): Implemented (M5) via CDP, but reachable ONLY through the `browser` CLI subcommands (doctor/launch/navigate/screenshot). Browser steps in plans are NOT yet wired into the engine — a plan containing `browser.*` steps fails validation today (expected). Plan-lane wiring is milestone M12 in MASTER_PLAN.md.

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

### Testing
- Rust: `cargo test` — unit tests in each crate's source files (128 tests)
- Swift: `cd macos-helper && swift test` — XCTest target at `Tests/OperatorMacOSHelperTests/` (9 tests; selector parsing + criteria). AX-dependent behavior is covered by the live acceptance harness.
- Acceptance: `make check-examples` validates every golden plan in `docs/examples/` and dry-run-expands every skill. **No example or skill ships without passing it.** Run a plan live via `operator-jack run --plan-file <path> --mode unsafe --yes`.

## Spec Files

- **MASTER_PLAN.md** — Authoritative forward roadmap (M7-M15). Read this for what's next.
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
| M5 | DONE | Browser executor (CDP) — via `browser` CLI subcommands only; plan-lane wiring is M12 |
| M6 | DONE | Skills system (manifest → plan expansion; skill list/validate/run) |
| Phase B | DONE | Stabilization (v0.7.1): audit fixes, Swift correctness, security fixes, docs |
| M7-M15 | NOT STARTED | New roadmap in MASTER_PLAN.md (M7=perception, M8=safety, M9=MCP, M10=agent loop, M11=long-horizon, M12=browser wiring, M13=control flow, M14=AppleScript/OCR, M15=trust) |

## Important Rules

1. **Spec is normative.** Check the relevant SPEC_FREEZE (V0.2 base; V0.3 UI; V0.4 planner; V0.5 skills; V0.6 browser) before inventing behavior; check MASTER_PLAN.md for anything M7+.
2. **Async allowed (M5+).** tokio is now a dependency (`operator-cli`, `operator-exec-browser`) for the browser/CDP lane. The old "no async / no tokio" rule is obsolete.
3. **Every change must compile.** Run `cargo build && cargo test && cd macos-helper && swift build`.
4. **Update PROJECT_CONTEXT.md** after completing milestones.
5. **Redact before logging.** Apply redaction rules from spec Section 18.
6. **ULIDs everywhere.** For all IDs across the system.
7. **Two-mode policy.** Safe (confirm medium+high) / Unsafe (allow all). Not three-tier.
8. **Raw ApplicationServices.** No AXSwift. Direct AXUIElement* API calls.

## Current Handler Registry (15 handlers)

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
| ui.selectMenu | handleSelectMenu | Methods/SelectMenu.swift |
| ui.setValue | handleSetValue | Methods/SetValue.swift |
| ui.inspect | handleInspect | Methods/Inspect.swift |

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
operator-jack exec <plan-id>           # Execute a saved plan
operator-jack plan validate --plan-file <path>
operator-jack plan save --plan-file <path>
operator-jack logs [run-id] [--full]
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

### Browser (M5 — CLI-only, not yet wired into plans)
```
operator-jack browser doctor                      # Check Chrome availability
operator-jack browser launch [--port 9222]        # Launch Chrome with remote debugging
operator-jack browser navigate <url> [--port N]   # Navigate to a URL via CDP
operator-jack browser screenshot --output <path> [--full-page] [--port N]
```

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
