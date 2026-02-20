# Operator CLI Spec Freeze v0.1

Status: Frozen for implementation  
Version: `0.1.0`  
Date: `2026-02-20`

This document is the normative contract for building Operator milestones M0-M3.
If implementation behavior conflicts with this spec, this spec wins unless updated by a new freeze doc.

## 1) Scope

In scope for v0.1:
- Core plan model and validation
- Sequential runtime engine
- System executor (Rust, local machine actions)
- macOS AX UI executor via Swift helper over stdio JSON-RPC
- SQLite persistence and JSONL audit logging
- Policy gates, retries, timeout, cancellation, redaction
- CLI surface needed to plan, run, inspect logs, stop runs

Out of scope for v0.1:
- Browser/CDP executor (M5+)
- Planner from natural language (M4+)
- Parallel step execution
- Recorder/watch mode
- Plugin system, remote execution, scheduling

## 2) Normative Terms

- MUST: required behavior
- SHOULD: recommended behavior unless a documented reason exists
- MAY: optional behavior

## 3) High-Level Architecture

Execution lanes:
- `system`: Rust-native executor for local OS/file/shell actions
- `ui`: Rust runtime + Swift helper for macOS Accessibility APIs
- `browser`: reserved; unsupported in v0.1 (validation error if used)

Core principle:
- Every step is typed and auditable.
- Every step passes through policy gates before execution.
- Runtime is deterministic and sequential for v0.1.

## 4) Process Lifecycle and Helper Management

### 4.1 Helper process ownership
- Rust CLI runtime MUST spawn `operator-macos-helper` as a child process on demand (first `ui.*` step).
- Helper lifetime is per run.
- Runtime MUST terminate helper on run completion, failure, or cancellation.

### 4.2 Discovery order
Helper resolution order:
1. CLI flag: `--helper-path`
2. Env: `OPERATOR_HELPER_PATH`
3. Config: `helper_path`
4. `$PATH` lookup (`operator-macos-helper`)
5. Relative dev fallback: `../macos-helper/.build/release/operator-macos-helper`

If none resolves, step fails with `HELPER_NOT_FOUND`.

### 4.3 Handshake
- Runtime MUST call `ui.ping` immediately after spawn.
- Response MUST include `protocol_version` and `helper_version`.
- Runtime MUST reject incompatible protocol versions with `HELPER_PROTOCOL_MISMATCH`.

### 4.4 Crash and hang handling
- If helper exits unexpectedly during a step: step fails with `HELPER_CRASHED`.
- If helper call exceeds step timeout: runtime MUST kill helper process, mark step `timed_out`, and optionally retry based on policy.
- Subsequent UI steps MAY respawn helper.

## 5) Runtime Paths and Files (XDG)

Default locations (expand `~`):
- Config: `~/.config/operator/config.toml`
- Data root: `~/.local/share/operator/`
- SQLite DB: `~/.local/share/operator/operator.db`
- Logs dir: `~/.local/share/operator/logs/`
- Per-run log: `~/.local/share/operator/logs/<run_id>.jsonl`
- PID file: `~/.local/share/operator/operator.pid`

Implementation SHOULD use `dirs` crate for platform-correct base paths.

## 6) Configuration Contract

Config format: TOML.

```toml
schema_version = 1
helper_path = "/usr/local/bin/operator-macos-helper"
default_mode = "safe"
interactive_default = true
allow_apps = ["TextEdit", "Notes", "Google Chrome"]
allow_domains = ["google.com", "github.com"]
log_dir = "~/.local/share/operator/logs"
db_path = "~/.local/share/operator/operator.db"
default_step_timeout_ms = 30000
default_retries = 0
default_retry_backoff_ms = 1000
```

Supported keys:
- `schema_version` (int, required, currently `1`)
- `helper_path` (string, optional)
- `default_mode` (`"safe"` or `"unsafe"`, default `"safe"`)
- `interactive_default` (bool, default `true`)
- `allow_apps` (string array, default empty = unrestricted)
- `allow_domains` (string array, default empty = unrestricted)
- `log_dir` (string path, default XDG)
- `db_path` (string path, default XDG)
- `default_step_timeout_ms` (int, default `30000`)
- `default_retries` (int, default `0`)
- `default_retry_backoff_ms` (int, default `1000`)

