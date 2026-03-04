# Operator CLI Spec Freeze v0.3

Status: Frozen for implementation
Version: `0.3.0`
Date: `2026-03-03`
Predecessor: Spec Freeze v0.2

This document is an **additive delta** to Spec Freeze v0.2. Everything in v0.2 remains normative unless explicitly overridden below. This document covers M3 (UI Executor v1) additions.

---

## 1) Scope Delta (v0.3 additions)

New in v0.3:
- `ui.list_windows` step type for window enumeration
- `ui.focus_window` step type for window targeting
- Window scoping in UI selectors (`window` field)
- Implicit wait infrastructure for UI action steps (`implicit_wait_ms`)
- Expanded error taxonomy for UI operations
- Selector disambiguation UX (interactive candidate prompt)
- `anyOf` selector strategy (fallback locators) — M3b
- `element_ref` concept (stateful element references) — M3b
- `operator ui inspect` CLI command — M3b
- Evidence hooks (mini-dump in step outputs) — M3b

---

## 2) New Step Types

### `ui.list_windows`

Purpose: Enumerate windows for a target application.

Params:
- `app` (string, required)

Return:
```json
{
  "windows": [
    { "title": "Document.txt", "index": 0, "main": true, "focused": true },
    { "title": "Preferences", "index": 1, "main": false, "focused": false }
  ]
}
```

Risk level: **Low**

IPC method: `ui.listWindows`

### `ui.focus_window`

Purpose: Bring a specific window to the foreground deterministically.

Params:
- `app` (string, required)
- `window` (object, required): One of:
  - `{ "index": <int> }` — 0-based window index from `ui.list_windows` ordering
  - `{ "title_contains": "<string>" }` — first window whose title contains substring

Specifying both `index` and `title_contains` is a validation error. Specifying neither is a validation error.

Return:
```json
{
  "focused": true,
  "window": { "title": "Document.txt", "index": 0 }
}
```

Risk level: **Medium**

IPC method: `ui.focusWindow`

---

## 3) Selector Additions

### 3.1 Window Scoping

Add an optional `window` field to the UI selector object (Section 8.4 of v0.2):

```json
{
  "role": "AXTextArea",
  "window": { "title_contains": "My Document" }
}
```

- `window.index` (int, optional): 0-based window index
- `window.title_contains` (string, optional): substring match on window title
- Mutually exclusive: specifying both is a validation error
- When omitted: search defaults to the frontmost window of the app
- Applied before tree traversal begins — narrows the AX root to the specified window

### 3.2 anyOf Selector Strategy (M3b)

Add an optional `anyOf` field to the UI selector object:

```json
{
  "anyOf": [
    { "role": "AXTextArea", "name_contains": "Body" },
    { "identifier": "note-body" },
    { "role": "AXTextArea", "window": { "index": 0 } }
  ],
  "index": 0
}
```

- `anyOf` (array of selector objects, optional): Fallback locator stack
- Tried in order; first strategy returning exactly 1 match wins
- If no strategy returns exactly 1 match, disambiguation applies on the first strategy that returned any matches
- `index` at the top level applies to the winning strategy's results
- If `anyOf` is present, no other selector fields (except `index`) should be set at the top level — validation error otherwise

---

## 4) Implicit Waits

UI action steps (`ui.click`, `ui.type_text`, `ui.read_text`, `ui.key_press`, `ui.set_value`, `ui.select_menu`) MUST support implicit waiting for the target element.

- `implicit_wait_ms` (int, optional): Available at step level. Default: 2000ms.
- Polling interval: 200ms
- Before reporting `ELEMENT_NOT_FOUND`, the handler polls for the element at the polling interval up to `implicit_wait_ms` total elapsed time.
- Explicit `ui.wait_for` remains for complex conditions (text_equals, enabled, visible).
- `implicit_wait_ms = 0` disables implicit waiting for that step.

---

## 5) element_ref (M3b)

### 5.1 Concept

When `ui.find` returns matches, each match includes an `element_ref` string (a ULID). Subsequent action steps (`ui.click`, `ui.read_text`, `ui.type_text`, `ui.set_value`) MAY accept `element_ref` as an alternative to `selector`.

### 5.2 Behavior

- `element_ref` is resolved from an in-memory map in the Swift helper, keyed by the ULID.
- Lifetime: per-run only (helper process dies at run end, all refs are invalidated).
- Staleness: if the referenced AXUIElement is no longer valid (UI changed), the helper MUST fall back to re-resolving via the original selector metadata stored with the ref.
- If both `element_ref` and `selector` are provided, `element_ref` is tried first.

### 5.3 Validation

When `element_ref` is provided, `selector` is optional for: `ui.click`, `ui.read_text`, `ui.type_text`, `ui.set_value`. The `app` param remains required.

---

## 6) Error Codes (Additions)

Add the following to Section 13 of v0.2:

