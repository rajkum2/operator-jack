# Operator CLI Spec Freeze v0.12

Status: Frozen for implementation
Version: `0.13.0`
Date: `2026-07-09`
Predecessor: Spec Freeze v0.11

Additive delta to v0.11. Covers **M12 — web autonomy**: the browser lane (M5's
CDP executor) is finished and wired into plans, the agent loop, and MCP, behind
a hard profile-isolation boundary. This unlocks the "scrape a website" scenario.

---

## 1) Profile isolation (lands before any model drives a browser)

The M5 executor launched Chrome with **no `--user-data-dir`** (riding the user's
real, logged-in profile) and attached to any Chrome already on the debug port.
M12 closes that hole:

- `launch_chrome(port, user_data_dir, headless)` now **requires** an
  operator-owned profile dir and always passes `--user-data-dir`.
- `BrowserLaunchOptions { user_data_dir, headless, allow_real_profile }`. Default
  = a fresh **ephemeral** profile per run (removed on close), headless, and
  `allow_real_profile: false`.
- `connect()` in isolated mode **refuses** to attach to a Chrome it did not
  launch ("refusing to attach to a non-owned browser") — attaching to the user's
  real session requires the escalating `browser.real_profile` capability.
- A named `user_data_dir` gives a persistent profile ("stay logged in to site X").

---

## 2) The browser lane & the sync↔async bridge

`operator-runtime` gains an `operator-exec-browser` dependency and a
`browser_lane` module:

- **`BrowserLane`** owns a dedicated OS thread running its own multi-threaded
  tokio runtime; the synchronous engine talks to it over **std** channels only.
  No tokio primitive touches the caller's thread, so it is safe from the sync CLI
  and from within any other runtime — sidestepping the "nested
  `Runtime::block_on` panics" trap. The reactor stays live on the lane thread, so
  the executor's background WebSocket tasks keep running while the engine blocks
  on a reply.
- **`BrowserHandle`** is a lazily-spawned, per-run wrapper: Chrome launches only
  on the first `browser.*` step, and is torn down (process killed, ephemeral
  profile removed) at run end.
- The engine's lane dispatch gains a `"browser"` arm; `execute_plan` and
  `RunSession` each hold a `BrowserHandle`. Browser steps now route through the
  same policy gate, redaction, and audit as every other step.
- The `browser.*` hard-reject in `validation.rs` is removed.

---

## 3) Step types

Existing (now reachable in plans): `browser.navigate`, `browser.click`,
`browser.type`, `browser.get_text`, `browser.get_attribute`,
`browser.execute_js`, `browser.screenshot`, `browser.wait_for`, `browser.scroll`.

New (M12):
- `browser.get_text_all {selector}` → `{count, items: [...]}` (querySelectorAll).
- `browser.get_attributes_all {selector, attribute}` → array (href/src resolved
  to absolute URLs when the DOM property exists).
- `browser.snapshot {max_elements?}` → `{title, url, elements: [{tag, type, text,
  href, selector}]}` — the browser analogue of `ui.snapshot`, so the agent loop
  is lane-agnostic.
- `browser.download {url? | selector?, dir?, timeout_ms?}` → `{path, size_bytes}`
  via `Page.setDownloadBehavior` + wait-for-complete (ignores `.crdownload`).

Robustness:
- **Auto-waiting selectors**: `get_text`/`get_attribute`/`click`/`type` poll for
  the element (up to 5s) before acting, like Playwright — so a step run right
  after navigation doesn't fail on a not-yet-rendered element.
- **Real navigation waits**: `browser.navigate` polls `document.readyState ===
  'complete'` (and that we've left `about:blank`), up to `wait_ms` (default 15s),
  instead of a blind fixed sleep.
- **Screenshots write a file** and return its `path` — never base64 into the
  store (audit-DB bloat).

---

## 4) Agent loop & MCP

- The agent loop (M10) gains web actions: `browser_navigate`, `browser_snapshot`,
  `browser_get_text`, `browser_get_text_all`, `browser_get_attributes_all`,
  `browser_click`, `browser_type`, `browser_wait_for`, `browser_screenshot`,
  `browser_download`, described in its system prompt (scrape lists with
  `get_text_all`, paginate with `click` then re-scrape).
- MCP mode (M9) exposes the browser tools too (they run through the session's
  `BrowserHandle`, same gate/audit).

---

## 5) Domain allow-list

`allow_domains` (plan + config) threads into the lane; the executor enforces it
per navigation (`is_url_allowed`) in addition to the policy gate — belt and
suspenders. `data:` URLs (no domain) are allowed when the allow-list is empty.

---

## 6) Acceptance

- Verified live (Chrome present): a plan `navigate → get_text → get_text_all →
  get_attributes_all → snapshot` scraped a page end-to-end — `get_text_all`
  returned `["Alpha","Beta","Gamma"]`, `get_attributes_all(href)` returned
  `["/a","/b","/c"]`, `snapshot` listed elements with `selector` refs. (Tests use
  a self-contained `data:` URL because this machine's antivirus intercepts live
  `http` navigations — the interception itself confirmed navigation + extraction
  read the *served* page correctly, title "Bitdefender Alert Page".)
- Profile isolation verified: the lane launches its own Chrome with a dedicated
  `--user-data-dir`; a foreign Chrome on the port is refused without the grant.
- `docs/examples/browser-navigate.json` and `browser-scrape.json` validate under
  `check-examples.sh`.
- Full gate green: fmt, clippy `-D warnings`, 155 Rust tests, 10 Swift tests.

Known-partial (documented, not blocking): downloads are implemented but not
exercised against a live download server here; `browser.snapshot` is a flat
interactive-element list (not a full a11y tree); the `browser.*` CLI
subcommands' `--dry-run`/`--json` polish is deferred.
