# Contributing to Operator Jack

Thank you for your interest in contributing! This document covers the development setup, coding conventions, and pull request process.

## Development Setup

### Prerequisites

- **macOS 13+** (Ventura or later)
- **Rust** (stable, installed via `brew install rust` or rustup)
- **Swift** (included with Xcode Command Line Tools: `xcode-select --install`)
- **Git**

### Building

```bash
# Clone the repo
git clone https://github.com/rajkum2/operator-jack.git
cd operator-jack

# Build everything
cargo build                          # Rust workspace
cd macos-helper && swift build && cd ..  # Swift helper

# Run tests
cargo test

# Run the CLI
./target/debug/operator-jack doctor
```

### Project Structure

```
operator-jack/
  crates/
    operator-cli/          # CLI binary (clap)
    operator-core/         # Types, validation, interpolation, redaction
    operator-runtime/      # Execution engine
    operator-store/        # SQLite persistence
    operator-exec-system/  # sys.* step handlers
    operator-ipc/          # NDJSON IPC to Swift helper
  macos-helper/            # Swift helper for macOS Accessibility API
  docs/                    # Architecture, security, selectors, permissions
```

## Code Style

### Rust
- Format with `cargo fmt` before committing
- No clippy warnings: `cargo clippy -- -D warnings`
- Use `thiserror` for library crate errors, `anyhow` for CLI
- ULIDs for all generated IDs
- No `async`/`tokio` until M5 (browser automation)

### Swift
- Standard Swift conventions
- All handlers follow the signature: `([String: JSONValue]) throws -> [String: JSONValue]`
- Raw `ApplicationServices` API — no third-party AX wrappers

## Pull Request Process

1. **Create a branch** from `main`:
   ```bash
   git checkout -b feat/your-feature
   ```

2. **Make your changes** with clear, focused commits.

3. **Verify everything builds and tests pass:**
   ```bash
   cargo build && cargo test && cd macos-helper && swift build && cd ..
   ```

4. **Open a PR** against `main` with:
   - A clear title describing the change
   - A description of what and why
   - Reference any related issues

5. **CI must pass** — the GitHub Actions workflow runs `cargo fmt --check`, `cargo clippy`, `cargo test`, and `swift build` on every PR.

## Adding a New Step Type

1. Add the variant to `StepType` enum in `crates/operator-core/src/types.rs`
2. Update `lane()`, `as_str()`, and `Display` for the new variant
3. Add risk classification in `crates/operator-core/src/policy.rs`
4. Add parameter validation in `crates/operator-core/src/validation.rs`
5. Implement the handler:
   - **sys.*** → `crates/operator-exec-system/src/executor.rs`
   - **ui.*** → `macos-helper/Sources/OperatorMacOSHelper/Methods/YourHandler.swift` + register in `main.swift`
   - **browser.*** → `crates/operator-exec-browser/` (M5+)
6. Add method name translation in `crates/operator-ipc/src/client.rs` (for ui.* types)
7. Add unit tests
8. Add a golden plan JSON in `docs/examples/`

## Reporting Issues

Please open an issue on [GitHub](https://github.com/rajkum2/operator-jack/issues) with:
- What you expected to happen
- What actually happened
- Steps to reproduce
- Your macOS version and terminal app

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
