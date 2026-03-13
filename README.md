# Operator Jack

Operator Jack is a local-first, privacy-respecting command-line tool for automating macOS tasks through structured JSON plans. It executes automation across three lanes -- system operations (files, processes, URLs), UI automation (macOS Accessibility API), and browser control (Chrome DevTools Protocol) -- with built-in policy gates, secret redaction, and an append-only audit log. Everything runs locally; there are no cloud calls, no telemetry, and no hidden background processes.

## Current Status

**Milestone 4 (Rule-based Planner) is complete.** All previous milestones plus natural language plan generation via LLM providers (Kimi, OpenAI, Anthropic, Ollama). 108 tests passing.

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
| `operator-jack plan list` | List previously executed plans from the store. |
| `operator-jack doctor` | Check environment: accessibility permission, helper binary, store. |
| `operator-jack stop` | Stop a running operator-jack process (sends SIGTERM via PID file). |
| `operator-jack ui inspect --app <name>` | Dump the accessibility tree for an app. |

### Common Flags

| Flag | Description |
|---|---|
| `--yes` | Auto-approve all policy gate prompts. |
| `--dry-run` | Validate and simulate without executing. |
| `--verbose` | Show detailed output including step timings. |
| `--quiet` | Suppress all output except errors. |
| `--set key=value` | Override a plan variable from the command line. |

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

Operator Jack is organized as a Cargo workspace with seven crates:

```
operator-jack/
  Cargo.toml                  # Workspace root
  crates/
    operator-cli/             # CLI entry point (Clap argument parsing, output rendering)
    operator-runtime/         # Execution engine (step loop, retries, cancellation)
    operator-core/            # Shared types, plan validation, variable interpolation, redaction
    operator-store/           # SQLite persistence, JSONL audit log
    operator-exec-system/     # sys.* executor (files, processes, URLs)
    operator-ipc/             # NDJSON stdio bridge to Swift macOS helper
    operator-planner/         # Rule-based planner (M4) - LLM providers
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
- [Roadmap](docs/ROADMAP.md) -- Milestone plan from M0 through future vision.

## License

MIT
