//! Anthropic Claude provider implementation.

use operator_core::types::Plan;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

use crate::error::PlannerError;
use crate::prompt::{system_prompt, user_prompt};
use crate::provider::{LlmProvider, ProviderConfig};

/// Anthropic Claude API provider.
pub struct AnthropicProvider {
    config: ProviderConfig,
}

impl AnthropicProvider {
    /// Creates a new Anthropic provider with the given configuration.
    pub fn new(config: ProviderConfig) -> Result<Self, PlannerError> {
        if config.api_key.is_none() {
            return Err(PlannerError::ApiKeyNotFound {
                provider: "anthropic".to_string(),
                env_var: "ANTHROPIC_API_KEY".to_string(),
            });
        }
        Ok(Self { config })
    }

    fn base_url(&self) -> &str {
        self.config
            .base_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com/v1")
    }

    fn model(&self) -> &str {
        self.config
            .model
            .as_deref()
            .unwrap_or("claude-3-haiku-20240307")
    }

    fn extract_plan_from_response(&self, text: &str) -> Result<Plan, PlannerError> {
        // Try to extract JSON from markdown code blocks
        let json_str = if let Some(start) = text.find("```json") {
            let after_start = &text[start + 7..];
            if let Some(end) = after_start.find("```") {
                after_start[..end].trim()
            } else {
                text.trim()
            }
        } else if let Some(start) = text.find("```") {
            let after_start = &text[start + 3..];
            if let Some(end) = after_start.find("```") {
                after_start[..end].trim()
            } else {
                text.trim()
            }
        } else {
            text.trim()
        };

        let plan: Plan = serde_json::from_str(json_str)
            .map_err(|e| PlannerError::ParseError(format!("Invalid JSON: {}", e)))?;

        crate::prompt::validate_plan_structure(&plan).map_err(PlannerError::ParseError)?;

        Ok(plan)
    }
}

impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &'static str {
        "anthropic"
    }

    fn default_model(&self) -> &'static str {
        "claude-3-haiku-20240307"
    }

    fn generate_plan(&self, instruction: &str) -> Result<Plan, PlannerError> {
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or_else(|| PlannerError::ApiKeyNotFound {
                provider: "anthropic".to_string(),
                env_var: "ANTHROPIC_API_KEY".to_string(),
            })?;

        let request = AnthropicRequest {
            model: self.model().to_string(),
            max_tokens: self.config.max_tokens.unwrap_or(4096),
            temperature: self.config.temperature.unwrap_or(0.2),
            system: system_prompt(),
            messages: vec![Message {
                role: "user".to_string(),
                content: user_prompt(instruction),
            }],
        };

        trace!("Sending request to Anthropic API");
        debug!("Request model: {}", request.model);

        let timeout = std::time::Duration::from_secs(self.config.timeout_seconds.unwrap_or(60));

        let response = ureq::post(&format!("{}/messages", self.base_url()))
            .set("x-api-key", api_key)
            .set("anthropic-version", "2023-06-01")
            .set("Content-Type", "application/json")
            .timeout(timeout)
            .send_json(&request)
            .map_err(|e| match e {
                ureq::Error::Status(401, _) => PlannerError::AuthenticationFailed,
                ureq::Error::Status(429, _) => PlannerError::RateLimited,
                ureq::Error::Status(code, response) => {
                    let msg = response.into_string().unwrap_or_default();
                    PlannerError::HttpError {
                        status: code,
                        message: msg,
                    }
                }
                ureq::Error::Transport(e) => PlannerError::ConnectionError(e.to_string()),
            })?;

        let anthropic_response: AnthropicResponse = response
            .into_json()
            .map_err(|e| PlannerError::InvalidResponse(e.to_string()))?;

        if anthropic_response.content.is_empty() {
            return Err(PlannerError::EmptyPlan);
        }

        let content = &anthropic_response.content[0].text;
        trace!("Anthropic response: {}", content);

        self.extract_plan_from_response(content)
    }
}

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    temperature: f32,
    #[serde(skip_serializing_if = "String::is_empty")]
    system: String,
    messages: Vec<Message>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    _type: String,
    text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_model() {
        let provider = AnthropicProvider::new(ProviderConfig {
            api_key: Some("test".to_string()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(provider.default_model(), "claude-3-haiku-20240307");
    }

    #[test]
    fn test_api_key_required() {
        let result = AnthropicProvider::new(ProviderConfig::default());
        assert!(result.is_err());
        match result {
            Err(PlannerError::ApiKeyNotFound { provider, env_var }) => {
                assert_eq!(provider, "anthropic");
                assert_eq!(env_var, "ANTHROPIC_API_KEY");
            }
            _ => panic!("Expected ApiKeyNotFound error"),
        }
    }
}
