# Operator Jack — Project Context & Session Resume File

> **Purpose:** This file is the single source of truth for resuming work across sessions.
> Any AI assistant (or human) should read THIS FILE FIRST before doing anything.
> It tracks: what exists, what was decided, what's next, and how to continue.

---

## Quick Start (For New Session)

1. Read this file completely
2. Read the roadmap: `MASTER_PLAN.md` (authoritative, M7-M15). Newest spec: `SPEC_FREEZE_V0.8.md` (M8 safety envelope); prior: `SPEC_FREEZE_V0.7.md` (M7 perception), `SPEC_FREEZE_V0.6.md` (M5 browser), `SPEC_FREEZE_V0.5.md` (M6 skills, retroactive); base contract: `SPEC_FREEZE_V0.2.md`
3. Check `VERSION_LOG` below for what changed last
4. Check `CURRENT_STATE` below for what's built vs pending
5. Check `NEXT_ACTIONS` for what to do next
6. If code exists, verify it builds: `cd operator-jack && cargo build`

---

## Project Identity

- **Name:** Operator Jack (`operator-jack`)
- **What:** macOS-first CLI tool for deterministic computer automation
- **Architecture:** Instruction → Plan (typed JSON steps) → Policy Gate → Executors → Logs → Result
- **Three execution lanes:** System (Rust), UI/Accessibility (Swift helper via IPC), Browser/CDP (M5 — CLI subcommands only; plan-lane wiring is M12)
- **Repo root:** `/Users/kirankumaralugonda/operator-jack/`
- **Language:** Rust (core) + Swift (macOS helper)

---

## Key Files Map

| File | Purpose |
|------|---------|
| `PROJECT_CONTEXT.md` | THIS FILE — resume context, version log, state tracker |
| `MASTER_PLAN.md` | Authoritative forward roadmap (M7-M15) |
| `SPEC_FREEZE_V0.8.md` | M8 spec — safety envelope (grants, budgets + Partial status, escalation classifier, redaction hardening) |
| `SPEC_FREEZE_V0.7.md` | M7 spec — observation API (ui.snapshot/ui.screenshot, refs, Electron AX, path selectors) |
| `SPEC_FREEZE_V0.6.md` | M5 spec delta — browser executor / CDP |
| `SPEC_FREEZE_V0.5.md` | M6 spec — skills system (written retroactively) |
| `SPEC_FREEZE_V0.4.md` | M4 spec delta — LLM planner + `do` command |
| `SPEC_FREEZE_V0.3.md` | M3 spec additive delta (new step types, window scoping, error taxonomy) |
| `SPEC_FREEZE_V0.2.md` | Base normative spec (frozen contract for M0-M3) |
| `SPEC_FREEZE_V0.1.md` | Previous spec version (archived, do not implement from) |
| `crates/operator-cli/` | CLI binary (`operator-jack`, clap) |
| `crates/operator-core/` | Types, validation, interpolation, redaction, config |
| `crates/operator-runtime/` | Execution loop, policy gates, retry/timeout |
| `crates/operator-store/` | SQLite persistence (rusqlite) |
| `crates/operator-exec-system/` | sys.* step handlers |
| `crates/operator-exec-browser/` | browser.* CDP executor (M5) — tokio-tungstenite + reqwest |
| `crates/operator-ipc/` | NDJSON IPC client for Swift helper |
| `crates/operator-planner/` | LLM planner (M4) — Kimi/OpenAI/Anthropic/Ollama |
| `crates/operator-skills/` | Skills system (M6) — manifest parse/validate/expand |
| `macos-helper/` | Swift SPM package for AX automation |
| `docs/` | Architecture, security, selectors, permissions, roadmap |
| `docs/examples/` | Example plan JSON files |
| `Makefile` | Build automation (build, test, install, universal) |
| `.github/workflows/` | CI (test/lint) + Release (universal binaries) |
| `homebrew/` | Homebrew formula template |

---

## Tech Stack Decisions (Locked)

