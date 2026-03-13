//! # Operator Exec Browser
//!
//! Browser automation executor for Operator Jack using Chrome DevTools Protocol.
//!
//! ## Example
//!
//! ```no_run
//! use operator_exec_browser::BrowserExecutor;
//!
//! #[tokio::main]
//! async fn main() {
//!     let mut executor = BrowserExecutor::new(vec![])
//!         .with_port(9222);
//!     
//!     executor.connect().await.unwrap();
//!     
//!     // Use executor...
//!     
//!     executor.close().await;
//! }
//! ```

pub mod cdp;
pub mod client;
pub mod error;
pub mod executor;

// Re-exports
pub use client::{discover_chrome_ws_url, launch_chrome, BrowserClient};
pub use error::BrowserError;
pub use executor::BrowserExecutor;

use operator_core::types::Plan;
use serde_json::Value;

/// Execute a browser plan asynchronously.
pub async fn execute_browser_plan(
    plan: &Plan,
    allow_domains: Vec<String>,
) -> Result<Vec<(String, Value)>, BrowserError> {
    let mut executor = BrowserExecutor::new(allow_domains);
    executor.connect().await?;

    let mut results = Vec::new();

    for step in &plan.steps {
        if matches!(step.step_type.lane(), "browser") {
            match executor.execute_step(step).await {
                Ok(result) => {
                    results.push((step.id.clone(), result));
                }
                Err(e) => {
                    executor.close().await;
                    return Err(e);
                }
            }
        }
    }

    executor.close().await;
    Ok(results)
}

/// Check if Chrome is available on the system.
pub fn is_chrome_available() -> bool {
    let chrome_paths = vec![
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
    ];

    chrome_paths
        .into_iter()
        .any(|p| std::path::Path::new(p).exists())
}
