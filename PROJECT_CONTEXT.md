# Operator Jack — Project Context & Session Resume File

> **Purpose:** This file is the single source of truth for resuming work across sessions.
> Any AI assistant (or human) should read THIS FILE FIRST before doing anything.
> It tracks: what exists, what was decided, what's next, and how to continue.

---

## Quick Start (For New Session)

1. Read this file completely
2. Read the roadmap: `MASTER_PLAN.md` (authoritative, M7-M15 — now fully implemented as of v0.16.0). Newest spec: `SPEC_FREEZE_V0.15.md` (M15 trust, residency & measurement — undo journal, watch/daemon mode, eval harness, signing scaffolding); prior: `SPEC_FREEZE_V0.14.md` (M14 native macOS integration lane), `SPEC_FREEZE_V0.13.md` (M13 hands completion & deterministic control flow), `SPEC_FREEZE_V0.12.md` (M12 web autonomy — browser lane wiring), `SPEC_FREEZE_V0.11.md` (M11 long-horizon & supervision), `SPEC_FREEZE_V0.10.md` (M10 native agent loop), `SPEC_FREEZE_V0.9.md` (M9 MCP server mode), `SPEC_FREEZE_V0.8.md` (M8 safety envelope), `SPEC_FREEZE_V0.7.md` (M7 perception), `SPEC_FREEZE_V0.6.md` (M5 browser), `SPEC_FREEZE_V0.5.md` (M6 skills, retroactive); base contract: `SPEC_FREEZE_V0.2.md`
3. Check `VERSION_LOG` below for what changed last
4. Check `CURRENT_STATE` below for what's built vs pending
5. Check `NEXT_ACTIONS` for what to do next
6. If code exists, verify it builds: `cd operator-jack && cargo build`

---

## Project Identity

- **Name:** Operator Jack (`operator-jack`)
- **What:** macOS-first CLI tool for deterministic computer automation
- **Architecture:** Instruction → Plan (typed JSON steps) → Policy Gate → Executors → Logs → Result
- **Three execution lanes:** System (Rust), UI/Accessibility (Swift helper via IPC), Browser/CDP (M5 executor; wired into plans/agent/MCP in M12 via an isolated per-run Chrome)
- **Repo root:** `/Users/kirankumaralugonda/operator-jack/`
- **Language:** Rust (core) + Swift (macOS helper)

---

## Key Files Map

| File | Purpose |
|------|---------|
| `PROJECT_CONTEXT.md` | THIS FILE — resume context, version log, state tracker |
| `MASTER_PLAN.md` | Authoritative forward roadmap (M7-M15) — fully implemented as of v0.16.0; now design context + the Future backlog |
| `SPEC_FREEZE_V0.15.md` | M15 spec — trust, residency & measurement (undo journal + `operator-jack undo`, watch/daemon mode + launchd + single-instance run mutex, eval harness, signing/notarization scaffolding) |
| `SPEC_FREEZE_V0.14.md` | M14 spec — native macOS integration lane (`script.applescript`/`script.jxa`/`shortcuts.run` exec-grade, `doc.pdf_text` PDFKit, `doc.ocr` Vision, doctor Full Disk Access + Automation checks, calendar recipe skill) |
| `SPEC_FREEZE_V0.13.md` | M13 spec — hands completion & deterministic control flow (data/net steps `sys.list_dir`/`data.parse_csv`/`data.parse_json`/`http.request`, `ui.scroll`, `control.foreach`/`if`/`repeat_until`) |
| `SPEC_FREEZE_V0.12.md` | M12 spec — web autonomy (browser lane wired into plans/agent/MCP, profile isolation, browser.snapshot/get_text_all/get_attributes_all/download) |
| `SPEC_FREEZE_V0.11.md` | M11 spec — long-horizon & supervision (Paused status, checkpoint/approval tables, ui.human_takeover, resume, logs --follow/prune) |
| `SPEC_FREEZE_V0.10.md` | M10 spec — native agent loop (provider `chat`, observe→propose→execute→re-observe, agent mode semantics) |
| `SPEC_FREEZE_V0.9.md` | M9 spec — MCP server mode (RunSession, mcp serve, 15-tool surface over JSON-RPC-over-stdio) |
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
| `crates/operator-runtime/` | Execution loop, policy gates, retry/timeout; browser lane bridge (M12: `browser_lane`) |
| `crates/operator-store/` | SQLite persistence (rusqlite) |
| `crates/operator-exec-system/` | sys.* step handlers |
| `crates/operator-exec-browser/` | browser.* CDP executor — tokio-tungstenite + reqwest (M5; wired into plans/agent/MCP in M12) |
| `crates/operator-ipc/` | NDJSON IPC client for Swift helper |
| `crates/operator-planner/` | LLM planner (M4) — Kimi/OpenAI/Anthropic/Ollama |
| `crates/operator-skills/` | Skills system (M6) — manifest parse/validate/expand |
| `crates/operator-mcp/` | MCP server (M9) — JSON-RPC over stdio, tools → RunSession |
| `crates/operator-agent/` | Native agent loop (M10) — observe→propose→execute→re-observe orchestrator on RunSession |
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

### v0.17.0 — 2026-07-13
**Status:** Post-roadmap feature (not a MASTER_PLAN milestone). **Local settings & control dashboard.** `operator-jack dashboard [--port 7311] [--no-open]` serves a self-contained web UI on `127.0.0.1` to *view* status/runs/approvals/skills/capabilities and *edit* the config (mode, providers, timeouts, retries, allow-lists, grants, budget) + manage keychain secrets — without hand-writing TOML. New `crates/operator-cli/src/dashboard.rs` (+ embedded `dashboard.html`) served with `tiny_http`; new `Store::set_busy_timeout` for safe concurrent DB access under WAL. **Security:** loopback-only bind; a random per-session token gates every `/api/*` call; `Host`-header validation (DNS-rebinding defense); `application/json`-only POSTs; no CORS. API keys are never written to config (env/keychain only); secret VALUES are never returned; config saves load via `load_from` (no env-override contamination) and write atomically. Verified live: config save round-trip incl. grants/budget, validation, live runs/approvals/skills reads, keychain secret set/list/delete, and the auth + Host guards. 169 Rust tests (3 new) + 10 Swift tests; fmt + clippy `-D warnings` clean; check-examples valid.

