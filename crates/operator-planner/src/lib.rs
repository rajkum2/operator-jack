//! # Operator Planner
//!
//! Rule-based planner for Operator Jack that converts natural language
//! instructions into structured automation plans using LLM providers.
//!
//! ## Supported Providers
//!
//! - **Kimi (Moonshot AI)** - Requires `KIMI_API_KEY` env var
//! - **OpenAI** - Requires `OPENAI_API_KEY` env var  
//! - **Anthropic Claude** - Requires `ANTHROPIC_API_KEY` env var
//! - **Ollama (Local)** - Requires Ollama running locally
//!
//! ## Example
//!
//! ```no_run
//! use operator_planner::{Planner, PlannerConfig, ProviderType};
//!
//! let planner = Planner::default();
//! let plan = planner.plan("open Notes and type hello").unwrap();
//! ```

pub mod anthropic;
pub mod error;
pub mod kimi;
pub mod ollama;
pub mod openai;
pub mod planner;
pub mod prompt;
pub mod provider;

// Re-exports for convenience
pub use error::PlannerError;
pub use planner::{select_provider_interactive, Planner, PlannerConfig};
pub use provider::{LlmProvider, ProviderConfig, ProviderType};
