# Operator CLI Security Model

## Threat Model

Operator automates macOS tasks by executing structured plans that interact with the filesystem, running applications, and web browsers. The security model addresses three primary threats:

### 1. Accidental Data Loss

Plans can write files, delete directories, and interact with application UI. A malformed or misunderstood plan could overwrite important files or trigger destructive application actions. Mitigations:

- **Policy gates** classify every step by risk level and require confirmation for medium and high risk actions in safe mode.
- **Allowlists** restrict which applications and domains a plan can interact with.
- **Dry-run mode** (`--dry-run`) simulates execution without performing any actions.

### 2. Unintended Application Actions

UI automation can click buttons, type text, and select menus. Without guardrails, a plan could interact with the wrong application or trigger unintended workflows. Mitigations:

- **`allow_apps`** restricts UI steps to a specific set of applications. Steps targeting other apps are rejected.
- **Selector disambiguation** requires user confirmation when multiple UI elements match a selector in interactive mode. In non-interactive mode, ambiguous selectors fail deterministically with `SELECTOR_AMBIGUOUS`.
- **Safe mode** prompts the user before any write or click action.

### 3. Secret Leakage in Logs

Step outputs may contain passwords, API keys, tokens, or other sensitive data. Without redaction, these would appear in terminal output, SQLite records, and audit logs. Mitigations:

- **Key-name redaction** replaces values associated with secret-like key names.
- **Pattern-based redaction** catches base64 blobs, hex strings, and JWT tokens in output text.
- **Redaction applies before storage** -- secrets never reach the SQLite database or JSONL audit log.

## Policy Gates

Every step type has a risk classification: **low**, **medium**, or **high**.

| Risk Level | Safe Mode | Unsafe Mode |
|---|---|---|
| Low (read-only) | Auto-approved | Auto-approved |
| Medium (writes data) | Prompt user for confirmation | Auto-approved |
| High (destructive/exec) | Prompt user for confirmation | Auto-approved |

### The `--yes` Flag

Passing `--yes` on the command line auto-approves all policy prompts. This is the only way to run medium/high risk steps without manual confirmation in safe mode. Use `--yes` only when you trust the plan content completely.

### Non-Interactive Behavior

When there is no TTY attached (e.g., running in a cron job or piped script), operator cannot prompt the user. In this case:

- **Low risk** steps proceed normally.
- **Medium and high risk** steps in safe mode **fail deterministically** with a clear error indicating that confirmation is required but no TTY is available.
- The `--yes` flag overrides this behavior and allows non-interactive execution.

This design ensures operator never hangs waiting for input that will never arrive.

## Allowlists

### Application Allowlist (`allow_apps`)

The `allow_apps` field in a plan restricts which macOS applications UI steps can target. When set:

- Only applications whose names appear in the list can be targeted by `ui.*` steps.
- Steps targeting unlisted applications are rejected before execution.
- The `sys.open_app` step type is also gated by `allow_apps` when the list is present.

### Domain Allowlist (`allow_domains`)

The `allow_domains` field restricts which web domains browser steps can navigate to. When set:

- `sys.open_url` checks that the URL's domain matches (or is a subdomain of) an allowed domain.
- `browser.navigate` applies the same check.
- Requests to unlisted domains are rejected.

### Narrowing, Not Widening

Allowlists can only **narrow** the scope of what a plan can do. They cannot grant access to capabilities the system does not already support. A plan cannot use `allow_apps` to bypass security restrictions or access protected system resources -- it can only limit the set of apps the plan itself will touch.

## Redaction Rules

Redaction is applied to step outputs, error messages, and any data written to logs or the store.

### Key-Name Matching

Values are redacted when they appear as the value of a key whose name matches any of these patterns (case-insensitive, substring match):

- `password`
- `secret`
- `token`
- `api_key`, `apikey`
- `access_key`
- `private_key`
- `authorization`
- `credential`
- `passphrase`

