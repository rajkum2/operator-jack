use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// A UI element selector per spec v0.2 Section 8.4.
///
/// All fields are optional, but at least one identifying field must be set.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Selector {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subrole: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_contains: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description_contains: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_contains: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<u32>,
}

impl Selector {
    /// Validates the selector, ensuring at least one identifying field is set
    /// and that mutually exclusive field pairs are not both present.
    pub fn validate(&self) -> Result<(), CoreError> {
        // At least one identifying field must be present
        let has_identifier = self.path.is_some()
            || self.identifier.is_some()
            || self.name.is_some()
            || self.name_contains.is_some()
            || self.role.is_some()
            || self.value.is_some()
            || self.value_contains.is_some()
            || self.description.is_some()
            || self.description_contains.is_some();

        if !has_identifier {
            return Err(CoreError::Validation(
                "Selector must have at least one identifying field (path, identifier, name, \
                 name_contains, role, value, value_contains, description, or description_contains)"
                    .to_string(),
            ));
        }

        // Mutually exclusive pairs
        if self.name.is_some() && self.name_contains.is_some() {
            return Err(CoreError::Validation(
                "Selector cannot have both 'name' and 'name_contains'".to_string(),
            ));
        }

        if self.description.is_some() && self.description_contains.is_some() {
            return Err(CoreError::Validation(
                "Selector cannot have both 'description' and 'description_contains'".to_string(),
            ));
        }

        if self.value.is_some() && self.value_contains.is_some() {
            return Err(CoreError::Validation(
                "Selector cannot have both 'value' and 'value_contains'".to_string(),
            ));
        }

        Ok(())
    }

    /// Returns the effective max depth for tree traversal, defaulting to 12.
    pub fn effective_max_depth(&self) -> u32 {
        self.max_depth.unwrap_or(12)
    }
}
