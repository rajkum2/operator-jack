//! OpenAI provider implementation.

use operator_core::types::Plan;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

use crate::error::PlannerError;
use crate::prompt::{system_prompt, user_prompt};
use crate::provider::{LlmProvider, ProviderConfig};

/// OpenAI API provider.
pub struct OpenAiProvider {
    config: ProviderConfig,
}

impl OpenAiProvider {
    /// Creates a new OpenAI provider with the given configuration.
    pub fn new(config: ProviderConfig) -> Result<Self, PlannerError> {
        if config.api_key.is_none() {
            return Err(PlannerError::ApiKeyNotFound {
                provider: "openai".to_string(),
                env_var: "OPENAI_API_KEY".to_string(),
            });
        }
        Ok(Self { config })
    }

    fn base_url(&self) -> &str {
        self.config
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com/v1")
    }

    fn model(&self) -> &str {
        self.config.model.as_deref().unwrap_or("gpt-4o-mini")
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

impl LlmProvider for OpenAiProvider {
    fn name(&self) -> &'static str {
        "openai"
    }

    fn default_model(&self) -> &'static str {
        "gpt-4o-mini"
    }

    fn generate_plan(&self, instruction: &str) -> Result<Plan, PlannerError> {
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or_else(|| PlannerError::ApiKeyNotFound {
                provider: "openai".to_string(),
                env_var: "OPENAI_API_KEY".to_string(),
            })?;

        let request = OpenAiRequest {
            model: self.model().to_string(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    content: system_prompt(),
                },
                Message {
                    role: "user".to_string(),
                    content: user_prompt(instruction),
                },
            ],
            temperature: self.config.temperature.unwrap_or(0.2),
            max_tokens: self.config.max_tokens.unwrap_or(4096),
        };

        trace!("Sending request to OpenAI API");
        debug!("Request model: {}", request.model);

        let timeout = std::time::Duration::from_secs(self.config.timeout_seconds.unwrap_or(60));

        let response = ureq::post(&format!("{}/chat/completions", self.base_url()))
            .set("Authorization", &format!("Bearer {}", api_key))
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

        let openai_response: OpenAiResponse = response
            .into_json()
            .map_err(|e| PlannerError::InvalidResponse(e.to_string()))?;

        if openai_response.choices.is_empty() {
            return Err(PlannerError::EmptyPlan);
        }

        let content = &openai_response.choices[0].message.content;
        trace!("OpenAI response: {}", content);

        self.extract_plan_from_response(content)
    }
}

#[derive(Debug, Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f32,
    max_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_model() {
        let provider = OpenAiProvider::new(ProviderConfig {
            api_key: Some("test".to_string()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(provider.default_model(), "gpt-4o-mini");
    }

    #[test]
    fn test_api_key_required() {
        let result = OpenAiProvider::new(ProviderConfig::default());
        assert!(result.is_err());
        match result {
            Err(PlannerError::ApiKeyNotFound { provider, env_var }) => {
                assert_eq!(provider, "openai");
                assert_eq!(env_var, "OPENAI_API_KEY");
            }
            _ => panic!("Expected ApiKeyNotFound error"),
        }
    }
}
