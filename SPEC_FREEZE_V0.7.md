# Operator CLI Spec Freeze v0.7

Status: Frozen for implementation
Version: `0.8.0`
Date: `2026-07-08`
Predecessor: Spec Freeze v0.6

This document is an **additive delta** to Spec Freeze v0.6. Everything in v0.6
and its predecessors remains normative unless explicitly overridden below. This
document covers **M7 — Perception: the Observation API**.

The purpose of M7 is to give a model *eyes*: a compact, deterministic
representation of the current UI that it can reason over and act on. It is the
foundation for MCP server mode (M9) and the native agent loop (M10).

---

## 1) Scope Delta (v0.7 additions)

- `ui.snapshot` — a compact, LLM-oriented serialization of an app's AX tree with
  stable, actionable element references.
- `ui.screenshot` — a downscaled screen/window capture written to disk (vision
  fallback and evidence).
- Element **refs**: short handles (`e1`, `e2`, …) minted per snapshot and backed
  by the existing `ElementRefStore`; actions (`ui.click`/`ui.read_text`/
  `ui.set_value`) accept a `ref` in place of a `selector`.
- **Accessibility enablement** for Chromium/Electron apps so their web content is
  visible in snapshots.
- Completion of the `any_of` and `path` selector semantics (M6/Phase B left
  `path` rejected-when-alone; M7 implements matching).

Both new steps run in the **ui** lane and are **Low risk** (read-only).

---

## 2) `ui.snapshot`

Produce a compact textual snapshot of an application's accessibility tree,
optimized for LLM consumption and cheap to tokenize.

### Params
- `app` (string, required) — application name.
- `window` (object, optional) — window scope: `{ index: N }` or
  `{ title_contains: "..." }`. Default: frontmost window.
- `max_depth` (int, optional) — traversal depth cap. Default 12.
- `interactive_only` (bool, optional) — when true (default), only actionable
  elements (and the structural ancestors needed to place them) appear. When
  false, all elements are included. Interactive-only keeps large apps (Electron)
  within budget.

### Output
- `snapshot` (string) — the indented text serialization (see §2.1).
- `ref_count` (int) — number of refs minted.
- `truncated` (bool) — true if traversal hit `max_depth` or an internal node cap.
- `token_estimate` (int) — `ceil(chars / 4)` of the snapshot string (the
  normative, tokenizer-independent measure used by acceptance budgets).

### 2.1 Serialization format (normative)
One element per line, two spaces of indentation per depth level:

```
<role> "<label>" [<ref>] {<attrs>}
```

- `<role>` — the AX role with the leading `AX` stripped and lowercased
  (`AXButton` → `button`, `AXStaticText` → `statictext`). Token economy.
- `"<label>"` — the element's title, else description, else value; omitted if
  empty. Truncated to 80 chars at a char boundary.
- `[<ref>]` — present ONLY on interactive elements (see §2.2). This is the handle
  actions use.
