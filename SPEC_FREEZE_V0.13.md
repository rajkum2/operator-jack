# Operator CLI Spec Freeze v0.13

Status: Frozen for implementation
Version: `0.14.0`
Date: `2026-07-09`
Predecessor: Spec Freeze v0.12

Additive delta to v0.12. Covers **M13 — hands completion & deterministic control
flow**: the data/network primitives that turn scraped output into structured
input, deterministic control flow so a discovered procedure becomes a
repeatable auditable plan, and the missing `ui.scroll` actuation verb.

---

## 1) Data / filesystem / network steps (system lane)

- **`sys.list_dir {path, pattern?}`** → `{count, entries:[{name, path, is_dir,
  size, modified_epoch}]}`. `pattern` is a glob matched on entry names. Entries
  are name-sorted (deterministic). Risk Low.
- **`data.parse_csv {text|path, headers?}`** → `{count, headers?, rows}`. With
  `headers` (default true), rows are objects keyed by header; else arrays. A
  small RFC-4180-ish parser: quoted fields, embedded commas, `""` escapes, CRLF.
  Risk Low.
- **`data.parse_json {text|path, pointer?}`** → `{value}`. `pointer` is an RFC
  6901 JSON pointer (e.g. `/results/0/name`). Risk Low.
- **`http.request {url, method?, headers?, body?}`** → `{status, ok, headers,
  body, truncated}`. Blocking request, 30s timeout, body capped at 1 MiB. Risk
  Medium; **gated by the domain allow-list AND grants** exactly like
  `sys.open_url` (added to both the allow-domains check and `check_grants`'s
  `Domain` capability).

`data.*` accept their input inline (`text`) or from a file (`path`).

---

## 2) `ui.scroll` (UI lane)

`ui.scroll {app, dy?, dx?, clicks?, to?}` — the first scroll verb in the UI
lane. Brings `app` to the front, then posts synthesized scroll-wheel
`CGEvent`s. `dy` negative scrolls content down (macOS wheel convention; default
-5), `clicks` repeats, `to: "top"|"bottom"` does a large repeated scroll to an
edge. Risk Medium. Swift handler `handleScroll` (`Methods/Scroll.swift`),
registered as `ui.scroll` (a single-word verb, same name in both conventions).

---

## 3) Deterministic control flow (control lane)

Three control steps carry their body as sub-steps **inside their `params`**, so
the flat `Step`/plan schema is unchanged:

- **`control.foreach {items, as, steps, max_iterations?}`** — iterate `items`,
  binding each element to the `as` variable (and the ordinal to `__index`), then
  run `steps`. `items` may be a literal array, a `${var}` naming an array, or an
  object of the `{count, rows|items}` shape (so a `data.parse_csv` /
  `browser.get_text_all` output feeds a loop directly).
- **`control.if {condition, then, else?}`** — run one branch by a condition.
- **`control.repeat_until {steps, condition, max_iterations}`** — run `steps`
  until `condition` holds or the (required) cap is hit; `__iteration` is bound.

**Conditions** are `{left, op, right?}`; operands resolve against variables
(`${var}` preserves type). Ops: `== != < <= > >= contains exists empty`
(aliases `eq/ne/gt/lt/gte/lte/truthy`). Numeric compare when both sides parse as
numbers, else string compare (so `3 == "3"`).

**Safety:** every loop has a **mandatory iteration cap** (foreach: item count +
optional `max_iterations`, both under a 100k hard ceiling; repeat_until: required
`max_iterations`); nesting depth is bounded at 16.

**Interpreter:** a recursive interpreter (`operator-runtime::control`) over a
`LeafRunner`. Leaf steps run through **`RunSession::execute_step`** — the same
policy gate, secret resolution, redaction, and audit as any other step — so each
loop iteration is independently gated and recorded (iteration-aware step
results). Leaf params are interpolated with the live variables before execution;
an optional `save_as` binds a leaf's output to a variable for later steps.

**Execution routing:** `operator-jack run`/`exec` detect a control-flow plan
(`control::has_control_flow`) and execute it via the interpreter over a
`RunSession` (with the plan's grants/budget/allow-lists/mode); flat plans keep
the existing `Engine::execute_plan` path unchanged (no regression). Control-flow
plans are non-interactive for confirmations (like MCP/agent), so safe-mode
medium/high leaf steps need `--yes` or a grant.

---

## 4) Agent & MCP surface

- The agent loop gains `list_dir`, `parse_csv`, `parse_json`, `http_request`
  actions (control flow is not an agent action — the model decides each step).
- MCP mode adds `list_dir`, `parse_csv`, `parse_json`, `http_request` tools
  (completing the M9 surface) plus the M12 browser tools.

---

## 5) Acceptance

- Verified live: a plan `data.parse_csv (save_as data) → control.foreach over
  ${data} → control.if (${row.qty} >= 5) → sys.write_file ${row.name}=${row.qty}`
  ran to `Succeeded`, producing per-row files with the correct branch (Alpha/3 →
  plain; Beta/7, Gamma/5 → BIG), each step audited under one run. Standalone
  `data.parse_csv` (quoted commas + `""` escapes), `data.parse_json` (pointer),
  and `sys.list_dir` (glob) verified via a plan.
- Unit tests: control interpreter (foreach/if/repeat_until/conditions/detection,
  6), CSV quoting + JSON pointer (2). 163 Rust tests total; 10 Swift.
- `docs/examples/control-flow-foreach.json` validates under check-examples.
- Full gate green: fmt, clippy `-D warnings`, 163 Rust tests, 10 Swift tests.

Known-partial (documented, not blocking): of the M13 actuation-verb list, only
`ui.scroll` landed this milestone; right-click/double-click/hover/drag,
coordinate `ui.click_at`/`scroll_at` (Retina mapping), typed `ui.set_value`
(bool/number), window management, and Open/Save dialog helpers are deferred.
Resume-aware batches (skip completed iterations on `run --resume`) and the skill
compiler stretch are deferred. `ui.scroll` compiles and is wired end-to-end but
was not live-exercised here (needs Accessibility permission + a live window).