### v0.16.0 — 2026-07-09
**Status:** M15 COMPLETE (partial-scope — see item 6). **The M7-M15 roadmap is now fully implemented.** Trust, residency & measurement — reversibility (an undo journal), unattended triggers (watch/daemon mode), a measured success rate (eval harness), and release-signing scaffolding: what lets a user run this unattended on their real machine. Crate count unchanged at 11 (undo ships as a new `operator-runtime` module, not a crate). 166 Rust tests + 10 Swift tests pass.
**Changes:**
1. **Undo journal (reversibility)**: copy-before-mutate for filesystem `sys.*` steps. `EngineConfig.undo_dir` (the CLI sets it to `<data_dir>/undo` for every real, non-dry-run run) makes the engine snapshot prior state before each mutating step — backup target or record newly-created for `write_file`/`append_file`/`copy_path`, copy-aside for `delete_path`, record from→to for `move_path`, record created dir for `mkdir` — into a manifest at `<undo_dir>/<run_id>/manifest.jsonl`. Applies to plan runs AND `RunSession` (agent/MCP) runs. `operator-jack undo <run-id>` replays the manifest in reverse (restore backups, remove created files, move back, restore deleted). New `operator-runtime::undo` module.
2. **Watch/daemon mode**: `operator-jack watch --plan-file <p> [--interval N] [--path <watch>] [--install-launchd]`. Interval-only fires each tick; `--path` fires on mtime change (baselined at startup). A single-instance run mutex (PID file + live-PID check) serializes runs — a trigger that fires while a run is in flight is QUEUED (pending), not dropped or run concurrently. `--install-launchd` writes `~/Library/LaunchAgents/com.operator-jack.watch.plist` (RunAtLoad + KeepAlive) and prints the `launchctl load`/`unload` commands.
3. **Eval harness (measurement)**: `operator-jack eval <suite-dir>` runs every `*.json` plan in a suite, scores each pass (Succeeded, 0 failed steps) / fail, prints a results table + pass rate (`--json` supported), and exits non-zero if any plan fails.
4. **Signing & notarization scaffolding**: `release.yml` gains a "Sign & notarize" step (imports a Developer ID cert from secrets into a temp keychain, `codesign` hardened-runtime + timestamp, `notarytool` notarizes each tarball); it skips cleanly when signing secrets are unset. Required secrets: `MACOS_CERT_P12`, `MACOS_CERT_PASSWORD`, `MACOS_SIGN_IDENTITY`, `AC_API_KEY_ID`, `AC_API_ISSUER_ID`, `AC_API_KEY_P8`.
5. Version bumped to 0.16.0 (workspace + Swift `helperVersion`). Created `SPEC_FREEZE_V0.15.md` (M15 trust/residency/measurement spec).
6. **Verified live**: `undo` reversed an overwrite + create + delete run (3 mutations reversed, 0 failed); `watch` fired a plan on a file change (a new audited run) and stopped cleanly, and `--install-launchd` wrote a valid plist; `eval` ran a 3-plan suite → 3/3 (100%), exit 0. Full gate green: `cargo fmt` clean, `cargo clippy -D warnings` clean, 166 Rust tests, 10 Swift tests, `check-examples.sh` all valid.
7. **Known-partial (stated honestly)**: exec sandboxing (seatbelt) is DEFERRED (a stretch item; the M8 grant/escalation gate + the undo journal + budgets already bound `sys.exec`); signing/notarization is SCAFFOLDED but requires an Apple Developer ID cert (unavailable here), so no Gatekeeper-passing build was produced; watch user-activity-pause is NOT implemented; the eval CI runner is not wired (it needs a self-hosted macOS session).

### v0.15.0 — 2026-07-09
**Status:** M14 COMPLETE (partial-scope — see item 7). Native macOS integration lane — a script lane (AppleScript/JXA/Shortcuts, exec-grade) and a read-only document lane (PDF text + OCR via PDFKit/Vision) that reach the parts of macOS the Accessibility tree can't. Crate count unchanged at 11. Swift handler registry 19 → 21. 164 Rust tests + 10 Swift tests pass.
**Changes:**
1. **Script lane (system lane, High risk, exec-grade)**: `script.applescript {script}` and `script.jxa {script}` run via `osascript -l AppleScript` / `-l JavaScript` (reusing the `sys.exec` timeout/capture path; return `{stdout, stderr, exit_code, language}`); `shortcuts.run {name, input_path?}` invokes the macOS `shortcuts` CLI. All three map to the `exec` grant capability (the script body / shortcut name is the match target), and the always-escalate classifier SCANS script bodies — `do shell script`, `administrator privileges`, `sudo`, `rm -rf`, and shelling out escalate to a human. The first `script.*` to an app triggers macOS's one-time Automation (AppleEvents) consent dialog.
2. **Document lane (Swift helper / UI lane, Low risk, read-only)**: `doc.pdf_text {path, max_pages?}` extracts text via PDFKit → `{text, page_count, chars}`; `doc.ocr {path, fast?}` runs Vision's `VNRecognizeTextRequest` on-device/offline → `{text, line_count}` (also the perception fallback for scanned/image content). New Swift handlers `handlePdfText` / `handleOcr` (`Methods/Document.swift`), registered as `doc.pdf_text` / `doc.ocr`; Swift handler registry 19 → 21.
3. **doctor**: gains a Full Disk Access probe (lists `~/Library/Mail`) and an Automation (AppleEvents) consent note (the first `script.*` to an app triggers a one-time macOS dialog).
4. **Recipe skill + i18n guidance**: `skills/calendar-add-event.yaml` creates a Calendar event via `script.applescript` (params `summary` / `calendar` / `duration_minutes`; validates under `check-examples`); i18n guidance prefers identifier-based selectors (name-based selectors break on non-English macOS), and a US-layout audit of the `KeyPress.swift` keycode table is noted for the future.
5. Version bumped to 0.15.0 (workspace + Swift `helperVersion`). Created `SPEC_FREEZE_V0.14.md` (M14 native macOS integration lane spec); added `skills/calendar-add-event.yaml`.
6. **Verified live**: AppleScript `2+3` → `5`, JXA `6*7` → `42`, an AppleScript current-date returning the year; `doc.pdf_text` extracted a real PDF's text; `doc.ocr` OCR'd a rendered PNG → "INVOICE 42". Full gate green: `cargo fmt` clean, `cargo clippy -D warnings` clean, 164 Rust tests, 10 Swift tests, `check-examples.sh` all valid.
7. **Known-partial (stated honestly)**: the "save all PDF attachments from unread invoices to a CSV" end-to-end recipe and the OCR-to-agent data-entry flow are ENABLED by these primitives but were NOT assembled/run as one scripted acceptance here (they need Mail data + Automation + Full Disk Access consent on the host); Mail/Finder recipe skills beyond Calendar are DEFERRED; `shortcuts.run` is wired but not exercised (no user Shortcuts on the host); the i18n keycode-table audit is documented, not applied.

