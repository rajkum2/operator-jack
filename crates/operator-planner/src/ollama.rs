//! Ollama local LLM provider implementation.

use operator_core::types::Plan;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

use crate::error::PlannerError;
use crate::prompt::{system_prompt, user_prompt};
use crate::provider::{LlmProvider, ProviderConfig};

/// Ollama local provider.
pub struct OllamaProvider {
    config: ProviderConfig,
}

impl OllamaProvider {
    /// Creates a new Ollama provider with the given configuration.
    pub fn new(config: ProviderConfig) -> Self {
        Self { config }
    }

    fn base_url(&self) -> &str {
        self.config
            .base_url
            .as_deref()
            .unwrap_or("http://localhost:11434")
    }

    fn model(&self) -> &str {
        self.config.model.as_deref().unwrap_or("llama3.2")
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

    /// Checks if Ollama is available at the configured URL.
    pub fn is_available(&self) -> bool {
        ureq::get(&format!("{}/api/tags", self.base_url()))
            .timeout(std::time::Duration::from_secs(5))
            .call()
            .is_ok()
    }

    /// Lists available models from the Ollama server.
    pub fn list_models(&self) -> Result<Vec<String>, PlannerError> {
        let response = ureq::get(&format!("{}/api/tags", self.base_url()))
            .timeout(std::time::Duration::from_secs(10))
            .call()
            .map_err(|e| PlannerError::ConnectionError(format!("Failed to list models: {}", e)))?;

        let tags: TagsResponse = response
            .into_json()
            .map_err(|e| PlannerError::InvalidResponse(e.to_string()))?;

        Ok(tags.models.into_iter().map(|m| m.name).collect())
    }
}

impl LlmProvider for OllamaProvider {
    fn name(&self) -> &'static str {
        "ollama"
    }

    fn default_model(&self) -> &'static str {
        "llama3.2"
    }

    fn generate_plan(&self, instruction: &str) -> Result<Plan, PlannerError> {
        let request = OllamaRequest {
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
            stream: false,
            options: Options {
                temperature: self.config.temperature.unwrap_or(0.2),
                num_predict: self.config.max_tokens.unwrap_or(4096),
            },
        };

        trace!("Sending request to Ollama API");
        debug!("Request model: {}", request.model);
        debug!("Base URL: {}", self.base_url());

        let timeout = std::time::Duration::from_secs(self.config.timeout_seconds.unwrap_or(120));

        let response = ureq::post(&format!("{}/api/chat", self.base_url()))
            .set("Content-Type", "application/json")
            .timeout(timeout)
            .send_json(&request)
            .map_err(|e| match e {
                ureq::Error::Status(code, response) => {
                    let msg = response.into_string().unwrap_or_default();
                    if code == 404 {
                        PlannerError::ProviderError(format!(
                            "Model '{}' not found. Run: ollama pull {}",
                            self.model(),
                            self.model()
                        ))
                    } else {
                        PlannerError::HttpError {
                            status: code,
                            message: msg,
                        }
                    }
                }
                ureq::Error::Transport(e) => PlannerError::ConnectionError(format!(
                    "Cannot connect to Ollama at {}. Is it running? Error: {}",
                    self.base_url(),
                    e
                )),
            })?;

        let ollama_response: OllamaResponse = response
            .into_json()
            .map_err(|e| PlannerError::InvalidResponse(e.to_string()))?;

        let content = &ollama_response.message.content;
        trace!("Ollama response: {}", content);

        self.extract_plan_from_response(content)
    }
}

#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<Message>,
    stream: bool,
    options: Options,
}

#[derive(Debug, Serialize)]
struct Options {
    temperature: f32,
    num_predict: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    message: Message,
}

#[derive(Debug, Deserialize)]
struct TagsResponse {
    models: Vec<ModelInfo>,
}

#[derive(Debug, Deserialize)]
struct ModelInfo {
    name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_model() {
        let provider = OllamaProvider::new(ProviderConfig::default());
        assert_eq!(provider.default_model(), "llama3.2");
    }

    #[test]
    fn test_custom_model() {
        let provider = OllamaProvider::new(ProviderConfig {
            model: Some("mistral".to_string()),
            ..Default::default()
        });
        assert_eq!(provider.model(), "mistral");
    }
}