| Component | Choice | Rationale |
|-----------|--------|-----------|
| SQLite | `rusqlite` + `rusqlite_migration` | Sync, bundled, fast compile, simple migrations |
| IPC | stdin/stdout NDJSON pipes | Simplest for parent-child; no socket cleanup |
| IDs | ULID (`ulid` crate) | Sortable, no coordination needed |
| CLI | `clap` (derive) | Standard Rust CLI |
| Async | Sync for M0-M4; tokio added at M5 for the CDP browser lane (operator-cli + operator-exec-browser) | Async only where CDP requires it |
| Serialization | `serde` + `serde_json` | Standard |
| Error handling | `anyhow` (CLI) + `thiserror` (libraries) | Standard pattern |
| Logging | `tracing` | Structured, flexible |
| Paths | `dirs` crate | XDG-compliant platform paths |
| Swift AX | Raw ApplicationServices (no AXSwift) | AXSwift unmaintained; raw API is ~10 functions |
| CDP (M5) | Hand-rolled CDP client over `tokio-tungstenite` + `reqwest` | Full protocol control; no heavyweight browser-automation dependency |

---

## VERSION_LOG

### v0.9.0 — 2026-07-08
**Status:** M8 core complete. The Safety Envelope — a grantable, bounded, auditable policy model so a model can drive the executors without constant prompts or unbounded blast radius. 139 Rust tests + 10 Swift tests pass. (Keychain secret store deferred — see item 7.)
**Changes:**
1. **Task-scoped grants** on Plan + Config: `grants: { fs_read, fs_write, exec, domains }` (glob / command / host patterns). Additive restrictions (absent = unrestricted); the config and plan layers must BOTH allow (most restrictive wins). Enforced in the runtime PolicyGate on interpolated params; fs paths are canonicalized (`~`, `..`) so they cannot escape a grant. A violation is DENIED with a message naming the exact grant and surfaces process exit code 3. Verified live: a write inside the grant succeeds; a write outside is denied (exit 3, no file created).
2. **Run budgets** on Plan + Config: `budget: { max_steps, max_wall_ms, max_bytes_written, max_llm_calls, max_llm_cost_usd }` (the LLM fields are dormant until M10). Checked in the engine loop; on exhaustion the run ends with a NEW terminal status **`Partial`** (completed step results preserved). Verified live: max_steps=1 over a 3-step plan → status `partial` (1 succeeded, 2 skipped).
3. **Always-escalate classifier** (non-overridable by grants or `--yes`): `sys.exec` with sudo / `rm -rf` / pipe-to-shell; `sys.delete_path` that would remove > 25 entries; `ui.click`/`ui.key_press` on Send/Buy/Pay/Checkout-like controls in Mail/Messages/Slack; and a hard Swift-helper refusal to type or set-value into a secure text field (`AXSecureTextField`). Verified live: `sudo` under `--mode unsafe --yes` is still denied ("Escalation required (a human must approve)"), exit code 3, sudo never ran.
4. **Redaction hardening**: `plan_json` is now redacted at rest in the store (was the one unredacted CRUD write); the blanket "any string with a slash is exempt" rule is replaced by a precise looks-like-a-path test, so slash-bearing base64 secrets are redacted again while real paths stay exempt.
5. **Exit codes**: policy/grant/escalation denial → 3; `Partial` (budget) → 7.
6. Created `SPEC_FREEZE_V0.8.md` (M8 safety envelope spec); added `docs/examples/grants-scoped-write.json` (fs_write grant + budget).
7. **Deferred (0.9.x follow-on, M8.5):** the Keychain secret store + `{{secret:name}}` injection. The secure-field refusal (the safety half) is done; the credential-injection half is a self-contained follow-on whose consumers (M9 MCP, M10 agent loop) are not built yet.

