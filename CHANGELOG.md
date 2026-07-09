# Changelog

All notable changes to Operator Jack are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

The M7-M15 roadmap in [`MASTER_PLAN.md`](MASTER_PLAN.md) is now COMPLETE (v0.16.0). Next: post-M15 hardening (close out the honestly-deferred items — exec seatbelt sandboxing, a Gatekeeper-passing signed build, watch user-activity-pause, an eval CI runner) and the MASTER_PLAN "Supersession & Future" backlog.

## [0.16.0] - 2026-07-09

M15 — Trust, residency & measurement. The final roadmap milestone: reversibility (an undo journal), unattended triggers (watch/daemon mode), a measured success rate (eval harness), and release-signing scaffolding — what lets a user run this unattended on their real machine.

### Added
- **Undo journal (reversibility)**: copy-before-mutate for filesystem `sys.*` steps. The CLI sets `EngineConfig.undo_dir` to `<data_dir>/undo` for every real (non-dry-run) run, and the engine snapshots prior state before each mutating step — backup target or record newly-created for `write_file`/`append_file`/`copy_path`, copy-aside for `delete_path`, record from→to for `move_path`, record created dir for `mkdir` — into a manifest at `<undo_dir>/<run_id>/manifest.jsonl`. Applies to plan runs AND `RunSession` (agent/MCP) runs. `operator-jack undo <run-id>` replays the manifest in reverse (restore backups, remove created files, move back, restore deleted). New `operator-runtime::undo` module.
- **Watch/daemon mode**: `operator-jack watch --plan-file <p> [--interval N] [--path <watch>] [--install-launchd]`. Interval-only fires each tick; `--path` fires on mtime change (baselined at startup). A single-instance run mutex (PID file + live-PID check) serializes runs — a trigger that fires during a run is QUEUED (pending), not dropped or run concurrently. `--install-launchd` writes `~/Library/LaunchAgents/com.operator-jack.watch.plist` (RunAtLoad + KeepAlive) and prints the `launchctl load`/`unload` commands.
- **Eval harness (measurement)**: `operator-jack eval <suite-dir>` runs every `*.json` plan in a suite, scores each pass (Succeeded, 0 failed steps) / fail, prints a results table + pass rate (`--json` supported), and exits non-zero if any plan fails.
- **Signing & notarization scaffolding**: `release.yml` gains a "Sign & notarize" step (imports a Developer ID cert from secrets into a temp keychain, `codesign` with hardened runtime + timestamp, `notarytool` notarizes each tarball). It skips cleanly when signing secrets are unset. Required secrets: `MACOS_CERT_P12`, `MACOS_CERT_PASSWORD`, `MACOS_SIGN_IDENTITY`, `AC_API_KEY_ID`, `AC_API_ISSUER_ID`, `AC_API_KEY_P8`.
- SPEC_FREEZE_V0.15.md (M15 trust/residency/measurement spec).

### Note
- Verified live: `undo` reversed an overwrite + create + delete run (3 mutations reversed, 0 failed); `watch` fired a plan on a file change (a new audited run) and stopped cleanly, and `--install-launchd` wrote a valid plist; `eval` ran a 3-plan suite → 3/3 (100%), exit 0. Full gate green: `cargo fmt`, `cargo clippy -D warnings`, 166 Rust tests, 10 Swift tests, and `check-examples.sh` all pass. Version 0.15.0 → 0.16.0 (workspace + Swift `helperVersion`).
- Known-partial (stated honestly): exec sandboxing (seatbelt) is DEFERRED (a stretch item; the M8 grant/escalation gate + the undo journal + budgets already bound `sys.exec`); signing/notarization is SCAFFOLDED but needs an Apple Developer ID cert (unavailable here), so no Gatekeeper-passing build was produced; watch user-activity-pause is NOT implemented; the eval CI runner is not wired (it needs a self-hosted macOS session).

## [0.15.0] - 2026-07-09