Precedence (highest to lowest):
1. CLI flags
2. Environment variables
3. Config file
4. Built-in defaults

Environment variable overrides:
- `OPERATOR_CONFIG_PATH`
- `OPERATOR_DB_PATH`
- `OPERATOR_LOG_DIR`
- `OPERATOR_MODE`
- `OPERATOR_INTERACTIVE`
- `OPERATOR_HELPER_PATH`

## 7) IDs, Mutability, and Versioning

- IDs for `plan_id`, `run_id`, `step_result_id`, and IPC `id` MUST be ULIDs.
- Plans are immutable once saved.
- Plan updates MUST create a new plan row (optional `parent_plan_id` link).
- Runs are append-only for events; status fields may update as run progresses.
- Every plan MUST declare `schema_version`.
- Runtime MUST reject plans with unsupported `schema_version`.

## 8) Data Model

### 8.1 Plan JSON

```json
{
  "schema_version": 1,
  "name": "Open notes and type text",
  "description": "Example plan",
  "variables": {
    "note_title": "Sprint Notes"
  },
  "steps": [
    {
      "id": "open_notes",
      "type": "sys.open_app",
      "params": { "app": "Notes" }
    },
    {
      "id": "type_body",
      "type": "ui.type",
      "params": { "app": "Notes", "text": "Hello from Operator" },
      "timeout_ms": 10000,
      "retries": 1,
      "retry_backoff_ms": 1000,
      "on_fail": "abort"
    }
  ]
}
```

Plan fields:
- `schema_version` (int, required, must be `1`)
- `name` (string, required)
- `description` (string, optional)
- `variables` (object<string, JSON value>, optional)
- `steps` (array<Step>, required, non-empty)

### 8.2 Step object

Required:
- `id` (string, regex `^[a-zA-Z][a-zA-Z0-9_-]{0,63}$`, unique in plan)
- `type` (string, known step type)
- `params` (object)

Optional:
- `timeout_ms` (int > 0)
- `retries` (int >= 0)
- `retry_backoff_ms` (int > 0)
- `on_fail` (`"abort" | "continue" | "ask"`)

Defaults (if omitted):
- `timeout_ms = default_step_timeout_ms` (default 30000)
- `retries = default_retries` (default 0)
- `retry_backoff_ms = default_retry_backoff_ms` (default 1000)
- `on_fail = "abort"`

### 8.3 Supported step types in v0.1

System lane:
- `sys.open_app` params: `app` (string)
- `sys.open_url` params: `url` (string)
- `sys.read_file` params: `path` (string)
- `sys.write_file` params: `path` (string), `content` (string), `create_parent` (bool, default false)
- `sys.append_file` params: `path` (string), `content` (string), `create_parent` (bool, default false)
- `sys.mkdir` params: `path` (string), `parents` (bool, default true)
- `sys.move_path` params: `from` (string), `to` (string), `overwrite` (bool, default false)
- `sys.copy_path` params: `from` (string), `to` (string), `overwrite` (bool, default false)
- `sys.delete_path` params: `path` (string), `recursive` (bool, default false)
- `sys.exec` params: `command` (string), `args` (string[], default []), `cwd` (string, optional), `env` (object<string,string>, optional)

UI lane:
- `ui.check_accessibility_permission` params: `prompt` (bool, default false)
- `ui.list_apps` params: none
- `ui.focus_app` params: `app` (string)
- `ui.find` params: `app` (string), `selector` (object)
- `ui.click` params: `app` (string), `selector` (object)
- `ui.type` params: `app` (string), `text` (string), `selector` (object, optional)
- `ui.key_press` params: `app` (string), `key` (string), `modifiers` (string[], optional)
- `ui.read_text` params: `app` (string), `selector` (object)
- `ui.wait_for` params: `app` (string), `selector` (object), `timeout_ms` (int, optional override)

Unsupported in v0.1:
- Any `browser.*` step type (`UNSUPPORTED_STEP_TYPE`)

### 8.4 UI selector contract