### v0.8.0 — 2026-07-08
**Status:** M7 complete. Perception / Observation API — models can now see the screen (AX-tree snapshots + screenshots). 129 Rust tests + 10 Swift tests pass.
**Changes:**
1. `ui.snapshot` step + IPC method + Swift handler (`Methods/Snapshot.swift` → `handleSnapshot`): compact indented AX-tree serialization (one line per element: role, name, value, enabled/focused flags), actionable elements tagged with short refs (`e1`, `e2`…) backed by `ElementRefStore` and invalidated per-snapshot; params `app`/`window`/`max_depth`/`interactive_only`; returns snapshot text + `ref_count` + `truncated` + `token_estimate`. Verified snapshot→ref-click on Calculator (refs e2/e13/e16/e14/e20 → display reads 3)
2. `ui.screenshot` step + Swift handler (`Methods/Screenshot.swift` → `handleScreenshot`): downscaled PNG of a window/display written to disk via `screencapture` + `sips`; returns path + width + height, never image bytes; requires Screen Recording permission (fails cleanly with a helpful message when not granted)
3. `ui.checkScreenRecordingPermission` handler (`Methods/Screenshot.swift` → `handleCheckScreenRecording`); Swift handler registry grows 15 → 18
4. Snapshot refs are accepted as `element_ref` by `ui.click`/`ui.read_text`/`ui.set_value` (falls back to selector re-resolution when stale)
5. Chromium/Electron accessibility enablement: `axAppElement` sets `AXManualAccessibility` + a messaging timeout; a first-observation "settle" waits for the lazily-built a11y tree (Slack 1719 tokens, Chrome 399 tokens, Calculator 327 tokens all surface real content)
6. Selector `path` is now a real matcher (suffix of the computed traversal path) — reverses Phase B's path-only rejection; `any_of` works in find/waitFor
7. `doctor` now reports Screen Recording permission status (alongside Accessibility) and provider readiness
8. Created `SPEC_FREEZE_V0.7.md` (M7 observation API spec); added `docs/examples/snapshot-calculator.json`

### v0.7.1 — 2026-07-08
**Status:** Phase B complete. Stabilization & reconciliation — no new features; audit fixes, Swift helper correctness, shipped-code security fixes, docs. 128 Rust tests + 9 Swift tests pass.
**Changes:**
1. (B1) Pipeline/repo hygiene: `cargo fmt` the workspace; committed the regenerated `Cargo.lock`; added `--locked` to CI/release build steps
2. (B2) Swift helper correctness: `findElements` now matches the root element; `waitFor` resolves the window inside the poll loop (was instant WINDOW_NOT_FOUND); wired `resolveElementRef` into Click/ReadText/SetValue; guarded the `SelectMenu` AXUIElement cast; synced the hardcoded helper version to 0.7.1
3. (B2) Created the Swift test target `macos-helper/Tests/OperatorMacOSHelperTests/` (9 tests) — previously zero Swift tests existed
4. (B3) Fixed shipped examples/skills: `docs/examples/calculator-buttons.json`, `skills/create-note.yaml`, `skills/take-screenshot.yaml`; `skill run` now validates the plan before executing; skill numeric params preserve integer type
5. (B3) Added `make check-examples` harness (validate all example plans + dry-run all skills); wired into CI
6. (B4) Runtime/security fixes: ui-lane IPC read timeout with helper kill+respawn; `sys.exec` kills its child process on timeout; **mode-ceiling security fix** — `plan.mode` can no longer escalate above the configured mode (effective = min(plan, config)); browser JS-injection escaping fix; char-boundary slicing panic fixes; `resolve_helper_path` now consults the config `helper_path`
7. (B4) Planner: fence-extraction fix; Anthropic default model bumped to `claude-haiku-4-5-20251001`
8. (B5) Distribution: Homebrew formula bumped to v0.7.1 (release workflow gains a tap-update step); README gains an Installation section; removed false `plan list`/`--set` claims; documented real flags
9. (B6) Docs reconciliation: `MASTER_PLAN.md` is now the authoritative M7-M15 roadmap; updated CLAUDE.md / PROJECT_CONTEXT / CHANGELOG; `doctor` gains a provider-readiness check; `docs/ROADMAP.md` marked superseded

### v0.7.0 — 2026-03
**Status:** M5 complete. Browser executor via Chrome DevTools Protocol (CDP). Workspace grows to 9 crates. First async in the codebase.
**Changes:**
1. Created `operator-exec-browser` crate — a hand-rolled CDP client over `tokio-tungstenite` (WebSocket) + `reqwest` (Chrome discovery); NOT chromiumoxide
2. Added `tokio` async runtime (`operator-cli` + `operator-exec-browser`)
3. Chrome process management: launch with `--remote-debugging-port`, connect, cleanup
4. New `browser.*` step types (see SPEC_FREEZE_V0.6.md); `allow_domains` enforcement for browser navigation
5. CLI: `operator-jack browser doctor|launch|navigate|screenshot`; browser executor tests added
6. Created SPEC_FREEZE_V0.6.md (M5 browser spec)
7. **Caveat:** the browser lane is reachable ONLY via the `browser` CLI subcommands. Browser steps in plans are NOT yet wired into the engine (a plan with `browser.*` steps fails validation) — plan-lane wiring is milestone M12 in MASTER_PLAN.md

