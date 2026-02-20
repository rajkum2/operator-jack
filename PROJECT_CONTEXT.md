# Operator CLI — Project Context & Session Resume File

> **Purpose:** This file is the single source of truth for resuming work across sessions.
> Any AI assistant (or human) should read THIS FILE FIRST before doing anything.
> It tracks: what exists, what was decided, what's next, and how to continue.

---

## Quick Start (For New Session)

1. Read this file completely
2. Read the active spec: `SPEC_FREEZE_V0.2.md`
3. Check `VERSION_LOG` below for what changed last
4. Check `CURRENT_STATE` below for what's built vs pending
5. Check `NEXT_ACTIONS` for what to do next
6. If code exists, verify it builds: `cd operator && cargo build`

---

## Project Identity

- **Name:** Operator CLI (`operator`)
- **What:** macOS-first CLI tool for deterministic computer automation
- **Architecture:** Instruction → Plan (typed JSON steps) → Policy Gate → Executors → Logs → Result
- **Three execution lanes:** System (Rust), UI/Accessibility (Swift helper via IPC), Browser/CDP (future)
- **Repo root:** `/Users/kirankumaralugonda/operator-jack/`
- **Language:** Rust (core) + Swift (macOS helper)

---

## Key Files Map

| File | Purpose |
|------|---------|
| `PROJECT_CONTEXT.md` | THIS FILE — resume context, version log, state tracker |
| `SPEC_FREEZE_V0.2.md` | Active normative spec (frozen contract for M0-M3) |
| `SPEC_FREEZE_V0.1.md` | Previous spec version (archived, do not implement from) |
| `operator/` | Rust workspace root (when created) |
| `operator/crates/operator-cli/` | CLI binary (clap) |
| `operator/crates/operator-core/` | Types, validation, interpolation, redaction |
| `operator/crates/operator-runtime/` | Execution loop, policy gates, retry/timeout |
| `operator/crates/operator-store/` | SQLite persistence (rusqlite) |
| `operator/crates/operator-exec-system/` | sys.* step handlers |
| `operator/crates/operator-ipc/` | NDJSON IPC client for Swift helper |
| `operator/macos-helper/` | Swift SPM package for AX automation |
| `operator/docs/` | Architecture, security, selectors, permissions, roadmap |
| `operator/docs/examples/` | Example plan JSON files |

---

## Tech Stack Decisions (Locked)

| Component | Choice | Rationale |
|-----------|--------|-----------|
| SQLite | `rusqlite` + `rusqlite_migration` | Sync, bundled, fast compile, simple migrations |
| IPC | stdin/stdout NDJSON pipes | Simplest for parent-child; no socket cleanup |
| IDs | ULID (`ulid` crate) | Sortable, no coordination needed |
| CLI | `clap` (derive) | Standard Rust CLI |
| Async | None for M0-M3 (sync). Tokio added at M5 for CDP | Avoids unnecessary complexity |
| Serialization | `serde` + `serde_json` | Standard |
| Error handling | `anyhow` (CLI) + `thiserror` (libraries) | Standard pattern |
| Logging | `tracing` | Structured, flexible |
| Paths | `dirs` crate | XDG-compliant platform paths |
| Swift AX | Raw ApplicationServices (no AXSwift) | AXSwift unmaintained; raw API is ~10 functions |
| CDP (M5+) | `chromiumoxide` | Most downloads, adequate |

---

## VERSION_LOG

### v0.2.3 — 2026-02-20
**Status:** M2 complete. Swift helper v1 with real IPC. 73 tests pass.
**Changes:**
1. Created Swift SPM package in `macos-helper/` with NDJSON stdin/stdout server
2. Implemented `ui.ping` handler (protocol handshake, version validation)
3. Implemented `ui.checkAccessibilityPermission` handler (AXIsProcessTrustedWithOptions)
4. Implemented `ui.listApps` handler (NSWorkspace.shared.runningApplications, .regular filter)
5. Created NDJSON framing module (`framing.rs`) with 1 MiB line cap, EOF detection
6. Replaced HelperClient stub with real implementation: process spawning, NDJSON communication, handshake, crash detection
7. Added method name translation (snake_case → camelCase) for all 11 ui.* step types
8. Added dev fallback helper path resolution (Swift .build/release and .build/debug)
9. Wired resolved helper path into EngineConfig (was passing raw CLI flag only)
10. Upgraded `operator doctor` with 4th accessibility check (spawn helper, ping, check permission)
11. Added 9 new tests (5 framing, 4 client), 73 total tests passing
12. Integration test: `ui.list_apps` executes end-to-end through the engine

