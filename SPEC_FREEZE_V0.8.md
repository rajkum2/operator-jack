# Operator CLI Spec Freeze v0.8

Status: Frozen for implementation
Version: `0.9.0`
Date: `2026-07-08`
Predecessor: Spec Freeze v0.7

Additive delta to v0.7. Everything prior remains normative unless overridden.
Covers **M8 — the Safety Envelope**: the grantable, bounded, auditable policy
model that lets a model (M9 MCP, M10 agent loop) drive the executors without
either constant confirmation prompts or unbounded blast radius.

The v0.2 two-mode policy (safe = confirm medium+high; unsafe = allow all) and the
Phase B mode-ceiling erratum remain in force. M8 adds four independent layers
**on top**: task-scoped grants, run budgets, an always-escalate classifier, and
injection-time secrets — plus redaction hardening.

---

## 1) Task-scoped grants

A `grants` object may appear on a Plan and in config. Grants are **additive
restrictions** (like `allow_apps`): a capability with no grant is unrestricted;
once a grant is present, the value must match it. When both config and plan
specify a grant for a capability, the value must satisfy **both** (most
restrictive wins). App scoping continues via `allow_apps`; grants add the
capabilities allowlists never covered.

```jsonc
"grants": {
  "fs_read":  ["~/data/**", "/tmp/**"],   // glob patterns
  "fs_write": ["~/scraped/**"],
  "exec":     ["pip install *", "python *"], // command-line glob patterns
  "domains":  ["example.com", "*.wikipedia.org"] // host suffixes / globs
}
```

### Enforcement (in the runtime PolicyGate, on interpolated params)
- **Filesystem.** For `sys.read_file` (`path`) → `fs_read`; `sys.write_file`,
  `sys.append_file`, `sys.mkdir`, `sys.delete_path` (`path`) → `fs_write`;
  `sys.move_path` (`from`+`to`) → both `fs_write`; `sys.copy_path` (`from`) →
  `fs_read`, (`to`) → `fs_write`. Paths are **canonicalized before matching**:
  `~` expands to the home dir; `.`/`..` are collapsed lexically (so
  `~/scraped/../secrets` cannot escape a `~/scraped/**` grant).
- **Exec.** `sys.exec` — the reconstructed command line (`command` + space-joined
  `args`) must match an `exec` pattern.
- **Domains.** `sys.open_url` and `browser.navigate` (`url`) — the host must match
  a `domains` pattern (suffix/glob). This generalizes v0.2's `allow_domains`
  (which covered only `sys.open_url`).

A step that violates a grant is **denied**. Until M11 adds pause/resume, denial
is a clean per-step failure whose message names the exact capability, value, and
that a grant is required (honoring the step's `on_fail`). Policy denial surfaces
process **exit code 3**.

Globbing uses `globset`. `**` matches across path separators; `*` does not cross
`/` in fs patterns but does in exec/domain patterns (which are not path-shaped).

---

## 2) Run budgets

A `budget` object may appear on a Plan and in config, bounding a whole run:

```jsonc
"budget": {
  "max_steps": 200,
  "max_wall_ms": 600000,
  "max_bytes_written": 104857600,
  "max_llm_calls": 100,       // dormant until M10 (plans make no run-time LLM calls)
  "max_llm_cost_usd": 5.0     // dormant until M10
}
```

Checked at the top of the engine's step loop and after each step. On exhaustion
the run terminates with the new terminal status **`Partial`** (see §4): steps
completed so far keep their results, a `BudgetExceeded` event is emitted, and no
further steps run. `max_bytes_written` accumulates the byte counts reported by
`sys.write_file`/`append_file`. The LLM fields are parsed and stored now but not
enforced until M10 (and never apply to M9 MCP sessions, where the model runs
client-side).

---

## 3) Always-escalate classifier

Certain actions must reach a human even in autonomous/unsafe mode. These rules
are evaluated on interpolated params and are **non-overridable** by grants or
`--yes`:

- `sys.exec` whose command line contains `sudo`, `rm -rf`, or a pipe-to-shell
  (`| sh`, `| bash`, `curl … | …sh`).
- `sys.delete_path` that expands to more than **N = 25** entries (a directory
  stat is taken before the gate).
- `ui.click` / `ui.key_press` on a control whose name matches a
  send/purchase pattern (`Send`, `Buy`, `Pay`, `Checkout`, `Place Order`,
  `Confirm Purchase`) when the frontmost app is a messaging/mail app
  (Mail, Messages, Slack, Mail-like). *(Refined in M12 for browser checkout URLs.)*
- **Typing into a secure text field.** The Swift helper refuses
  `ui.type_text`/`ui.set_value` when the target element's role/subrole is a
  secure text field (`AXSecureTextField`), regardless of mode. Credentials must
  come through the secret path (§5), not model-driven typing.

Until M11, an escalation is a clean failure with an `ESCALATION_REQUIRED`-style
message; it is never silently allowed.

---

## 4) `Partial` terminal run status

`RunStatus` gains a terminal variant `Partial`, used when a run stops early for a
non-error reason (budget exhaustion; later, an escalation pause boundary).
Completed step results are preserved and readable via `logs`. Distinct from
`Failed` (a step errored) and `Cancelled` (user stop). Plan mode and M10 agent
mode use it identically.

---

## 5) Injection-time secrets

Secrets live in the macOS Keychain (service `operator-jack`), managed by
`operator-jack secret set|list|rm <name>`. A step param may reference one as
`{{secret:name}}`.

- References are resolved **only at execution time**, inside the engine, just
  before dispatch — never during planning, validation, or storage.
- A resolved value is **tainted**: the containing param subtree is force-redacted
  in step-result `input_json`, evidence, JSONL logs, and the store. The literal
  secret never appears in a model prompt, a saved plan, or an audit record.
- The planner passes `{{secret:*}}` tokens through verbatim (the LLM proposes the
  reference, never the value).

Secrets are the sanctioned way to fill credential fields that §3 refuses to type
into directly.

---

## 6) Redaction hardening (errata to v0.2 §18)

- **Plans at rest are redacted.** `save_plan` now redacts `plan_json` (previously
  the one CRUD write that bypassed redaction). With `{{secret:*}}` refs, plans
  stay re-executable without embedding literals.
- **The slash exemption is narrowed.** v0.2 never redacted any string containing
  `/` or `\`. That is replaced by a *looks-like-a-path* test (starts with `/`,
  `~/`, `./`, `../`, or a drive-letter): URLs, connection strings, and
  slash-bearing tokens are pattern-scanned again.
- `ui.type_text` `text` is treated as sensitive when the value is tainted or the
  target is a secure field.

---

## 7) Config

`OperatorConfig` gains optional `grants` and `budget` blocks with the same shapes
as the plan-level objects; they are enforced alongside (intersected with) the
plan's. CLI flags may still tighten `allow_apps`/`allow_domains`.

---

## 8) Acceptance

- A plan that writes outside its `fs_write` grant is denied, naming the missing
  grant; exit code 3.
- `sys.exec sudo …` escalates even under `--mode unsafe --yes`.
- A `sys.exec` matching an `exec` grant runs; one that doesn't is denied.
- `ui.type_text` into a secure field is refused at the helper regardless of mode;
  a `{{secret:name}}` login plan fills the field with **no** secret value in
  SQLite, JSONL, or step output (asserted).
- A run exceeding `max_steps` (or `max_bytes_written`) ends `Partial` with prior
  results intact and a `BudgetExceeded` event.
- Unit tests cover path canonicalization escapes (`..`, `~`, symlink-style).