### v0.6.0 — 2026-03
**Status:** M6 complete. Skills system (reusable parameterized macros). 113 tests pass (5 new skill tests). Workspace grows to 8 crates.
**Changes:**
1. Created `operator-skills` crate — YAML/JSON manifest parsing, parameter validation/interpolation, expansion to a full plan at execution time (no new runtime concepts)
2. Skill registry — discovers skills from multiple paths (project `skills/`, user dir)
3. CLI: `operator-jack skill list|validate|run` (`--param key=value`)
4. Added 3 example skills in `skills/`: open-and-type, create-note, take-screenshot
5. (Note: the M6 skills spec, SPEC_FREEZE_V0.5.md, was written retroactively in Phase B — see v0.7.1 above)

### v0.5.0 — 2026-03-14
**Status:** M4 Complete. Rule-based planner with multi-provider LLM support.
**Changes:**
1. Created `operator-planner` crate with provider abstraction trait
2. Implemented Kimi (Moonshot AI) provider with `moonshot-v1-8k` default model
3. Implemented OpenAI provider with `gpt-4o-mini` default model
4. Implemented Anthropic provider with `claude-3-haiku` default model
5. Implemented Ollama local provider with `llama3.2` default model
6. Added `operator-jack do <instruction>` command for natural language execution
7. Added interactive provider selection when multiple providers available
8. Added `--provider`, `--show-plan`, `--save-plan` flags to `do` command
9. Added planner configuration to `config.toml` with per-provider settings
10. Added comprehensive prompt template for plan generation
11. Added 12 new tests for planner module (108 total tests)

### v0.4.0 — 2026-03-04
**Status:** Phase A complete. Production packaging & distribution. Binary renamed to `operator-jack`. 96 tests pass (5 new config tests).
**Changes (Phase A):**
1. Renamed binary from `operator` to `operator-jack` (Cargo.toml, clap, data dirs, PID files, DB name)
2. Added config file system: `~/.config/operator-jack/config.toml` with TOML parsing, env var overrides, `operator-jack init` command
3. Added Makefile with targets: build, build-release, test, lint, clean, install, uninstall, universal
4. Added GitHub Actions CI: ci.yml (test/lint on push/PR) + release.yml (universal binaries + GitHub Release on tag)
5. Added Homebrew tap formula template in `homebrew/operator-jack.rb`
6. Added `install.sh` script for non-Homebrew users
7. Created LICENSE (MIT), CHANGELOG.md (retrospective M0-M3), CONTRIBUTING.md
8. Enhanced `operator-jack doctor`: first-run detection, terminal-specific accessibility instructions, version display, config path info, helper install guidance
9. Updated all documentation: README.md, CLAUDE.md, ARCHITECTURE.md, SECURITY.md, PERMISSIONS_MACOS.md, ROADMAP.md, PROJECT_CONTEXT.md

### v0.3.1 — 2026-03-03
**Status:** M3 complete (M3a + M3b). Full UI executor v1. 91 tests pass. Swift helper has 15 handlers.
**Changes (M3b):**
1. Implemented `ui.selectMenu` handler — menu bar navigation with submenu polling
2. Implemented `ui.setValue` handler — AX value setting with settability check and read-back verification
3. Implemented `ui.inspect` handler — recursive AX tree dump with configurable depth
4. Added `operator ui inspect --app <name> --depth N` CLI command with pretty-printed tree output
5. Added `anyOf` selectors — fallback locator stack (Rust validation + Swift resolution, first exact match wins)
6. Added `element_ref` system — ULID-keyed AXUIElement cache in Swift, returned by `ui.find`, accepted by click/readText/setValue; falls back to selector re-resolution when stale
7. Updated Rust validation: `selector` optional when `element_ref` provided for click, readText, setValue
8. Added evidence hooks — `gatherEvidence()` returns `_evidence` key on all action handler outputs (active app, window, element subtree)
9. Added 6 new Rust tests: anyOf validation (5), element_ref validation (1); total 91 tests
10. Created 3 new acceptance test plans: select-menu-test, element-ref-reuse, full-end-to-end; total 11 golden plans
11. Created CLAUDE.md — project conventions, build instructions, handler registry, architecture guide

