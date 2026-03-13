//! High-level planner interface for generating plans from natural language.

use operator_core::types::Plan;
use tracing::{info, warn};

use crate::error::PlannerError;
use crate::provider::{LlmProvider, ProviderConfig, ProviderType};

/// Configuration for the planner.
#[derive(Debug, Clone)]
pub struct PlannerConfig {
    /// The default provider to use.
    pub provider: ProviderType,
    /// Configuration for Kimi provider.
    pub kimi: ProviderConfig,
    /// Configuration for OpenAI provider.
    pub openai: ProviderConfig,
    /// Configuration for Anthropic provider.
    pub anthropic: ProviderConfig,
    /// Configuration for Ollama provider.
    pub ollama: ProviderConfig,
}

impl Default for PlannerConfig {
    fn default() -> Self {
        Self {
            provider: ProviderType::Ollama, // Default to local first
            kimi: ProviderConfig {
                api_key: std::env::var("KIMI_API_KEY").ok(),
                base_url: None,
                model: None,
                max_tokens: Some(4096),
                temperature: Some(0.2),
                timeout_seconds: Some(60),
            },
            openai: ProviderConfig {
                api_key: std::env::var("OPENAI_API_KEY").ok(),
                base_url: None,
                model: None,
                max_tokens: Some(4096),
                temperature: Some(0.2),
                timeout_seconds: Some(60),
            },
            anthropic: ProviderConfig {
                api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
                base_url: None,
                model: None,
                max_tokens: Some(4096),
                temperature: Some(0.2),
                timeout_seconds: Some(60),
            },
            ollama: ProviderConfig {
                api_key: None,
                base_url: None,
                model: Some("llama3.2".to_string()),
                max_tokens: Some(4096),
                temperature: Some(0.2),
                timeout_seconds: Some(120),
            },
        }
    }
}

/// The Planner generates automation plans from natural language instructions.
pub struct Planner {
    config: PlannerConfig,
}

impl Planner {
    /// Creates a new planner with the given configuration.
    pub fn new(config: PlannerConfig) -> Self {
        Self { config }
    }

    /// Creates a planner with default configuration.
    pub fn default() -> Self {
        Self::new(PlannerConfig::default())
    }

    /// Generates a plan from a natural language instruction using the default provider.
    pub fn plan(&self, instruction: &str) -> Result<Plan, PlannerError> {
        self.plan_with_provider(instruction, self.config.provider)
    }

    /// Generates a plan using a specific provider.
    pub fn plan_with_provider(
        &self,
        instruction: &str,
        provider_type: ProviderType,
    ) -> Result<Plan, PlannerError> {
        info!("Generating plan using {} provider", provider_type.display_name());

        let provider = self.create_provider(provider_type)?;
        provider.generate_plan(instruction)
    }

    /// Lists available providers based on API key configuration.
    pub fn available_providers(&self) -> Vec<(ProviderType, bool)> {
        ProviderType::all()
            .iter()
            .map(|&pt| {
                let available = match pt {
                    ProviderType::Kimi => self.config.kimi.api_key.is_some(),
                    ProviderType::Openai => self.config.openai.api_key.is_some(),
                    ProviderType::Anthropic => self.config.anthropic.api_key.is_some(),
                    ProviderType::Ollama => {
                        // Check if Ollama is running
                        crate::ollama::OllamaProvider::new(self.config.ollama.clone())
                            .is_available()
                    }
                };
                (pt, available)
            })
            .collect()
    }

    /// Returns the first available provider, preferring local Ollama.
    pub fn first_available_provider(&self) -> Option<ProviderType> {
        // Prefer Ollama if available (local, no API key needed)
        let ollama = crate::ollama::OllamaProvider::new(self.config.ollama.clone());
        if ollama.is_available() {
            return Some(ProviderType::Ollama);
        }

        // Otherwise check for configured API keys
        if self.config.kimi.api_key.is_some() {
            return Some(ProviderType::Kimi);
        }
        if self.config.openai.api_key.is_some() {
            return Some(ProviderType::Openai);
        }
        if self.config.anthropic.api_key.is_some() {
            return Some(ProviderType::Anthropic);
        }

        None
    }

    /// Creates a provider instance from the configuration.
    fn create_provider(&self, provider_type: ProviderType) -> Result<Box<dyn LlmProvider>, PlannerError> {
        match provider_type {
            ProviderType::Kimi => {
                use crate::kimi::KimiProvider;
                Ok(Box::new(KimiProvider::new(self.config.kimi.clone())?))
            }
            ProviderType::Openai => {
                use crate::openai::OpenAiProvider;
                Ok(Box::new(OpenAiProvider::new(self.config.openai.clone())?))
            }
            ProviderType::Anthropic => {
                use crate::anthropic::AnthropicProvider;
                Ok(Box::new(AnthropicProvider::new(self.config.anthropic.clone())?))
            }
            ProviderType::Ollama => {
                use crate::ollama::OllamaProvider;
                Ok(Box::new(OllamaProvider::new(self.config.ollama.clone())))
            }
        }
    }
}

/// Interactive provider selection.
pub fn select_provider_interactive(planner: &Planner) -> Option<ProviderType> {
    let available = planner.available_providers();

    println!("\nAvailable LLM Providers:");
    println!("------------------------");

    let mut options = Vec::new();
    for (i, (provider, is_available)) in available.iter().enumerate() {
        let status = if *is_available {
            "✓ available"
        } else {
            match provider {
                ProviderType::Ollama => "✗ not running (start with: ollama serve)",
                _ => &format!("✗ set {} env var", provider.api_key_env_var()),
            }
        };
        println!("  {}. {} - {}", i + 1, provider.display_name(), status);
        options.push(*provider);
    }

    println!("\nSelect provider (1-{}), or 'q' to quit:", options.len());

    use std::io::{self, Write};
    let mut input = String::new();
    print!("> ");
    io::stdout().flush().ok()?;
    io::stdin().read_line(&mut input).ok()?;

    let input = input.trim();
    if input == "q" {
        return None;
    }

    if let Ok(choice) = input.parse::<usize>() {
        if choice >= 1 && choice <= options.len() {
            let selected = options[choice - 1];
            let (_, is_available) = available[choice - 1];
            if !is_available {
                warn!("Provider {} is not available", selected.display_name());
            }
            return Some(selected);
        }
    }

    println!("Invalid selection");
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_planner_config() {
        let config = PlannerConfig::default();
        assert!(matches!(config.provider, ProviderType::Ollama));
    }

    #[test]
    fn test_available_providers_structure() {
        let planner = Planner::default();
        let available = planner.available_providers();
        assert_eq!(available.len(), 4);
    }
}
