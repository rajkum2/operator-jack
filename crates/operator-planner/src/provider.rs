//! Provider trait for LLM backends.

use operator_core::types::Plan;

use crate::error::PlannerError;

/// Configuration for an LLM provider.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    /// The API key or access token.
    pub api_key: Option<String>,
    /// Base URL for the API (optional, for custom endpoints).
    pub base_url: Option<String>,
    /// Model name to use.
    pub model: Option<String>,
    /// Maximum tokens to generate.
    pub max_tokens: Option<u32>,
    /// Temperature (0.0 - 2.0).
    pub temperature: Option<f32>,
    /// Timeout in seconds.
    pub timeout_seconds: Option<u64>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: None,
            model: None,
            max_tokens: Some(4096),
            temperature: Some(0.2),
            timeout_seconds: Some(60),
        }
    }
}

/// A provider that can generate plans from natural language instructions.
pub trait LlmProvider: Send + Sync {
    /// Returns the provider name.
    fn name(&self) -> &'static str;

    /// Returns the default model for this provider.
    fn default_model(&self) -> &'static str;

    /// Generates a plan from a natural language instruction.
    fn generate_plan(&self, instruction: &str) -> Result<Plan, PlannerError>;
}

/// Supported LLM providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    /// Kimi (Moonshot AI)
    Kimi,
    /// OpenAI
    Openai,
    /// Anthropic Claude
    Anthropic,
    /// Local Ollama
    Ollama,
}

impl ProviderType {
    /// Returns all available provider types.
    pub fn all() -> &'static [ProviderType] {
        &[
            ProviderType::Kimi,
            ProviderType::Openai,
            ProviderType::Anthropic,
            ProviderType::Ollama,
        ]
    }

    /// Returns the display name for this provider.
    pub fn display_name(&self) -> &'static str {
        match self {
            ProviderType::Kimi => "Kimi (Moonshot AI)",
            ProviderType::Openai => "OpenAI",
            ProviderType::Anthropic => "Anthropic Claude",
            ProviderType::Ollama => "Ollama (Local)",
        }
    }

    /// Returns the environment variable name for the API key.
    pub fn api_key_env_var(&self) -> &'static str {
        match self {
            ProviderType::Kimi => "KIMI_API_KEY",
            ProviderType::Openai => "OPENAI_API_KEY",
            ProviderType::Anthropic => "ANTHROPIC_API_KEY",
            ProviderType::Ollama => "", // Ollama doesn't need an API key by default
        }
    }

    /// Returns the default base URL for this provider.
    pub fn default_base_url(&self) -> &'static str {
        match self {
            ProviderType::Kimi => "https://api.moonshot.cn/v1",
            ProviderType::Openai => "https://api.openai.com/v1",
            ProviderType::Anthropic => "https://api.anthropic.com/v1",
            ProviderType::Ollama => "http://localhost:11434",
        }
    }

    /// Returns the default model for this provider.
    pub fn default_model(&self) -> &'static str {
        match self {
            ProviderType::Kimi => "moonshot-v1-8k",
            ProviderType::Openai => "gpt-4o-mini",
            ProviderType::Anthropic => "claude-3-haiku-20240307",
            ProviderType::Ollama => "llama3.2",
        }
    }

    /// Returns true if this provider requires an API key.
    pub fn requires_api_key(&self) -> bool {
        !matches!(self, ProviderType::Ollama)
    }
}

impl std::fmt::Display for ProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

impl std::str::FromStr for ProviderType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "kimi" | "moonshot" => Ok(ProviderType::Kimi),
            "openai" => Ok(ProviderType::Openai),
            "anthropic" | "claude" => Ok(ProviderType::Anthropic),
            "ollama" | "local" => Ok(ProviderType::Ollama),
            _ => Err(format!("Unknown provider: {}", s)),
        }
    }
}