### v0.3.0 — 2026-03-03
**Status:** M3a complete. UI executor v1 core. 85 tests pass (12 new). Swift helper has 12 handlers.
**Changes (M3a):**
1. Created SPEC_FREEZE_V0.3.md — additive delta documenting all M3 additions (new step types, window scoping, error taxonomy, implicit waits, disambiguation)
2. Added `ui.list_windows` and `ui.focus_window` StepType variants in Rust (types, validation, policy, IPC translation)
3. Added `WindowScope` struct to Selector with `index`/`title_contains` (mutually exclusive validation)
4. Added `details: Option<serde_json::Value>` to `IpcError::HelperError` for structured error metadata
5. Created `AXUtilities.swift` (~350 lines): shared foundation for all UI handlers — app resolution, AX wrappers, window enumeration, selector matching engine (DFS traversal), implicit waits, disambiguation, element serialization
6. Implemented 9 Swift handlers: `ui.focusApp`, `ui.listWindows`, `ui.focusWindow`, `ui.find`, `ui.waitFor`, `ui.click`, `ui.typeText`, `ui.readText`, `ui.keyPress`
7. Added interactive disambiguation UX in Rust engine — intercepts ELEMENT_AMBIGUOUS errors, displays candidates, lets user choose index
8. Made ELEMENT_AMBIGUOUS non-retryable (requires user disambiguation, not blind retry)
9. Added 12 new Rust unit tests: selector window scope (6), policy (2), validation (4)
10. Created 5 acceptance test plans: calculator-buttons, notes-create-m3, system-settings-nav, multi-window, disambiguation-test
11. All 85 Rust tests passing, Swift helper builds clean

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
- [x] Rust workspace scaffolding (9 crates: cli, core, runtime, store, exec-system, exec-browser, ipc, planner, skills)
- [x] operator-core: types, validation, interpolation, redaction, events, policy
- [x] operator-store: SQLite migrations, CRUD repos (12 unit tests passing)
- [x] operator-ipc: protocol types, real IPC client with NDJSON framing
- [x] operator-exec-system: real implementations for all 13 sys.* step types (21 unit tests)
- [x] operator-runtime: execution engine, policy gates, JSONL logging
- [x] operator-cli: all commands (doctor, plan, exec, run, do, logs, stop, ui inspect, init, skill, browser)
- [x] Documentation: ARCHITECTURE.md, SECURITY.md, SELECTORS.md, PERMISSIONS_MACOS.md, ROADMAP.md
- [x] Example plans: 11 golden plans in docs/examples/ (open-app, file-operations, notes-automation, chrome-search, calculator-buttons, notes-create-m3, system-settings-nav, multi-window, disambiguation-test, select-menu-test, element-ref-reuse, full-end-to-end)
- [x] README.md
- [x] CLAUDE.md — project conventions, build instructions, handler registry
- [x] Swift helper (M3): NDJSON server, 15 handlers (ping, accessibility, listApps, focusApp, listWindows, focusWindow, find, waitFor, click, typeText, readText, keyPress, selectMenu, setValue, inspect)
- [x] AXUtilities.swift: shared AX foundation (app resolution, window scoping, selector matching, implicit waits, disambiguation, anyOf resolution, element_ref store, evidence gathering)
- [x] Real system executor implementations (M1) — all 13 sys.* types working
- [x] Rust-side disambiguation UX for ELEMENT_AMBIGUOUS (interactive candidate selection)
- [x] `operator-jack ui inspect` CLI command for AX tree debugging
- [x] Phase A: production packaging (binary rename, config, Makefile, CI, Homebrew formula, install script, LICENSE, CHANGELOG, CONTRIBUTING, enhanced doctor)
- [x] SPEC_FREEZE_V0.4.md (M4 spec)
- [x] operator-planner crate with provider abstraction trait
- [x] Kimi provider (Moonshot AI)
- [x] OpenAI provider
- [x] Anthropic provider (Claude)
- [x] Ollama provider (local LLM)
- [x] `operator-jack do` command for natural language execution
- [x] Interactive provider selection UI
- [x] Plan preview (`--show-plan`) and save (`--save-plan`) options
- [x] 108 tests passing (12 new planner tests)
- [x] SPEC_FREEZE_V0.5.md (M6 spec)
- [x] operator-skills crate with manifest parsing and expansion
- [x] Skill registry for discovering skills from multiple paths
- [x] Parameter validation and interpolation
- [x] CLI commands: skill list, validate, run
- [x] Example skills: open-and-type, create-note, take-screenshot
- [x] 113 tests passing (5 new skill tests)
- [x] SPEC_FREEZE_V0.6.md (M5 browser spec)
- [x] operator-exec-browser crate — hand-rolled CDP client (tokio-tungstenite + reqwest, NOT chromiumoxide)
- [x] tokio async runtime added (operator-cli + operator-exec-browser)
- [x] CLI: browser doctor/launch/navigate/screenshot (browser lane is CLI-only; plan-lane wiring is M12)
- [x] Phase B (v0.7.1): audit fixes, Swift helper correctness, security fixes (mode ceiling, JS-injection escaping, exec/ui-lane timeouts, char-boundary panics), docs reconciliation
- [x] Swift test target (macos-helper/Tests/OperatorMacOSHelperTests): 9 tests
- [x] `make check-examples` harness (validate examples + dry-run skills), wired into CI
- [x] MASTER_PLAN.md — authoritative forward roadmap (M7-M15)
- [x] SPEC_FREEZE_V0.7.md (M7 observation API spec)
- [x] M7 perception: `ui.snapshot` (compact AX serialization + actionable refs) and `ui.screenshot` (downscaled PNG to disk) step types + Swift handlers; snapshot refs accepted as `element_ref` by click/read_text/set_value
- [x] Swift helper now has 18 handlers (added snapshot, screenshot, checkScreenRecordingPermission)
- [x] Chromium/Electron AX enablement (AXManualAccessibility + messaging timeout + first-read settle); `path` selector matching; `doctor` Screen Recording permission check
- [x] docs/examples/snapshot-calculator.json
- [x] 129 Rust tests + 10 Swift tests passing (v0.8.0)
- [x] SPEC_FREEZE_V0.8.md (M8 safety envelope spec)
- [x] M8 task-scoped grants: `grants: {fs_read, fs_write, exec, domains}` on Plan + Config (intersected, path-canonicalized), enforced in the runtime PolicyGate on interpolated params; denial names the exact grant
- [x] M8 run budgets: `budget: {max_steps, max_wall_ms, max_bytes_written, max_llm_calls, max_llm_cost_usd}` checked in the engine loop; new terminal run status `Partial` on exhaustion (completed step results preserved; LLM fields dormant until M10)
- [x] M8 always-escalate classifier (non-overridable by grants/`--yes`): sudo / `rm -rf` / pipe-to-shell exec, bulk-delete > 25 entries, send/purchase controls in Mail/Messages/Slack, and a Swift-helper refusal to type into `AXSecureTextField`
- [x] M8 redaction hardening: `plan_json` redacted at rest; slash-exemption narrowed to a looks-like-path test
- [x] M8 exit codes: policy/grant/escalation denial → 3; `Partial` (budget) → 7
- [x] docs/examples/grants-scoped-write.json (fs_write grant + budget)
- [x] 139 Rust tests + 10 Swift tests passing (v0.9.0)
- [ ] DEFERRED (0.9.x / M8.5): Keychain secret store + `{{secret:name}}` injection (secure-field refusal done; credential-injection half pending — consumers M9/M10 not built yet)