### v0.2.2 — 2026-02-20
**Status:** M1 complete. All 13 sys.* executors implemented. 64 tests pass.
**Changes:**
1. Replaced all 13 stub executors with real implementations
2. sys.open_app: `open -a` via Command
3. sys.open_url: `open` via Command
4. sys.quit_app: graceful via osascript, force via pkill
5. sys.read_file: std::fs::read_to_string with tilde expansion
6. sys.write_file: std::fs::write with create_parent support
7. sys.append_file: OpenOptions::append with create_parent
8. sys.mkdir: create_dir / create_dir_all based on parents flag
9. sys.move_path: std::fs::rename with overwrite guard
10. sys.copy_path: std::fs::copy for files, recursive copy for dirs
11. sys.delete_path: remove_file / remove_dir_all with recursive flag
12. sys.exec: direct exec (no shell), $HOME default cwd, env_clean, 1MiB capture cap
13. sys.clipboard_get: pbpaste
14. sys.clipboard_set: pipe into pbcopy
15. Added 21 unit tests for executor (file ops, exec, clipboard, edge cases)
16. Manual acceptance tests passed: TextEdit open/quit, file ops, URL, clipboard

### v0.2.1 — 2026-02-20
**Status:** M0 code review findings fixed. 43 tests pass.
**Fixes (13 findings):**
1. P1: Step timeout enforcement — execute_with_timeout wraps step execution with channel-based timeout
2. P1: SIGTERM handler added alongside SIGINT
3. P1: Redaction applied to all SQLite writes (input_json, output_json, error_json, payload_json, error on runs)
4. P1: Policy gate now uses interpolated params (check_step_with_params) instead of original step params
5. P2: Retry loop checks error retryability (is_retryable_error) before retrying
6. P2: on_fail=ask retry actually re-executes the step once
7. P2: Allowlist violations respect on_fail instead of unconditional abort
8. P2: Terminal status: aborted→Failed, non-aborted with failures→CompletedWithErrors
9. P2: PID file now writes run_id (via on_run_created callback) instead of plan_id
10. P2: Real TTY detection via isatty(2) instead of always returning true
11. P3: Helper binary name corrected to "operator-macos-helper"
12. P3: Hyphens allowed in interpolation variable paths (e.g. step.step-1.output)
13. Added 31 unit tests (interpolation, redaction, policy)

### v0.2.0 — 2026-02-20
**Status:** Spec frozen. No code written yet.
**Changes from v0.1:**
1. Added `ui.select_menu` step type + helper method (critical for menu automation)
2. Added `sys.quit_app` step type
3. Added `sys.clipboard_get` / `sys.clipboard_set` step types
4. Split `ui.type` into `ui.set_value` (AX value) + `ui.type_text` (CGEvent keystrokes)
5. Added plan-level `mode`, `allow_apps`, `allow_domains` (additive restrictions)
6. Explicit crate structure: core, runtime, store, exec-system, ipc, cli
7. Tightened `sys.exec`: direct exec, 1MiB capture cap, $HOME default cwd, env_clean
8. Added CLI flags: `--allow-apps`, `--allow-domains`, `--json`, `-v`, `-q`
9. Selector suffix convention: `name`/`name_contains` replaces global `match` field
10. Added `step_id` to events table
11. Expanded `operator logs` (list runs, detail, --full)
12. Documented safe/unsafe mode simplification from 3-tier to 2-tier

### v0.1.0 — 2026-02-20
**Status:** Archived. Superseded by v0.2.
**Origin:** First spec freeze covering M0-M3 scope.

---

## CURRENT_STATE

### What Exists
- [x] SPEC_FREEZE_V0.1.md (archived)
- [x] SPEC_FREEZE_V0.2.md (active spec)
- [x] PROJECT_CONTEXT.md (this file)
- [x] Rust workspace scaffolding (6 crates)
- [x] operator-core: types, validation, interpolation, redaction, events, policy
- [x] operator-store: SQLite migrations, CRUD repos (12 unit tests passing)
- [x] operator-ipc: protocol types, real IPC client with NDJSON framing
- [x] operator-exec-system: real implementations for all 13 sys.* step types (21 unit tests)
- [x] operator-runtime: execution engine, policy gates, JSONL logging
- [x] operator-cli: all commands (doctor, plan, exec, run, logs, stop)
- [x] Documentation: ARCHITECTURE.md, SECURITY.md, SELECTORS.md, PERMISSIONS_MACOS.md, ROADMAP.md
- [x] Example plans: open-app.json, file-operations.json, notes-automation.json, chrome-search.json
- [x] README.md
- [x] Swift helper (M2): NDJSON server, ping, accessibility check, listApps
- [x] Real system executor implementations (M1) — all 13 sys.* types working

