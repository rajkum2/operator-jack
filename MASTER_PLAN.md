# Operator Jack — Master Plan v1.1

> **Status:** ACTIVE — this is the consolidated roadmap that supersedes `docs/ROADMAP.md` (M7/M8 as
> defined there are absorbed/re-scoped below; see "Supersession" section).
> **Implementation status:** M7-M15 implemented as of v0.16.0 (see PROJECT_CONTEXT.md VERSION_LOG;
> some items are partial-scope — see the per-milestone specs and PROJECT_CONTEXT NEXT_ACTIONS). The
> deliverable checkboxes below are the original plan of record and are intentionally left unchecked.
> **Created:** 2026-07-08, from two verified analyses: the full-codebase audit (build/test/run
> verification of v0.7.0) and the autonomy vision gap analysis (code-grounded + 2026 industry
> research). Revised same day after a three-way adversarial review (feasibility vs code,
> completeness vs sources, internal coherence).
> **How to use:** Work top-to-bottom. Each milestone ends with a working build, passing tests, a
> regression run of ALL shipped examples, and an update to `PROJECT_CONTEXT.md`. Do not start a
> milestone whose dependencies are unfinished.

---

## 1. Vision

Operator Jack becomes a **fully autonomous macOS computer-use agent**: given a natural-language
instruction ("scrape this website — install whatever you need", "enter this spreadsheet into that
form", "watch my email and file invoices"), it perceives the screen, reasons, acts, observes the
result, and self-corrects — under a policy envelope the user granted once, with a complete audit
trail, on the user's real machine.

## 2. Strategy (why this order)

Three theses drive the sequencing:

1. **The hands are built; the eyes and brain are not.** v0.7.0 is a deterministic, policy-gated,
   audited executor (verified working end-to-end for sys.* and ui.* lanes). But the LLM is consulted
   exactly once, blind (`prompt.rs` sends no screen state), plans are frozen linear lists (no loops,
   no replanning), and no perception output ever reaches a model. Autonomy = adding an
   observe→reason→act loop **on top of** the existing executor, not replacing it.
2. **The 2026 industry converged on our architecture's missing half.** Accessibility-tree-first
   observation with vision fallback (Playwright-MCP-style compact ref-ID snapshots), client-executed
   agent loops, and **executor-enforced** safety gates are the winning pattern. Our AX-tree bet and
   policy-gate-in-the-runtime design are exactly right; what's missing is the loop and the snapshot.
3. **MCP mode ships the vision earliest.** Exposing Operator Jack as an MCP server ("brain rented,
   hands owned") lets Claude/GPT/Gemini drive our audited executors months before our native agent
   loop matures — and forces the exact tool-surface design the native loop will reuse. No existing
   macOS MCP server has our policy/audit/redaction layer; that is the moat, and the window is closing.

**The safety envelope (M8) is a hard precondition for both MCP and agent modes.** Today autonomous
operation requires `--mode unsafe --yes` — i.e. zero policy. No model drives the executors until
grants, budgets, secrets, and the escalation classifier exist.

## 3. Milestone Overview

| Milestone | Version | Spec | Theme | Size | Depends on |
|---|---|---|---|---|---|
| **Phase B** | v0.7.1 | V0.5 (retroactive) | Stabilize & reconcile (audit fixes, docs, distribution) | M | — |
| **M7** | v0.8.0 | V0.7 | Perception: observation API (snapshot, screenshot, Electron AX) | L | Phase B |
| **M8** | v0.9.0 | V0.8 | Safety envelope: grants, budgets, secrets, escalation | L | Phase B |
| **M9** | v0.10.0 | V0.9 | MCP server mode — first shippable autonomy | M/L | M7, M8 |
| **M10** | v0.11.0 | V0.10 | Native agent loop: provider layer v2 + orchestrator | L | M7, M8, M9 (RunSession + tool surface) |
| **M11** | v0.12.0 | V0.11 | Long-horizon: pause/resume, approvals, takeover, supervision | L | M8, M10 |
| **M12** | v0.13.0 | V0.12 | Web autonomy: browser isolation → plan wiring → extraction | L | M7, M8, M10; M9 (tool catalog); M11 (auth takeover) |
| **M13** | v0.14.0 | V0.13 | Hands completion + deterministic control flow + data steps | L | M7 (refs); M11 (resume-aware batches only); M10 (skill-compiler stretch only) |
| **M14** | v0.15.0 | V0.14 | Native macOS lane: AppleScript/Shortcuts, PDF, OCR | M | M8; M10 (OCR-to-agent acceptance item only) |
| **M15** | v0.16.0 | V0.15 | Trust & residency: undo, watch mode, eval harness, signing | L | M8, M10 |

Parallelization: M7 and M8 can proceed concurrently (different layers). M13's actuation verbs,
control flow, and data steps can run parallel to M10–M12, but its resume-aware-batches deliverable
(and the resume acceptance criterion) gate on M11, and the skill-compiler stretch gates on M10.
M14 can run parallel to M12/M13 once M8 is done (its OCR-to-agent acceptance item waits for M10).
Everything else is ordered.

**What ships the vision when:**
- After **M9**: first demoable autonomy — Claude drives Operator Jack's gated hands via MCP.
- After **M12**: the "scrape a website, install what you need" scenario works end-to-end.
- After **M13**: the "data entry from CSV" scenario works deterministically with resume.
- After **M14+M15**: the "watch my email and file invoices" scenario works unattended.

---

## Phase B — Stabilize & Reconcile (v0.7.1)

**Goal:** Make what exists true, green, and shippable. Every later milestone builds on artifacts the
audit proved broken or stale. No new features — but shipped-code security bugs are in scope.
**Spec:** `SPEC_FREEZE_V0.5.md` written retroactively (see B6) — covers the M6 skills manifest as
built + fixed, plus **security errata to V0.2**: the mode-ceiling semantics change and the
newly-enforced ui-lane timeout behavior (B4), so the "spec is normative" rule holds through Phase B.

### B1. Pipeline & repo hygiene (S)
- [ ] `cargo fmt` the workspace (CI is red on main at the fmt check: `main.rs:327`, `main.rs:1756`).
- [ ] Commit the regenerated `Cargo.lock` — the lock **at HEAD** records 0.6.0 crate versions
      (verified via `git show HEAD:Cargo.lock`), so `--locked` builds fail from a clean checkout;
      the working tree already carries the 0.7.0 regeneration, it just was never committed.
- [ ] Verify `cargo clippy` clean; add `--locked` to release.yml build steps.

### B2. Swift helper correctness (M) — blocks M7 reliability
- [ ] Fix `findElements` root-skip: `AXUtilities.swift:308` (`currentDepth > 0`) makes any
      `{role: AXWindow}` selector unmatchable. Match the root too.
- [ ] Fix `WaitFor.swift:49`: window is resolved BEFORE the poll loop → instant `WINDOW_NOT_FOUND`
      instead of polling out the timeout. Move resolution inside the loop.
- [ ] Fix waitFor `index >= 1` never matching (`maxResults: 1` truncation before index post-filter).
- [ ] Fix the shipped silent-no-op selector shapes (same defect class, shipped in M3b): `any_of`
      is ignored by find/waitFor and the `path` field is parsed but never matched — both currently
      **match everything** (wrong-element-action hazard). Implement, or at minimum reject loudly at
      Rust validation until M7 implements them properly.
- [ ] Wire `resolveElementRef` (dead code at `AXUtilities.swift:615`) into Click/ReadText/SetValue —
      Rust validation already accepts element_ref-only params; Swift rejects them (integration break).
- [ ] Replace the `as! AXUIElement` force cast in `SelectMenu.swift:44` with a guarded cast.
- [ ] Sync the hardcoded helper version `"0.1.0"` in `Ping.swift:8` with the workspace version.
- [ ] **Create the Swift test target** (none exists — 1,910 LOC, 0 tests). Seed with regression
      tests for the waitFor/findElements bugs above.

### B3. Shipped examples & skills must pass (M)
- [ ] Fix `docs/examples/calculator-buttons.json` (modern Calculator exposes identifiers
      `One/Add/Two/Equals`, not names "1"/"+" — verified working plan exists from the audit).
- [ ] Fix `skills/create-note.yaml` (`path` → `menu_path`; `keys` array → `key` + `modifiers`) and
      `skills/take-screenshot.yaml` (same `keys` bug, missing `app`, dead `mode` param).
- [ ] `cmd_skill_run` must call `validate_plan` before executing (it skips validation entirely today).
- [ ] Fix skill numeric interpolation: params degrade to floats (`manifest.rs:394-398`) that Swift's
      `.int` matching silently drops — emit integers for integer-typed values.
- [ ] Add `make check-examples`: validate all example plans + dry-run all skills. Run it in CI.
      **Rule going forward: no example or skill ships without passing this.**

### B4. Runtime & shipped-code correctness (M)
- [ ] Enforce ui-lane `timeout_ms`: helper IPC is child-process stdin/stdout pipes, which have no
      read-deadline API — enforcement needs a reader-thread + `mpsc::recv_timeout` wrapper around
      the NDJSON read in `HelperClient::send` (mirroring the system lane), constructing the dead
      `IpcError::Timeout` variant. Note: after a timeout the response stream is desynced — kill and
      respawn the helper (crash-recovery path already exists).
- [ ] Kill the child process on `sys.exec` timeout. This changes the `operator-exec-system` exec
      handler contract (timeout moves into the executor, or a kill handle is returned) — the Child
      handle is not reachable from `engine.rs` today; not an engine-only tweak.
- [ ] **Security fix — mode ceiling:** `effective_mode` lets `plan.mode` override the configured
      mode upward (`engine.rs:126-129`), so a generated plan can self-escalate to unsafe. Config/CLI
      mode becomes a ceiling: `effective = min(plan.mode, config.mode)`. Documented as V0.2 errata
      in the retroactive SPEC_FREEZE_V0.5 (see B6).
- [ ] **Security fix — browser JS injection:** the browser executor's JS string escaping handles
      only single quotes, and second interpolations aren't escaped at all (`executor.rs:167-211`) —
      injectable, and the browser CLI subcommands ship today. Fix escaping (quotes, backslashes,
      newlines) now; don't wait for M12.
- [ ] Fix non-char-boundary byte slicing panics: `main.rs:1267` (`&v[..40]`), `main.rs:1794` (`&desc[..28]`).
- [ ] Make `resolve_helper_path` consult config `helper_path` (parsed but never read — dead setting
      that also explains stale-helper pickup via `which`).
- [ ] Planner quick fixes (code, not docs): fix the fence-extraction bug (unclosed ```json fence
      guarantees a ParseError); bump default model IDs to current-generation equivalents
      (interim — superseded by M10's provider layer v2); remove the misleading "configure in config
      file" from `ApiKeyNotFound` (no such option exists).

### B5. Distribution (S–M)
- [ ] Bump the Homebrew tap to v0.7.1 and add a tap-update step to `release.yml` (tap is frozen at
      0.4.0 — brew users get a 3-versions-old binary today).
- [ ] Add an **Installation** section to README (install.sh, brew, releases — none are mentioned today).
- [ ] Remove false README claims (`plan list`, `--set` — neither exists) and document the real
      flags (`--interactive`, `--no-interactive`, `--allow-domains`).
- [ ] Refresh the local install (machine has stale 0.4.0 at `/opt/homebrew/bin`).

### B6. Documentation reconciliation (M)
- [ ] `CLAUDE.md`: milestone table (M5/M6 are DONE), workspace tree (+`operator-exec-browser`,
      `operator-skills`, `skills/`), test count (118), CLI commands (+`skill`, `browser` trees),
      handler file paths (`Methods/` not `Handlers/`), tokio rule (obsolete post-M5), active spec refs.
- [ ] `PROJECT_CONTEXT.md`: add v0.6.0/v0.7.0/v0.7.1 VERSION_LOG entries, fix CURRENT_STATE (9 crates,
      M5 artifacts), fix NEXT_ACTIONS + footer (currently contradict the milestone table), correct the
      chromiumoxide claim (implementation is hand-rolled tokio-tungstenite).
- [ ] `CHANGELOG.md`: backfill 0.4.0 → 0.7.1.
- [ ] **Write `SPEC_FREEZE_V0.5.md` retroactively** — the M6 skills spec that was never written
      (phantom-referenced by PROJECT_CONTEXT and by V0.6's "predecessor" header). Document skills
      as-built + the B3 fixes, plus the B4 security errata (mode ceiling, ui-lane timeout).
- [ ] Mark `docs/ROADMAP.md` as superseded by this file (keep for history; update statuses).
- [ ] Planner truth-in-naming: it is an LLM planner, not "rule-based" — fix `lib.rs:3`, CLAUDE.md.
- [ ] `doctor` gains a **provider-readiness check** (API key present per provider / Ollama model
      pulled) so `do` being unusable is diagnosed, not discovered.

**Acceptance:** CI green on main. `cargo build --locked` from a clean checkout works. All examples
validate; calculator-buttons passes live; all 3 skills dry-run clean. `do "…" --show-plan` works
end-to-end against at least one configured provider, and `doctor` reports provider readiness. Tap
installs v0.7.1. Docs agree with the code and each other.

---

## M7 — Perception: the Observation API (v0.8.0)

**Goal:** Give models eyes. Everything autonomous perceives through this layer.
**Spec:** `SPEC_FREEZE_V0.7.md` (observation API: snapshot format, ref semantics, screenshot params,
and the token-measurement rule — use a fixed chars/4 heuristic so budgets are testable).

### Deliverables
- [ ] **`ui.snapshot`** step + IPC method + Swift handler: compact indented-text serialization of
      the AX tree (one line per element: role, name, value, enabled/focused flags), interactive
      elements tagged with short refs (`e1`, `e2`…) backed by `ElementRefStore` ULIDs, refs
      invalidated per-snapshot (Playwright MCP pattern). Depth, window-scope, and
      **interactive-only filtering** controls — filtering is the hard part of this milestone, not
      serialization: Electron apps expose thousands of nodes once B7 lands.
- [ ] **`ui.screenshot`** step: ScreenCaptureKit, screen- or window-scoped, downscaled (~1370px long
      edge), writes PNG to run dir + returns path (never base64 into the store). Screen Recording
      permission check added to `doctor`.
- [ ] **Electron/Chromium AX enablement:** set `AXManualAccessibility` / `AXEnhancedUserInterface`
      on the target app element on attach. One attribute; unlocks Slack/VS Code/Discord/Chrome web
      content, which are invisible to the helper today. (S — do this first.)
- [ ] **`AXUIElementSetMessagingTimeout`** on app elements so a beach-balling target degrades
      gracefully instead of hanging each attribute read.
- [ ] Actions accept refs: `ui.click`/`ui.read_text`/`ui.set_value` accept `ref` from the last
      snapshot (resolves via ElementRefStore; falls back to selector re-resolution when stale —
      completing the element_ref system started in M3b).
- [ ] ElementRefStore eviction (unbounded today; clear on snapshot, cap size).
- [ ] Full implementation of `any_of` and `path` selector semantics (B2 made them fail loudly;
      this makes them work everywhere, including find/waitFor).
- [ ] Helper polish: `listWindows` frontmost assumption, `FocusWindow` title-collision index recovery.

**Acceptance:** Snapshot of Calculator ≤ 500 tokens (chars/4) with clickable refs; `click ref=eN`
works. Snapshot of VS Code/Slack shows web/Electron content **at ≤ 2,000 tokens with
interactive-only filtering at default depth**. Screenshot lands in the run dir and `doctor` reports
Screen Recording status. Swift tests cover snapshot serialization and ref resolution.

---

## M8 — The Safety Envelope (v0.9.0)

**Goal:** Replace "confirm every step OR trust everything" with a grantable, bounded, auditable
envelope — the precondition for any model-driven execution, and the product's moat. Includes the
secret store, because a hard refusal on password fields without a secrets path would leave M9/M10
unable to perform any authenticated task.
**Spec:** `SPEC_FREEZE_V0.8.md` (grants schema, budgets + the `Partial` terminal run status,
escalation rules, `{{secret:*}}` syntax, redaction changes).

### Deliverables
- [ ] **Task-scoped grants**: `Grants` struct on Plan + Config (intersected, like allowlists today):
      `fs_read`/`fs_write` glob lists, `exec` command patterns, per-lane `domains`, `apps`. Enforced
      in `check_step_with_params` (which already receives interpolated params — the right seam).
      Use `globset`; canonicalize paths (symlinks, `..`) before matching. Deny → escalate (M11 adds
      pause; until then, clean fail with a "grant needed" message naming the exact grant).
- [ ] **Run budgets**: `RunBudget {max_steps, max_wall_ms, max_llm_calls, max_llm_cost, max_bytes_written}`
      checked at the top of the engine step loop. Budget exhaustion ends the run with a new
      **`Partial { reason }` terminal status** (completed-step results preserved) — defined here in
      V0.8 and used identically by plan mode and M10's agent mode. Note: the LLM-budget fields are
      dormant until M10 (plans make no run-time LLM calls) and do not apply to M9 MCP sessions
      (the model runs client-side, invisible to the engine).
- [ ] **Always-escalate classifier** (non-overridable by grants or `--yes`): `sys.exec` containing
      `sudo`/`rm -rf`/pipe-to-shell; `sys.delete_path` expanding to > N entries; `ui.click`/`key_press`
      on Send-like controls when frontmost app ∈ {Mail, Messages, Slack, …}; **payment/purchase
      actions** (Buy/Pay/Checkout/Place Order-like controls; checkout/payment URL patterns once M12
      threads grants into the browser lane); **hard executor-level refusal to type into
      `AXSecureTextField`** (enforced in the Swift helper — structural detection pixel agents can't
      have; secrets flow via the secret store below).
- [ ] **Keychain secret store** (pulled forward from the original M15 slot so authenticated flows
      work from M9 onward): `operator-jack secret set/list/rm` via security-framework (currently a
      declared-but-dead cargo feature); `{{secret:name}}` refs resolved ONLY at execution time in
      the engine; resolved subtrees tainted → force-redacted in step results, evidence, and logs;
      the secret value is injected into the target field by the helper without ever entering a
      model prompt or the store.
- [ ] **Redaction hardening**: redact `plan_json` at rest (secret refs make plans safely
      re-executable); replace the blanket "contains a slash → never redact" exemption
      (`redaction.rs:74-77`) with a looks-like-path test so URLs/connection strings get scanned;
      treat `ui.type_text` text as sensitive when the target is a secure field.
- [ ] Generalize `allow_domains` beyond `sys.open_url` (thread into the browser executor for M12).
- [ ] Surface policy denials with exit code 3 (documented but never emitted today).

**Acceptance:** A plan writing outside its fs_write grant is blocked with the exact missing grant
named. `sys.exec sudo …` escalates even with `--yes`. Typing into a password field is refused at
the helper regardless of mode, while a `{{secret:*}}`-ref login plan succeeds with no secret value
appearing in SQLite, JSONL, or output (test asserts). A budget-exhausted run ends `Partial` with
prior step results intact. Unit tests for glob canonicalization edge cases (`..`, symlinks).

---

## M9 — MCP Server Mode (v0.10.0) — first shippable autonomy

**Goal:** "Brain rented, hands owned." Any MCP client (Claude Code/Desktop, others) drives Operator
Jack's gated executors. Sized M/L because it owns the engine refactor that single-step gated
execution requires (M10 then consumes it).
**Spec:** `SPEC_FREEZE_V0.9.md` (MCP tool surface — this design is reused by M10's native loop).

### Deliverables
- [ ] **RunSession refactor (first task; shared foundation for M10):** today the only gated path is
      `Engine::execute_plan` — one ~600-line function whose PolicyGate, HelperClient, RunLogger, and
      variables map are per-run locals. Extract a `RunSession` (open a run → execute gated steps one
      at a time against it → close), so a single step can be executed through the full gate + audit +
      redaction path outside a pre-built plan. `execute_plan` becomes a thin loop over it; M10's agent
      loop and M9's MCP tools both drive the same primitive. This is the one structural refactor the
      autonomy work depends on — counted in M9's size, not deferred.
- [ ] New crate `operator-mcp` using the official `rmcp` Rust SDK over stdio;
      `operator-jack mcp serve` subcommand.
- [ ] Tool surface (borrowing Playwright MCP's shape): `snapshot`, `screenshot`, `click(ref|selector)`,
      `type_text`, `key_press`, `set_value`, `select_menu`, `wait_for`, `list_apps`, `focus_app`,
      plus gated sys tools (`read_file`, `write_file`, `exec`; `list_dir` and `data.*` are added to
      the catalog when M13 lands them — M13 owns that follow-up).
- [ ] **Every tool call routes through `RunSession`** — an MCP session is a run in the store, so
      grants, budgets, the escalation classifier, audit, and redaction apply to model-driven actions
      identically to plan steps. Denied calls return structured policy errors the model can reason
      about (not crashes); the session continues.
- [ ] **Session bootstrap + grant preview:** grants supplied at serve time
      (`mcp serve --grants <file>` or a config profile); the server renders the effective grant set
      as a human-readable "this session may: …" summary the user approves once before tools unlock.
      (This is the concrete deliverable the "envelope too coarse" risk mitigation points at.)
- [ ] Docs + demo: registering with Claude Code, a scripted end-to-end demo, safety notes aligned
      with the M8 envelope and Claude Code's per-app-approval UX.

**Acceptance:** Claude Code connected to `operator-jack mcp serve` completes "open Notes and type X"
and a Calculator task end-to-end, with audit rows for every action and a login flow using a
`{{secret:*}}` ref (M8). A tool call outside grants returns a policy error and the session continues.

---

## M10 — Native Agent Loop (v0.11.0)

**Goal:** Operator Jack's own closed loop: observe → propose ONE gated step → execute → feed
result + fresh snapshot back → repeat. The definitional autonomy feature.
**Spec:** `SPEC_FREEZE_V0.10.md` (agent mode semantics, `expect` post-conditions, transcript persistence).

### Deliverables
- [ ] **Provider layer v2** in `operator-planner`: `chat(messages, tools)` with tool_use and image
      content blocks, structured outputs, retry-on-transient; current-generation default models
      (e.g. `claude-sonnet-5` primary, `claude-haiku-4-5` for cheap turns; equivalents per provider —
      supersedes B4's interim model-ID bump). Keep `generate_plan` as a shim for `do --show-plan`.
      De-duplicate the 4 copies of response extraction.
- [ ] **`operator-agent` crate** — the orchestrator, built on M9's `RunSession`: budgeted loop of
      ui.snapshot → model proposes one **typed step** (same schema → same validation/policy/audit
      path) → RunSession executes → result + error + fresh snapshot appended → until the model signals
      done/failed or a budget hits. The M8 LLM-budget fields (`max_llm_calls`, `max_llm_cost`) are
      consumed and enforced here.
- [ ] **`ELEMENT_AMBIGUOUS` as one-turn recovery**: candidates (role/name/path/ref) rendered into
      the next model turn; model picks a ref; executor acts on the cached element.
- [ ] **`expect` post-conditions** on steps (Validator pattern): optional
      `expect: {selector, exists|value_equals|focused}` evaluated via the wait_for machinery;
      mismatch feeds back to the model as the self-correction signal.
- [ ] `operator-jack agent "instruction"` CLI, with the same grant-preview gate as M9 before the loop
      starts; transcript turns persisted as events in the store.
- [ ] Seed the eval harness: 5–10 golden agent tasks (Calculator math, Notes entry, TextEdit
      save-dialog flow) with pass/fail scoring — grown in M15.

**Acceptance:** Agent mode completes "open Calculator and compute 2+3" and a Notes task with NO
hand-written plan, with target-app windows at randomized positions/sizes and no task-specific
examples in the prompt (the operational meaning of "state it has never seen", encoded in the eval
seed). It recovers from a deliberately ambiguous selector in one turn. A deliberately impossible task
stops cleanly at the budget with a `Partial` result.

---

## M11 — Long-Horizon Autonomy & Supervision (v0.12.0)

**Goal:** Runs that pause, survive process exit, resume after approval, hand control to the human at
auth walls, and can be watched and steered live. Absorbs old-M7's checkpoint/resume scope.
**Spec:** `SPEC_FREEZE_V0.11.md` (Paused run status, checkpoint tables, `ui.human_takeover` step, IPC abort message).

### Deliverables
- [ ] **Pause/persist/resume**: checkpoint tables in `operator-store` (run cursor, variables map,
      agent transcript, pending approval); run status `Paused`; `operator-jack approvals list/approve/deny`;
      `operator-jack run --resume <run-id>` / `agent --resume`.
- [ ] **Escalation → pause** (replaces M8's interim clean-fail): on escalation, checkpoint, notify
      (osascript notification), exit 0; resume after out-of-band approval.
- [ ] **`ui.human_takeover`** step: pause, human drives (login/CAPTCHA — the agent never types
      credentials or solves CAPTCHAs, per industry policy; secret-ref injection from M8 covers the
      credential case where allowed), resume on Enter or a wait_for condition.
- [ ] **Kill-switch hardening**: SIGTERM handler sets the cancel flag mid-step; IPC `abort` message
      so the helper can stop a polling wait; reap helper/Chrome children; `RunCancelled` always
      written (today SIGKILL truncates the audit trail).
- [ ] **Supervision DX**: `logs --follow` (tail the per-run JSONL — already flushed per event);
      mid-run steering (`operator-jack steer <run-id> "use the other file"` → injected into the agent
      transcript at the next turn); milestone notifications.
- [ ] **Context compaction** for long agent runs (summarize transcript beyond a token threshold).
- [ ] Store retention/pruning APIs (`logs prune`) — the audit's store analysis found the DB grows
      unboundedly (no DELETE paths in `repo.rs`).

**Acceptance:** An agent run pauses at an escalation, the process exits, `approvals approve` +
`--resume` completes it. A login wall triggers takeover and the run resumes cleanly after. `stop`
during a hung UI step yields a complete audit trail.

---

## M12 — Web Autonomy: Browser Lane Completion (v0.13.0)

**Goal:** Finish M5 properly. Isolation first, then plan/agent wiring, then real extraction.
Unlocks the "scrape a website" scenario.
**Spec:** `SPEC_FREEZE_V0.12.md` (browser lane integration, new step types, profile model).

### Deliverables — ordered; isolation strictly before wiring
- [ ] **Profile isolation**: `--user-data-dir` under app data (per-run ephemeral by default; named
      persistent profiles for "stay logged in to site X"); refuse `discover_chrome_ws_url` attach
      unless the target is operator-owned OR the plan grants `browser.real_profile` (escalates).
      Headless/headed toggle (headed needed for takeover). *Today CDP rides the user's real
      logged-in Chrome profile — this must land before any model drives a browser.*
- [ ] **Plan/agent-lane wiring**: remove the `browser.*` hard-reject (`validation.rs:70-73`); add the
      `"browser"` arm to `engine.rs` lane dispatch. The browser session is a per-run local mirroring
      `HelperClient`'s lifecycle inside `execute_plan`/RunSession (a per-run `BrowserClient` + tokio
      handle); the sync↔async bridge must use `Handle::block_on` / a dedicated runtime thread so it
      works both from the sync CLI and from inside M9's tokio-based MCP server (a nested
      `Runtime::new().block_on` panics). `operator-runtime` gains the `operator-exec-browser`
      dependency; grants/`allow_domains` thread into the executor; browser steps route through
      policy/redaction/audit (they bypass all three today).
- [ ] **Extraction**: `browser.get_text_all`/`get_attributes_all` (querySelectorAll → JSON arrays);
      **`browser.snapshot`** (trimmed DOM/a11y tree with ref IDs — same format as M7's ui.snapshot so
      the agent loop is lane-agnostic); real navigation waits (`Page.loadEventFired`, not fixed sleeps).
- [ ] **Downloads**: `Page.setDownloadBehavior` + wait-for-download-complete step.
- [ ] **Remaining hardening** (the JS-injection escaping fix already landed in Phase B B4): surface
      the 6 browser step types (click/type/get_text/get_attribute/wait_for/scroll) through plans, MCP
      tools, and agent mode; screenshot results as file paths, never base64 into the store; honor
      `--dry-run`/`--json` on the browser CLI subcommands.
- [ ] Prompt/tool catalogs updated (browser tools in MCP + agent mode; planner prompt).

**Acceptance:** Agent mode (M10) scrapes a paginated listing site into JSON/CSV end-to-end in an
isolated profile, downloading a file along the way. A scripted scrape plan runs via
`operator-jack run`; `docs/examples/browser-navigate.json` executes. Attaching to a non-owned Chrome
is refused without the grant.

---

## M13 — Hands Completion & Deterministic Control Flow (v0.14.0)

**Goal:** Fill the missing verbs, then make repeatable batch work cheap: the agent discovers a
procedure once; control flow makes it a deterministic, auditable, resumable plan.
**Spec:** `SPEC_FREEZE_V0.13.md` (plan schema v2: control flow; new step types).

### Deliverables
- [ ] **Actuation verbs** (Swift; pure additions, parallelizable with M10–M12): `ui.scroll` (none
      exists in the UI lane today), right-click, double-click, hover, drag; coordinate
      `ui.click_at`/`ui.scroll_at` with correct Retina point↔pixel and multi-display origin mapping
      (the execution half of the vision fallback); typed `ui.set_value` (bool/number via
      CFBoolean/CFNumber — checkboxes/sliders); composite `ui.select_option`; window management
      (`ui.move_window`/`resize`/`minimize`); basic Open/Save dialog helpers.
- [ ] **Control flow** (schema v2) — the largest single engine/store/schema restructure in the
      roadmap, not a one-line bullet: `control.foreach {items, as, steps}`, `control.if`,
      `control.repeat_until`, all with **mandatory iteration caps**. Blast radius to scope up front:
      `StepResult` identity/index must become iteration-aware (store migration); `validation.rs`
      duplicate-id and forward-ref checks and the policy walk must recurse into nested steps; the
      engine's flat `skip = total - idx - 1` skip/abort accounting must be rewritten for nesting;
      per-iteration variable scoping added to the single run-scoped map.
- [ ] **Data & network steps**: `sys.list_dir` (glob + metadata), `data.parse_csv`/`data.parse_json`
      (arrays consumable by foreach), `http.request` lane under domain grants. **Add `sys.list_dir`
      and `data.*` to the MCP tool catalog — completes the M9 surface.**
- [ ] **Resume-aware batches** (gates on M11's checkpoints): `run --resume` skips completed iterations.
- [ ] **Skill compiler (stretch; gates on M10)**: save an agent-discovered procedure as a
      parameterized skill — bridges M6 skills to autonomy and cuts repeat-task cost to zero LLM calls.
- [ ] Old-M7 robustness absorbed: retry jitter, selector-confidence ranking on disambiguation.

**Acceptance:** "For each row in contacts.csv, fill the form and submit" runs deterministically over
50 rows, survives an induced failure at row N, and resumes from row N (resume via M11). "Organize
Downloads by type and date" works in typed steps — no `sys.exec zsh -c` escape hatch.

---

## M14 — Native macOS Integration Lane (v0.15.0)

**Goal:** Structured access to first-party apps — where AppleScript returns data in one call that
would take fifty fragile AX clicks. The highest-coverage macOS-native gap.
**Spec:** `SPEC_FREEZE_V0.14.md` (script lane, TCC model, document steps).

### Deliverables
- [ ] **`script.applescript` / `script.jxa` / `shortcuts.run`** step types (High risk, exec-grade
      grant classification; script bodies policy-scanned like exec commands).
- [ ] **Automation TCC** handling: per-target-app `kTCCServiceAppleEvents` checks in `doctor`
      (first script to an app triggers the OS consent dialog; doctor must explain this), plus a
      Full Disk Access check (needed for Mail/Messages data).
- [ ] **Document steps**: `doc.pdf_text` (PDFKit via helper), `doc.ocr` (Vision.framework
      `VNRecognizeTextRequest` — free, on-device; doubles as the perception fallback for scans and
      image-only screen content).
- [ ] Recipe skills: Mail message/attachment enumeration, Calendar event creation, Finder batch ops.
- [ ] i18n guidance: identifier-first selector convention in docs/prompts (name-based selectors
      break on non-English macOS); audit the `KeyPress.swift` keycode table's US-layout assumptions.

**Acceptance:** "Save all PDF attachments from unread invoices into ~/Invoices and log them to a
CSV" works as a plan combining script + doc + sys steps. OCR of a scanned invoice feeds a data-entry
agent task (agent mode via M10).

---

## M15 — Trust, Residency & Measurement (v0.16.0)

**Goal:** What makes a user run this unattended on their real machine — reversibility, unattended
triggers, a measured success rate, and a signed install. (The secret store moved forward to M8.)
**Spec:** `SPEC_FREEZE_V0.15.md` (undo-journal manifest format, watch-trigger config + concurrency/queueing model).

### Deliverables
- [ ] **Undo journal**: copy-before-mutate for `sys.write/append/move/copy(overwrite)/delete` into
      `<data_dir>/undo/<run_id>/…` with a manifest; `operator-jack undo <run-id>`; deletes go to
      Trash by default; journal size capped by the write-volume budget.
- [ ] **Watch/daemon mode**: `operator-jack watch` — FSEvents file triggers + interval polling
      firing saved plans/skills; launchd plist install helper. **Concurrency model (deliberate):** a
      single-instance run mutex serializes runs; watch triggers that fire while a run holds the mutex
      are queued (not dropped, not run concurrently). True concurrent runs are out of scope (Future) —
      one agent, one desktop, one mouse. User-activity detection pauses a run when the human is active.
- [ ] **Eval harness matured**: mini-OSWorld-for-macOS — end-to-end agent tasks scored pass/fail from
      the audit trail, on a self-hosted macOS runner (AX needs a real session); success-rate tracked
      per release. Every capability ships measured from here on.
- [ ] **Exec sandboxing tier (stretch, L)**: seatbelt profile generated from the run's fs grants
      wrapping `sys.exec` (default remains accept-and-audit + undo + budgets).
- [ ] **Signing & notarization**: Developer ID signing + notarization in release.yml (unsigned
      binaries are Gatekeeper-blocked today; a stable signed helper path also makes TCC grants durable).

**Acceptance:** `undo` restores a run's file mutations. A watch trigger fires a plan unattended; a
second trigger during a run is queued and runs after. The eval suite reports a success percentage in
CI. A browser-downloaded release passes Gatekeeper.

---

## Supersession & Future

- **Old M7 (Robustness & Recovery)** → absorbed: checkpoint/resume → M11; retry jitter + selector
  ranking → M13; plan replay stays on the Future list.
- **Old M8 (Offline STT)** → moved to Future (voice input/confirm pairs naturally with a menubar
  supervisor app; both ride on M11's approval channel).
- **Future (unscheduled):** menubar supervisor app (approve/deny/steer UI), voice input + confirm
  (whisper.cpp), Safari/Firefox via WebDriver(BiDi), plan replay from audit logs, **true concurrent
  runs** (second login session / virtual display / VM desktop isolation for real background
  autonomy), Windows (UIA) / Linux (AT-SPI) ports, multi-machine orchestration.

## Engineering rules for every milestone

1. Ends green: `cargo build --locked && cargo test && swift build && swift test && make check-examples`.
2. Schema/protocol changes get the next `SPEC_FREEZE_V0.x.md` (never edit frozen specs). Spec numbers
   now track milestones (M7→V0.7 … M15→V0.15); V0.5 is the retroactive skills spec, V0.6 already
   exists (M5 browser).
3. Update `PROJECT_CONTEXT.md` (VERSION_LOG + CURRENT_STATE + milestone table) and `CHANGELOG.md`.
4. Tag + release + tap update on every milestone version.
5. New executors/handlers ship with tests (Rust) and at least one example plan or eval task that
   exercises them end-to-end — examples are regression-tested artifacts, not documentation.
6. Anything model-driven goes through the policy gate via `RunSession`. No side doors. (The browser
   CLI subcommands bypassing policy/audit are the cautionary tale.)

## Top risks

| Risk | Mitigation |
|---|---|
| Agent-loop quality depends on snapshot quality | M7 acceptance gates on real apps incl. Electron with a token bound; eval harness from M10 measures task success |
| Safety envelope too coarse → users fall back to `unsafe --yes` | Grant-preview deliverable in M9 (session bootstrap) and M10 (agent CLI) renders "this run may: …" before execution; escalation must name the exact missing grant |
| MCP/agent can't do authenticated tasks | Secret store pulled forward into M8; `{{secret:*}}` refs usable from M9; human-takeover in M11 for the rest |
| Sync engine ↔ tokio bridging (M9 MCP + M12 browser) | One bridge via `Handle::block_on`/dedicated runtime thread; never a nested `Runtime::new().block_on`; per-run session locals mirror HelperClient |
| Control-flow restructure (M13) underestimated | Blast radius scoped in the milestone body (StepResult identity, store migration, nested validation/policy, skip accounting); actuation verbs can ship first as independent Swift work |
| macOS API friction (ScreenCaptureKit, TCC dialogs, notarization) | Everything surfaced in `doctor` first; docs per permission; signing in M15 |
| Scope creep per milestone | Each milestone has a demoable acceptance scenario; stretch items are marked and droppable |

---

*Maintained alongside PROJECT_CONTEXT.md. When a milestone completes, check it off here, update
PROJECT_CONTEXT, and re-verify the dependency table before starting the next.*