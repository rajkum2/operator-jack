# Changelog

All notable changes to Operator Jack are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Changed
- Renamed binary from `operator` to `operator-jack` to avoid naming conflicts
- Added LICENSE file (MIT)
- Added CHANGELOG.md and CONTRIBUTING.md
- Added Makefile with build/test/install/universal targets
- Added GitHub Actions CI (test + release workflows)
- Added Homebrew tap formula and install script
- Added config file system (`~/.config/operator-jack/config.toml`)
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