M14 — Native macOS integration lane. A script lane (AppleScript/JXA/Shortcuts) and a read-only document lane (PDF text + OCR) that let plans and the agent reach the parts of macOS the Accessibility tree can't — app scripting dictionaries, Shortcuts, and the text inside PDFs and scanned images.

### Added
- **Script lane** (system lane, High risk, exec-grade): `script.applescript {script}` and `script.jxa {script}` run via `osascript -l AppleScript` / `-l JavaScript` (reusing the `sys.exec` timeout/capture path; return `{stdout, stderr, exit_code, language}`); `shortcuts.run {name, input_path?}` invokes the macOS `shortcuts` CLI. All three map to the `exec` grant capability (the script body / shortcut name is the match target), and the always-escalate classifier now SCANS script bodies — `do shell script`, `administrator privileges`, `sudo`, `rm -rf`, and shelling out escalate to a human.
- **Document lane** (Swift helper / UI lane, Low risk, read-only): `doc.pdf_text {path, max_pages?}` extracts text via PDFKit → `{text, page_count, chars}`; `doc.ocr {path, fast?}` runs Vision's `VNRecognizeTextRequest` on-device/offline → `{text, line_count}` (also the perception fallback for scanned/image content). New Swift handlers `handlePdfText` / `handleOcr` (`Methods/Document.swift`), registered as `doc.pdf_text` / `doc.ocr`; Swift handler registry 19 → 21.
- **`doctor`** gains a Full Disk Access probe (lists `~/Library/Mail`) and an Automation (AppleEvents) consent note (the first `script.*` to an app triggers a one-time macOS dialog).
- **Recipe skill**: `skills/calendar-add-event.yaml` (creates a Calendar event via `script.applescript`; params `summary` / `calendar` / `duration_minutes`; validates under `check-examples`).
- **i18n guidance**: prefer identifier-based selectors (name-based selectors break on non-English macOS); a US-layout audit of the `KeyPress.swift` keycode table is noted for the future.
- SPEC_FREEZE_V0.14.md (M14 native macOS integration lane spec).

### Note
- Verified live: AppleScript `2+3` → `5`, JXA `6*7` → `42`, an AppleScript current-date returning the year; `doc.pdf_text` extracted a real PDF's text; `doc.ocr` OCR'd a rendered PNG → "INVOICE 42". Full gate green: `cargo fmt`, `cargo clippy -D warnings`, 164 Rust tests, 10 Swift tests, and `check-examples.sh` all pass. Version 0.14.0 → 0.15.0 (workspace + Swift `helperVersion`).
- Known-partial: the "save all PDF attachments from unread invoices to a CSV" end-to-end recipe and the OCR-to-agent data-entry flow are ENABLED by these primitives but were not assembled/run as one scripted acceptance here (they need Mail data + Automation + Full Disk Access consent on the host); Mail/Finder recipe skills beyond Calendar are DEFERRED; `shortcuts.run` is wired but not exercised (no user Shortcuts on the host); the i18n keycode-table audit is documented, not applied.

## [0.14.0] - 2026-07-09

M13 — Hands completion & deterministic control flow. Data/network/filesystem steps, the first scroll verb, and deterministic control flow (foreach/if/repeat_until) so plans can loop, branch, and process data without a round-trip to an LLM.

