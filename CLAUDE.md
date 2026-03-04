# Operator Jack — CLAUDE.md

> Instructions for AI assistants working on this project.
> Read `PROJECT_CONTEXT.md` first for state tracking. This file covers conventions and build instructions.

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

# Run Rust tests (91 tests)
cargo test

# Build Swift helper
cd macos-helper && swift build

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
│   └── operator-ipc/        # NDJSON IPC client for Swift helper
├── macos-helper/            # Swift SPM package
│   └── Sources/OperatorMacOSHelper/
│       ├── main.swift       # NDJSON server loop, handler registration
│       ├── IpcProtocol.swift # Request/response types, JSONValue enum
│       ├── MethodDispatcher.swift # Method routing, HelperError type
│       ├── AXUtilities.swift # Shared AX foundation (selectors, traversal, waits, disambiguation)
│       └── Methods/         # One file per handler (FocusApp, Find, Click, etc.)
├── docs/
│   ├── examples/            # Golden plan JSONs for acceptance testing
│   └── *.md                 # Architecture, security, selectors, permissions, roadmap
├── SPEC_FREEZE_V0.3.md     # Active spec delta (M3 additions)
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
- **Browser lane** (`browser.*`): Not yet implemented (M5, will use CDP)

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
- Rust: `cargo test` — unit tests in each crate's source files
- Swift: No unit test framework yet — manual testing via NDJSON pipe or CLI
- Acceptance: Golden plan JSONs in `docs/examples/`, run via `operator-jack run --plan-file <path> --mode unsafe --yes`

## Spec Files

- **SPEC_FREEZE_V0.3.md** — Active delta: new step types (list_windows, focus_window), window scoping, anyOf, element_ref, error taxonomy, evidence hooks, `operator-jack ui inspect`
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
| M4 | NOT STARTED | Rule-based planner (natural language → typed steps) |
| M5 | NOT STARTED | Browser executor (CDP) |
| M6-M8 | NOT STARTED | Skills, robustness, STT |

## Important Rules

1. **Spec is normative.** Check SPEC_FREEZE_V0.3.md + V0.2.md before inventing behavior.
2. **No async until M5.** Use sync Rust. No tokio dependency.
3. **Every change must compile.** Run `cargo build && cargo test && cd macos-helper && swift build`.
4. **Update PROJECT_CONTEXT.md** after completing milestones.
5. **Redact before logging.** Apply redaction rules from spec Section 18.
6. **ULIDs everywhere.** For all IDs across the system.
7. **Two-mode policy.** Safe (confirm medium+high) / Unsafe (allow all). Not three-tier.
8. **Raw ApplicationServices.** No AXSwift. Direct AXUIElement* API calls.

## Current Handler Registry (15 handlers)

| IPC Method | Handler | File |
|-----------|---------|------|
| ui.ping | handlePing | Handlers/Ping.swift |
| ui.checkAccessibilityPermission | handleCheckAccessibility | Handlers/CheckAccessibility.swift |
| ui.listApps | handleListApps | Handlers/ListApps.swift |
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
```

## Common Flags

```
--mode safe|unsafe    # Execution mode (default: safe)
--yes                 # Auto-approve all policy gates
--dry-run             # Simulate without side effects
--json                # JSON output format
--helper-path <path>  # Custom helper binary path
--allow-apps <list>   # Comma-separated app allowlist
-v / -q               # Verbose / quiet
```
