//! Skill manifest types and parsing.

use operator_core::types::Plan;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::SkillError;

/// A skill manifest defines a reusable automation macro.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SkillManifest {
    /// Skill schema version.
    pub schema_version: u32,

    /// Skill name (kebab-case, used as identifier).
    pub name: String,

    /// Human-readable description.
    pub description: Option<String>,

    /// Author information.
    pub author: Option<String>,

    /// Version of the skill itself.
    pub version: Option<String>,

    /// Parameter definitions.
    #[serde(default)]
    pub parameters: Vec<ParameterDef>,

    /// The sequence of steps this skill executes.
    pub steps: Vec<SkillStep>,
}

/// Definition of a skill parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterDef {
    /// Parameter name (snake_case).
    pub name: String,

    /// Human-readable description.
    pub description: Option<String>,

    /// Parameter type.
    #[serde(rename = "type")]
    pub param_type: ParameterType,

    /// Whether this parameter is required.
    #[serde(default = "default_true")]
    pub required: bool,

    /// Default value if not provided.
    pub default: Option<serde_yaml::Value>,

    /// Validation regex pattern (for string types).
    pub pattern: Option<String>,

    /// Allowed values (enum constraint).
    pub allowed_values: Option<Vec<serde_yaml::Value>>,
}

/// Parameter data types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterType {
    String,
    Integer,
    Boolean,
    Array,
    Object,
}

/// A step within a skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStep {
    /// Step identifier (unique within the skill).
    pub id: String,

    /// The step type (e.g., "sys.open_app", "ui.click").
    #[serde(rename = "type")]
    pub step_type: String,

    /// Step parameters (can contain variable references like "${param.name}").
    pub params: serde_yaml::Value,

    /// Optional timeout override.
    pub timeout_ms: Option<u64>,

    /// Optional retry count.
    pub retries: Option<u32>,

    /// Optional on_fail behavior.
    pub on_fail: Option<String>,

    /// Human-readable description of what this step does.
    pub description: Option<String>,
}

/// Parsed and validated skill with resolved parameters.
#[derive(Debug, Clone)]
pub struct ResolvedSkill {
    /// The original manifest.
    pub manifest: SkillManifest,

    /// Resolved parameter values.
    pub parameters: HashMap<String, serde_json::Value>,
}

