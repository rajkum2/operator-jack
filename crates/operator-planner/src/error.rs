//! Error types for the planner module.

use thiserror::Error;

/// Errors that can occur during plan generation.
#[derive(Debug, Error)]
pub enum PlannerError {
    #[error("LLM provider error: {0}")]
    ProviderError(String),

    #[error("API key not found for provider '{provider}'. Set {env_var} environment variable or configure in config file.")]
    ApiKeyNotFound { provider: String, env_var: String },

    #[error("Failed to connect to LLM provider: {0}")]
    ConnectionError(String),

    #[error("Invalid response from LLM provider: {0}")]
    InvalidResponse(String),

    #[error("Failed to parse generated plan: {0}")]
    ParseError(String),

    #[error("LLM returned empty or invalid plan")]
    EmptyPlan,

    #[error("Rate limit exceeded. Please try again later.")]
    RateLimited,

    #[error("Authentication failed. Check your API key.")]
    AuthenticationFailed,

    #[error("HTTP error {status}: {message}")]
    HttpError { status: u16, message: String },

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl PlannerError {
    /// Returns true if the error is retryable.
    pub fn is_retryable(&self) -> bool {
        match self {
            PlannerError::ConnectionError(_) => true,
            PlannerError::RateLimited => true,
            PlannerError::HttpError { status, .. } if *status >= 500 => true,
            _ => false,
        }
    }
}