Selector object fields:
- `role` (string, optional, example `AXButton`)
- `subrole` (string, optional)
- `name` (string, optional)
- `description` (string, optional)
- `value` (string, optional)
- `identifier` (string, optional)
- `path` (string, optional, exact AX path when known)
- `index` (int, optional, 0-based candidate index after ordering/filtering)
- `match` (`"exact"` or `"contains"`, default `"exact"`)
- `max_depth` (int, optional, default `12`)

Rules:
- Selector MUST include at least one of: `path`, `identifier`, `name`, `role`, `value`, `description`.
- Candidate ordering MUST be deterministic (pre-order tree traversal).
- `path` is strongest constraint and MUST be applied first when present.
- If multiple candidates remain and no `index` is supplied, behavior follows disambiguation policy.

### 8.5 Step output contract

Each successful step output MUST be JSON and MAY include lane-specific fields.

Common fields:
- `step_id` (string)
- `attempt` (int)
- `duration_ms` (int)

Type-specific minimum fields:
- `sys.open_app`: `app` (string), `launched` (bool)
- `sys.open_url`: `url` (string)
- `sys.read_file`: `path` (string), `content` (string), `size_bytes` (int)
- `sys.write_file`: `path` (string), `bytes_written` (int)
- `sys.append_file`: `path` (string), `bytes_written` (int)
- `sys.mkdir`: `path` (string), `created` (bool)
- `sys.move_path`: `from` (string), `to` (string)
- `sys.copy_path`: `from` (string), `to` (string)
- `sys.delete_path`: `path` (string), `deleted` (bool)
- `sys.exec`: `command` (string), `exit_code` (int), `stdout` (string), `stderr` (string)
- `ui.check_accessibility_permission`: `trusted` (bool)
- `ui.list_apps`: `apps` (array<string>)
- `ui.focus_app`: `app` (string), `focused` (bool)
- `ui.find`: `matches` (array<object>)
- `ui.click`: `clicked` (bool), `target` (object)
- `ui.type`: `typed` (bool), `chars` (int)
- `ui.key_press`: `sent` (bool)
- `ui.read_text`: `text` (string)
- `ui.wait_for`: `found` (bool), `target` (object, optional)

## 9) Variable Interpolation

Variable store is a runtime `HashMap<String, serde_json::Value>`.

Sources:
- Plan variables: `plan.variables`
- Runtime variables:
  - `run.id`
  - `plan.id`
  - `step.<step_id>.output`
  - `step.<step_id>.status`

Syntax:
- Exact token: `$var_name` or `${var_name}`
- Dotted token: `${step.open_notes.output}` or `${step.read_file.output.path}`
- Template strings: `"file-${run.id}.txt"` supported for string variables only

Rules:
- Interpolation occurs immediately before step execution.
- Runtime MUST recursively walk `params` values.
- If exact token resolves to non-string JSON, replace with typed JSON.
- If template segment resolves to non-string JSON, fail with `INTERPOLATION_TYPE_ERROR`.
- Missing variable fails step with `INTERPOLATION_MISSING`.
- Forward references to future step outputs are invalid and fail validation.

## 10) Validation Rules

Validation MUST run before execution.

Checks:
- Valid JSON shape and `schema_version`
- Non-empty `steps`, unique step IDs
- Step type known and lane supported
- Required params present and param types valid
- `on_fail` value valid
- Variable references resolvable
- UI step `app` names pass `allow_apps` if configured
- `sys.open_url` domain passes `allow_domains` if configured
- Any browser steps rejected in v0.1

Validation errors MUST return:
- Error code: `VALIDATION_ERROR`
- Human-readable message
- `details` object with path (for example `steps[3].params.url`)

## 11) Policy Gates

Modes:
- `safe` (default)
- `unsafe`

Risk classes:
- `low`: `sys.read_file`, `ui.list_apps`, `ui.find`, `ui.read_text`, `ui.wait_for`, `ui.check_accessibility_permission`
- `medium`: `sys.open_app`, `sys.open_url`, `ui.focus_app`, `ui.click`, `ui.type`, `ui.key_press`
- `high`: `sys.write_file`, `sys.append_file`, `sys.mkdir`, `sys.move_path`, `sys.copy_path`, `sys.delete_path`, `sys.exec`