- `{<attrs>}` — optional compact flags, space-free where possible:
  `id=<identifier>` when set; `disabled` when not enabled; `focused` when focused;
  `value="..."` when the element both has a title/description AND a distinct value
  worth showing (e.g. a text field's contents). Absent attrs are omitted.

Example (Calculator, `interactive_only=true`):
```
window "Calculator"
  button "All Clear" [e1] {id=AllClear}
  button "1" [e2] {id=One}
  button "+" [e3] {id=Add}
  button "=" [e4] {id=Equals}
  statictext "3"
```

### 2.2 Which elements are "interactive"
An element is interactive (and gets a ref) if EITHER:
- it supports the `AXPress` action (buttons, checkboxes, radios, links,
  menu items, popups), OR
- its `AXValue` attribute is settable (text fields, text areas, sliders).

Non-interactive elements (groups, static text, images) are shown for context
(subject to `interactive_only` filtering) but do not receive a ref.

### 2.3 Refs (normative)
- A ref (`e1`, `e2`, …) is a short, snapshot-local alias for an AX element.
- Each ref is registered in `ElementRefStore` (the same ULID-keyed cache
  `ui.find` uses); the snapshot returns the short alias and the store maps it to
  the cached `AXUIElement` (with selector metadata for stale-fallback).
- Refs are numbered per snapshot in traversal order. A new `ui.snapshot`
  **invalidates** prior refs for that run by clearing the store before minting
  (Playwright-MCP semantics): act on the latest snapshot, not a stale one.
- Actions accept refs — see §4.

---

## 3) `ui.screenshot`

Capture a downscaled image of the screen or a window and write it to disk.

### Params
- `app` (string, optional) — when set (with optional `window` scope), capture
  that app's window; otherwise capture the main display.
- `window` (object, optional) — window scope, as in `ui.snapshot`.
- `max_width` (int, optional) — long-edge downscale target in pixels. Default
  1370 (matches common computer-use tooling; keeps image tokens bounded).

### Output
- `path` (string) — absolute path to the written PNG.
- `width` / `height` (int) — pixel dimensions of the written image.

The image is written under the run's log/evidence directory. **The raw image
bytes are never returned inline** (no base64 in the step result), so audit logs
and the store never carry unbounded image data — only the path.

### Permission
Screen capture requires macOS **Screen Recording** permission for the host
(terminal) app. `operator-jack doctor` reports its status.

---

## 4) Actions accept refs

`ui.click`, `ui.read_text`, and `ui.set_value` already accept either a
`selector` or an `element_ref` (Phase B). A snapshot `ref` (`e1`) is an
`element_ref`: pass `{ "element_ref": "e1" }`. Resolution:
1. If the ref is live in `ElementRefStore`, act on the cached element.
2. If stale, fall back to the ref's stored selector (if any).
3. Otherwise `ELEMENT_NOT_FOUND`.

---

## 5) Chromium / Electron accessibility

Chromium-based apps (Chrome web content, Slack, VS Code, Discord, …) expose
their internal a11y tree only once an assistive client is detected. On attach,
the helper sets `AXManualAccessibility = true` (and, defensively,
`AXEnhancedUserInterface`) on the application element so `ui.snapshot`/`ui.find`
can see web/Electron content. This is best-effort; failure is non-fatal.

To bound cost on deep or busy trees, the helper sets an
`AXUIElementSetMessagingTimeout` on the application element so a beach-balling
target degrades gracefully instead of hanging each attribute read.

---

## 6) Selector completion: `any_of` and `path`

- `any_of` — resolved in `ui.find`/`ui.wait_for` as elsewhere (first alternative
  returning exactly one match wins).
- `path` — now a real matcher: it matches an element whose computed traversal
  path (e.g. `AXWindow/AXGroup[0]/AXButton[2]`) **ends with** the given `path`
  string. A `path`-only selector is therefore valid again (Phase B rejected it
  as non-matching); combined with other fields, all must match.

---

## 7) Token measurement (normative)

Wherever a spec or acceptance test bounds snapshot size "in tokens", the measure
is the tokenizer-independent heuristic `ceil(chars / 4)` over the snapshot
string. This keeps budgets reproducible without pinning a specific tokenizer.

---

## 8) Acceptance

- `ui.snapshot` of Calculator (`interactive_only=true`) is ≤ 500 tokens
  (chars/4), lists the digit/operator buttons with `[eN]` refs, and a subsequent
  `ui.click { element_ref: "eN" }` presses that button.
- `ui.snapshot` of an Electron app (VS Code or Slack) shows web/Electron content
  (proving `AXManualAccessibility`) and stays ≤ 2000 tokens with
  `interactive_only=true` at the default depth.
- `ui.screenshot` writes a PNG to the run directory and returns its path +
  dimensions; no image bytes appear in the step result.
- `operator-jack doctor` reports Screen Recording permission status.
