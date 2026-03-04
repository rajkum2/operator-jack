use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// Window scoping for selectors — restricts element search to a specific window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowScope {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_contains: Option<String>,
}

/// A UI element selector per spec v0.3 Section 8.4.
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowScope>,
    /// Fallback locator stack: try each selector in order, first returning
    /// exactly 1 match wins. If present, other direct selector fields should
    /// be empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub any_of: Option<Vec<Selector>>,
}

impl Selector {
    /// Validates the selector, ensuring at least one identifying field is set
    /// and that mutually exclusive field pairs are not both present.
    pub fn validate(&self) -> Result<(), CoreError> {
        // anyOf validation: if present, direct selector fields should be empty
        if let Some(ref alternatives) = self.any_of {
            let has_direct = self.path.is_some()
                || self.identifier.is_some()
                || self.name.is_some()
                || self.name_contains.is_some()
                || self.role.is_some()
                || self.value.is_some()
                || self.value_contains.is_some()
                || self.description.is_some()
                || self.description_contains.is_some()
                || self.subrole.is_some();

            if has_direct {
                return Err(CoreError::Validation(
                    "Selector with 'any_of' must not have other identifying fields".to_string(),
                ));
            }

            if alternatives.is_empty() {
                return Err(CoreError::Validation(
                    "Selector 'any_of' must contain at least one alternative".to_string(),
                ));
            }

            // Validate each alternative
            for (i, alt) in alternatives.iter().enumerate() {
                if let Err(e) = alt.validate() {
                    return Err(CoreError::Validation(format!(
                        "any_of[{}]: {}",
                        i, e
                    )));
                }
            }

            return Ok(());
        }

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

        // Window scope validation: if present, exactly one of index/title_contains
        if let Some(ref w) = self.window {
            match (&w.index, &w.title_contains) {
                (Some(_), Some(_)) => {
                    return Err(CoreError::Validation(
                        "Window scope cannot have both 'index' and 'title_contains'".to_string(),
                    ));
                }
                (None, None) => {
                    return Err(CoreError::Validation(
                        "Window scope must have either 'index' or 'title_contains'".to_string(),
                    ));
                }
                _ => {} // exactly one is set — valid
            }
        }

        Ok(())
    }

    /// Returns the effective max depth for tree traversal, defaulting to 12.
    pub fn effective_max_depth(&self) -> u32 {
        self.max_depth.unwrap_or(12)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_scope_index_only_is_valid() {
        let sel = Selector {
            role: Some("AXButton".into()),
            window: Some(WindowScope {
                index: Some(0),
                title_contains: None,
            }),
            ..Default::default()
        };
        assert!(sel.validate().is_ok());
    }

    #[test]
    fn test_window_scope_title_contains_only_is_valid() {
        let sel = Selector {
            role: Some("AXButton".into()),
            window: Some(WindowScope {
                index: None,
                title_contains: Some("Untitled".into()),
            }),
            ..Default::default()
        };
        assert!(sel.validate().is_ok());
    }

    #[test]
    fn test_window_scope_both_set_is_invalid() {
        let sel = Selector {
            role: Some("AXButton".into()),
            window: Some(WindowScope {
                index: Some(0),
                title_contains: Some("Untitled".into()),
            }),
            ..Default::default()
        };
        let err = sel.validate().unwrap_err();
        assert!(err.to_string().contains("both"));
    }

    #[test]
    fn test_window_scope_neither_set_is_invalid() {
        let sel = Selector {
            role: Some("AXButton".into()),
            window: Some(WindowScope {
                index: None,
                title_contains: None,
            }),
            ..Default::default()
        };
        let err = sel.validate().unwrap_err();
        assert!(err.to_string().contains("either"));
    }

    #[test]
    fn test_selector_no_window_is_valid() {
        let sel = Selector {
            role: Some("AXButton".into()),
            name: Some("OK".into()),
            window: None,
            ..Default::default()
        };
        assert!(sel.validate().is_ok());
    }

    #[test]
    fn test_selector_window_scope_roundtrip() {
        let sel = Selector {
            role: Some("AXTextField".into()),
            window: Some(WindowScope {
                index: Some(1),
                title_contains: None,
            }),
            ..Default::default()
        };
        let json = serde_json::to_value(&sel).unwrap();
        assert_eq!(json["window"]["index"], 1);
        assert!(json["window"].get("title_contains").is_none());

        let deserialized: Selector = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.window.as_ref().unwrap().index, Some(1));
    }

    #[test]
    fn test_any_of_valid() {
        let sel = Selector {
            any_of: Some(vec![
                Selector {
                    role: Some("AXButton".into()),
                    name: Some("Save".into()),
                    ..Default::default()
                },
                Selector {
                    role: Some("AXButton".into()),
                    name: Some("OK".into()),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        };
        assert!(sel.validate().is_ok());
    }

    #[test]
    fn test_any_of_with_direct_fields_is_invalid() {
        let sel = Selector {
            role: Some("AXButton".into()),
            any_of: Some(vec![Selector {
                name: Some("Save".into()),
                ..Default::default()
            }]),
            ..Default::default()
        };
        let err = sel.validate().unwrap_err();
        assert!(err.to_string().contains("any_of"));
    }

    #[test]
    fn test_any_of_empty_is_invalid() {
        let sel = Selector {
            any_of: Some(vec![]),
            ..Default::default()
        };
        let err = sel.validate().unwrap_err();
        assert!(err.to_string().contains("at least one"));
    }

    #[test]
    fn test_any_of_invalid_alternative_propagates() {
        let sel = Selector {
            any_of: Some(vec![Selector {
                // No identifying fields — should fail validation
                ..Default::default()
            }]),
            ..Default::default()
        };
        let err = sel.validate().unwrap_err();
        assert!(err.to_string().contains("any_of[0]"));
    }

    #[test]
    fn test_any_of_roundtrip() {
        let sel = Selector {
            any_of: Some(vec![
                Selector {
                    role: Some("AXButton".into()),
                    name: Some("Save".into()),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        };
        let json = serde_json::to_value(&sel).unwrap();
        assert!(json["any_of"].is_array());
        assert_eq!(json["any_of"][0]["role"], "AXButton");

        let deserialized: Selector = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.any_of.as_ref().unwrap().len(), 1);
    }
}