Behavior:
- In `unsafe` mode, gates allow all valid steps.
- In `safe` mode:
  - `low`: auto-allow
  - `medium` and `high`: confirmation required unless `--yes`
- In non-interactive mode without `--yes`, gate-required steps fail with `POLICY_CONFIRMATION_REQUIRED`.
- `allow_apps` and `allow_domains` are deny-by-default constraints when configured.

`--dry-run` behavior:
- MUST execute validation + policy gates.
- MUST NOT perform side-effecting execution.
- MUST log simulated outcomes.

## 12) Execution and Failure Semantics

### 12.1 Sequencing
- Steps execute strictly in declared order.
- No parallel execution in v0.1.

### 12.2 Retry
- `retries` means additional attempts after first attempt.
- Backoff formula: `retry_backoff_ms * (2 ^ (attempt_index - 1))` where first retry uses multiplier 1.
- Retries only occur for retryable errors.
- Non-retryable errors skip retry and apply `on_fail`.

### 12.3 Timeout
- Timeout is per step, wall clock.
- On timeout:
  - Mark attempt as `timed_out`
  - Kill in-flight external process for that step (including helper process for UI steps)
  - Classify as retryable unless step type explicitly marks otherwise

### 12.4 `on_fail` semantics
- `abort`: run status becomes `failed`; remaining steps marked `skipped`.
- `continue`: current step marked failed/timed_out; next step executes.
- `ask`: interactive prompt:
  - `[c]ontinue`, `[a]bort`, `[r]etry-now`
  - In non-interactive mode, treat as `abort` with `ASK_REQUIRES_INTERACTIVE`.

### 12.5 Run and step statuses

Run statuses:
- `queued`
- `running`
- `succeeded`
- `completed_with_errors`
- `failed`
- `cancelled`

Step statuses:
- `pending`
- `running`
- `succeeded`
- `failed`
- `timed_out`
- `skipped`
- `cancelled`

Terminal run status rules:
- All steps succeeded: `succeeded`
- At least one failed/timed_out, none aborted run: `completed_with_errors`
- Aborted due to failure policy: `failed`
- User stop/signal: `cancelled`

### 12.6 Allowed state transitions

Run transitions:
- `queued -> running`
- `running -> succeeded`
- `running -> completed_with_errors`
- `running -> failed`
- `running -> cancelled`

Step transitions:
- `pending -> running`
- `running -> succeeded`
- `running -> failed`
- `running -> timed_out`
- `pending -> skipped`
- `running -> cancelled`

## 13) Error Model

Standard error object:

```json
{
  "code": "POLICY_DENIED",
  "message": "Step sys.delete_path denied in safe mode",
  "retryable": false,
  "details": {}
}
```

Required fields:
- `code` (string, stable)
- `message` (string)
- `retryable` (bool)
- `details` (object)

Core error codes:
- `VALIDATION_ERROR`
- `UNSUPPORTED_STEP_TYPE`
- `INTERPOLATION_MISSING`
- `INTERPOLATION_TYPE_ERROR`
- `POLICY_DENIED`
- `POLICY_CONFIRMATION_REQUIRED`
- `ASK_REQUIRES_INTERACTIVE`
- `HELPER_NOT_FOUND`
- `HELPER_SPAWN_FAILED`
- `HELPER_PROTOCOL_MISMATCH`
- `HELPER_CRASHED`
- `IPC_TIMEOUT`
- `IPC_INVALID_RESPONSE`
- `SELECTOR_NOT_FOUND`
- `SELECTOR_AMBIGUOUS`
- `EXEC_TIMEOUT`
- `EXEC_FAILED`
- `STOP_REQUESTED`
- `INTERNAL_ERROR`

## 14) Interactive UX Requirements

### 14.1 Policy confirmation prompt

Example:
```
Step 3/8 [sys.delete_path] is high-risk in safe mode.
Path: /tmp/example.txt
Approve? [y/N]:
```

### 14.2 Selector disambiguation prompt

Example:
```
Multiple UI matches for selector in app "Notes":
1. AXButton name="Save" path="Window[0]/Button[2]"
2. AXButton name="Save" path="Sheet[0]/Button[0]"
Choose [1-2] or q to abort:
```