### Added
- **Data/network/filesystem steps** (system lane): `sys.list_dir {path, pattern?}` (glob + metadata, name-sorted); `data.parse_csv {text|path, headers?}` (RFC-4180-ish — quoted fields, embedded commas, `""` escapes → row objects); `data.parse_json {text|path, pointer?}` (RFC-6901 JSON pointer); `http.request {url, method?, headers?, body?}` (blocking, 30s timeout, 1 MiB body cap; gated by the domain allow-list AND grants, like `sys.open_url`).
- **`ui.scroll`** (UI lane) — the first scroll verb; the Swift handler posts scroll-wheel CGEvents (params `app`, `dy`, `dx`, `clicks`, `to: "top"|"bottom"`). New Swift handler `handleScroll` (`Methods/Scroll.swift`); handler registry 18 → 19.
- **Deterministic control flow** (control lane): `control.foreach {items, as, steps, max_iterations?}`, `control.if {condition, then, else?}`, and `control.repeat_until {steps, condition, max_iterations}`. Bodies carry sub-steps inside `params` (the flat plan schema is unchanged). Conditions are `{left, op, right}` with ops `== != < <= > >= contains exists empty` (numeric or string compare). Iteration caps are mandatory (100k hard ceiling; `repeat_until` requires `max_iterations`) and nesting is bounded at 16. Variables: `${var}` (type-preserving), `save_as` binds a leaf's output, `foreach` binds the loop var + `__index`. The interpreter (`operator-runtime::control`) runs each leaf through `RunSession::execute_step` — the same policy gate, `{{secret:*}}` resolution, redaction, and audit as plan execution — and audits each iteration independently. `operator-jack run`/`exec` auto-detect control-flow plans and route them through the interpreter over a RunSession (flat plans keep the existing engine path). Control-flow plans are non-interactive for confirmations (they need `--yes` or a grant for safe-mode medium/high steps).
- **Agent + MCP catalogs** — the agent loop gains actions `list_dir`, `parse_csv`, `parse_json`, `http_request`; the MCP catalog gains the matching tools `list_dir`, `parse_csv`, `parse_json`, `http_request` (23 → 27 tools, completing the M9 surface).
- `docs/examples/control-flow-foreach.json` (new example; validates under `check-examples`).
- SPEC_FREEZE_V0.13.md (M13 hands completion & control-flow spec).

### Note
- Verified live: a `parse_csv → foreach → if → write_file` per-row plan ran `Succeeded` with correct branches; standalone `parse_csv` (quoted/escaped fields), `parse_json` (pointer), and `list_dir` (glob) were each verified. Full gate green: `cargo fmt`, `cargo clippy -D warnings`, 163 Rust tests, 10 Swift tests, and `check-examples.sh` all pass.
- Known-partial: only `ui.scroll` of the planned M13 actuation verbs landed — right-click/double-click/hover/drag, coordinate `ui.click_at`/`ui.scroll_at` (Retina mapping), typed `ui.set_value` (bool/number), window management, and Open/Save dialog helpers are deferred, as are resume-aware batches and the skill-compiler stretch. `ui.scroll` compiles and is wired end-to-end but was not live-exercised (it needs Accessibility permission and a live window).

## [0.13.0] - 2026-07-09

M12 — Web autonomy (browser lane wiring). The M5 CDP executor — previously reachable only via the `browser` CLI subcommands, with `browser.*` plan steps hard-rejected — is now a first-class lane wired into plans, the agent loop, and MCP, running against an isolated per-run Chrome. Unlocks the "scrape a website, install what you need" scenario.

### Added
- **Browser lane wired into plans, the agent loop, and MCP** — the `browser.*` hard-reject in `validation.rs` is removed and a `"browser"` arm was added to the engine's lane dispatch; browser steps now route through policy, redaction, and audit like every other lane.
- **Profile isolation** (security-critical, landed first) — the lane launches its OWN Chrome with a dedicated `--user-data-dir` (ephemeral per-run by default, removed on close; or a named persistent profile), never the user's real logged-in profile. It refuses to attach to a Chrome it didn't launch unless the escalating `browser.real_profile` capability is granted. Headless by default (headed available for human takeover).
- **Sync↔async bridge** — a new `operator-runtime::browser_lane` module. `BrowserLane` runs a dedicated OS thread with its own multi-threaded tokio runtime; the sync engine talks to it over std channels only, so it is safe from the sync CLI and from within any other runtime (no nested-runtime panic). `BrowserHandle` lazily spawns Chrome on the first browser step and tears it down at run end. `operator-runtime` gains an `operator-exec-browser` dependency.
- **New step types**: `browser.get_text_all` (querySelectorAll → array), `browser.get_attributes_all` (attribute from all matches → array; href/src resolved to absolute), `browser.snapshot` (compact interactive-element list with CSS-selector refs — the browser analogue of `ui.snapshot`), and `browser.download` (`Page.setDownloadBehavior` + wait-for-complete → file path).
- **Robustness**: selectors on get_text/get_attribute/click/type auto-wait up to 5s for the element (Playwright-style); navigation waits poll `document.readyState` complete instead of a fixed sleep; screenshots write a PNG file and return its path (never base64 into the store).
- **Agent + MCP browser catalogs** — MCP gains 8 browser tools (`browser_navigate`, `browser_snapshot`, `browser_get_text`, `browser_get_text_all`, `browser_get_attributes_all`, `browser_click`, `browser_type`, `browser_download`; 23 tools total); the agent loop gains those plus `browser_screenshot` and `browser_wait_for` (10 browser actions).
- `docs/examples/browser-navigate.json` and a new `docs/examples/browser-scrape.json` now validate (moved out of the removed `pending-m12/` holding dir).
- SPEC_FREEZE_V0.12.md (M12 web-autonomy spec).