### Milestone Status

| Milestone | Status | Description |
|-----------|--------|-------------|
| M0 | DONE | Scaffolding: workspace, types, store, CLI skeleton, stub executors, logging |
| M1 | DONE | System executor: sys.* handlers, policy gates, --dry-run, --yes |
| M2 | DONE | Swift helper v1: IPC server, ping, accessibility check, listApps |
| M3 | NOT STARTED | UI executor v1: find/click/setValue, selector matching, menu selection |
| M4 | NOT STARTED | Rule-based planner (natural language → typed steps) |
| M5 | NOT STARTED | Browser executor (CDP) |
| M6 | NOT STARTED | Skills system (macros) |
| M7 | NOT STARTED | Robustness + recovery |
| M8 | NOT STARTED | Offline STT input |

---

## NEXT_ACTIONS

**Immediate next step: Build M3 (UI Executor v1)**

M3 deliverables (from spec Section 23):
1. `ui.find` — find elements using selector matching against the accessibility tree
2. `ui.click` — click a UI element
3. `ui.set_value` — set text field value via AX API
4. `ui.type_text` — type text via CGEvent keystrokes
5. `ui.key_press` — simulate key presses
6. `ui.select_menu` — navigate application menus
7. `ui.wait_for` — wait for element to appear
8. `ui.read_text` — read element value
9. Selector matching engine in Swift helper

**Before starting M3:** Ask user for confirmation.

---

## CRITICAL RULES (Read Before Writing Code)

1. **Spec is normative.** If unsure about behavior, check SPEC_FREEZE_V0.2.md. Do not invent behavior.
2. **No async until M5.** Use sync Rust for M0-M3. No tokio dependency.
3. **Every milestone must compile and run.** No broken intermediate states.
4. **Keep modules decoupled.** No giant files. Each crate has clear responsibility.
5. **No network calls** unless user explicitly runs browser automation.
6. **Safe by default.** Medium/high risk steps require confirmation in safe mode.
7. **Append-only audit log.** Every step gets logged with timestamp, params, result.
8. **Redact before logging.** Apply redaction rules from spec Section 18.
9. **ULIDs everywhere.** For plan_id, run_id, step_result_id, event_id, IPC message id.
10. **XDG paths.** Use `dirs` crate. Config in `~/.config/operator/`, data in `~/.local/share/operator/`.

---

## DESIGN NOTES & CONTEXT

### Why Rust + Swift (not pure Rust)?
macOS Accessibility (AXUIElement) is a C/HIServices API. While Rust FFI crates exist (`accessibility-sys`), they're thin and unmaintained. Swift has first-class access to ApplicationServices and better ergonomics for the AX tree traversal. The IPC boundary (NDJSON over stdio) keeps the two codebases cleanly separated.

### Why not tokio from day one?
The system executor (file ops, `open -a`, `pbcopy`) is synchronous. The AX helper is a child process with sync stdio. SQLite via rusqlite is sync. Adding tokio adds compile time, complexity, and `spawn_blocking` wrappers for no benefit until CDP (M5).

### Why two-mode (safe/unsafe) instead of three?
The original spec proposed safe/normal/power. Two tiers are simpler: safe = confirm medium+high, unsafe = allow all. A middle tier would need nuanced per-risk-level rules that aren't worth the complexity for v0.2.

### Variable interpolation scope
Variables resolve just-in-time before each step. Forward references are invalid (caught at validation). `${step.foo.output.bar}` traverses the JSON output of step `foo` at key `bar`. This enables data flow between steps without complex piping.

### Selector disambiguation
When multiple AX elements match and no `index` is given: interactive mode prompts the user, non-interactive mode fails with SELECTOR_AMBIGUOUS + candidate list. Selection may be cached per-run by `(app, selector_hash)`.

---

## HOW TO UPDATE THIS FILE

- **After completing a milestone:** Update CURRENT_STATE, move milestone to "DONE", update NEXT_ACTIONS
- **After changing the spec:** Create new SPEC_FREEZE_V0.x.md, add entry to VERSION_LOG, update active spec reference
- **After making tech stack decisions:** Update Tech Stack Decisions table
- **After discovering important patterns:** Add to DESIGN NOTES
- **Keep VERSION_LOG in reverse chronological order** (newest first)
- **Never delete old spec freeze files** — they're the audit trail

---

*Last updated: 2026-02-20 — M2 COMPLETE. Swift helper v1 with real IPC, 73 tests pass, integration tests pass. Ready for M3.*