Rules:
- In interactive mode, runtime asks user.
- In non-interactive mode, fail with `SELECTOR_AMBIGUOUS` and include candidates in `details`.
- Selection cache MAY be kept in-memory per run keyed by `(app, selector_hash)`.

## 15) Helper IPC Protocol (Rust <-> Swift)

Transport:
- stdin/stdout newline-delimited JSON (NDJSON)
- one JSON object per line
- UTF-8 encoding
- stdout reserved for protocol frames only
- helper diagnostics go to stderr

Request:
```json
{"id":"01JABC...","method":"ui.ping","params":{}}
```

Success response:
```json
{"id":"01JABC...","ok":true,"result":{"protocol_version":"1","helper_version":"0.1.0"}}
```

Error response:
```json
{"id":"01JABC...","ok":false,"error":{"code":"AX_PERMISSION_DENIED","message":"Accessibility permission required","retryable":false,"details":{}}}
```

Protocol requirements:
- `id` correlation is mandatory.
- Unknown `method` returns `METHOD_NOT_FOUND`.
- Max line length SHOULD be capped (recommended 1 MiB) and overflow returns `IPC_INVALID_RESPONSE`.
- Helper MUST process requests serially in v0.1.

Threading and hang-safety requirements:
- Helper MUST execute AX operations on a dedicated serial `DispatchQueue`.
- Helper MUST NOT require main-thread blocking for per-step AX actions.
- If a request risks blocking indefinitely, parent runtime timeout handling is authoritative and may terminate helper.
- Observer/event-loop features (watch/record) are out of scope for v0.1 and MUST NOT block request handling.

Supported helper methods in v0.1:
- `ui.ping`
- `ui.checkAccessibilityPermission`
- `ui.listApps`
- `ui.focusApp`
- `ui.find`
- `ui.click`
- `ui.type`
- `ui.keyPress`
- `ui.readText`
- `ui.waitFor`

## 16) macOS Accessibility Permission Flow

`operator doctor` MUST:
1. Call helper `ui.checkAccessibilityPermission` with `prompt=true`.
2. Helper calls `AXIsProcessTrustedWithOptions` and requests system prompt.
3. CLI prints clear guidance:
   - Permission is required for the terminal host app (Terminal/iTerm), not just helper binary.
   - Restart terminal after granting permission.
4. If still untrusted, doctor exits non-zero with remediation text.

## 17) Persistence (SQLite)

SQLite library: `rusqlite` (+ migration support).

Required tables:
- `plans`
  - `id` TEXT PK (ULID)
  - `schema_version` INTEGER
  - `name` TEXT
  - `description` TEXT NULL
  - `plan_json` TEXT (canonical JSON)
  - `parent_plan_id` TEXT NULL
  - `created_at` TEXT (RFC3339 UTC)
- `runs`
  - `id` TEXT PK (ULID)
  - `plan_id` TEXT
  - `status` TEXT
  - `mode` TEXT
  - `started_at` TEXT
  - `ended_at` TEXT NULL
  - `error_json` TEXT NULL
- `step_results`
  - `id` TEXT PK (ULID)
  - `run_id` TEXT
  - `step_id` TEXT
  - `step_index` INTEGER
  - `status` TEXT
  - `attempt` INTEGER
  - `started_at` TEXT
  - `ended_at` TEXT NULL
  - `input_json` TEXT
  - `output_json` TEXT NULL
  - `error_json` TEXT NULL
- `events`
  - `id` TEXT PK (ULID)
  - `run_id` TEXT
  - `ts` TEXT
  - `event_type` TEXT
  - `payload_json` TEXT

Indexes:
- `runs(plan_id, started_at)`
- `step_results(run_id, step_index, attempt)`
- `events(run_id, ts)`

## 18) Logging and Redaction

Each run MUST create JSONL audit log file `<log_dir>/<run_id>.jsonl`.

Event envelope:
```json
{
  "ts": "2026-02-20T12:34:56Z",
  "run_id": "01J...",
  "step_id": "open_notes",
  "event": "step_finished",
  "data": {}
}
```

Minimum events:
- `run_started`
- `step_started`
- `step_retry_scheduled`
- `step_finished`
- `step_failed`
- `run_finished`
- `run_cancelled`

Redaction MUST apply before writing to:
- JSONL logs
- DB `error_json`, `output_json`, `payload_json`
- console error output