### v0.14.0 — 2026-07-09
**Status:** M13 COMPLETE (partial-scope — see item 7). Hands completion & deterministic control flow — data/network/filesystem steps, the first scroll verb, and deterministic control flow (foreach/if/repeat_until) that runs each leaf through a RunSession. Crate count unchanged at 11. 163 Rust tests + 10 Swift tests pass.
**Changes:**
1. **Data/network/filesystem steps (system lane)**: `sys.list_dir {path, pattern?}` (glob + metadata, name-sorted); `data.parse_csv {text|path, headers?}` (RFC-4180-ish: quoted fields, embedded commas, `""` escapes → row objects); `data.parse_json {text|path, pointer?}` (RFC-6901 JSON pointer); `http.request {url, method?, headers?, body?}` (blocking, 30s timeout, 1 MiB body cap; gated by the domain allow-list AND grants, like `sys.open_url`).
2. **`ui.scroll` (UI lane)** — the first scroll verb; new Swift handler `handleScroll` (`Methods/Scroll.swift`) posts scroll-wheel CGEvents (params `app`, `dy`, `dx`, `clicks`, `to: "top"|"bottom"`); Swift handler registry 18 → 19.
3. **Deterministic control flow (control lane)**: `control.foreach {items, as, steps, max_iterations?}`, `control.if {condition, then, else?}`, `control.repeat_until {steps, condition, max_iterations}`. Bodies carry sub-steps inside `params` (the flat plan schema is unchanged). Conditions are `{left, op, right}` with ops `== != < <= > >= contains exists empty` (numeric or string compare). Mandatory iteration caps (100k hard ceiling; `repeat_until` requires `max_iterations`); nesting bounded at 16. Variables: `${var}` (type-preserving), `save_as` binds a leaf's output, `foreach` binds the loop var + `__index`. The interpreter (`operator-runtime::control`) runs each leaf through `RunSession::execute_step` — the same policy gate, `{{secret:*}}` resolution, redaction, and audit as plan execution — and audits each iteration independently. `operator-jack run`/`exec` auto-detect control-flow plans and route them through the interpreter over a RunSession (flat plans keep the existing engine path). Control-flow plans are non-interactive for confirmations (they need `--yes` or a grant for safe-mode medium/high steps).
4. **Agent + MCP catalogs**: the agent loop gains actions `list_dir`, `parse_csv`, `parse_json`, `http_request`; the MCP catalog gains the matching tools (`list_dir`, `parse_csv`, `parse_json`, `http_request`), 23 → 27 tools — completing the M9 surface.
5. Version bumped to 0.14.0 (workspace + Swift `helperVersion`). Created `SPEC_FREEZE_V0.13.md` (M13 hands completion & control-flow spec); added `docs/examples/control-flow-foreach.json`.
6. **Verified live**: a `parse_csv → foreach → if → write_file` per-row plan ran `Succeeded` with correct branches; standalone `parse_csv` (quoted/escaped fields), `parse_json` (pointer), and `list_dir` (glob) each verified. Full gate green: `cargo fmt` clean, `cargo clippy -D warnings` clean, 163 Rust tests, 10 Swift tests, `check-examples.sh` all valid.
7. **Known-partial (stated honestly)**: only `ui.scroll` of the planned M13 actuation-verb list landed — right-click/double-click/hover/drag, coordinate `ui.click_at`/`ui.scroll_at` (Retina mapping), typed `ui.set_value` (bool/number), window management, and Open/Save dialog helpers are DEFERRED; resume-aware batches and the skill-compiler stretch are DEFERRED. `ui.scroll` compiles and is wired end-to-end but was not live-exercised (needs Accessibility permission + a live window).

### v0.13.0 — 2026-07-09
**Status:** M12 COMPLETE. Web autonomy — the browser lane is wired into plans, the agent loop, and MCP, running against an isolated per-run Chrome. The M5 CDP executor (previously reachable only via the `browser` CLI subcommands, with `browser.*` plan steps hard-rejected) is now a first-class lane. Crate count unchanged at 11. 155 Rust tests + 10 Swift tests pass.
**Changes:**
1. **Browser lane wired into plans/agent/MCP** — the `browser.*` hard-reject in `validation.rs` is removed and a `"browser"` arm was added to the engine's lane dispatch; browser steps now route through policy, redaction, and audit like every other lane.
2. **Profile isolation** (security-critical, landed first): the lane launches its OWN Chrome with a dedicated `--user-data-dir` — ephemeral per-run by default (removed on close) or a named persistent profile — never the user's real logged-in profile. It refuses to attach to a Chrome it didn't launch unless the escalating `browser.real_profile` capability is granted. Headless by default (headed available for human takeover).
3. **Sync↔async bridge** — new `operator-runtime::browser_lane` module. `BrowserLane` runs a dedicated OS thread with its own multi-threaded tokio runtime; the sync engine talks to it over std channels only, so it is safe from the sync CLI and from within any other runtime (no nested-runtime panic). `BrowserHandle` lazily spawns Chrome on the first browser step and tears it down at run end. `operator-runtime` gains an `operator-exec-browser` dependency.
4. **New step types**: `browser.get_text_all` (querySelectorAll → array), `browser.get_attributes_all` (attribute from all matches → array; href/src resolved to absolute), `browser.snapshot` (compact interactive-element list with CSS-selector refs — the browser analogue of `ui.snapshot`), `browser.download` (`Page.setDownloadBehavior` + wait-for-complete → file path).
5. **Robustness**: auto-waiting selectors (get_text/get_attribute/click/type poll up to 5s for the element, Playwright-style); real navigation waits (poll `document.readyState` complete, not a fixed sleep); screenshots write a PNG file and return its path (never base64 into the store).
6. **Agent + MCP browser catalogs**: MCP gains 8 browser tools (browser_navigate, browser_snapshot, browser_get_text, browser_get_text_all, browser_get_attributes_all, browser_click, browser_type, browser_download; 23 tools total); the agent loop gains those plus browser_screenshot and browser_wait_for (10 browser actions).
7. Examples: `docs/examples/browser-navigate.json` and a new `docs/examples/browser-scrape.json` now validate (moved out of the removed `pending-m12/` holding dir).
8. Version bumped to 0.13.0 (workspace + Swift `helperVersion`). Created `SPEC_FREEZE_V0.12.md` (M12 web-autonomy spec).
9. **Verified live**: a plan navigate → get_text → get_text_all → get_attributes_all → snapshot scraped a page (get_text_all → ["Alpha","Beta","Gamma"], get_attributes_all(href) → ["/a","/b","/c"]); tests use self-contained `data:` URLs because this machine's antivirus intercepts live http navigations. Full gate green: `cargo fmt` clean, `cargo clippy -D warnings` clean, 155 Rust tests, 10 Swift tests, `check-examples.sh` all valid.
10. **Known-partial**: downloads implemented but not exercised against a live download server here; `browser.snapshot` is a flat interactive-element list, not a full a11y tree; browser CLI subcommands' `--dry-run`/`--json` polish deferred.

