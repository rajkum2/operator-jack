# Operator CLI Spec Freeze v0.11

Status: Frozen for implementation
Version: `0.12.0`
Date: `2026-07-09`
Predecessor: Spec Freeze v0.10

Additive delta to v0.10. Covers **M11 — long-horizon autonomy & supervision**:
runs that pause, survive process exit, resume after out-of-band approval, hand
control to the human at auth walls, and can be watched, steered, and pruned.

---

## 1) Paused run status + checkpoints

`RunStatus` gains `Paused` — a non-terminal, resumable state (distinct from
`Partial`, which is terminal). A paused run is not an error: its CLI exit code
is `0`.

Two new store tables (migration #2):

- **`checkpoints`** (one, latest, per run): `run_id` (PK), `plan_id`, `kind`
  (`plan` | `agent`), `cursor_json`, `variables_json`, `transcript_json`,
  `goal`, `reason`, timestamps. Persists enough to resume after the process
  exits. `cursor` is `{"step_index": N}` for plans; agents carry `goal` +
  `transcript`. Redaction is applied to `cursor`/`variables` before write.
- **`approvals`**: `id` (PK, ULID), `run_id`, `kind` (`escalation` |
  `takeover`), `summary`, `status` (`pending` | `approved` | `denied`),
  `created_at`, `decided_at`.

Store API: `save_checkpoint` (upsert), `load_checkpoint`, `delete_checkpoint`;
`create_approval`, `list_approvals(status?)`, `get_approval`,
`latest_approval_for_run`, `decide_approval`.

---

## 2) Escalation → pause (replaces the interim clean-fail)

When the always-escalate classifier flags an action:

- **Agent loop / RunSession**: `execute_step` returns `SessionError::Escalation`
  (a pause point, not a failure — no failed step result recorded). The agent
  loop checkpoints (goal + transcript), opens a pending `escalation` approval,
  raises a notification, and returns `AgentStatus::Paused`.
- **Plan execution (non-interactive)**: the run checkpoints at the escalating
  step (`cursor.step_index = idx`, so resume re-runs it), opens an approval, and
  ends `Paused`. Interactive plan runs keep the prior hard-fail behavior (a
  human is present).
- **MCP**: no run-loop to pause; escalation is surfaced as a tool error
  (`isError=true`) so the client's model adapts or asks its human.

Resume is gated on approval and consumes a **one-shot escalation allowance**:
`PolicyGate::grant_one_escalation()` lets exactly one always-escalate action
through on the resumed run. The classifier remains non-overridable by
`--yes`/grants otherwise.

---

## 3) `ui.human_takeover` step

A new control-lane step type (`ui.human_takeover`), risk `Low`, params optional
(`prompt`, `wait_for`). The agent never types credentials or solves CAPTCHAs;
the human does, then the run resumes. Behavior:

- **Interactive plan run**: prints the prompt and blocks for Enter, then treats
  the step as completed and continues.
- **Non-interactive plan run**: pauses at the step (`cursor.step_index = idx+1`,
  so resume continues *past* it — the human already did it), opens a `takeover`
  approval, ends `Paused`.
- **Agent loop**: the model may emit `human_takeover`; it is a control action
  (not dispatched) that checkpoints + opens a `takeover` approval + pauses.

---

## 4) Resume

- `operator-jack agent --resume <run-id>`: verifies the run's latest approval is
  `approved` (else refuses), reloads the goal + transcript, re-opens a session
  under the paused run's grant envelope (looked up from its plan row), grants a
  one-shot escalation if the pause was an escalation, and continues the loop
  seeded with the saved transcript.
- `operator-jack run --resume <run-id>`: verifies approval, then re-executes the
  plan skipping steps before `cursor.step_index` (restoring `variables`), with a
  one-shot escalation allowance for an escalation pause.

Resume creates a fresh run (linked by plan); the paused run remains in history.
The checkpoint is consumed (deleted) on resume so the same pause can't be
resumed twice.

---

## 5) Approvals CLI

```
operator-jack approvals list [--all]          # pending by default
operator-jack approvals approve <id>
operator-jack approvals deny <id>
```

Approving an `escalation` allows its action through once on resume; approving a
`takeover` means "I did the manual step, continue".

---

## 6) Supervision DX

- **`operator-jack logs --follow [<run-id>]`** / **`logs follow`**: tail the
  per-run JSONL event log (defaults to the newest run); stops on Ctrl-C or when
  the run reaches a terminal status.
- **`operator-jack steer <run-id> "<message>"`**: append guidance to a per-run
  steer file (`<log-dir>/<run-id>.steer`); the agent drains it at the start of
  its next turn and injects it into the transcript as `USER STEER: …`.
- **Notifications**: a macOS `osascript` notification is raised when a run
  pauses (approval needed / your turn).

---

## 7) Context compaction

When an agent transcript grows past a char budget (~24k chars), the oldest turns
are folded into a single summary breadcrumb (action names preserved) while the
most recent turns are kept verbatim, so long runs survive the context window.

---

## 8) Retention / pruning

`operator-jack logs prune [--older-than-days N] [--keep M]` (or `logs --prune`)
deletes runs older than `N` days (default 30), always keeping at least the `M`
most recent (default 20), and cascades to their step_results, events,
checkpoints, and approvals. Fixes the audit's unbounded-growth finding
(`repo.rs` had no DELETE paths). Plans are left intact.

---

## 9) Kill-switch

The SIGINT/SIGTERM handler (already registered) sets the cancel flag; the engine
checks it between steps/retries and always writes a terminal `Cancelled` status
+ `RunCancelled` event, disconnecting the helper child on exit. (SIGKILL remains
uncatchable by design.)

---

## 10) Acceptance

- Verified live (no LLM/helper needed): a plan with a `ui.human_takeover` step
  run non-interactively **paused** after step 0 (only `before.txt` on disk, a
  `takeover` approval + checkpoint persisted, exit 0); resuming *before*
  approval was **refused**; after `approvals approve`, `run --resume` completed
  with status **Succeeded**, skipped the already-run step, and produced
  `after.txt`.
- `logs prune` reclaimed 82 accumulated runs; `steer` queued a message;
  `approvals list` showed pending/approved rows.
- Store round-trip + lifecycle covered by unit tests (checkpoint upsert/delete,
  approval pending→approved with `decided_at`, prune keep-min + cascade).
- Agent transcript compaction covered by unit tests (folds when over budget,
  no-op under budget).
