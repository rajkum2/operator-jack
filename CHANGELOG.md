# Changelog

All notable changes to Operator Jack are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

See [`MASTER_PLAN.md`](MASTER_PLAN.md) for the authoritative M7-M15 roadmap (safety envelope, MCP server, native agent loop, and beyond).

## [0.8.0] - 2026-07-08

M7 — Perception / Observation API. Models can now see the screen.

### Added
- `ui.snapshot` observation step — compact indented AX-tree serialization (one line per element: role, name, value, enabled/focused flags) with actionable elements tagged as short refs (`e1`, `e2`…); params `app`/`window`/`max_depth`/`interactive_only`; returns snapshot text, `ref_count`, `truncated`, and `token_estimate`
- `ui.screenshot` step — downscaled PNG of a window or display written to disk (`screencapture` + `sips`); returns the file path plus width/height, never image bytes
- Snapshot refs are usable as `element_ref` in `ui.click`, `ui.read_text`, and `ui.set_value`
- Chromium/Electron accessibility enablement — `AXManualAccessibility` + a messaging timeout on the app element, plus a first-observation "settle" for the lazily-built a11y tree (Slack/Chrome web content is now visible to the helper)
- `path` selector matching — suffix match on the computed traversal path (previously parsed but rejected when used alone)
- `doctor` now reports Screen Recording permission status (alongside Accessibility)
- Three new Swift helper handlers (registry 15 → 18): `ui.snapshot`, `ui.screenshot`, `ui.checkScreenRecordingPermission`
- SPEC_FREEZE_V0.7.md (M7 observation API spec)
- `docs/examples/snapshot-calculator.json`

### Note
- `ui.screenshot` requires Screen Recording permission (granted in System Settings). Without it the step fails cleanly with a helpful message; `doctor` surfaces the status.

## [0.7.1] - 2026-07-08

Phase B — stabilization and reconciliation. No new features; audit fixes, Swift helper correctness, shipped-code security fixes, and documentation.

### Fixed
- Swift helper: `findElements` now matches the root element (a `{role: AXWindow}` selector was previously unmatchable)
- Swift helper: `waitFor` resolves the window inside the poll loop instead of failing instantly with `WINDOW_NOT_FOUND`
- Swift helper: wired `resolveElementRef` into Click/ReadText/SetValue (element_ref-only params were accepted by Rust but rejected by Swift)
- Swift helper: guarded the `SelectMenu` AXUIElement cast (was a force cast); synced the hardcoded helper version to 0.7.1
- Fixed shipped examples/skills: `docs/examples/calculator-buttons.json`, `skills/create-note.yaml`, `skills/take-screenshot.yaml`
- `skill run` now validates the generated plan before executing; skill numeric params preserve integer type
- Planner: fixed the fence-extraction bug (an unclosed JSON code fence guaranteed a ParseError); Anthropic default model bumped to `claude-haiku-4-5-20251001`
- Non-char-boundary byte-slicing panics in CLI output
- `resolve_helper_path` now consults the config `helper_path` setting

### Security
- **Mode ceiling:** `plan.mode` can no longer escalate above the configured mode — effective mode is now `min(plan.mode, config.mode)`, so a generated plan cannot self-escalate to unsafe
- **Browser JS-injection:** fixed JS string escaping in the browser executor (quotes, backslashes, newlines; second interpolations were previously unescaped)
- ui-lane IPC now enforces a read timeout and kills+respawns the helper on timeout
- `sys.exec` now kills its child process on timeout

### Added
- Swift test target `macos-helper/Tests/OperatorMacOSHelperTests/` (9 tests) — previously there were no Swift tests
- `make check-examples` harness (validate all example plans + dry-run all skills), wired into CI
- `cargo fmt` applied; committed `Cargo.lock`; `--locked` builds in CI/release
- `MASTER_PLAN.md` — authoritative forward roadmap (M7-M15)
- `doctor` provider-readiness check (API key present per provider / Ollama model pulled)

### Changed
- Homebrew formula bumped to v0.7.1 (release workflow gains a tap-update step)
- README: added an Installation section; removed false `plan list` and `--set` claims; documented the real global flags
- Documentation reconciled with code (CLAUDE.md, PROJECT_CONTEXT.md); `docs/ROADMAP.md` marked superseded by MASTER_PLAN.md

## [0.7.0] - 2026-03

M5 — Browser executor via the Chrome DevTools Protocol (CDP).

### Added
- `operator-exec-browser` crate — a hand-rolled CDP client over `tokio-tungstenite` (WebSocket) + `reqwest` (Chrome discovery)
- `tokio` async runtime (`operator-cli` + `operator-exec-browser`) — the first async in the codebase
- `browser` CLI subcommands: `doctor`, `launch`, `navigate`, `screenshot`
- Chrome process management (launch with `--remote-debugging-port`, connect, cleanup); `allow_domains` enforcement for browser navigation
- SPEC_FREEZE_V0.6.md (M5 browser spec)

