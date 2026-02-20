# Operator CLI Selectors

## What Are Selectors?

Selectors are JSON objects that identify UI elements on macOS via the Accessibility (AX) API. When a `ui.*` step needs to find a button, text field, window, or any other interface element, it uses a selector to describe what to look for.

The macOS Accessibility API exposes a tree of `AXUIElement` objects for each running application. Each element has properties like role, name, description, value, and a unique identifier. Selectors match against these properties to locate the target element.

## Selector Fields

A selector is a JSON object with one or more of the following fields. All fields are optional, but at least one must be present.

| Field | Type | Description |
|---|---|---|
| `role` | string | The AX role of the element (e.g., `"AXButton"`, `"AXTextField"`, `"AXWindow"`). |
| `subrole` | string | The AX subrole (e.g., `"AXCloseButton"`, `"AXSecureTextField"`). |
| `name` | string | Exact match against the element's `AXTitle` or `AXDescription` used as its accessible name. |
| `name_contains` | string | Substring match against the element's accessible name. |
| `description` | string | Exact match against `AXDescription`. |
| `description_contains` | string | Substring match against `AXDescription`. |
| `value` | string | Exact match against the element's `AXValue`. |
| `value_contains` | string | Substring match against `AXValue`. |
| `identifier` | string | Match against the element's `AXIdentifier` (set by the app developer). |
| `path` | string[] | An ordered list of `role:name` pairs from the root to the target element (see Path Matching below). |
| `index` | integer | Zero-based index for disambiguation when multiple elements match all other criteria. |
| `max_depth` | integer | Maximum tree depth to search. Default: `12`. Limits traversal for performance. |

## Suffix Convention

Selector fields follow a consistent naming convention for match types:

- **Bare field name** (e.g., `name`, `description`, `value`) requires an **exact match** against the element's property.
- **`_contains` suffix** (e.g., `name_contains`, `description_contains`, `value_contains`) requires a **substring match** -- the element's property must contain the specified string.

This convention makes it immediately clear from the field name whether the match is exact or partial.

## Mutual Exclusion

For each property, you may specify **either** the exact match field **or** the substring match field, but **not both**. The following combinations are invalid and will cause a validation error:

- `name` and `name_contains` together
- `description` and `description_contains` together
- `value` and `value_contains` together

Choose the match type that best fits your use case. Use exact match when you know the full text; use substring match when the text may vary or you only know a portion of it.

## Matching Rules

### Tree Traversal

The selector engine traverses the accessibility tree in **pre-order** (depth-first, parent before children). For each element encountered:

1. Check all specified selector fields against the element's properties.
2. If all fields match, the element is a candidate.
3. Continue traversal until `max_depth` is reached or the entire tree has been searched.

### Field Priority

When multiple fields are specified, they are combined with logical AND -- an element must match **all** specified fields to be a candidate.

### Path Matching

The `path` field provides the strongest matching mechanism. It specifies an ordered list of `"role:name"` strings that describe the path from the application's root element down to the target:

```json
{
  "path": ["AXWindow:Document", "AXGroup:Main", "AXButton:Save"]
}
```

Each entry in the path is matched against elements at the corresponding level of the tree. The name portion after the colon uses exact matching. If an element at any level does not match, that path is abandoned.

Path matching is the most precise way to identify an element and is least susceptible to ambiguity.

### max_depth

The `max_depth` field limits how deep into the accessibility tree the selector engine will search. The default value is `12`, which covers most application UIs. Increase this value only if the target element is deeply nested; decrease it to improve search performance when you know the element is near the top of the tree.

### Index-Based Disambiguation

When multiple elements match all other selector fields, the `index` field selects among them by position (zero-based, in tree traversal order):

- `"index": 0` -- the first matching element (default if `index` is not specified and only one match exists).
- `"index": 1` -- the second matching element.
- `"index": 2` -- the third matching element, and so on.

## Disambiguation Behavior

When a selector matches **more than one element** and no `index` field is specified:

### Interactive Mode (TTY attached)

The user is presented with a numbered list of matching elements and their properties. The user selects which element to target, and execution continues with that choice.

### Non-Interactive Mode (no TTY)

The step fails immediately with the error code `SELECTOR_AMBIGUOUS`. The error message includes the number of matching elements and their summary properties to help the user refine their selector.

This ensures operator never guesses which element to interact with -- ambiguity is always resolved explicitly.

## Examples

### Find a Button by Exact Name

Locate a button labeled "Save" in the active application:

```json
{
  "role": "AXButton",
  "name": "Save"
}
```

### Find a Text Field Containing Specific Text

Locate a text field whose current value contains the word "search":

```json
{
  "role": "AXTextField",
  "value_contains": "search"
}
```

### Find an Element by Developer Identifier

Some applications assign stable identifiers to UI elements. Use `identifier` to match them:

```json
{
  "identifier": "com.apple.notes.bodyTextField"
}
```

### Find the Second OK Button

When a dialog has multiple "OK" buttons (e.g., in nested panels), use `index` to pick the second one:

```json
{
  "role": "AXButton",
  "name": "OK",
  "index": 1
}
```

### Find a Menu Item by Path

Locate the "New Note" item under the File menu in Notes:

```json
{
  "path": ["AXMenuBar:", "AXMenu:File", "AXMenuItem:New Note"]
}
```

### Find a Close Button by Subrole

Locate the close button of a window (macOS close buttons have a specific subrole):

```json
{
  "role": "AXButton",
  "subrole": "AXCloseButton"
}
```

### Limit Search Depth

Find a top-level window without searching deep into the tree:

```json
{
  "role": "AXWindow",
  "max_depth": 2
}
```

### Combine Multiple Fields

Find a checkbox with a specific name inside a group with a known description:

```json
{
  "role": "AXCheckBox",
  "name": "Enable notifications",
  "description_contains": "notification preferences"
}
```
