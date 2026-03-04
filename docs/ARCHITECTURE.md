# Operator Jack Architecture

## Overview

Operator Jack is a local-first, privacy-respecting CLI tool for automating macOS tasks through structured JSON plans. It bridges system-level operations (file I/O, process management, URL opening) with UI automation (accessibility-based element interaction) and browser control (Chrome DevTools Protocol).

Operator Jack uses a **three-lane execution model**:

1. **System lane (`sys.*`)** -- File operations, process launching, shell-free command execution. Available from M1.
2. **UI lane (`ui.*`)** -- macOS Accessibility-based interaction: finding elements, clicking, typing, menu selection. Available from M3.
3. **Browser lane (`browser.*`)** -- Chrome DevTools Protocol automation for web pages. Available from M5.

Each lane has its own executor implementation, but all share the same plan schema, policy gate, variable interpolation, and logging infrastructure.

## Crate Dependency Graph

```
operator-cli
  └── operator-runtime
        ├── operator-core        (plan types, validation, variable interpolation, redaction)
        ├── operator-store       (SQLite persistence, JSONL audit logs)
        ├── operator-exec-system (sys.* executor: files, processes, URLs)
        └── operator-ipc         (NDJSON stdio bridge to Swift macOS helper)
```

### Crate Responsibilities

| Crate | Purpose |
|---|---|
| `operator-cli` | Clap-based CLI entry point. Parses arguments, wires up runtime, renders output. |
| `operator-runtime` | Execution engine. Loads plans, drives the step loop, manages cancellation and retries. |
| `operator-core` | Shared types: `Plan`, `Step`, `StepResult`, `Selector`, `PolicyLevel`. Validation logic. Variable interpolation. Redaction filters. |
| `operator-store` | SQLite database for plans, runs, and step results. JSONL append-only audit log writer. |
| `operator-exec-system` | Implements all `sys.*` step types: `sys.open_app`, `sys.exec`, `sys.read_file`, `sys.write_file`, `sys.mkdir`, `sys.open_url`, etc. |
| `operator-ipc` | Manages the NDJSON-over-stdio IPC channel to the Swift macOS helper binary. Sends requests, receives responses, handles timeouts. |

## Data Flow

A complete execution flows through the system as follows:

```
CLI arguments
  │
  ▼
Plan JSON file (or inline)
  │
  ▼
Validation (operator-core)
  │  - Schema version check
  │  - Step type existence
  │  - Variable reference resolution
  │  - Selector field mutual exclusion
  │  - Allowlist consistency
  │
  ▼
Store (operator-store)
  │  - Insert plan record
  │  - Create run record with status "running"
  │
  ▼
Engine (operator-runtime)
  │  - Sequential step loop
  │  - For each step:
  │      1. Variable interpolation (just-in-time)
  │      2. PolicyGate check (risk classification)
  │      3. Executor dispatch (sys.* / ui.* / browser.*)
  │      4. Capture StepResult (success/failure/skipped)
  │      5. Apply on_fail policy (abort/continue/retry)
  │      6. Log to store + audit log
  │      7. Check cancel flag
  │
  ▼
StepResult
  │  - status: success | failure | skipped
  │  - output: redacted string or structured data
  │  - duration_ms
  │  - error (if failed)
  │
  ▼
Logs
  - SQLite step_results table
  - JSONL audit log (append-only)
  - Terminal output (respects --quiet, --verbose)
```

## Plan Model

A plan is a JSON document conforming to the following structure:

| Field | Type | Required | Description |
|---|---|---|---|
| `schema_version` | integer | yes | Must be `1` for current spec. |
| `name` | string | yes | Human-readable plan name. |
| `description` | string | no | What the plan does. |
| `mode` | string | no | `"safe"` (default) or `"unsafe"`. Controls policy gate strictness. |
| `allow_apps` | string[] | no | Restrict UI automation to these app names only. |
| `allow_domains` | string[] | no | Restrict browser automation to these domains only. |
| `variables` | object | no | Key-value pairs for interpolation. Values are strings. |
| `constraints` | object | no | Execution constraints: `max_step_duration_ms`, `max_total_duration_ms`. |
| `steps` | Step[] | yes | Ordered list of steps to execute. |

### Step Schema

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | Unique identifier within the plan. |
| `type` | string | yes | Step type (e.g., `sys.open_app`, `ui.click`, `browser.navigate`). |
| `params` | object | yes | Type-specific parameters. |
| `on_fail` | string | no | `"abort"` (default), `"continue"`, or `"retry"`. |
| `retry` | object | no | `{ "max_attempts": N, "backoff_ms": M }`. |
| `label` | string | no | Human-readable description of the step. |

## Execution Engine

The engine in `operator-runtime` drives plan execution with the following algorithm:

```
load plan from file or store
validate plan (operator-core)
create run record in store
set cancel_flag = false

for each step in plan.steps:
    if cancel_flag:
        mark remaining steps as skipped
        break

    interpolate variables into step params (just-in-time)
    classify risk level for step type
    check PolicyGate:
        if denied → mark step as skipped, apply on_fail
        if needs confirmation → prompt user (or fail in non-interactive)

    attempt = 0
    loop:
        attempt += 1
        result = executor.execute(step)
        if result.success:
            store result
            break
        if step.on_fail == "retry" and attempt < max_attempts:
            sleep(backoff_ms * attempt)
            continue
        if step.on_fail == "abort":
            store result
            mark run as failed
            return
        if step.on_fail == "continue":
            store result
            break

mark run as completed (or failed if any abort triggered)
```

### Cancellation

The engine checks a shared `AtomicBool` cancel flag before each step. The flag is set by:

- SIGINT / SIGTERM signal handler
- `operator stop` command (sends SIGTERM to the PID recorded in the PID file)

When cancelled, remaining steps are marked as `skipped` and the run status is set to `cancelled`.

### Retry and Backoff

When `on_fail` is `"retry"`, the engine uses the step's `retry` config:

- `max_attempts`: Maximum number of tries (default: 3).
- `backoff_ms`: Base backoff in milliseconds, multiplied by attempt number (linear backoff).

## Policy Gates

Every step type has a **risk classification**:

| Level | Description | Examples |
|---|---|---|
| **low** | Read-only, no side effects | `sys.read_file`, `ui.find_element`, `sys.open_app` |
| **medium** | Writes data, modifies state | `sys.write_file`, `sys.mkdir`, `ui.click`, `ui.type_text` |
| **high** | Destructive or hard to undo | `sys.exec`, `sys.rm`, `browser.execute_js` |

### Mode Behavior

| Mode | Low | Medium | High |
|---|---|---|---|
| `safe` (default) | auto-approve | prompt user | prompt user |
| `unsafe` | auto-approve | auto-approve | auto-approve |

The `--yes` flag auto-approves all prompts (equivalent to piping `yes`). In **non-interactive** mode (no TTY), medium and high risk steps in safe mode **fail deterministically** rather than hanging on a prompt.

## IPC Protocol (M2+)

Operator Jack communicates with a Swift helper binary (`macos-helper`) over stdio using **NDJSON** (newline-delimited JSON). Each message is a single JSON object terminated by `\n`.

### Request Format

```json
{"id": "uuid", "method": "ui.findElement", "params": {"app": "Notes", "selector": {"role": "AXButton", "name": "OK"}}}
```

### Response Format

```json
{"id": "uuid", "result": {"found": true, "element": {"role": "AXButton", "name": "OK", "position": [100, 200], "size": [80, 30]}}}
```

### Error Format

```json
{"id": "uuid", "error": {"code": "ELEMENT_NOT_FOUND", "message": "No element matching selector"}}
```

The IPC manager in `operator-ipc` handles:

- Spawning the helper process
- Correlating request/response by `id`
- Timeout enforcement per request
- Graceful shutdown (close stdin, wait for exit)

## Storage

### SQLite Schema

The `operator-store` crate manages a SQLite database at `~/Library/Application Support/operator-jack/operator-jack.db` with the following tables:

**plans**
| Column | Type | Description |
|---|---|---|
| id | TEXT PK | UUID |
| name | TEXT | Plan name |
| schema_version | INTEGER | Always 1 |
| plan_json | TEXT | Full plan JSON |
| created_at | TEXT | ISO 8601 timestamp |

**runs**
| Column | Type | Description |
|---|---|---|
| id | TEXT PK | UUID |
| plan_id | TEXT FK | References plans.id |
| status | TEXT | running, completed, failed, cancelled |
| started_at | TEXT | ISO 8601 |
| finished_at | TEXT | ISO 8601 or NULL |
| mode | TEXT | safe or unsafe |

**step_results**
| Column | Type | Description |
|---|---|---|
| id | TEXT PK | UUID |
| run_id | TEXT FK | References runs.id |
| step_id | TEXT | Step id from plan |
| step_type | TEXT | e.g., sys.open_app |
| status | TEXT | success, failure, skipped |
| output | TEXT | Redacted output |
| error | TEXT | Error message or NULL |
| duration_ms | INTEGER | Execution time |
| created_at | TEXT | ISO 8601 |

**events**
| Column | Type | Description |
|---|---|---|
| id | TEXT PK | UUID |
| run_id | TEXT FK | References runs.id |
| event_type | TEXT | step_start, step_end, policy_prompt, etc. |
| payload | TEXT | JSON payload |
| created_at | TEXT | ISO 8601 |

### JSONL Audit Log

In addition to SQLite, every significant event is appended to the run-specific JSONL log. This file is append-only and never truncated by Operator Jack. Each line is a self-contained JSON object:

```json
{"ts":"2025-06-15T10:30:00Z","event":"step_end","run_id":"...","step_id":"write_file","status":"success","duration_ms":12}
```

## Redaction

Before any output is stored or displayed, Operator Jack applies redaction rules to prevent secret leakage.

### Key-Name Matching

If a step's parameters or output contain keys whose names match common secret patterns, their values are replaced with `[REDACTED]`:

- `password`, `secret`, `token`, `api_key`, `apikey`, `access_key`, `private_key`
- `authorization`, `credential`, `passphrase`
- Case-insensitive, substring match on key names

### Pattern Matching

Values are scanned for patterns that look like secrets:

- **Base64 blobs**: 40+ characters of `[A-Za-z0-9+/=]`
- **Hex strings**: 40+ characters of `[0-9a-fA-F]`
- **JWT tokens**: `eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+`

### Never Redacted

The following are explicitly excluded from redaction to preserve usability:

- File paths and directory names
- Application names
- Domain names and URLs (the URL itself, not query parameters containing secrets)
- Step IDs and plan names

## Variable Interpolation

Plans support variable interpolation using two syntaxes:

- **Simple**: `$variable_name` -- replaced with the value of the variable.
- **Dotted path**: `${dotted.path}` -- supports nested access if variables contain structured data.

Interpolation is performed **just-in-time** before each step executes, not at plan load time. This means:

1. A step's output can be captured into a variable for use by later steps.
2. Variables are resolved against the current state at execution time.
3. Undefined variables cause the step to fail with a clear error message.

Variables are defined in the plan's `variables` object and can be overridden via `--set key=value` on the command line.