### Note
- Verified live: a plan navigate → get_text → get_text_all → get_attributes_all → snapshot scraped a page (`get_text_all` → `["Alpha","Beta","Gamma"]`, `get_attributes_all(href)` → `["/a","/b","/c"]`). Tests use self-contained `data:` URLs because this machine's antivirus intercepts live http navigations. Full gate green: `cargo fmt`, `cargo clippy -D warnings`, 155 Rust tests, 10 Swift tests, and `check-examples.sh` all pass.
- Known-partial: downloads are implemented but not exercised against a live download server here; `browser.snapshot` is a flat interactive-element list, not a full a11y tree; the browser CLI subcommands' `--dry-run`/`--json` polish is deferred.

## [0.12.0] - 2026-07-09

M11 — Long-horizon autonomy & supervision. Runs can now pause, survive process exit, and resume after human approval — plus live supervision (follow, steer), pause notifications, transcript compaction, and audit-log pruning. Turns the one-shot executor into a supervisable, resumable agent.

### Added
- **Pause/resume infrastructure** — a new non-terminal `RunStatus::Paused` (resumable, exit code 0) and two SQLite tables (migration #2): `checkpoints` (one per run: cursor, variables, agent transcript, goal, reason) and `approvals` (id, run_id, kind `escalation`|`takeover`, status `pending`|`approved`|`denied`). New store methods: `save`/`load`/`delete_checkpoint`, `create_approval`, `list_approvals`, `get_approval`, `latest_approval_for_run`, `decide_approval`, `prune_runs`.
- **Escalation → pause** (replaces M8's interim clean-fail): when the always-escalate classifier flags an action, the run checkpoints, opens a pending approval, posts a macOS notification, and ends `Paused` (exit 0) instead of hard-failing. Resume consumes a one-shot escalation allowance (`PolicyGate::grant_one_escalation`). (MCP still surfaces escalation as a tool error, since it has no run loop.)
- **`ui.human_takeover` step** — a new control-lane step type (risk Low): an interactive plan run blocks for Enter; a non-interactive run pauses for out-of-band resume; the agent loop can emit it as a control action. The agent never types credentials or solves CAPTCHAs.
- **Resume**: `operator-jack agent --resume <run-id>` and `operator-jack run --resume <run-id>` — both verify the run's approval is granted, then continue (the agent re-seeds the saved transcript; a plan skips completed steps via the checkpoint cursor).
- **Approvals CLI**: `operator-jack approvals list [--all]`, `approvals approve <id>`, `approvals deny <id>`.
- **Supervision DX**: `operator-jack logs --follow [<run-id>]` (tail per-run JSONL; also `logs follow`) and `operator-jack steer <run-id> "<message>"` (inject guidance into a running agent's next turn); macOS notifications on pause.
- **Context compaction**: long agent transcripts (> ~24k chars) fold their oldest turns into a summary breadcrumb, keeping recent turns verbatim.
- **Retention/pruning**: `operator-jack logs prune [--older-than-days N] [--keep M]` (also `logs --prune`) deletes old runs and cascades to `step_results`/`events`/`checkpoints`/`approvals` — fixes the audit's unbounded-growth finding.
- **Kill-switch hardening**: SIGINT/SIGTERM sets the cancel flag; the engine always writes a terminal `Cancelled` status + `RunCancelled` event and disconnects the helper.
- SPEC_FREEZE_V0.11.md (M11 long-horizon & supervision spec).

### Note
- Verified live: a plan with a `ui.human_takeover` step paused non-interactively (checkpoint + approval written, exit 0), refused `--resume` before approval, and completed after `approvals approve` + `--resume`; `logs prune` reclaimed 82 runs. Full gate green: `cargo fmt`, `cargo clippy -D warnings`, 152 Rust tests, 10 Swift tests, and `check-examples.sh` all pass.

## [0.11.0] - 2026-07-09

M10 — Native agent loop. Operator Jack drives *itself*: observe the screen → propose ONE JSON action → execute it → re-observe, repeating until the goal is done or a limit is hit. The definitional autonomy feature — no hand-written plan.

### Added
- `operator-agent` crate — the native autonomous loop (`operator_agent::run_agent(goal, provider, &mut RunSession, max_iters) -> AgentOutcome`, with `AgentStatus` = `Done | MaxIterations | Aborted`): each turn observes the screen, asks the model to propose ONE JSON action, executes it through a `RunSession` (the same policy gate, grants, escalation classifier, `{{secret:*}}` resolution, redaction, and audit as plan execution), then re-observes — repeating until the model emits `done`, `--max-iters` is reached, or the provider errors. Agent actions: `open_app`, `focus_app`, `snapshot`, `click`, `type_text`, `key_press`, `set_value`, `select_menu`, `read_text`, `wait_for`, `read_file`, `write_file`, `exec`. `snapshot` returns a compact AX tree with element refs (`e1`, `e2`…) that the model then acts on; refs survive across turns via the persistent helper. Malformed or unknown actions and `ELEMENT_AMBIGUOUS` are fed back to the model for self-correction.
- `operator-jack agent "<goal>" [--max-iters N] [--provider P] [--grants <file>]` — runs the agent loop. It is autonomous and non-interactive (auto-approves confirmations; safety comes from the grant envelope + escalation classifier + secure-field refusal). It prints the effective grant envelope first; the whole run is one session-run in the store, and every executed action is audited.
- `LlmProvider::chat(system, user) -> Result<String, PlannerError>` on the provider trait, implemented for all four providers (Kimi, OpenAI, Anthropic, Ollama); `generate_plan` is now `chat` plus plan extraction.
- SPEC_FREEZE_V0.10.md (M10 native agent-loop spec).

### Note
- Verified live against a local Ollama model (`llama3.2:3b`): open_app → snapshot → click → snapshot → click on Calculator, 6 audited steps under a single run. Task-completion quality tracks the model — a stronger provider via API completes goals a small local model may not. Full gate green: `cargo fmt`, `cargo clippy -D warnings`, 146 Rust tests, 10 Swift tests, and `check-examples.sh` all pass.

## [0.10.0] - 2026-07-09

M9 — MCP server mode. Operator Jack's gated executors exposed as MCP tools so any MCP client (Claude Code/Desktop, others) can drive the audited hands — "brain rented, hands owned".

### Added
- **RunSession** (`operator_runtime::engine::RunSession`) — an ad-hoc single-step gated execution primitive (`open` → `execute_step(&Step)` → `finish`). Each step runs through the same policy gate (grants, escalation classifier), `{{secret:*}}` resolution, store-boundary redaction, and audit as plan execution; it holds a persistent helper connection so snapshot element-refs survive across calls, and records one run per session. `execute_plan` is unchanged — both share the security-critical helpers. This is the shared substrate M10's agent loop will consume.
- `operator-mcp` crate and `operator-jack mcp serve [--grants <file>]` — a hand-rolled, spec-compliant MCP server over stdio (newline-delimited JSON-RPC 2.0: `initialize`, `tools/list`, `tools/call`, `ping`), exposing **15 tools**: `snapshot`, `screenshot`, `click`, `type_text`, `key_press`, `set_value`, `select_menu`, `read_text`, `wait_for`, `list_apps`, `focus_app`, `open_app`, `read_file`, `write_file`, `exec` (click/read_text/set_value take a selector or an element_ref from a prior snapshot). The server prints the effective grant envelope ("this session may: …") to STDERR before serving (stdout is protocol-only) and warns if unrestricted; because stdio is the protocol channel, the session auto-approves confirmations and relies on the grant envelope + escalation classifier + secure-field refusal for safety.
- Every MCP tool call is gated and audited through RunSession as one session-run — grants, the escalation classifier, redaction, and the audit trail apply to model-driven actions exactly as to plan steps. A denied call returns `isError` (the session continues), not a JSON-RPC error. Verified live: a `write_file` inside an fs_write grant succeeded, while the same write outside the grant returned `isError` ("Grant required: 'fs_write'") with no file created.
- SPEC_FREEZE_V0.9.md (M9 MCP server spec).

### Note
- The MCP server is hand-rolled rather than built on the `rmcp` Rust SDK: rmcp's async/Send bounds don't fit the synchronous RunSession (rusqlite + child-process pipes). This is a deliberate, documented choice — rmcp can be adopted later without changing the tool surface.

## [0.9.1] - 2026-07-08

Completes M8 (the last deferred sub-feature).

### Added
- Keychain secret store: `operator-jack secret set|list|rm`, storing generic
  passwords under the `operator-jack` service in the macOS login keychain.
- `{{secret:name}}` references in any plan/skill param, resolved to the keychain
  value at execution time — only for dispatch. The audited params (`input_json`),
  JSONL logs, and the saved plan keep the reference form, so the literal secret
  never enters the store or logs (verified: end-to-end injection with no leak in
  SQLite or JSONL).

## [0.9.0] - 2026-07-08

M8 — the Safety Envelope. A grantable, bounded, auditable policy model so a model can drive the executors without constant prompts or unbounded blast radius.

### Added
- **Task-scoped grants** on plans and config: `grants: { fs_read, fs_write, exec, domains }` (glob / command / host patterns). Restrictions are additive (absent = unrestricted) and the config and plan layers must both allow (most restrictive wins). Enforced in the runtime policy gate on interpolated params; filesystem paths are canonicalized (`~`, `..`) so they cannot escape a grant. A violation is denied with a message naming the exact grant.
- **Run budgets** on plans and config: `budget: { max_steps, max_wall_ms, max_bytes_written, max_llm_calls, max_llm_cost_usd }` (the LLM fields are dormant until M10). Checked in the engine step loop; on exhaustion the run ends with a new terminal `Partial` run status, with completed step results preserved.
- **Always-escalate classifier** (non-overridable by grants or `--yes`): `sys.exec` containing `sudo`, `rm -rf`, or a pipe-to-shell; `sys.delete_path` that would remove more than 25 entries; `ui.click`/`ui.key_press` on Send/Buy/Pay/Checkout-like controls in Mail/Messages/Slack; and a hard Swift-helper refusal to type or set a value into a secure text field (`AXSecureTextField`).
- Exit codes: policy/grant/escalation denial → `3`; a `Partial` (budget-exhausted) run → `7`.
- SPEC_FREEZE_V0.8.md (M8 safety envelope spec).
- `docs/examples/grants-scoped-write.json` (fs_write grant + budget).

### Security
- `plan_json` is now redacted at rest in the store (previously the one unredacted CRUD write).
- The blanket "any string containing a slash is exempt from redaction" rule is replaced by a precise looks-like-a-path test, so slash-bearing base64 secrets are redacted again while real filesystem paths stay exempt.

### Note
- The Keychain secret store and `{{secret:name}}` injection are deferred to a 0.9.x follow-on (M8.5). The secure-field refusal (the safety half) shipped here; the credential-injection half follows once its consumers (M9 MCP, M10 agent loop) exist.

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
