//! # Operator Skills
//!
//! Reusable automation macros for Operator Jack.
//!
//! Skills are YAML or JSON manifest files that define parameterized
//! sequences of automation steps. They can be shared and reused
//! across different automation workflows.
//!
//! ## Example Skill Manifest
//!
//! ```yaml
//! schema_version: 1
//! name: open-and-type
//! description: Open an app and type some text
//! parameters:
//!   - name: app
//!     type: string
//!     required: true
//!     description: The application to open
//!   - name: text
//!     type: string
//!     required: true
//!     description: Text to type
//! steps:
//!   - id: open
//!     type: sys.open_app
//!     params:
//!       app: ${app}
//!   - id: type
//!     type: ui.type_text
//!     params:
//!       app: ${app}
//!       selector:
//!         role: AXTextArea
//!       text: ${text}
//! ```

pub mod error;
pub mod manifest;
pub mod registry;

// Re-exports for convenience
pub use error::SkillError;
pub use manifest::{ParameterDef, ParameterType, ResolvedSkill, SkillManifest, SkillStep};
pub use registry::{ensure_user_skills_dir, SkillRegistry};

use std::collections::HashMap;

/// Load and resolve a skill by name.
pub fn load_skill(name: &str) -> Result<SkillManifest, SkillError> {
    let mut registry = SkillRegistry::new();
    registry.discover()?;
    
    registry
        .get(name)
        .cloned()
        .ok_or_else(|| SkillError::NotFound(name.to_string()))
}

/// Run a skill with the given parameters.
pub fn run_skill(
    name: &str,
    params: HashMap<String, String>,
) -> Result<operator_core::types::Plan, SkillError> {
    let manifest = load_skill(name)?;
    let resolved = manifest.resolve(params)?;
    resolved.to_plan()
}

/// List all available skills.
pub fn list_skills() -> Result<Vec<SkillManifest>, SkillError> {
    let mut registry = SkillRegistry::new();
    registry.discover()?;
    Ok(registry.list().into_iter().cloned().collect())
}

/// Validate a skill file at the given path.
pub fn validate_skill_file(path: &std::path::Path) -> Result<(), SkillError> {
    let registry = SkillRegistry::new();
    registry.load_from_path(path)?;
    Ok(())
}