Redaction rules:
- Key-name match (case-insensitive): `password`, `token`, `secret`, `api_key`, `authorization`, `credential`
- Pattern match:
  - strings longer than 20 chars comprised only of base64-like charset `[A-Za-z0-9+/=]`
  - hex-like strings of length >= 32 (`[A-Fa-f0-9]+`)
  - JWT-like strings with 3 dot-separated base64url segments
- Replace value with `[REDACTED]`

Do not redact:
- Step type names
- App names
- File paths

## 19) Stop Command and Signal Handling

### 19.1 Run tracking
- Runtime MUST write PID metadata to `operator.pid`:
  - `pid`
  - `run_id`
  - `started_at`
- Remove PID file on normal exit.

### 19.2 `operator stop`
- Reads PID file.
- Sends `SIGTERM`.
- Waits up to 5 seconds.
- If process still alive, sends `SIGKILL`.

### 19.3 Runtime termination behavior
- On `SIGTERM`, mark run cancellation requested.
- Current step:
  - If safe to stop immediately, mark `cancelled`.
  - If blocked/hung external process, kill subprocess/helper and mark `cancelled`.
- Remaining pending steps marked `skipped`.
- Run terminal status `cancelled`.

## 20) Concurrency Model

v0.1 MUST be sequential:
- Single active step per run.
- No intra-run parallel groups.
- Optional future `parallel` plan fields are rejected by validator in v0.1.

Multi-run execution:
- v0.1 MUST enforce one active run per workspace DB via lock file to simplify stop semantics.

## 21) CLI Contract (v0.1)

Required commands:
- `operator doctor`
- `operator plan validate --plan-file <path>`
- `operator plan save --plan-file <path>`
- `operator exec <plan_id>`
- `operator run --plan-file <path>` (validate + persist plan + exec)
- `operator logs <run_id>`
- `operator stop`

Required flags:
- Global: `--mode safe|unsafe`, `--interactive|--no-interactive`, `--yes`, `--dry-run`, `--helper-path <path>`

Non-interactive behavior:
- Any required prompt without `--yes` MUST fail deterministically with explicit code.

## 22) Exit Codes

- `0`: success (`succeeded`)
- `1`: runtime failure (`failed` or `completed_with_errors`)
- `2`: validation failure
- `3`: policy denied / confirmation required
- `4`: helper/IPC failure
- `5`: cancelled/stopped
- `6`: usage/CLI argument error

## 23) Build Order (Implementation Plan)

M0:
1. Workspace + crates (`core`, `store`, `runtime`, `cli`)
2. Core types + schema validation
3. SQLite migrations + repositories
4. CLI skeleton commands
5. Stub executors + full run engine semantics
6. JSONL logging + redaction
7. Example plans + docs

M1:
1. Implement `sys.*` step handlers
2. Policy middleware
3. `--dry-run` and `--yes`
4. Manual acceptance scenarios

M2-M3:
1. Swift helper protocol + handshake
2. Accessibility permission doctor flow
3. `ui.*` steps with selector resolution and disambiguation
4. Robust timeout/crash recovery

## 24) Extension Backlog (Post-v0.1)

Near-term:
- Conditional steps (`if/then/else`)
- Loop steps (`foreach`)
- Undo metadata + `operator undo <run_id>`
- Screenshot capture step
- OCR step (Vision)
- Notification step
- HTTP request step
- Gated shell step hardening

Medium-term:
- Plan composition/sub-plans
- Watch mode (AX observer events)
- Record mode (macro recorder)
- Scheduled runs
- Visual before/after diff artifacts
- Plugin executor protocol

Long-term:
- Cross-platform executors (Windows UIA / Linux AT-SPI)
- Local LLM planner
- Multi-machine orchestration

## 25) Resolved v0.1 Decisions

- `completed_with_errors` is non-zero exit (`1`) for deterministic CI behavior.
- `operator run --plan-file` always persists the plan before execution.
- Exactly one active run per workspace DB is allowed in v0.1.
- Redaction includes JWT-like token detection in addition to key and pattern heuristics.

---

Implementation note: v0.1 intentionally optimizes for determinism and operability over feature breadth.