### Notes
- The browser lane is reachable ONLY via the `browser` CLI subcommands. Browser steps in plans are not yet wired into the engine (a plan with `browser.*` steps fails validation) — plan-lane wiring is milestone M12.

## [0.6.0] - 2026-03

M6 — Skills system (reusable parameterized macros).

### Added
- `operator-skills` crate — YAML/JSON manifest parsing, parameter validation and interpolation, expansion into a full plan at execution time
- `skill` CLI subcommands: `list`, `validate`, `run` (`--param key=value`)
- Skill registry that discovers skills from multiple paths (project `skills/`, user directory)
- 3 example skills in `skills/`: open-and-type, create-note, take-screenshot

## [0.5.0] - 2026-03-14

M4 — Natural-language planner (LLM-backed).

### Added
- `operator-planner` crate with a provider abstraction and four LLM providers: Kimi (Moonshot), OpenAI, Anthropic, and local Ollama
- `operator-jack do "<instruction>"` command for natural-language execution
- `--provider`, `--show-plan`, `--save-plan` flags on `do`; interactive provider selection
- Planner configuration in `config.toml` (per-provider settings)
- SPEC_FREEZE_V0.4.md (M4 planner spec)

### Notes
- The planner is LLM-backed (it requires a reachable provider); there is no offline rule-based fallback.

## [0.4.0] - 2026-03-04

Phase A — production packaging and distribution.

### Changed
- Renamed binary from `operator` to `operator-jack` to avoid naming conflicts

### Added
- LICENSE file (MIT)
- CHANGELOG.md and CONTRIBUTING.md
- Makefile with build/test/install/universal targets
- GitHub Actions CI (test + release workflows)
- Homebrew tap formula and install script
- Config file system (`~/.config/operator-jack/config.toml`)
- Enhanced `operator-jack doctor` with first-run experience

## [0.3.1] - 2026-03-03

### Added
- `ui.selectMenu` handler — menu bar navigation with submenu polling
- `ui.setValue` handler — AX value setting with verification
- `ui.inspect` handler + `operator-jack ui inspect` CLI command
- `anyOf` selectors — fallback locator stack (first strategy returning exactly 1 match wins)
- `element_ref` system — ULID-keyed AXUIElement cache with stale detection and fallback
- Evidence hooks — `_evidence` key on all action handler outputs
- 3 new acceptance test plans (selectMenu, element_ref reuse, full end-to-end)

## [0.3.0] - 2026-03-03

### Added
- 9 UI handlers: focusApp, listWindows, focusWindow, find, waitFor, click, typeText, readText, keyPress
- Window scoping in selectors (`window.index`, `window.title_contains`)
- Implicit wait infrastructure (200ms polling, default 2000ms)
- Interactive disambiguation UX for ELEMENT_AMBIGUOUS errors
- Expanded error taxonomy (APP_NOT_FOUND, ELEMENT_NOT_FOUND, ELEMENT_AMBIGUOUS, etc.)
- SPEC_FREEZE_V0.3.md — additive delta for M3
- 12 new Rust unit tests (selectors, validation, policy)
- 5 acceptance test plan JSONs

## [0.2.0] - 2026-03-02

### Added
- Swift macOS helper binary with NDJSON IPC over stdin/stdout
- `ui.ping` handler for connectivity testing and protocol handshake
- `ui.checkAccessibilityPermission` handler with system prompt dialog
- `ui.listApps` handler listing running applications
- `operator-ipc` crate: process spawning, NDJSON framing, handshake, method name translation
- `operator-jack doctor` upgraded with accessibility check
- Helper auto-discovery: CLI flag, env var, PATH, sibling binary, dev fallback
- 9 new IPC unit tests

## [0.1.1] - 2026-03-01

### Added
- All 13 `sys.*` step executors with real implementations
- Policy gate integration with risk classification per step type
- `--dry-run` and `--yes` flags
- 21 unit tests for system executors
- Manual acceptance tests passed (TextEdit, file ops, URL, clipboard)

## [0.1.0] - 2026-02-28

### Added
- Cargo workspace with 6 crates
- Core types: Plan, Step, StepResult, Selector, PolicyLevel, RunStatus
- Plan JSON parsing and validation
- SQLite store with schema migrations
- CLI skeleton with Clap: run, plan validate, plan list, doctor, stop
- Stub executors for all step types
- Variable interpolation engine
- Redaction filters (key-name and pattern matching)
- JSONL audit log writer
- SPEC_FREEZE_V0.1.md and V0.2.md