### Milestone Status

| Milestone | Status | Description |
|-----------|--------|-------------|
| M0 | DONE | Scaffolding: workspace, types, store, CLI skeleton, stub executors, logging |
| M1 | DONE | System executor: sys.* handlers, policy gates, --dry-run, --yes |
| M2 | DONE | Swift helper v1: IPC server, ping, accessibility check, listApps |
| M3a | DONE | UI executor v1 core: focusApp/Window, find, waitFor, click, typeText, readText, keyPress, disambiguation |
| M3b | DONE | UI executor v1 polish: selectMenu, setValue, inspect, anyOf selectors, element_ref, evidence hooks |
| Phase A | DONE | Production packaging: binary rename, config system, Makefile, CI, Homebrew, install script, LICENSE, CHANGELOG, enhanced doctor |
| M4 | DONE | LLM planner (natural language → typed steps; Kimi/OpenAI/Anthropic/Ollama) |
| M5 | DONE | Browser executor (CDP) — `browser` CLI subcommands only; plan-lane wiring is M12 |
| M6 | DONE | Skills system (manifest → plan expansion; skill list/validate/run) |
| Phase B | DONE | Stabilization (v0.7.1): audit/security fixes, Swift correctness + test target, docs reconciliation |
| M7 | DONE | Perception / Observation API: ui.snapshot (compact AX + refs), ui.screenshot, Electron/Chromium AX enablement, path selectors, doctor Screen Recording check |
| M8 | DONE* | Safety envelope: task-scoped grants (fs/exec/domains), run budgets + `Partial` status, always-escalate classifier, secure-field refusal, redaction hardening, exit code 3. *Keychain secret store (`{{secret:}}`) deferred to a 0.9.x follow-on (M8.5) |
| M9-M15 | NOT STARTED | New roadmap in MASTER_PLAN.md — M9 MCP server (next), M10 agent loop, M11 long-horizon, M12 browser wiring, M13 control flow, M14 AppleScript/OCR, M15 trust |

