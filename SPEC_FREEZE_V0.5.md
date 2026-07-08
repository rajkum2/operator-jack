# Operator CLI Spec Freeze v0.5

Status: Frozen for implementation
Version: `0.6.0`
Date: `2026-07-08` (written retroactively during Phase B — see note)
Predecessor: Spec Freeze v0.4

> **Retroactive note.** M6 (the Skills system) shipped in v0.6.0 without a spec
> freeze; `SPEC_FREEZE_V0.6.md` (M5 browser) nonetheless names "v0.5" as its
> predecessor. This document was authored during Phase B (v0.7.1) to close that
> gap and satisfy the project's "spec is normative" rule. It documents the
> skills system **as built and as fixed in Phase B**, plus two security errata
> to v0.2. It does not change runtime behavior beyond what Phase B already
> implemented.

This is an **additive delta** to Spec Freeze v0.4. Everything in v0.4 (and its
predecessors v0.2/v0.3) remains normative unless explicitly overridden below.
This document covers **M6 — Skills (reusable, parameterized automation macros)**.

---

## 1) Scope Delta (v0.5 additions)

New in v0.5:
- Skill manifests: parameterized templates that expand into a typed Plan.
- Skill registry: discovery of skill manifests from well-known directories.
- Parameter definitions with types, defaults, validation, and interpolation.
- CLI commands: `skill list`, `skill validate`, `skill run`.

---

## 2) Skill Manifest Format

A skill manifest is a YAML (or JSON) document with this shape:

```yaml
schema_version: 1                 # required, must be 1
name: create-note                 # required, kebab-case, unique identifier
description: Create a note...      # optional
version: "1.0.0"                  # optional (the skill's own version)
author: Operator Jack             # optional
parameters:                       # optional list of parameter definitions
  - name: title                   # snake_case, unique
    type: string                  # string | integer | boolean | array | object
    required: true                # default: true
    default: ""                   # optional; used when not provided
    description: ...              # optional
    pattern: "^.+$"              # optional regex (string params)
    allowed_values: [a, b]        # optional enum constraint
steps:                            # required, >= 1 step
  - id: step_one                  # unique within the skill
    type: sys.open_app            # any valid StepType (sys.*/ui.*)
    params: { app: Notes }        # step params (may contain ${param} refs)
    timeout_ms: 3000              # optional
    retries: 0                    # optional
    on_fail: abort                # optional
    description: ...              # optional
```

### Validation rules (manifest structural)
- `schema_version` must equal `1`.
- `name` must be non-empty and kebab-case.
- At least one step is required; step `id`s must be unique.
- Parameter `name`s must be unique and snake_case; regex `pattern`s must compile.

### Step-type / param validation
The manifest validator checks structure only. The **expanded Plan** is validated
against the full step-type parameter rules (`operator-core::validation`) before
execution — see §5. (Prior to Phase B, `skill run` skipped this expanded-plan
validation; Phase B fixed it so a skill with bad step params fails loudly at the
CLI rather than mid-run inside the helper.)

---

## 3) Parameter Resolution & Interpolation

Provided values arrive as strings (from `--param key=value`) and are converted
to the declared `type`:
- `string` → JSON string
- `integer` → JSON integer (parse error if not an integer)
- `boolean` → JSON bool (`true/yes/1` / `false/no/0`)
- `array` / `object` → parsed as JSON

`allowed_values` and `pattern` are enforced during conversion. A missing value
falls back to `default`, else errors if `required`, else `null`.

### Interpolation semantics (normative — fixed in Phase B)
- A placeholder is `${name}` referencing a parameter.
- **Whole-value substitution:** if a param string is *exactly* a single
  `${name}` placeholder, the parameter's **typed** JSON value is substituted
  (an integer stays an integer, a bool stays a bool). This matters because the
  Swift helper matches integer params as `.int`; a stringified `"2000"` would be
  silently ignored.
- **Embedded substitution:** a placeholder inside surrounding text
  (`"Hello, ${name}!"`) is string-substituted (the value is stringified).
- **Numeric literals** in step params (e.g. `timeout_ms: 3000`) preserve their
  integer type through expansion (they are not coerced to floats).
- An unresolved `${...}` after substitution is an error.

---

## 4) Skill Registry & Discovery

Skills are discovered from (in order):
1. The repo-local `skills/` directory (dev builds resolve `exe/../../skills`).
2. `~/.operator-jack/skills/` (user skills).
3. `/usr/local/share/operator-jack/skills/` (system skills).

`skill validate --skill-file <path>` validates a single manifest by path.

---

## 5) Expansion to Plan

`ResolvedSkill::to_plan()` produces a `Plan`:
- `schema_version = 1`
- `name` / `description` copied from the manifest
- `mode = safe` (the effective mode is then bounded by the CLI/config ceiling —
  see the erratum in §7)
- Steps mapped 1:1 from skill steps with interpolated params
- `allow_apps` / `allow_domains` are not set by the skill (governed by CLI/config)

The expanded plan goes through the **same** validate → store → policy gate →
engine path as a hand-written plan.

---

## 6) CLI Commands

```
operator-jack skill list                          # discover & list skills
operator-jack skill validate --skill-file <path>  # validate one manifest
operator-jack skill run <name> [--param k=v ...]  # expand + validate + execute
```

`skill run` honors the global flags (`--mode`, `--yes`, `--dry-run`, `--json`,
`--helper-path`). Under `--dry-run` the plan is expanded and validated but no
step executes and the helper is not spawned.

---

## 7) Security Errata to v0.2 (applied in Phase B)

These correct behaviors specified (or under-specified) in v0.2 and are normative
from v0.7.1 onward:

### 7.1 Mode is a ceiling, not a default
The effective execution mode is the **more restrictive** of the configured
(CLI/config) mode and the plan's declared `mode`. A plan (including a
skill-expanded or LLM-generated plan) may make itself *more* restrictive (safe)
but must **never escalate** to `unsafe` over a safe configuration. Formally:
`effective = safe if either config or plan is safe, else unsafe`.

### 7.2 UI-lane step timeouts are enforced
UI (`ui.*`) steps dispatched to the Swift helper are bounded by a read timeout.
If the helper does not respond within the step's timeout (plus grace to allow
the helper's own implicit waits to complete), the helper process is killed and
the step fails with a timeout error; the next UI step reconnects. A hung helper
can no longer block a run indefinitely. (v0.2 left `ui.*` timeouts unenforced.)

### 7.3 `sys.exec` kills its child on timeout
When a `sys.exec` step exceeds its timeout, the spawned child process is killed
rather than left running orphaned after the engine abandons its worker thread.
