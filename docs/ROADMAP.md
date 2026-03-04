# Operator Jack Roadmap

## Milestones

### M0: Scaffolding -- DONE

**Status:** Complete

Established the project foundation:

- Cargo workspace with six crates: `operator-cli`, `operator-runtime`, `operator-core`, `operator-store`, `operator-exec-system`, `operator-ipc`.
- Core types defined: `Plan`, `Step`, `StepResult`, `Selector`, `PolicyLevel`, `RunStatus`.
- Plan JSON parsing and validation.
- SQLite store with schema migrations.
- CLI skeleton with Clap: `run`, `plan validate`, `plan list`, `doctor`, `stop` commands.
- Stub executors for all step types (return `StepResult` with status `skipped` and a message indicating the executor is not yet implemented).
- Variable interpolation engine.
- Redaction filters (key-name and pattern matching).
- JSONL audit log writer.

### M1: System Executor -- DONE

**Status:** Complete

All 13 `sys.*` step types implemented with real functionality:

- `sys.open_app` -- Launch applications via `open -a` Command.
- `sys.open_url` -- Open URLs via `open` Command, with `allow_domains` enforcement.
- `sys.quit_app` -- Graceful quit via osascript, force quit via pkill.
- `sys.read_file` -- Read file contents via `std::fs::read_to_string` with tilde expansion.
- `sys.write_file` -- Write file contents via `std::fs::write` with `create_parent` option.
- `sys.append_file` -- Append to files via `OpenOptions::append` with `create_parent` option.
- `sys.mkdir` -- Create directories with optional `parents` flag.
- `sys.move_path` -- Move/rename via `std::fs::rename` with overwrite guard.
- `sys.copy_path` -- Copy files via `std::fs::copy`, recursive copy for directories.
- `sys.delete_path` -- Remove files/directories via `remove_file`/`remove_dir_all` with recursive flag.
- `sys.exec` -- Direct command execution (no shell), with 1MiB capture cap, `env_clean`, $HOME default cwd, and timeout enforcement.
- `sys.clipboard_get` -- Read clipboard via `pbpaste`.
- `sys.clipboard_set` -- Write clipboard by piping into `pbcopy`.
- Policy gate integration: risk classification per step type, interactive prompting, `--yes` bypass.
- `--dry-run` mode: validate and simulate without executing.
- `--yes` flag: auto-approve all policy prompts.
- 21 unit tests for executor, 64 total tests passing.
- Manual acceptance tests passed: TextEdit open/quit, file ops, URL, clipboard.

### M2: macOS Helper v1 -- DONE

**Status:** Complete

Built the Swift helper binary and IPC bridge:

- Swift SPM executable in `macos-helper/` communicating via NDJSON over stdin/stdout.
- IPC protocol: JSON request/response with ULID correlation, error codes, 1 MiB line cap.
- `ui.ping` method for connectivity testing and protocol handshake (validates `protocol_version == "1"`).
- `ui.checkAccessibilityPermission` method with optional system prompt dialog, integrated into `operator doctor`.
- `ui.listApps` method listing running applications (dock-visible, with name/bundleId/pid/active).
- Graceful shutdown: close stdin to signal EOF, wait 2s, kill if still alive.
- `operator-ipc` crate: real process spawning, NDJSON framing, handshake validation, method name translation (snake_case to camelCase), crash detection.
- `operator-jack doctor` upgraded with 4th accessibility check.
- Helper auto-discovery: CLI flag, env var, PATH, sibling, dev fallback.
- 9 new unit tests (5 framing + 4 client), 73 total tests passing.

### M3: UI Executor v1

**Status:** Planned

Implement core UI automation steps:

- `ui.find_element` -- Find elements using selector matching against the accessibility tree.
- `ui.click` -- Click a UI element (single click, double click).
- `ui.set_value` -- Set the value of a text field, checkbox, or other input element.
- `ui.type_text` -- Type text by simulating keyboard input.
- `ui.key_press` -- Simulate key presses (Return, Tab, Escape, modifier combos).
- `ui.select_menu` -- Navigate application menus by path (e.g., `["File", "New Note"]`).
- `ui.wait_for` -- Wait for an element matching a selector to appear, with timeout.
- `ui.get_value` -- Read the current value of a UI element.
- Selector matching engine: pre-order traversal, all field types, index disambiguation.
- Interactive disambiguation: present options to user when multiple elements match.
- Non-interactive disambiguation: fail with `SELECTOR_AMBIGUOUS`.
- `allow_apps` enforcement for all UI steps.