impl SkillManifest {
    /// Parse a skill manifest from YAML string.
    pub fn from_yaml(yaml: &str) -> Result<Self, SkillError> {
        let manifest: Self = serde_yaml::from_str(yaml)
            .map_err(|e| SkillError::ParseError(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Parse a skill manifest from JSON string.
    pub fn from_json(json: &str) -> Result<Self, SkillError> {
        let manifest: Self = serde_json::from_str(json)
            .map_err(|e| SkillError::ParseError(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validate the skill manifest structure.
    pub fn validate(&self) -> Result<(), SkillError> {
        if self.schema_version != 1 {
            return Err(SkillError::ValidationError(format!(
                "Unsupported schema version: {} (expected 1)",
                self.schema_version
            )));
        }

        if self.name.is_empty() {
            return Err(SkillError::ValidationError(
                "Skill name cannot be empty".to_string(),
            ));
        }

        // Validate name is kebab-case
        if !is_kebab_case(&self.name) {
            return Err(SkillError::ValidationError(format!(
                "Skill name '{}' must be kebab-case (lowercase with hyphens)",
                self.name
            )));
        }

        if self.steps.is_empty() {
            return Err(SkillError::ValidationError(
                "Skill must have at least one step".to_string(),
            ));
        }

        // Check for duplicate step IDs
        let mut ids = std::collections::HashSet::new();
        for step in &self.steps {
            if !ids.insert(&step.id) {
                return Err(SkillError::ValidationError(format!(
                    "Duplicate step ID: {}",
                    step.id
                )));
            }
        }

        // Check for duplicate parameter names
        let mut param_names = std::collections::HashSet::new();
        for param in &self.parameters {
            if !param_names.insert(&param.name) {
                return Err(SkillError::ValidationError(format!(
                    "Duplicate parameter name: {}",
                    param.name
                )));
            }

            // Validate parameter name is snake_case
            if !is_snake_case(&param.name) {
                return Err(SkillError::ValidationError(format!(
                    "Parameter name '{}' must be snake_case",
                    param.name
                )));
            }

            // Validate pattern regex if provided
            if let Some(ref pattern) = param.pattern {
                regex::Regex::new(pattern).map_err(|e| {
                    SkillError::ValidationError(format!(
                        "Invalid regex pattern for parameter '{}': {}",
                        param.name, e
                    ))
                })?;
            }
        }

        Ok(())
    }

    /// Resolve parameters with provided values.
    pub fn resolve(
        &self,
        provided: HashMap<String, String>,
    ) -> Result<ResolvedSkill, SkillError> {
        let mut resolved = HashMap::new();

        for param_def in &self.parameters {
            let value = if let Some(provided_val) = provided.get(&param_def.name) {
                // Validate and convert the provided value
                self.validate_and_convert(param_def, provided_val)?
            } else if let Some(ref default) = param_def.default {
                // Convert default value to JSON
                serde_json::to_value(default).map_err(|e| {
                    SkillError::ValidationError(format!(
                        "Failed to convert default value for '{}': {}",
                        param_def.name, e
                    ))
                })?
            } else if param_def.required {
                return Err(SkillError::MissingParameter(param_def.name.clone()));
            } else {
                // Optional parameter with no default - use null
                serde_json::Value::Null
            };

            resolved.insert(param_def.name.clone(), value);
        }

        // Check for unknown parameters
        for key in provided.keys() {
            if !self.parameters.iter().any(|p| &p.name == key) {
                return Err(SkillError::ValidationError(format!(
                    "Unknown parameter: '{}'",
                    key
                )));
            }
        }

        Ok(ResolvedSkill {
            manifest: self.clone(),
            parameters: resolved,
        })
    }

    /// Validate and convert a string value to the appropriate type.
    fn validate_and_convert(
        &self,
        param_def: &ParameterDef,
        value: &str,
    ) -> Result<serde_json::Value, SkillError> {
        // Check allowed values first
        if let Some(ref allowed) = param_def.allowed_values {
            let allowed_strings: Vec<String> = allowed
                .iter()
                .map(|v| v.as_str().unwrap_or_default().to_string())
                .collect();
            if !allowed_strings.contains(&value.to_string()) {
                return Err(SkillError::InvalidParameterValue {
                    name: param_def.name.clone(),
                    reason: format!(
                        "Value must be one of: {:?}",
                        allowed_strings
                    ),
                });
            }
        }

        // Check pattern
        if let Some(ref pattern) = param_def.pattern {
            let regex = regex::Regex::new(pattern).map_err(|_| {
                SkillError::ValidationError(format!(
                    "Invalid pattern for parameter '{}'",
                    param_def.name
                ))
            })?;
            if !regex.is_match(value) {
                return Err(SkillError::InvalidParameterValue {
                    name: param_def.name.clone(),
                    reason: format!("Value does not match pattern: {}", pattern),
                });
            }
        }

        // Convert based on type
        match param_def.param_type {
            ParameterType::String => Ok(serde_json::Value::String(value.to_string())),
            ParameterType::Integer => {
                let int_val: i64 = value.parse().map_err(|_| {
                    SkillError::InvalidParameterValue {
                        name: param_def.name.clone(),
                        reason: "Value must be an integer".to_string(),
                    }
                })?;
                Ok(serde_json::Value::Number(int_val.into()))
            }
            ParameterType::Boolean => {
                let bool_val = match value.to_lowercase().as_str() {
                    "true" | "yes" | "1" => true,
                    "false" | "no" | "0" => false,
                    _ => {
                        return Err(SkillError::InvalidParameterValue {
                            name: param_def.name.clone(),
                            reason: "Value must be a boolean (true/false)".to_string(),
                        })
                    }
                };
                Ok(serde_json::Value::Bool(bool_val))
            }
            ParameterType::Array | ParameterType::Object => {
                // Try to parse as JSON
                serde_json::from_str(value).map_err(|_| {
                    SkillError::InvalidParameterValue {
                        name: param_def.name.clone(),
                        reason: "Value must be valid JSON".to_string(),
                    }
                })
            }
        }
    }
}

impl ResolvedSkill {
    /// Expand this resolved skill into a Plan.
    pub fn to_plan(&self) -> Result<operator_core::types::Plan, SkillError> {
        use operator_core::types::{Mode, OnFail, Step, StepType};
        use std::str::FromStr;

        let steps: Result<Vec<Step>, SkillError> = self
            .manifest
            .steps
            .iter()
            .map(|skill_step| {
                // Parse step type
                let step_type = StepType::from_str(&skill_step.step_type)
                    .map_err(|e| SkillError::ExpansionError(format!(
                        "Invalid step type '{}': {}",
                        skill_step.step_type, e
                    )))?;

                // Interpolate parameters in params
                let params_json = interpolate_params(
                    &skill_step.params,
                    &self.parameters,
                )?;

                // Parse on_fail
                let on_fail = skill_step
                    .on_fail
                    .as_ref()
                    .map(|s| match s.as_str() {
                        "abort" => Ok(OnFail::Abort),
                        "continue" => Ok(OnFail::Continue),
                        "ask" => Ok(OnFail::Ask),
                        other => Err(SkillError::ExpansionError(format!(
                            "Invalid on_fail value: {}",
                            other
                        ))),
                    })
                    .transpose()?;

                Ok::<Step, SkillError>(Step {
                    id: skill_step.id.clone(),
                    step_type,
                    params: params_json,
                    timeout_ms: skill_step.timeout_ms,
                    retries: skill_step.retries,
                    retry_backoff_ms: None,
                    on_fail,
                })
            })
            .collect();

        Ok(Plan {
            schema_version: 1,
            name: self.manifest.name.clone(),
            description: self.manifest.description.clone(),
            mode: Some(Mode::Safe),
            allow_apps: None,
            allow_domains: None,
            variables: None,
            steps: steps?,
        })
    }
}

/// Interpolate parameters in a YAML value.
fn interpolate_params(
    value: &serde_yaml::Value,
    params: &HashMap<String, serde_json::Value>,
) -> Result<serde_json::Value, SkillError> {
    match value {
        serde_yaml::Value::String(s) => {
            interpolate_string(s, params)
        }
        serde_yaml::Value::Number(n) => {
            Ok(serde_json::Value::Number(
                serde_json::Number::from_f64(n.as_f64().unwrap_or(0.0))
                    .unwrap_or_else(|| serde_json::Number::from(0)),
            ))
        }
        serde_yaml::Value::Bool(b) => Ok(serde_json::Value::Bool(*b)),
        serde_yaml::Value::Null => Ok(serde_json::Value::Null),
        serde_yaml::Value::Sequence(seq) => {
            let arr: Result<Vec<_>, _> = seq
                .iter()
                .map(|v| interpolate_params(v, params))
                .collect();
            Ok(serde_json::Value::Array(arr?))
        }
        serde_yaml::Value::Mapping(map) => {
            let obj: Result<serde_json::Map<String, serde_json::Value>, SkillError> = map
                .iter()
                .map(|(k, v)| {
                    let key = k.as_str().unwrap_or_default().to_string();
                    let val = interpolate_params(v, params)?;
                    Ok::<(String, serde_json::Value), SkillError>((key, val))
                })
                .collect();
            Ok(serde_json::Value::Object(obj?))
        }
        _ => Ok(serde_json::Value::Null),
    }
}

/// Interpolate parameters in a string.
fn interpolate_string(
    s: &str,
    params: &HashMap<String, serde_json::Value>,
) -> Result<serde_json::Value, SkillError> {
    // Simple interpolation: replace ${param.name} with value
    let mut result = s.to_string();
    
    for (name, value) in params {
        let placeholder = format!("${{{}}}", name);
        let replacement = match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        result = result.replace(&placeholder, &replacement);
    }

    // Check for unresolved placeholders
    if result.contains("${") {
        return Err(SkillError::ExpansionError(format!(
            "Unresolved parameter placeholder in: {}",
            result
        )));
    }

    Ok(serde_json::Value::String(result))
}

fn is_kebab_case(s: &str) -> bool {
    s.chars().all(|c| c.is_lowercase() || c == '-' || c.is_numeric())
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
}

fn is_snake_case(s: &str) -> bool {
    s.chars().all(|c| c.is_lowercase() || c == '_' || c.is_numeric())
        && !s.starts_with('_')
        && !s.ends_with('_')
        && !s.contains("__")
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_manifest_yaml() {
        let yaml = r#"
schema_version: 1
name: open-app
description: Open an application
parameters:
  - name: app_name
    type: string
    required: true
    description: The app to open
steps:
  - id: open
    type: sys.open_app
    params:
      app: ${app_name}
"#;
        let manifest = SkillManifest::from_yaml(yaml).unwrap();
        assert_eq!(manifest.name, "open-app");
        assert_eq!(manifest.steps.len(), 1);
    }

    #[test]
    fn test_validate_kebab_case_name() {
        let yaml = r#"
schema_version: 1
name: open_app
steps: []
"#;
        let result = SkillManifest::from_yaml(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_parameters() {
        let yaml = r#"
schema_version: 1
name: test-skill
parameters:
  - name: message
    type: string
    required: true
steps:
  - id: step1
    type: sys.clipboard_set
    params:
      content: ${message}
"#;
        let manifest = SkillManifest::from_yaml(yaml).unwrap();
        let mut params = HashMap::new();
        params.insert("message".to_string(), "hello".to_string());
        
        let resolved = manifest.resolve(params).unwrap();
        assert_eq!(
            resolved.parameters.get("message").unwrap(),
            &serde_json::Value::String("hello".to_string())
        );
    }
}