### v0.12.0 — 2026-07-09
**Status:** M11 COMPLETE. Long-horizon autonomy & supervision — runs now pause, survive process exit, and resume after human approval; plus live supervision (follow, steer), pause notifications, transcript compaction, kill-switch hardening, and audit-log pruning. Crate count unchanged at 11. 152 Rust tests + 10 Swift tests pass.
**Changes:**
1. **Pause/resume infrastructure** — a new non-terminal run status `RunStatus::Paused` (resumable, exit code 0) and two SQLite tables (migration #2): `checkpoints` (one per run: cursor, variables, agent transcript, goal, reason) and `approvals` (id, run_id, kind escalation|takeover, status pending|approved|denied). New store methods: save/load/delete_checkpoint, create_approval, list_approvals, get_approval, latest_approval_for_run, decide_approval, prune_runs.
2. **Escalation → pause** (replaces M8's interim clean-fail): when the always-escalate classifier flags an action, the run checkpoints, opens a pending approval, posts a macOS notification, and ends `Paused` (exit 0) instead of hard-failing. Resume consumes a one-shot escalation allowance (`PolicyGate::grant_one_escalation`). MCP still surfaces escalation as a tool error (it has no run loop).
3. **`ui.human_takeover` step** — a new control-lane step type (risk Low): an interactive plan run blocks for Enter; a non-interactive run pauses for out-of-band resume; the agent loop can emit it as a control action. The agent never types credentials or solves CAPTCHAs.
4. **Resume**: `operator-jack agent --resume <run-id>` and `operator-jack run --resume <run-id>` — both verify the run's approval is granted, then continue (the agent re-seeds the saved transcript; a plan skips completed steps via the checkpoint cursor).
5. **Approvals CLI**: `operator-jack approvals list [--all]`, `approvals approve <id>`, `approvals deny <id>`.
6. **Supervision DX**: `operator-jack logs --follow [<run-id>]` (tail per-run JSONL; also `logs follow`) and `operator-jack steer <run-id> "<message>"` (inject guidance into a running agent's next turn); macOS notifications on pause. **Context compaction**: long agent transcripts (> ~24k chars) fold their oldest turns into a summary breadcrumb, keeping recent turns verbatim.
7. **Retention/pruning**: `operator-jack logs prune [--older-than-days N] [--keep M]` (also `logs --prune`) deletes old runs and cascades to step_results/events/checkpoints/approvals — fixes the audit's unbounded-growth finding.
8. **Kill-switch hardening**: SIGINT/SIGTERM sets the cancel flag; the engine always writes a terminal `Cancelled` status + `RunCancelled` event and disconnects the helper.
9. Version bumped to 0.12.0 (workspace + Swift `helperVersion`). Created `SPEC_FREEZE_V0.11.md` (M11 long-horizon & supervision spec).
10. **Verified live**: a plan with a `ui.human_takeover` step paused non-interactively (checkpoint + approval written, exit 0), refused `--resume` before approval, and completed after `approvals approve` + `--resume`; `logs prune` reclaimed 82 runs. Full gate green: `cargo fmt` clean, `cargo clippy -D warnings` clean, 152 Rust tests, 10 Swift tests, `check-examples.sh` all valid.

### v0.11.0 — 2026-07-09
**Status:** M10 COMPLETE. Native agent loop — Operator Jack drives *itself*: observe → propose ONE JSON action → execute through a `RunSession` → re-observe, until the model signals `done`, `--max-iters` is hit, or the provider errors. The definitional autonomy feature. Workspace grows to 11 crates. 146 Rust tests + 10 Swift tests pass.
**Changes:**
1. **New crate `operator-agent`** (workspace now 11 crates) — the native autonomous loop: `operator_agent::run_agent(goal, provider, &mut RunSession, max_iters) -> AgentOutcome` (`AgentStatus` = `Done | MaxIterations | Aborted`). Each turn: observe the screen → the model proposes ONE JSON action → execute it through M9's `RunSession` (the SAME policy gate, grants, escalation classifier, `{{secret:*}}` resolution, redaction, and audit as plan execution) → re-observe. Agent actions: open_app, focus_app, snapshot, click, type_text, key_press, set_value, select_menu, read_text, wait_for, read_file, write_file, exec. `snapshot` returns a compact AX tree with element refs (e1, e2…) the model then acts on; refs survive across turns via the persistent helper. Malformed/unknown actions and `ELEMENT_AMBIGUOUS` are fed back to the model for self-correction.
2. **Provider layer — `chat`**: `LlmProvider` gains `chat(system, user) -> Result<String, PlannerError>`; `generate_plan` is now `chat` + plan extraction. Implemented for all four providers (Kimi, OpenAI, Anthropic, Ollama). The agent loop uses `chat` to request one action per turn.
3. **`operator-jack agent "<goal>" [--max-iters N] [--provider P] [--grants <file>]`** — runs the loop. Autonomous and non-interactive (auto-approves confirmations; safety comes from the grant envelope + escalation classifier + secure-field refusal). Prints the effective grant envelope first; the whole run is one session-run in the store and every executed action is audited.
4. Version bumped to 0.11.0 (workspace + Swift `helperVersion`).
5. **Verified live** with a local Ollama model (`llama3.2:3b`): open_app → snapshot → click → snapshot → click against Calculator — 6 audited steps under one run. Task-completion quality tracks the model (a stronger provider via API completes goals a small local model may not).
6. Created `SPEC_FREEZE_V0.10.md` (M10 agent-loop spec).
7. Full gate green: `cargo fmt` clean, `cargo clippy -D warnings` clean, 146 Rust tests, 10 Swift tests, `check-examples.sh` all valid.

### v0.10.0 — 2026-07-09
**Status:** M9 COMPLETE. MCP server mode — Operator Jack's gated executors exposed as MCP tools so any MCP client (Claude Code/Desktop) can drive the audited hands ("brain rented, hands owned"). Workspace grows to 10 crates. 143 Rust tests + 10 Swift tests pass.
**Changes:**
1. **RunSession** (`operator_runtime::engine::RunSession`) — ad-hoc single-step gated execution: `open` → `execute_step(&Step)` → `finish`. Each step runs through the SAME policy gate (grants/escalation), `{{secret:*}}` resolution, store-boundary redaction, and audit as plan execution (shared, security-critical helpers). Holds a PERSISTENT helper connection so snapshot element-refs survive across calls, and one run per session (session-as-run). `execute_plan` is unchanged; both paths share the same helpers. The shared substrate for M9 + M10.
2. **New crate `operator-mcp`** (workspace now 10 crates) — a minimal, spec-compliant, hand-rolled MCP server over stdio (newline-delimited JSON-RPC 2.0): `initialize`, `tools/list`, `tools/call`, `ping`. Deliberately NOT the rmcp SDK — rmcp's async/Send bounds don't fit the synchronous RunSession (rusqlite + child pipes); documented as a choice, with rmcp adoptable later without changing the tool surface.
3. **`operator-jack mcp serve [--grants <file>]`** — serves MCP on stdio. Because stdio is the protocol channel (no interactive terminal), the session auto-approves confirmations and relies on the grant envelope + escalation classifier + secure-field refusal for safety. Prints the effective grant envelope ("this session may: …") to STDERR before serving (stdout is protocol-only); warns if unrestricted.
4. **15 tools**: snapshot, screenshot, click, type_text, key_press, set_value, select_menu, read_text, wait_for, list_apps, focus_app, open_app, read_file, write_file, exec. click/read_text/set_value accept a selector OR an element_ref from a prior snapshot. Each tool call → a RunSession step; denials return `isError=true` (the session continues), not a JSON-RPC error.
5. **Verified live** (scripted JSON-RPC over stdio): initialize → serverInfo; tools/list → 15 tools; write_file inside an fs_write grant succeeded; the same write outside the grant returned isError ("denied by policy: Grant required: 'fs_write'…") and no file was created.
6. Created `SPEC_FREEZE_V0.9.md` (M9 MCP server spec — RunSession, `mcp serve`, tool surface).
7. 143 Rust tests + 10 Swift tests.

### v0.9.1 — 2026-07-08
**Status:** M8 COMPLETE. Finished the last deferred sub-feature: the Keychain secret store.
**Changes:**
1. `operator-core::secrets` — keychain get/set/delete/list (security-framework) under the `operator-jack` service, with a name index for `list`.
2. `{{secret:name}}` references resolved at execution time in the engine, for DISPATCH only; the audited params (`input_json`), JSONL logs, and saved plan keep the reference form.
3. `operator-jack secret set|list|rm` CLI (value from stdin or `--value`; never echoes values).
4. Verified live: a plan writing `{{secret:X}}` produced the resolved value on disk while the store/logs showed only the reference — no leak in SQLite or JSONL.
5. 142 Rust tests + 10 Swift tests.

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
7. **Caveat (as of v0.7.0):** the browser lane was reachable ONLY via the `browser` CLI subcommands; plan `browser.*` steps failed validation. *Resolved in M12 (v0.13.0): the browser lane is now wired into plans/agent/MCP via an isolated per-run Chrome.*

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
- [x] Rust workspace scaffolding (11 crates: cli, core, runtime, store, exec-system, exec-browser, ipc, planner, skills, mcp, agent)
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
- [x] CLI: browser doctor/launch/navigate/screenshot (M5); the browser lane is now also wired into plans/agent/MCP (M12)
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
- [x] Keychain secret store + `{{secret:name}}` injection (v0.9.1) — `secret set/list/rm`, resolved at execution time, never persisted (verified no-leak)
- [x] SPEC_FREEZE_V0.9.md (M9 MCP server spec)
- [x] M9 RunSession (`operator_runtime::engine::RunSession`) — ad-hoc single-step gated execution (open → execute_step → finish) through the same policy gate, `{{secret:*}}` resolution, redaction, and audit as plan execution; persistent helper (snapshot refs survive across calls); session-as-run. `execute_plan` unchanged (shared helpers). Shared substrate for M9 + M10
- [x] M9 `operator-mcp` crate (workspace → 10 crates) — hand-rolled JSON-RPC-2.0-over-stdio MCP server (`initialize`/`tools/list`/`tools/call`/`ping`); deliberately NOT rmcp (its async/Send bounds don't fit the sync RunSession; documented, rmcp adoptable later without changing the tool surface)
- [x] M9 `operator-jack mcp serve [--grants <file>]` — serves MCP on stdio; auto-approves confirmations (no interactive terminal) and relies on the grant envelope + escalation classifier + secure-field refusal; prints the effective grant envelope to STDERR before serving (stdout protocol-only), warns if unrestricted
- [x] M9 15 MCP tools: snapshot, screenshot, click, type_text, key_press, set_value, select_menu, read_text, wait_for, list_apps, focus_app, open_app, read_file, write_file, exec (click/read_text/set_value accept selector OR element_ref; each call = one RunSession step; denials return isError, session continues)
- [x] M9 verified live over stdio: write_file inside an fs_write grant succeeded; the same write outside the grant returned isError ("Grant required: 'fs_write'") with no file created
- [x] 143 Rust tests + 10 Swift tests passing (v0.10.0)
- [x] SPEC_FREEZE_V0.10.md (M10 agent-loop spec)
- [x] M10 `operator-agent` crate (workspace → 11 crates) — native autonomous loop `run_agent(goal, provider, &mut RunSession, max_iters) -> AgentOutcome` (`AgentStatus` = Done | MaxIterations | Aborted): observe → model proposes ONE JSON action → execute via M9's RunSession (same policy gate / grants / escalation / `{{secret:*}}` / redaction / audit) → re-observe; repeats until `done`, `--max-iters`, or provider error
- [x] M10 agent actions: open_app, focus_app, snapshot, click, type_text, key_press, set_value, select_menu, read_text, wait_for, read_file, write_file, exec; snapshot returns a compact AX tree + refs (e1, e2…) that survive across turns via the persistent helper; malformed/unknown actions and ELEMENT_AMBIGUOUS fed back for self-correction
- [x] M10 provider `chat(system, user)` on `LlmProvider` (Kimi/OpenAI/Anthropic/Ollama); `generate_plan` is now `chat` + plan extraction
- [x] M10 `operator-jack agent "<goal>" [--max-iters N] [--provider P] [--grants <file>]` — autonomous, non-interactive (auto-approves; relies on grant envelope + escalation classifier + secure-field refusal); prints the effective grant envelope first; one session-run, every action audited
- [x] M10 verified live with local Ollama (`llama3.2:3b`): open_app → snapshot → click → snapshot → click on Calculator, 6 audited steps under one run
- [x] 146 Rust tests + 10 Swift tests passing (v0.11.0)
- [x] SPEC_FREEZE_V0.11.md (M11 long-horizon & supervision spec)
- [x] M11 pause/resume infrastructure: new non-terminal run status `RunStatus::Paused` (resumable, exit code 0); migration #2 adds `checkpoints` (one per run: cursor, variables, agent transcript, goal, reason) + `approvals` (id, run_id, kind escalation|takeover, status pending|approved|denied); store methods save/load/delete_checkpoint, create_approval, list_approvals, get_approval, latest_approval_for_run, decide_approval, prune_runs
- [x] M11 escalation → pause (replaces M8's interim clean-fail): the always-escalate classifier now checkpoints + opens a pending approval + posts a macOS notification + ends `Paused` (exit 0) instead of hard-failing; resume consumes a one-shot escalation allowance (`PolicyGate::grant_one_escalation`); MCP still surfaces escalation as a tool error (no run loop)
- [x] M11 `ui.human_takeover` control-lane step (risk Low): interactive run blocks for Enter, non-interactive run pauses for out-of-band resume, the agent loop can emit it; the agent never types credentials or solves CAPTCHAs
- [x] M11 resume: `operator-jack run --resume <run-id>` and `agent --resume <run-id>` (both verify the run's approval is granted; the agent re-seeds the saved transcript, a plan skips completed steps via the checkpoint cursor)
- [x] M11 approvals CLI: `approvals list [--all]`, `approvals approve <id>`, `approvals deny <id>`
- [x] M11 supervision DX: `logs --follow [<run-id>]` (tail per-run JSONL; also `logs follow`), `steer <run-id> "<message>"` (inject guidance into a running agent's next turn), macOS notifications on pause, transcript compaction (> ~24k chars folds oldest turns into a summary breadcrumb, recent turns kept verbatim)
- [x] M11 retention/pruning: `logs prune [--older-than-days N] [--keep M]` (also `logs --prune`) deletes old runs + cascades to step_results/events/checkpoints/approvals
- [x] M11 kill-switch hardening: SIGINT/SIGTERM sets the cancel flag; the engine always writes a terminal `Cancelled` status + `RunCancelled` event and disconnects the helper
- [x] 152 Rust tests + 10 Swift tests passing (v0.12.0)
- [x] SPEC_FREEZE_V0.12.md (M12 web-autonomy spec)
- [x] M12 browser lane wired into plans/agent/MCP: the `browser.*` hard-reject is removed from `validation.rs` and a `"browser"` arm added to the engine lane dispatch; browser steps now route through policy/redaction/audit like every other lane
- [x] M12 profile isolation (security-critical, landed first): the lane launches its OWN Chrome with a dedicated `--user-data-dir` (ephemeral per-run by default, removed on close; or a named persistent profile) — never the user's real logged-in profile; refuses to attach to a Chrome it didn't launch unless the escalating `browser.real_profile` capability is granted; headless by default (headed available for human takeover)
- [x] M12 sync↔async bridge: new `operator-runtime::browser_lane` module — `BrowserLane` runs a dedicated OS thread with its own multi-threaded tokio runtime, the sync engine talks over std channels only (safe from the sync CLI and from within any other runtime, no nested-runtime panic); `BrowserHandle` lazily spawns Chrome on the first browser step and tears it down at run end; `operator-runtime` gains an `operator-exec-browser` dependency
- [x] M12 new step types: `browser.get_text_all` (querySelectorAll → array), `browser.get_attributes_all` (attribute from all matches → array; href/src resolved to absolute), `browser.snapshot` (compact interactive-element list with CSS-selector refs — the browser analogue of `ui.snapshot`), `browser.download` (`Page.setDownloadBehavior` + wait-for-complete → file path)
- [x] M12 robustness: auto-waiting selectors (get_text/get_attribute/click/type poll up to 5s for the element), real navigation waits (poll `document.readyState` complete, not a fixed sleep), screenshots write a PNG file and return its path (never base64 into the store)
- [x] M12 agent + MCP browser catalogs: MCP gains 8 browser tools (browser_navigate, browser_snapshot, browser_get_text, browser_get_text_all, browser_get_attributes_all, browser_click, browser_type, browser_download; 23 tools total); the agent loop gains those plus browser_screenshot and browser_wait_for (10 browser actions)
- [x] docs/examples/browser-navigate.json + docs/examples/browser-scrape.json (both validate; moved out of the removed pending-m12/ dir)
- [x] 155 Rust tests + 10 Swift tests passing (v0.13.0)
- [x] SPEC_FREEZE_V0.13.md (M13 hands completion & control-flow spec)
- [x] M13 data/network/filesystem steps (system lane): `sys.list_dir {path, pattern?}` (glob + metadata, name-sorted), `data.parse_csv {text|path, headers?}` (RFC-4180-ish → row objects), `data.parse_json {text|path, pointer?}` (RFC-6901 pointer), `http.request {url, method?, headers?, body?}` (blocking, 30s timeout, 1 MiB cap; gated by the domain allow-list AND grants, like `sys.open_url`)
- [x] M13 `ui.scroll` (UI lane) — first scroll verb; new Swift handler `handleScroll` (`Methods/Scroll.swift`) posts scroll-wheel CGEvents (params app/dy/dx/clicks/to:"top"|"bottom"); Swift handler registry 18 → 19 (wired end-to-end, not yet live-exercised)
- [x] M13 deterministic control flow (control lane): `control.foreach`/`control.if`/`control.repeat_until` — sub-steps inside `params` (flat schema unchanged), `{left, op, right}` conditions (`== != < <= > >= contains exists empty`), mandatory iteration caps (100k ceiling; repeat_until requires max_iterations), nesting ≤ 16, `${var}` (type-preserving) + `save_as` + foreach loop var/`__index`; interpreter `operator-runtime::control` runs each leaf through `RunSession::execute_step` (same gate/secret/redaction/audit, each iteration independently audited); `run`/`exec` auto-detect and route control-flow plans (flat plans keep the existing engine path); non-interactive for confirmations
- [x] M13 agent + MCP catalogs: agent actions + MCP tools gain `list_dir`, `parse_csv`, `parse_json`, `http_request` (MCP 23 → 27 tools, completing the M9 surface)
- [x] docs/examples/control-flow-foreach.json
- [x] 163 Rust tests + 10 Swift tests passing (v0.14.0)
- [ ] M13 deferred (NOT started): remaining actuation verbs — right-click/double-click/hover/drag, coordinate `ui.click_at`/`ui.scroll_at` (Retina mapping), typed `ui.set_value` (bool/number), window management, Open/Save dialog helpers; resume-aware batches; skill-compiler stretch (see MASTER_PLAN.md)
- [x] SPEC_FREEZE_V0.14.md (M14 native macOS integration lane spec)
- [x] M14 script lane (system lane, High risk / exec-grade): `script.applescript {script}` / `script.jxa {script}` via `osascript -l AppleScript` / `-l JavaScript` (reuse the `sys.exec` timeout/capture path; return `{stdout, stderr, exit_code, language}`); `shortcuts.run {name, input_path?}` via the macOS `shortcuts` CLI; all three map to the `exec` grant (the script body / shortcut name is the match target); the always-escalate classifier SCANS script bodies (`do shell script` / `administrator privileges` / `sudo` / `rm -rf` / shelling out → human); first `script.*` to an app triggers macOS's one-time Automation (AppleEvents) consent
- [x] M14 document lane (Swift helper / UI lane, Low risk / read-only): `doc.pdf_text {path, max_pages?}` (PDFKit → `{text, page_count, chars}`), `doc.ocr {path, fast?}` (Vision `VNRecognizeTextRequest`, on-device/offline → `{text, line_count}`; also the perception fallback for scanned/image content); new Swift handlers `handlePdfText` / `handleOcr` (`Methods/Document.swift`), registered `doc.pdf_text` / `doc.ocr`; Swift handler registry 19 → 21
- [x] M14 doctor: Full Disk Access probe (lists `~/Library/Mail`) + Automation (AppleEvents) consent note (first `script.*` to an app triggers a one-time macOS dialog)
- [x] M14 recipe skill: `skills/calendar-add-event.yaml` (Calendar event via `script.applescript`; params `summary` / `calendar` / `duration_minutes`; validates under `check-examples`)
- [x] M14 i18n guidance: prefer identifier-based selectors (name-based break on non-English macOS); `KeyPress.swift` keycode-table US-layout audit noted for the future
- [x] M14 verified live: AppleScript `2+3` → `5`, JXA `6*7` → `42`, AppleScript current-date → year; `doc.pdf_text` extracted a real PDF's text; `doc.ocr` OCR'd a rendered PNG → "INVOICE 42"
- [x] 164 Rust tests + 10 Swift tests passing (v0.15.0)
- [ ] M14 known-partial/deferred: the "save PDF attachments from unread invoices → CSV" end-to-end recipe and the OCR-to-agent data-entry flow are ENABLED but NOT assembled/run as one scripted acceptance here (need Mail data + Automation + Full Disk Access consent); Mail/Finder recipe skills beyond Calendar DEFERRED; `shortcuts.run` wired but not exercised (no user Shortcuts on host); i18n keycode-table audit documented, not applied
- [x] SPEC_FREEZE_V0.15.md (M15 trust, residency & measurement spec)
- [x] M15 undo journal (reversibility): copy-before-mutate for filesystem `sys.*` steps; `EngineConfig.undo_dir` (CLI sets it to `<data_dir>/undo` for every real, non-dry-run run) makes the engine snapshot prior state before `write_file`/`append_file`/`copy_path` (backup target or record newly-created), `delete_path` (copy aside), `move_path` (record from→to), `mkdir` (record created dir) into `<undo_dir>/<run_id>/manifest.jsonl`; applies to plan runs AND `RunSession` (agent/MCP) runs; `operator-jack undo <run-id>` replays the manifest in reverse; new `operator-runtime::undo` module
- [x] M15 watch/daemon mode: `operator-jack watch --plan-file <p> [--interval N] [--path <watch>] [--install-launchd]` — interval-only fires each tick, `--path` fires on mtime change (baselined at startup); single-instance run mutex (PID file + live-PID check) serializes runs (a trigger during a run is QUEUED, not dropped or concurrent); `--install-launchd` writes `~/Library/LaunchAgents/com.operator-jack.watch.plist` (RunAtLoad + KeepAlive) and prints `launchctl load`/`unload`
- [x] M15 eval harness (measurement): `operator-jack eval <suite-dir>` runs every `*.json` plan, scores pass (Succeeded, 0 failed steps) / fail, prints a table + pass rate (`--json` supported), exits non-zero if any plan fails
- [x] M15 signing & notarization scaffolding: `release.yml` "Sign & notarize" step (imports a Developer ID cert from secrets into a temp keychain, `codesign` hardened-runtime + timestamp, `notarytool` notarizes each tarball); skips cleanly if signing secrets unset (`MACOS_CERT_P12`, `MACOS_CERT_PASSWORD`, `MACOS_SIGN_IDENTITY`, `AC_API_KEY_ID`, `AC_API_ISSUER_ID`, `AC_API_KEY_P8`)
- [x] M15 verified live: `undo` reversed an overwrite + create + delete run (3 reversed, 0 failed); `watch` fired a plan on a file change (new audited run) and stopped cleanly + `--install-launchd` wrote a valid plist; `eval` ran a 3-plan suite → 3/3 (100%), exit 0
- [x] 166 Rust tests + 10 Swift tests passing (v0.16.0)
- [ ] M15 known-partial/deferred: exec sandboxing (seatbelt) DEFERRED (stretch; the M8 grant/escalation gate + undo + budgets already bound `sys.exec`); signing/notarization SCAFFOLDED but requires an Apple Developer ID cert (unavailable here) so no Gatekeeper-passing build was produced; watch user-activity-pause NOT implemented; the eval CI runner is not wired (needs a self-hosted macOS session)

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
| M5 | DONE | Browser executor (CDP) — `browser` CLI subcommands; plan/agent/MCP-lane wiring landed in M12 |
| M6 | DONE | Skills system (manifest → plan expansion; skill list/validate/run) |
| Phase B | DONE | Stabilization (v0.7.1): audit/security fixes, Swift correctness + test target, docs reconciliation |
| M7 | DONE | Perception / Observation API: ui.snapshot (compact AX + refs), ui.screenshot, Electron/Chromium AX enablement, path selectors, doctor Screen Recording check |
| M8 | DONE | Safety envelope: task-scoped grants (fs/exec/domains), run budgets + `Partial` status, always-escalate classifier, secure-field refusal, redaction hardening, exit code 3, Keychain secret store (`{{secret:name}}` injection) |
| M9 | DONE | MCP server mode: RunSession (ad-hoc gated single-step exec), operator-mcp crate (JSON-RPC-over-stdio, 15 tools), `operator-jack mcp serve --grants`, session-as-run audit; verified live grant enforcement over MCP |
| M10 | DONE | Native agent loop: operator-agent crate (observe→propose one JSON action→execute via RunSession→re-observe), provider `chat()`, `operator-jack agent` CLI; verified live on Calculator via local Ollama |
| M11 | DONE | Long-horizon autonomy & supervision: pause/resume (`RunStatus::Paused`, checkpoint + approval tables), escalation→pause, `ui.human_takeover`, `run/agent --resume`, `approvals` CLI, `logs --follow`/`steer`/notifications, context compaction, `logs prune`, kill-switch hardening; verified live |
| M12 | DONE | Web autonomy: browser lane wired into plans/agent/MCP, profile isolation (isolated per-run Chrome; `browser.real_profile` escalation), extraction verbs (`browser.get_text_all`/`get_attributes_all`/`snapshot`/`download`), sync↔async `browser_lane` bridge; verified live |
| M13 | DONE | Hands completion & deterministic control flow: data/net steps (`sys.list_dir`, `data.parse_csv`/`parse_json`, `http.request`), `ui.scroll`, `control.foreach`/`if`/`repeat_until` (interpreter over RunSession); agent + MCP gain list_dir/parse_csv/parse_json/http_request (MCP → 27 tools); verified live (only `ui.scroll` of the actuation verbs landed — rest deferred) |
| M14 | DONE | Native macOS integration lane: `script.applescript`/`jxa` + `shortcuts.run` (exec-grade grants, classifier scans script bodies), `doc.pdf_text` (PDFKit) + `doc.ocr` (Vision, on-device), doctor Full Disk Access + Automation checks, `calendar-add-event` recipe skill; verified live (Mail/Finder recipes + end-to-end invoice→CSV deferred; `shortcuts.run` not yet exercised) |
| M15 | DONE | Trust, residency & measurement: undo journal + `operator-jack undo` (copy-before-mutate for filesystem `sys.*`; `operator-runtime::undo` module), watch/daemon mode (interval + file triggers, single-instance run mutex with queued triggers, launchd install), eval harness (`operator-jack eval <suite-dir>`, pass/fail scoring + pass rate), signing/notarization scaffolding in `release.yml`; verified live (exec seatbelt sandboxing + a signed Gatekeeper build + watch user-activity-pause + the eval CI runner deferred) |

---

## NEXT_ACTIONS

**The M7-M15 roadmap in MASTER_PLAN.md is COMPLETE (v0.16.0).** M0-M15 plus Phase A and Phase B are all done — there are no more "NOT STARTED" milestones. Several milestones landed partial-scope; the honestly-deferred items below form the post-roadmap backlog. There is no next milestone number to start — pick from this backlog, or from the MASTER_PLAN "Supersession & Future" list.

**Post-roadmap backlog (honestly-deferred, no ordering implied):**

- **M15 — trust/residency:** exec sandboxing (a seatbelt profile generated from the run's fs grants wrapping `sys.exec` — a stretch item; the M8 grant/escalation gate + the undo journal + budgets already bound exec); a Gatekeeper-passing signed/notarized build (the `release.yml` step is scaffolded but needs an Apple Developer ID cert — unavailable here); watch **user-activity-pause** (pause a run when the human is active — not implemented); the **eval CI runner** (the harness runs locally but is not wired into CI — needs a self-hosted macOS session).
- **M14 — native macOS lane:** Mail/Finder recipe skills beyond Calendar; the end-to-end "save all PDF attachments from unread invoices → CSV" recipe and the OCR-to-agent data-entry flow (enabled by the M14 primitives but not assembled/run here — need Mail data + Automation + Full Disk Access consent on the host); a live `shortcuts.run` exercise (no user Shortcuts on the host); the i18n `KeyPress.swift` keycode-table US-layout audit (documented, not applied).
- **M13 — hands:** the remaining actuation verbs — right-click/double-click/hover/drag, coordinate `ui.click_at`/`ui.scroll_at` (Retina mapping), typed `ui.set_value` (bool/number), window management, and Open/Save dialog helpers — plus resume-aware batches and the skill-compiler stretch. (`ui.scroll` landed but was not live-exercised.)
- **M12 — web autonomy:** a live `browser.download` test against a real download server (implemented but exercised only against `data:` URLs here — this machine's antivirus intercepts live http navigations); a full a11y-tree `browser.snapshot` (today a flat interactive-element list); `--dry-run`/`--json` polish on the browser CLI subcommands.

**MASTER_PLAN.md** remains the authoritative reference for the M7-M15 design and the unscheduled Future list (menubar supervisor app, voice input, Safari/Firefox via WebDriver BiDi, plan replay, true concurrent runs, Windows/Linux ports).

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

*Last updated: 2026-07-09 — M15 COMPLETE (v0.16.0), partial-scope. **The M7-M15 roadmap is now fully implemented** — no milestones remain NOT STARTED. Trust, residency & measurement: an **undo journal** (copy-before-mutate for filesystem `sys.*` steps — `EngineConfig.undo_dir` = `<data_dir>/undo` for every real run — snapshotting prior state before `write_file`/`append_file`/`copy_path` (backup/record-created), `delete_path` (copy aside), `move_path` (from→to), `mkdir` (created dir) into `<undo_dir>/<run_id>/manifest.jsonl`, for plan runs AND `RunSession` (agent/MCP) runs; `operator-jack undo <run-id>` replays it in reverse; new `operator-runtime::undo` module); **watch/daemon mode** (`operator-jack watch --plan-file <p> [--interval N] [--path <watch>] [--install-launchd]` — interval + file-mtime triggers, a single-instance run mutex that QUEUES triggers arriving mid-run, and a launchd plist installer for `~/Library/LaunchAgents/com.operator-jack.watch.plist`); an **eval harness** (`operator-jack eval <suite-dir>` scores every `*.json` plan pass/fail + pass rate, `--json`, non-zero exit on any fail); and **signing/notarization scaffolding** (a `release.yml` "Sign & notarize" step — Developer ID `codesign` + `notarytool` — that skips cleanly when its secrets are unset). Crate count unchanged at 11. Known-partial: exec seatbelt sandboxing DEFERRED (the M8 gate + undo + budgets already bound exec); signing is scaffolded but needs an Apple Developer ID cert (unavailable here) so no Gatekeeper-passing build was produced; watch user-activity-pause NOT implemented; the eval CI runner is not wired (needs a self-hosted macOS session). Verified live: undo reversed a 3-mutation run (3 reversed, 0 failed); watch fired a plan on a file change and stopped cleanly + `--install-launchd` wrote a valid plist; eval scored a 3-plan suite 3/3. M0-M15 plus Phase A and Phase B done. 166 Rust tests + 10 Swift tests passing. Newest spec: SPEC_FREEZE_V0.15.md. No next milestone — see NEXT_ACTIONS for the post-roadmap backlog and MASTER_PLAN.md for the Future list.*
