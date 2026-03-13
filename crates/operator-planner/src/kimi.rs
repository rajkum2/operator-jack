//! Kimi (Moonshot AI) provider implementation.

use operator_core::types::Plan;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

use crate::error::PlannerError;
use crate::prompt::{system_prompt, user_prompt};
use crate::provider::{LlmProvider, ProviderConfig};

/// Kimi API provider.
pub struct KimiProvider {
    config: ProviderConfig,
}

impl KimiProvider {
    /// Creates a new Kimi provider with the given configuration.
    pub fn new(config: ProviderConfig) -> Result<Self, PlannerError> {
        if config.api_key.is_none() {
            return Err(PlannerError::ApiKeyNotFound {
                provider: "kimi".to_string(),
                env_var: "KIMI_API_KEY".to_string(),
            });
        }
        Ok(Self { config })
    }

    fn base_url(&self) -> &str {
        self.config
            .base_url
            .as_deref()
            .unwrap_or("https://api.moonshot.cn/v1")
    }

    fn model(&self) -> &str {
        self.config
            .model
            .as_deref()
            .unwrap_or("moonshot-v1-8k")
    }

    fn extract_plan_from_response(&self, text: &str) -> Result<Plan, PlannerError> {
        // Try to extract JSON from markdown code blocks
        let json_str = if let Some(start) = text.find("```json") {
            let after_start = &text[start + 7..];
            if let Some(end) = after_start.find("```") {
                &after_start[..end].trim()
            } else {
                text.trim()
            }
        } else if let Some(start) = text.find("```") {
            let after_start = &text[start + 3..];
            if let Some(end) = after_start.find("```") {
                &after_start[..end].trim()
            } else {
                text.trim()
            }
        } else {
            text.trim()
        };

        let plan: Plan = serde_json::from_str(json_str)
            .map_err(|e| PlannerError::ParseError(format!("Invalid JSON: {}", e)))?;

        crate::prompt::validate_plan_structure(&plan)
            .map_err(PlannerError::ParseError)?;

        Ok(plan)
    }
}

impl LlmProvider for KimiProvider {
    fn name(&self) -> &'static str {
        "kimi"
    }

    fn default_model(&self) -> &'static str {
        "moonshot-v1-8k"
    }

    fn generate_plan(&self, instruction: &str) -> Result<Plan, PlannerError> {
        let api_key = self
            .config
            .api_key
            .as_ref()
            .ok_or_else(|| PlannerError::ApiKeyNotFound {
                provider: "kimi".to_string(),
                env_var: "KIMI_API_KEY".to_string(),
            })?;

        let request = KimiRequest {
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

        trace!("Sending request to Kimi API");
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
                ureq::Error::Transport(e) => {
                    PlannerError::ConnectionError(e.to_string())
                }
            })?;

        let kimi_response: KimiResponse = response
            .into_json()
            .map_err(|e| PlannerError::InvalidResponse(e.to_string()))?;

        if kimi_response.choices.is_empty() {
            return Err(PlannerError::EmptyPlan);
        }

        let content = &kimi_response.choices[0].message.content;
        trace!("Kimi response: {}", content);

        self.extract_plan_from_response(content)
    }
}

#[derive(Debug, Serialize)]
struct KimiRequest {
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
struct KimiResponse {
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
    fn test_extract_plan_from_json_block() {
        let provider = KimiProvider {
            config: ProviderConfig {
                api_key: Some("test".to_string()),
                ..Default::default()
            },
        };

        let text = r#"```json
{
  "schema_version": 1,
  "name": "Test",
  "description": "Test plan",
  "steps": [
    {
      "id": "step1",
      "type": "sys.open_app",
      "params": { "app": "TextEdit" }
    }
  ]
}
```"#;

        let plan = provider.extract_plan_from_response(text).unwrap();
        assert_eq!(plan.name, "Test");
        assert_eq!(plan.steps.len(), 1);
    }

    #[test]
    fn test_extract_plan_raw_json() {
        let provider = KimiProvider {
            config: ProviderConfig {
                api_key: Some("test".to_string()),
                ..Default::default()
            },
        };

        let text = r#"{
  "schema_version": 1,
  "name": "Raw",
  "description": "Raw JSON",
  "steps": [
    {
      "id": "step1",
      "type": "sys.open_app",
      "params": { "app": "Notes" }
    }
  ]
}"#;

        let plan = provider.extract_plan_from_response(text).unwrap();
        assert_eq!(plan.name, "Raw");
    }
}