---

## NEXT_ACTIONS

**Immediate next steps (in order) — see MASTER_PLAN.md for full detail:**

1. **M8.5 — Keychain secret store (0.9.x follow-on)** — finish the deferred half of M8: `operator-jack secret set/list/rm` (security-framework) + `{{secret:name}}` refs resolved only at execution time, tainted/force-redacted in results, and injected into the target field by the helper without ever entering a model prompt or the store. The secure-field refusal (the safety half) already shipped in v0.9.0.
2. **M9 — MCP Server Mode (v0.10.0)** — expose Operator Jack as an MCP server (`operator-jack mcp serve`) so any MCP client drives the gated executors; owns the RunSession refactor. First shippable autonomy. Depends on M7 + M8 (both done).

**MASTER_PLAN.md is the authoritative roadmap for M7-M15.** M0-M8 core plus Phase B (v0.7.1) are complete (M8.5 Keychain secrets pending).

---

## CRITICAL RULES (Read Before Writing Code)

1. **Spec is normative.** If unsure about behavior, check SPEC_FREEZE_V0.2.md. Do not invent behavior.
2. **Async landed at M5.** M0-M4 are sync; tokio is now a dependency (operator-cli + operator-exec-browser) for the CDP browser lane.
3. **Every milestone must compile and run.** No broken intermediate states.
4. **Keep modules decoupled.** No giant files. Each crate has clear responsibility.
5. **No network calls** unless user explicitly runs browser automation.
6. **Safe by default.** Medium/high risk steps require confirmation in safe mode.
7. **Append-only audit log.** Every step gets logged with timestamp, params, result.
8. **Redact before logging.** Apply redaction rules from spec Section 18.
9. **ULIDs everywhere.** For plan_id, run_id, step_result_id, event_id, IPC message id.
10. **XDG paths.** Use `dirs` crate. Config in `~/.config/operator-jack/`, data in `~/Library/Application Support/operator-jack/`.

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

*Last updated: 2026-07-08 — M8 CORE COMPLETE (v0.9.0). The Safety Envelope: task-scoped grants (fs/exec/domains), run budgets + new `Partial` terminal status, always-escalate classifier (sudo / rm -rf / pipe-to-shell, bulk-delete > 25, send/purchase in messaging apps, hard secure-field typing refusal), redaction hardening (plan_json at rest, looks-like-path exemption), policy-denial exit code 3. M0-M8 core plus Phase B done. 139 Rust tests + 10 Swift tests passing. Keychain secret store + {{secret:}} injection deferred to a 0.9.x follow-on (M8.5). Next: M8.5 then M9 (MCP server) per MASTER_PLAN.md.*
