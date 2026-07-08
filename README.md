# Operator Jack

Operator Jack is a local-first, privacy-respecting command-line tool for automating macOS tasks through structured JSON plans. It executes automation across three lanes -- system operations (files, processes, URLs), UI automation (macOS Accessibility API), and browser control (Chrome DevTools Protocol) -- with built-in policy gates, secret redaction, and an append-only audit log. Automation runs locally with no telemetry and no hidden background processes. The only feature that may make a network call is the optional LLM planner (the `do` command), which can use a cloud provider (Kimi/OpenAI/Anthropic) or a fully local Ollama model.

## Current Status

**v0.7.1 — Milestones M0 through M6 are complete.** System, UI (Accessibility), and browser (CDP) lanes; an LLM-backed natural-language planner (Kimi, OpenAI, Anthropic, Ollama); and a skills system. 128 Rust tests + 9 Swift tests passing. See [`MASTER_PLAN.md`](MASTER_PLAN.md) for the M7-M15 roadmap.

> **Note:** the browser (CDP) lane is currently reachable only via the `operator-jack browser` CLI subcommands; browser steps inside plans are not yet wired into the engine (that is milestone M12).

## Installation

### Homebrew

```
brew install rajkum2/tap/operator-jack
```

The tap may lag the latest release; verify with `operator-jack --version`.

### Install script

```
curl -fsSL https://raw.githubusercontent.com/rajkum2/operator-jack/main/install.sh | bash
```

### Build from source