The key name itself is preserved; only the value is replaced with `[REDACTED]`.

### Pattern Matching

Even without a matching key name, values are scanned for patterns that commonly indicate secrets:

| Pattern | Description |
|---|---|
| Base64 blob | 40 or more characters matching `[A-Za-z0-9+/=]` |
| Hex string | 40 or more characters matching `[0-9a-fA-F]` |
| JWT token | Matches `eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+` |

### What Is Never Redacted

To preserve the usability of logs and output, the following are explicitly excluded from redaction:

- **File paths and directory names** -- e.g., `/Users/alice/Documents/report.pdf`
- **Application names** -- e.g., `TextEdit`, `Google Chrome`
- **Domain names and bare URLs** -- e.g., `google.com`, `https://example.com/page`
- **Step IDs and plan names** -- identifiers used for debugging and tracing
- **Numeric values** -- port numbers, durations, counts

## sys.exec Safety

The `sys.exec` step type runs external commands. It is classified as **high risk** and includes the following safety measures:

### Direct Execution (No Shell)

Commands are executed directly via `execvp` semantics -- they are **not** passed through a shell. This means:

- No shell expansion of `*`, `~`, `$`, or other metacharacters.
- No pipes, redirects, or command chaining via `&&`, `||`, or `;`.
- The `args` array is passed directly to the target binary.

This eliminates an entire class of shell injection vulnerabilities.

### Capture Limits

Standard output and standard error from executed commands are captured up to a configurable limit (default: 1 MB). Output beyond this limit is truncated. This prevents a runaway process from consuming unbounded memory.

### Environment Cleaning (`env_clean`)

When the `env_clean` parameter is set to `true`, the child process starts with a minimal environment containing only:

- `PATH` (system default)
- `HOME`
- `USER`
- `LANG`

All other environment variables (including any secrets in the parent environment) are stripped.

### Timeout

Every `sys.exec` invocation respects the step-level timeout and the plan-level `max_step_duration_ms` constraint. If the child process exceeds the timeout, it is sent SIGTERM, and after a grace period, SIGKILL.

## Network Isolation

Operator makes **no network calls** during normal operation. All plan execution, storage, and logging happen locally. The only exceptions are:

- **`sys.open_url`** -- opens a URL in the default browser. This is a user-visible action, not a background network call.
- **`browser.*` steps** -- connect to a local Chrome instance via the Chrome DevTools Protocol on localhost. Network requests made by the browser are subject to `allow_domains`.

There is no telemetry, no update checking, no cloud API calls, and no phoning home.

## Process Management

### PID File

When `operator run` starts, it writes its process ID to `~/.operator/operator.pid`. This allows:

- `operator stop` to find the running process and send SIGTERM.
- Detection of stale PID files (process no longer running).

### Graceful Shutdown

When `operator stop` is invoked:

1. Read the PID from `~/.operator/operator.pid`.
2. Send SIGTERM to the process.
3. Wait up to 5 seconds for graceful shutdown.
4. If still running, send SIGKILL.
5. Remove the PID file.

The running operator process handles SIGTERM by setting the cancel flag, which causes the engine to skip remaining steps, mark the run as cancelled, and exit cleanly.

### No Hidden Background Persistence

Operator does not:

- Install launch agents or daemons.
- Fork into the background (unless explicitly using `operator run --background`, which still writes a PID file).
- Start services that survive terminal closure.
- Register login items.

When operator exits, it is fully stopped. The PID file is cleaned up. There are no lingering processes.

## Append-Only Audit Log

The JSONL audit log at `~/.operator/audit.jsonl` is designed as an append-only record:

- Operator only appends to this file; it never truncates, rotates, or deletes entries.
- Each line is a complete JSON object with a timestamp, event type, and relevant metadata.
- Redaction is applied before writing, so the audit log never contains raw secrets.
- The log can be used for post-hoc analysis, debugging, or compliance review.
- Users can configure log retention externally (e.g., logrotate) if the file grows too large.
