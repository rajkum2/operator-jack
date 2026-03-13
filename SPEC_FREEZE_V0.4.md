# Operator CLI Spec Freeze v0.4

Status: Frozen for implementation  
Version: `0.5.0`  
Date: `2026-03-14`  
Predecessor: Spec Freeze v0.3  

This document is an **additive delta** to Spec Freeze v0.3. Everything in v0.3 remains normative unless explicitly overridden below. This document covers M4 (Rule-based Planner).

---

## 1) Scope Delta (v0.4 additions)

New in v0.4:
- Natural language to plan generation (`operator-jack do`)
- Multi-provider LLM support (Kimi, OpenAI, Anthropic, Ollama)
- Provider abstraction trait for extensibility
- Interactive provider selection
- Plan preview and save options
- Per-provider configuration in config.toml

---

## 2) CLI Command: `operator-jack do`

### Usage

```
operator-jack do "<natural language instruction>" [OPTIONS]
```

### Arguments

- `instruction` (string, required): Natural language description of the task to automate.

### Options

| Option | Description |
|--------|-------------|
| `--provider <name>` | LLM provider: `kimi`, `openai`, `anthropic`, `ollama` |
| `--show-plan` | Display the generated plan without executing |
| `--save-plan <path>` | Save the generated plan to a JSON file |
| `--yes` | Auto-approve all policy gates during execution |
| `--dry-run` | Validate and show plan only, no execution |

### Examples

```bash
# Basic usage with default provider
operator-jack do "open Notes and type hello world"

# Specify provider
operator-jack do "create a folder called Projects" --provider kimi

# Show generated plan without executing
operator-jack do "copy all files from Downloads to Archive" --show-plan

# Save generated plan for later
operator-jack do "open Calculator and calculate 1+2" --save-plan calc.json
```

### Output

On success:
- Generates a plan from the LLM
- Validates the generated plan
- Saves the plan to the store
- Executes the plan (unless `--show-plan` or `--dry-run`)
- Shows run summary

On failure:
- Connection errors (retryable)
- Authentication errors (check API key)
- Invalid response (LLM returned non-JSON)
- Validation errors (generated plan failed schema validation)

---

## 3) LLM Providers

### Supported Providers

| Provider | Default Model | Requires API Key | Environment Variable |
|----------|---------------|------------------|---------------------|
| **Kimi** | `moonshot-v1-8k` | Yes | `KIMI_API_KEY` |
| **OpenAI** | `gpt-4o-mini` | Yes | `OPENAI_API_KEY` |
| **Anthropic** | `claude-3-haiku-20240307` | Yes | `ANTHROPIC_API_KEY` |
| **Ollama** | `llama3.2` | No (local) | N/A |

### Provider Selection Order

1. CLI `--provider` flag (highest priority)
2. `OPERATOR_DEFAULT_PROVIDER` environment variable
3. First available provider with configured API key
4. Interactive selection (if TTY available)
5. Error with helpful setup instructions

### Provider Configuration

Configuration in `~/.config/operator-jack/config.toml`:

```toml
default_provider = "ollama"

[planner.kimi]
model = "moonshot-v1-8k"
base_url = "https://api.moonshot.cn/v1"
max_tokens = 4096
temperature = 0.2
timeout_seconds = 60

[planner.openai]
model = "gpt-4o-mini"
# ...

[planner.anthropic]
model = "claude-3-haiku-20240307"
# ...

[planner.ollama]
model = "llama3.2"
base_url = "http://localhost:11434"
max_tokens = 4096
temperature = 0.2
timeout_seconds = 120
```

---

## 4) Plan Generation

### System Prompt

The LLM receives a detailed system prompt describing:
- Plan JSON schema
- All available step types (sys.* and ui.*)
- Selector format and options
- Risk levels and mode selection
- Guidelines for deterministic automation

### Response Format

The LLM must respond with valid JSON matching the Plan schema:

```json
{
  "schema_version": 1,
  "name": "Short descriptive name",
  "description": "What this plan does",
  "mode": "safe",
  "steps": [
    {
      "id": "unique_step_id",
      "type": "sys.open_app",
      "params": { "app": "Notes" },
      "timeout_ms": 30000,
      "retries": 0,
      "on_fail": "abort"
    }
  ]
}
```

### Response Parsing

The planner handles:
- Raw JSON responses
- Markdown code blocks (```json ... ```)
- Validation of generated plans
- Error reporting for malformed responses

---

## 5) Architecture

### New Crate: `operator-planner`

```
crates/operator-planner/
  src/
    lib.rs           # Public exports
    error.rs         # PlannerError type
    provider.rs      # LlmProvider trait, ProviderType enum
    planner.rs       # Planner struct, provider selection
    prompt.rs        # System and user prompt templates
    kimi.rs          # Kimi API provider
    openai.rs        # OpenAI API provider
    anthropic.rs     # Anthropic API provider
    ollama.rs        # Ollama local provider
```

### Provider Trait

```rust
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn default_model(&self) -> &'static str;
    fn generate_plan(&self, instruction: &str) -> Result<Plan, PlannerError>;
}
```

### Dependencies

- `ureq` for synchronous HTTP requests
- `serde_json` for request/response serialization
- `operator-core` for Plan types

---

## 6) Error Handling

### Error Types

| Error | Description | Retryable |
|-------|-------------|-----------|
| `ApiKeyNotFound` | API key not configured | No |
| `ConnectionError` | Network/connection failed | Yes |
| `InvalidResponse` | LLM returned invalid JSON | No |
| `ParseError` | Failed to parse plan JSON | No |
| `EmptyPlan` | LLM returned empty response | No |
| `RateLimited` | API rate limit exceeded | Yes |
| `AuthenticationFailed` | Invalid API key | No |
| `HttpError` | HTTP error (4xx, 5xx) | 5xx only |

### User-Facing Error Messages

Errors should include:
- Clear description of what went wrong
- Suggested fix (e.g., "Set KIMI_API_KEY environment variable")
- Retry hint for retryable errors

---

## 7) Security Considerations

### API Key Storage

- API keys are read from environment variables
- Keys are not logged or persisted
- Keys are only held in memory during request

### Plan Validation

- All LLM-generated plans are validated before execution
- Policy gates still apply to generated plans
- User confirmation required for medium/high risk in safe mode

### Local-First Design

- Ollama provider requires no external API calls
- No telemetry or analytics sent to external services
- Plans generated locally when using local models

---

## 8) Testing

### Unit Tests

- Provider configuration parsing
- Plan extraction from various response formats
- Provider selection logic
- Error handling

### Integration Tests

- End-to-end with mock LLM responses
- Provider availability detection
- Configuration loading

---

## 9) Future Extensions (M6+)

- **Skills**: Pre-defined plan templates the LLM can reference
- **Multi-step planning**: Break complex tasks into sub-tasks
- **Feedback loop**: Learn from plan execution success/failure
- **Custom providers**: User-defined LLM endpoints

---

*End of Spec Freeze v0.4*