See [Quick Start](#quick-start) below.

## Quick Start

Build the project:

```
cargo build --release
cd macos-helper && swift build -c release && cd ..
```

Run the doctor command to check your environment:

```
./target/release/operator-jack doctor
```

### Natural Language (M4)

Execute a natural language instruction using an LLM provider:

```
# Using Ollama (local, no API key needed)
ollama pull llama3.2
ollama serve
./target/release/operator-jack do "open Notes and type hello world"

# Using other providers (requires API key)
export KIMI_API_KEY="your-key-here"
./target/release/operator-jack do "create a folder called Projects" --provider kimi
```

### Plan-based Execution

Validate a plan file without executing it:

```
./target/release/operator-jack plan validate --plan-file docs/examples/open-app.json
```

Execute a plan (the `--yes` flag auto-approves policy prompts):

```
./target/release/operator-jack run --plan-file docs/examples/file-operations.json --yes
```

Dry-run a plan to see what would happen without performing any actions:

```
./target/release/operator-jack run --plan-file docs/examples/file-operations.json --dry-run
```

## CLI Commands

### Natural Language (M4)

| Command | Description |
|---|---|
| `operator-jack do "<instruction>"` | Execute a natural language instruction. |
| `operator-jack do "<instruction>" --provider kimi` | Use specific LLM provider. |
| `operator-jack do "<instruction>" --show-plan` | Show generated plan without executing. |
| `operator-jack do "<instruction>" --save-plan plan.json` | Save generated plan to file. |

### Plan-based Execution

| Command | Description |
|---|---|
| `operator-jack run --plan-file <path>` | Execute a plan from a JSON file. |
| `operator-jack run --plan-file <path> --yes` | Execute with all policy prompts auto-approved. |
| `operator-jack run --plan-file <path> --dry-run` | Simulate execution without performing actions. |
| `operator-jack plan validate --plan-file <path>` | Validate a plan file and report errors. |
| `operator-jack plan save --plan-file <path>` | Save a plan to the store. |
| `operator-jack exec <plan-id>` | Execute a previously saved plan by its ID. |
| `operator-jack doctor` | Check environment: accessibility permission, helper binary, store, provider readiness. |
| `operator-jack stop` | Stop a running operator-jack process (sends SIGTERM via PID file). |
| `operator-jack ui inspect --app <name>` | Dump the accessibility tree for an app. |
| `operator-jack init` | Create a config file at ~/.config/operator-jack/config.toml. |

### Skills (M6)

| Command | Description |
|---|---|
| `operator-jack skill list` | List discovered skills. |
| `operator-jack skill validate <skill-file>` | Validate a skill manifest. |
| `operator-jack skill run <name> --param key=value` | Expand a skill into a plan and run it. |

### Browser (M5)

Chrome DevTools Protocol automation. Currently exposed only through these CLI subcommands; browser steps inside plans are not yet wired into the engine (milestone M12).

| Command | Description |
|---|---|
| `operator-jack browser doctor` | Check that Chrome is available. |
| `operator-jack browser launch [--port 9222]` | Launch Chrome with remote debugging. |
| `operator-jack browser navigate <url> [--port N]` | Navigate to a URL via CDP. |
| `operator-jack browser screenshot --output <path> [--full-page] [--port N]` | Capture a screenshot. |

### Common Flags

| Flag | Description |
|---|---|
| `--mode safe\|unsafe` | Execution mode (default: safe). |
| `--yes` | Auto-approve all policy gate prompts. |
| `--dry-run` | Validate and simulate without executing. |
| `--json` | Emit output as JSON. |
| `--helper-path <path>` | Path to the macOS helper binary. |
| `--allow-apps <list>` | Restrict UI automation to these apps (comma-separated). |
| `--allow-domains <list>` | Restrict URLs/navigation to these domains (comma-separated). |
| `--interactive` / `--no-interactive` | Force or disable interactive prompts. |
| `--verbose` / `--quiet` | Detailed output / errors only. |

## Example Plan

Here is a minimal plan that opens the TextEdit application:

```json
{
  "schema_version": 1,
  "name": "Open TextEdit",
  "description": "Opens the TextEdit application",
  "steps": [
    {
      "id": "open_textedit",
      "type": "sys.open_app",
      "params": { "app": "TextEdit" }
    }
  ]
}
```

More examples are available in [`docs/examples/`](docs/examples/):

- [`open-app.json`](docs/examples/open-app.json) -- Open an application.
- [`file-operations.json`](docs/examples/file-operations.json) -- Create folders, write files, read files back.
- [`notes-automation.json`](docs/examples/notes-automation.json) -- Open Notes and create a new note with UI automation.
- [`chrome-search.json`](docs/examples/chrome-search.json) -- Open Chrome and navigate to a search URL.
- [`calculator-buttons.json`](docs/examples/calculator-buttons.json) -- Click calculator buttons via UI automation.
- [`full-end-to-end.json`](docs/examples/full-end-to-end.json) -- Complete end-to-end UI automation workflow.

## Project Structure

Operator Jack is organized as a Cargo workspace with nine crates:

```
operator-jack/
  Cargo.toml                  # Workspace root
  crates/
    operator-cli/             # CLI entry point (Clap argument parsing, output rendering)
    operator-runtime/         # Execution engine (step loop, retries, cancellation)
    operator-core/            # Shared types, plan validation, variable interpolation, redaction
    operator-store/           # SQLite persistence, JSONL audit log
    operator-exec-system/     # sys.* executor (files, processes, URLs)
    operator-exec-browser/    # browser.* executor (Chrome DevTools Protocol, M5)
    operator-ipc/             # NDJSON stdio bridge to Swift macOS helper
    operator-planner/         # LLM planner (M4) - Kimi/OpenAI/Anthropic/Ollama providers
    operator-skills/          # Skills system (M6) - manifest parse/validate/expand
  macos-helper/               # Swift helper binary for macOS Accessibility API
  skills/                     # Built-in skill manifests (M6)
  docs/
    ARCHITECTURE.md           # System architecture and data flow
    SECURITY.md               # Security model and threat analysis
    SELECTORS.md              # UI selector reference
    PERMISSIONS_MACOS.md      # macOS permission setup guide
    ROADMAP.md                # Milestone roadmap
    examples/                 # Example plan JSON files
```

### Crate Dependency Graph

```
operator-cli
  -> operator-runtime
       -> operator-core
       -> operator-store
       -> operator-exec-system
       -> operator-ipc
```

## Documentation

- [Architecture](docs/ARCHITECTURE.md) -- System design, data flow, crate responsibilities, storage schema.
- [Security](docs/SECURITY.md) -- Threat model, policy gates, redaction, process isolation.
- [Selectors](docs/SELECTORS.md) -- UI element selector syntax and matching rules.
- [macOS Permissions](docs/PERMISSIONS_MACOS.md) -- Setting up Accessibility permission for UI automation.
- [Master Plan](MASTER_PLAN.md) -- Authoritative forward roadmap (M7-M15).
- [Roadmap](docs/ROADMAP.md) -- Historical milestone plan (M0-M6); superseded by the Master Plan.

## License

MIT
