//! Skill registry for discovering and loading skills.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use crate::error::SkillError;
use crate::manifest::SkillManifest;

/// Registry of available skills.
pub struct SkillRegistry {
    /// Loaded skills indexed by name.
    skills: HashMap<String, SkillManifest>,

    /// Directories to search for skills.
    search_paths: Vec<PathBuf>,
}

impl SkillRegistry {
    /// Creates a new skill registry with default search paths.
    pub fn new() -> Self {
        let search_paths = default_skill_paths();
        Self {
            skills: HashMap::new(),
            search_paths,
        }
    }

    /// Creates a registry with custom search paths.
    pub fn with_paths(paths: Vec<PathBuf>) -> Self {
        Self {
            skills: HashMap::new(),
            search_paths: paths,
        }
    }

    /// Discovers and loads all skills from search paths.
    pub fn discover(&mut self) -> Result<(), SkillError> {
        self.skills.clear();

        for path in &self.search_paths {
            if !path.exists() {
                debug!("Skill path does not exist: {}", path.display());
                continue;
            }

            info!("Scanning for skills in: {}", path.display());

            // Look for .yml, .yaml, and .json files
            let entries = std::fs::read_dir(path)?;
            for entry in entries {
                let entry = entry?;
                let path = entry.path();

                if let Some(ext) = path.extension() {
                    let ext = ext.to_string_lossy().to_lowercase();
                    if ext == "yml" || ext == "yaml" || ext == "json" {
                        match self.load_skill_file(&path) {
                            Ok(manifest) => {
                                info!("Loaded skill: {} from {}", manifest.name, path.display());
                                self.skills.insert(manifest.name.clone(), manifest);
                            }
                            Err(e) => {
                                warn!("Failed to load skill from {}: {}", path.display(), e);
                            }
                        }
                    }
                }
            }
        }

        info!("Discovered {} skills", self.skills.len());
        Ok(())
    }

    /// Load a single skill file.
    fn load_skill_file(&self, path: &Path) -> Result<SkillManifest, SkillError> {
        let content = std::fs::read_to_string(path)?;

        let manifest = if let Some(ext) = path.extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            if ext == "json" {
                SkillManifest::from_json(&content)?
            } else {
                SkillManifest::from_yaml(&content)?
            }
        } else {
            // Default to YAML
            SkillManifest::from_yaml(&content)?
        };

        Ok(manifest)
    }

    /// Get a skill by name.
    pub fn get(&self, name: &str) -> Option<&SkillManifest> {
        self.skills.get(name)
    }

    /// List all available skills.
    pub fn list(&self) -> Vec<&SkillManifest> {
        self.skills.values().collect()
    }

    /// Returns true if a skill exists.
    pub fn contains(&self, name: &str) -> bool {
        self.skills.contains_key(name)
    }

    /// Load a skill from a specific file path (for validation).
    pub fn load_from_path(&self, path: &Path) -> Result<SkillManifest, SkillError> {
        self.load_skill_file(path)
    }

    /// Get the search paths.
    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns the default skill search paths.
fn default_skill_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // User skills directory
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".operator-jack").join("skills"));
    }

    // System-wide skills (on macOS)
    #[cfg(target_os = "macos")]
    paths.push(PathBuf::from("/usr/local/share/operator-jack/skills"));

    // Built-in skills in the project
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Development path: next to executable
            paths.push(dir.join("skills"));
            // Also check ../../skills for dev builds
            paths.push(dir.join("../../skills"));
        }
    }

    paths
}

/// Ensure the user skills directory exists.
pub fn ensure_user_skills_dir() -> Option<PathBuf> {
    let path = dirs::home_dir()?.join(".operator-jack").join("skills");
    if !path.exists() {
        let _ = std::fs::create_dir_all(&path);
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_discover_skills() {
        let temp_dir = TempDir::new().unwrap();
        let skill_path = temp_dir.path().join("test-skill.yaml");
        
        let mut file = std::fs::File::create(&skill_path).unwrap();
        file.write_all(
            b"schema_version: 1\nname: test-skill\nsteps:\n  - id: s1\n    type: sys.open_app\n    params:\n      app: Notes"
        ).unwrap();

        let mut registry = SkillRegistry::with_paths(vec![temp_dir.path().to_path_buf()]);
        registry.discover().unwrap();

        assert!(registry.contains("test-skill"));
        assert_eq!(registry.list().len(), 1);
    }

    #[test]
    fn test_get_skill() {
        let temp_dir = TempDir::new().unwrap();
        let skill_path = temp_dir.path().join("test.yaml");
        
        let mut file = std::fs::File::create(&skill_path).unwrap();
        file.write_all(b"schema_version: 1\nname: my-skill\nsteps:\n  - id: s1\n    type: sys.open_app\n    params:\n      app: Notes").unwrap();

        let mut registry = SkillRegistry::with_paths(vec![temp_dir.path().to_path_buf()]);
        registry.discover().unwrap();

        let skill = registry.get("my-skill");
        assert!(skill.is_some());
        assert_eq!(skill.unwrap().name, "my-skill");
    }
}