| Code | Message Pattern | Retryable | When |
|------|----------------|-----------|------|
| `APP_NOT_FOUND` | Target app name does not match any known running application | false | focusApp, listWindows, any UI step |
| `APP_NOT_RUNNING` | Target app PID not found (terminated) | true | Any UI step |
| `WINDOW_NOT_FOUND` | Specified window index/title does not exist | false | focusWindow, find with window scope |
| `ELEMENT_NOT_FOUND` | Selector matched zero elements after implicit wait | true | click, readText, typeText, setValue, find |
| `ELEMENT_AMBIGUOUS` | Selector matched multiple elements, no index given | false | click, readText, typeText, setValue |
| `ELEMENT_NOT_ACTIONABLE` | Element exists but cannot perform requested action | false | click (disabled), setValue (not settable) |
| `PERMISSION_DENIED` | Accessibility permission not granted | false | Any UI step requiring AX access |
| `TIMEOUT` | Wait condition not met within timeout | true | waitFor |
| `INPUT_BLOCKED` | CGEvent keyboard simulation failed (secure input) | false | typeText, keyPress |

Note: `ELEMENT_NOT_FOUND` and `ELEMENT_AMBIGUOUS` replace the v0.2 codes `SELECTOR_NOT_FOUND` and `SELECTOR_AMBIGUOUS` respectively. The v0.2 codes remain accepted as aliases for backward compatibility.

---

## 7) Disambiguation UX

### 7.1 Swift helper behavior

When `ELEMENT_AMBIGUOUS` is thrown, the error details MUST include a `candidates` array:

```json
{
  "code": "ELEMENT_AMBIGUOUS",
  "message": "Selector matched 3 elements. Specify 'index' or refine selector.",
  "retryable": false,
  "details": {
    "candidates": [
      { "index": 0, "role": "AXButton", "name": "Save", "path": "Window[0]/Button[2]" },
      { "index": 1, "role": "AXButton", "name": "Save", "path": "Sheet[0]/Button[0]" }
    ]
  }
}
```

### 7.2 Rust runtime behavior

- In interactive mode: display candidates to user on stderr, prompt for choice `[1-N]` or `q` to abort, re-send IPC call with `selector.index` set to chosen value.
- In non-interactive mode: propagate `ELEMENT_AMBIGUOUS` as a non-retryable error.
- `--yes` flag does NOT auto-resolve ambiguity (too dangerous). User must choose explicitly.

### 7.3 IPC error details support

`IpcErrorPayload` gains an optional `details` field (JSON object). This is already present in the Rust `IpcErrorPayload` struct (`pub details: serde_json::Value`). The Swift side must be updated to support populating this field.

---

## 8) Evidence Hooks (M3b)

Each UI action step output MAY include an `_evidence` object for debugging and auditability:

```json
{
  "clicked": true,
  "target": { "role": "AXButton", "name": "Save" },
  "_evidence": {
    "active_app": "Notes",
    "active_window": "My Note",
    "element_summary": { "role": "AXButton", "name": "Save", "enabled": true, "path": "Window[0]/Button[2]" }
  }
}
```

- `_evidence` is optional and added automatically, not controlled by user params in M3b.
- Screenshots are deferred to a future milestone.

---

## 9) CLI Addition: `operator ui inspect` (M3b)

```
operator ui inspect --app <name> [--depth <n>] [--selector '<json>']
```

- Dumps the AX tree for the specified app to `depth` levels (default 5).
- Optional `--selector` filter: only show branches containing matching elements.
- Output format: indented text tree with role, name, value, path for each element.
- Requires helper binary and accessibility permission.

---

## 10) IPC Method Additions

Add to Section 15 of v0.2:

| Method | Direction | Purpose |
|--------|-----------|---------|
| `ui.listWindows` | request | Enumerate app windows |
| `ui.focusWindow` | request | Target specific window |
| `ui.inspect` | request | Dump AX tree (M3b) |

Method name translation additions:
- `ui.list_windows` (Rust) -> `ui.listWindows` (Swift)
- `ui.focus_window` (Rust) -> `ui.focusWindow` (Swift)

---

## 11) Risk Level Updates

Add to Section 11 of v0.2:

| Step Type | Risk |
|-----------|------|
| `ui.list_windows` | Low |
| `ui.focus_window` | Medium |

---

## 12) Step Output Updates

Add to Section 8.5 of v0.2:

- `ui.list_windows`: `windows` (array of `{ title: string, index: int, main: bool, focused: bool }`)
- `ui.focus_window`: `focused` (bool), `window` (object with title/index)
- `ui.find` gains optional `element_ref` per match (M3b)
- `ui.read_text` gains `source_attribute` (string) indicating which AX attribute was read

---

*Implementation note: M3a delivers the core 9 handlers + window scoping + implicit waits + disambiguation + error taxonomy. M3b adds anyOf, element_ref, evidence, setValue, selectMenu, ui inspect, and selection caching.*