### M4: Rule-Based Planner

**Status:** Planned

Enable natural language input for common automation patterns:

- Parser for approximately 20 command patterns covering frequent macOS tasks:
  - "open [app]"
  - "create folder [path]"
  - "write [content] to [file]"
  - "read [file]"
  - "click [button/element] in [app]"
  - "type [text] in [app]"
  - "go to [url]"
  - "take screenshot"
  - "select menu [path] in [app]"
  - "wait for [element] in [app]"
  - And more.
- Patterns are matched using keyword extraction and slot filling, not a language model.
- Generates a plan JSON from the parsed input.
- `operator-jack do "open Notes and type hello"` command interface.
- Graceful fallback: unrecognized input returns a clear error with suggestions.

### M5: Browser Executor (CDP)

**Status:** Planned

Add Chrome DevTools Protocol support for web automation:

- Connect to Chrome/Chromium via CDP (Chrome must be launched with `--remote-debugging-port`).
- `browser.navigate` -- Navigate to a URL.
- `browser.click` -- Click an element identified by CSS selector.
- `browser.type` -- Type text into a form field.
- `browser.get_text` -- Extract text content from elements.
- `browser.execute_js` -- Run JavaScript in the page context (high risk).
- `browser.screenshot` -- Capture a screenshot of the page or element.
- `browser.wait_for` -- Wait for a CSS selector to appear in the DOM.
- `allow_domains` enforcement: reject navigation to domains not in the allowlist.
- Connection lifecycle management: discover CDP port, connect WebSocket, handle disconnects.

### M6: Skills System

**Status:** Planned

Introduce reusable automation macros:

- Skills are YAML or JSON manifest files that define a parameterized sequence of steps.
- Skill manifests live in `~/.operator-jack/skills/` or the project's `skills/` directory.
- Skills declare inputs (required and optional parameters) and map them to step variables.
- `operator-jack skill run <name> --param key=value` invocation.
- `operator-jack skill list` to discover available skills.
- `operator-jack skill validate <name>` to check a skill manifest.
- Skills are expanded into a full plan at execution time -- they do not introduce new runtime concepts.
- Community sharing via copying skill files (no package manager, no network).

### M7: Robustness and Recovery

**Status:** Planned

Improve reliability for real-world automation:

- Enhanced retry logic with exponential backoff and jitter.
- Selector ranking: when multiple elements match, score them by confidence (exact name match > substring match > path match) and pick the best.
- Plan replay: re-run a previous run with the same inputs, skipping already-completed steps.
- Checkpoint/resume: save engine state periodically so a crashed run can be resumed from the last successful step.
- Better error diagnostics: structured error codes, suggested fixes, links to documentation.
- Flaky step detection: track step success rates across runs and warn about unreliable steps.

### M8: Offline Speech-to-Text Input

**Status:** Planned

Enable voice-driven automation using local speech recognition:

- Integrate whisper.cpp for offline speech-to-text (no cloud API, no network).
- `operator-jack listen` command: activate microphone, transcribe speech, pipe text to the rule-based planner (M4).
- Push-to-talk and voice-activity-detection modes.
- Audio stays local -- recorded audio is processed in memory and discarded after transcription.
- Configurable model size (tiny, base, small, medium) for speed vs. accuracy tradeoff.
- Works entirely offline after initial model download.

### Future

These milestones are not yet scheduled but represent the long-term vision:

- **Windows port (UIA)** -- Replace macOS Accessibility with Windows UI Automation. System executor adapts to Windows APIs. IPC bridge communicates with a C# or C++ helper.
- **Linux port (AT-SPI)** -- Replace macOS Accessibility with AT-SPI2 (Assistive Technology Service Provider Interface) for GNOME/GTK and Qt applications.
- **Local LLM planner** -- Replace or augment the rule-based planner (M4) with a local language model (e.g., llama.cpp) for more flexible natural language understanding. No cloud calls.
- **Multi-machine orchestration** -- Coordinate plans across multiple machines on a local network. Secure peer-to-peer communication, distributed step execution, aggregated results.
- **Screen recording and replay** -- Record user actions as a plan, then replay them. Uses accessibility events and screen capture to generate step sequences.
- **Plan editor GUI** -- A native macOS app (SwiftUI) for visually building and editing plans with drag-and-drop steps, live element picking, and real-time validation.
