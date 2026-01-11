use crate::config::Config;
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub allowed_tools: HashSet<String>,
    #[serde(default)]
    pub denied_tools: HashSet<String>,
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct SkillReference {
    pub path: String,
    pub description: Option<String>,
    pub loaded: bool,
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub content: String,
    pub allowed_tools: HashSet<String>,
    pub denied_tools: HashSet<String>,
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub tags: Vec<String>,
    pub references: Vec<SkillReference>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Skill {
    /// Parse a SKILL.md file into a Skill struct
    pub fn from_markdown(content: &str) -> Result<Self> {
        // Split on --- markers to extract YAML frontmatter
        let parts: Vec<&str> = content.split("---").collect();

        if parts.len() < 3 {
            return Err(anyhow!(
                "Invalid SKILL.md format: missing frontmatter delimiters"
            ));
        }

        // Parse YAML frontmatter (second part, index 1)
        let frontmatter_str = parts[1].trim();
        let frontmatter: SkillFrontmatter =
            serde_yaml::from_str(frontmatter_str).context("Failed to parse YAML frontmatter")?;

        // Extract markdown content (everything after second ---)
        let markdown_content = parts[2..].join("---").trim().to_string();

        // Parse references from markdown content
        let references = Self::parse_references(&markdown_content);

        let now = Utc::now();
        Ok(Skill {
            name: frontmatter.name,
            description: frontmatter.description,
            content: markdown_content,
            allowed_tools: frontmatter.allowed_tools,
            denied_tools: frontmatter.denied_tools,
            model: frontmatter.model,
            temperature: frontmatter.temperature,
            max_tokens: frontmatter.max_tokens,
            tags: frontmatter.tags,
            references,
            created_at: frontmatter.created_at.unwrap_or(now),
            updated_at: frontmatter.updated_at.unwrap_or(now),
        })
    }

    /// Convert Skill struct to SKILL.md format
    pub fn to_markdown(&self) -> String {
        let frontmatter = SkillFrontmatter {
            name: self.name.clone(),
            description: self.description.clone(),
            allowed_tools: self.allowed_tools.clone(),
            denied_tools: self.denied_tools.clone(),
            model: self.model.clone(),
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            tags: self.tags.clone(),
            created_at: Some(self.created_at),
            updated_at: Some(self.updated_at),
        };

        let yaml_str = serde_yaml::to_string(&frontmatter).unwrap_or_else(|_| {
            String::from("name: error\ndescription: Failed to serialize frontmatter")
        });

        format!("---\n{}\n---\n\n{}", yaml_str.trim(), self.content)
    }

    /// Parse @references/path/to/file.md syntax from markdown content
    fn parse_references(content: &str) -> Vec<SkillReference> {
        let mut references = Vec::new();
        let re = regex::Regex::new(r"@references/([^\s\)]+)").unwrap();

        for cap in re.captures_iter(content) {
            let path = cap[1].to_string();
            references.push(SkillReference {
                path,
                description: None,
                loaded: false,
            });
        }

        references
    }
}

pub struct SkillManager {
    skills_dir: PathBuf,
    skills: HashMap<String, Skill>,
    active_skills: HashSet<String>,
    config: Arc<RwLock<Config>>,
    config_path: PathBuf,
}

impl SkillManager {
    /// Initialize SkillManager from ~/.flexorama/skills/ directory
    pub fn new(config: Arc<RwLock<Config>>) -> Result<Self> {
        // Determine skills directory
        let skills_dir = Self::get_skills_dir()?;
        let config_path = Config::default_config_path();

        Ok(SkillManager {
            skills_dir,
            skills: HashMap::new(),
            active_skills: HashSet::new(),
            config,
            config_path,
        })
    }

    /// Get the ~/.flexorama/skills/ directory path
    fn get_skills_dir() -> Result<PathBuf> {
        let skills_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".flexorama")
            .join("skills");
        Ok(skills_dir)
    }

    #[cfg(test)]
    pub fn set_test_paths(&mut self, skills_dir: PathBuf, config_path: PathBuf) {
        self.skills_dir = skills_dir;
        self.config_path = config_path;
    }

    /// Load all skills from ~/.flexorama/skills/ directory
    pub async fn load_all_skills(&mut self) -> Result<()> {
        // Create directory if it doesn't exist
        if !self.skills_dir.exists() {
            fs::create_dir_all(&self.skills_dir).await?;
            return Ok(());
        }

        // Read all skill directories (SKILL.md) and legacy .md files
        let mut entries = fs::read_dir(&self.skills_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let skill_file = if path.is_dir() {
                let candidate = path.join("SKILL.md");
                if candidate.is_file() {
                    Some(candidate)
                } else {
                    None
                }
            } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
                Some(path)
            } else {
                None
            };

            if let Some(skill_file) = skill_file {
                match self.load_skill_from_file(&skill_file).await {
                    Ok(skill) => {
                        self.skills.insert(skill.name.clone(), skill);
                    }
                    Err(e) => {
                        eprintln!("Failed to load skill from {:?}: {}", skill_file, e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Load a single skill from file
    async fn load_skill_from_file(&self, path: &Path) -> Result<Skill> {
        let content = fs::read_to_string(path).await?;
        Skill::from_markdown(&content)
    }

    /// Save a skill to disk
    pub async fn save_skill(&self, skill: &Skill) -> Result<()> {
        // Create directory if it doesn't exist
        if !self.skills_dir.exists() {
            fs::create_dir_all(&self.skills_dir).await?;
        }

        let skill_dir = self.skills_dir.join(&skill.name);
        if !skill_dir.exists() {
            fs::create_dir_all(&skill_dir).await?;
        }

        let file_path = skill_dir.join("SKILL.md");
        let content = skill.to_markdown();
        fs::write(&file_path, content).await?;

        Ok(())
    }

    /// Create a new skill
    pub async fn create_skill(&mut self, mut skill: Skill) -> Result<()> {
        // Check if skill already exists
        if self.skills.contains_key(&skill.name) {
            return Err(anyhow!("Skill '{}' already exists", skill.name));
        }

        // Set timestamps
        let now = Utc::now();
        skill.created_at = now;
        skill.updated_at = now;

        // Save to disk
        self.save_skill(&skill).await?;

        // Add to in-memory map
        self.skills.insert(skill.name.clone(), skill);

        Ok(())
    }

    /// Update an existing skill
    pub async fn update_skill(&mut self, skill: &Skill) -> Result<()> {
        // Check if skill exists
        if !self.skills.contains_key(&skill.name) {
            return Err(anyhow!("Skill '{}' does not exist", skill.name));
        }

        // Save to disk
        self.save_skill(skill).await?;

        // Update in-memory map
        self.skills.insert(skill.name.clone(), skill.clone());

        Ok(())
    }

    /// Delete a skill
    pub async fn delete_skill(&mut self, name: &str) -> Result<()> {
        // Remove from active skills if present
        self.active_skills.remove(name);

        // Delete file
        let skill_dir = self.skills_dir.join(name);
        let skill_file = skill_dir.join("SKILL.md");
        if skill_file.exists() {
            fs::remove_file(&skill_file).await?;
        }
        if skill_dir.exists() {
            fs::remove_dir_all(&skill_dir).await?;
        }

        let legacy_file = self.skills_dir.join(format!("{}.md", name));
        if legacy_file.exists() {
            fs::remove_file(&legacy_file).await?;
        }

        // Remove from in-memory map
        self.skills.remove(name);

        // Update config if it was active
        let mut config = self.config.write().await;
        config.skills.active_skills.retain(|s| s != name);
        config.skills.deactivated_skills.retain(|s| s != name);
        config.save(self.config_path.to_str()).await?;

        Ok(())
    }

    /// Activate a skill
    pub async fn activate_skill(&mut self, name: &str) -> Result<()> {
        // Check if skill exists
        if !self.skills.contains_key(name) {
            return Err(anyhow!("Skill '{}' does not exist", name));
        }

        // Add to active skills
        self.active_skills.insert(name.to_string());

        // Update config
        let mut config = self.config.write().await;
        if !config.skills.active_skills.contains(&name.to_string()) {
            config.skills.active_skills.push(name.to_string());
        }
        config.skills.deactivated_skills.retain(|s| s != name);
        config.save(self.config_path.to_str()).await?;

        Ok(())
    }

    /// Deactivate a skill
    pub async fn deactivate_skill(&mut self, name: &str) -> Result<()> {
        // Remove from active skills
        self.active_skills.remove(name);

        // Update config
        let mut config = self.config.write().await;
        config.skills.active_skills.retain(|s| s != name);
        if !config.skills.deactivated_skills.contains(&name.to_string()) {
            config.skills.deactivated_skills.push(name.to_string());
        }
        config.save(self.config_path.to_str()).await?;

        Ok(())
    }

    /// Get list of active skills
    pub fn get_active_skills(&self) -> Vec<&Skill> {
        self.active_skills
            .iter()
            .filter_map(|name| self.skills.get(name))
            .collect()
    }

    /// Get combined content of all active skills for system prompt (metadata only for progressive disclosure)
    pub fn get_active_skills_content(&self) -> String {
        let active = self.get_active_skills();

        if active.is_empty() {
            return String::new();
        }

        let mut content = String::from("# Active Skills\n\n");

        content.push_str("## What are Skills?\n\n");
        content.push_str("Skills are specialized knowledge modules that provide you with expert capabilities in specific domains. ");
        content.push_str(
            "Each skill contains detailed instructions, best practices, and guidelines.\n\n",
        );

        content.push_str("## How to Use Skills\n\n");
        content.push_str("- Skills are listed below with their descriptions and metadata\n");
        content.push_str("- **IMPORTANT**: Use the `use_skill` tool to load the full content of a skill when you need its detailed instructions\n");
        content.push_str("- Call `use_skill` with the skill name when the user's request matches a skill's domain\n");
        content.push_str("- Skills may restrict which tools you can use (check allowed_tools and denied_tools)\n");
        content.push_str("- Multiple skills can be active simultaneously - load and combine their knowledge as needed\n\n");

        content.push_str(&format!(
            "## Currently Active Skills ({})\n\n",
            active.len()
        ));

        // Include metadata for each skill (progressive disclosure - full content loaded via use_skill tool)
        for skill in active {
            content.push_str(&format!("### {}\n\n", skill.name));

            let desc = if skill.description.is_empty() {
                "No description provided"
            } else {
                &skill.description
            };
            content.push_str(&format!("**Description**: {}\n\n", desc));

            // Add metadata
            if let Some(model) = &skill.model {
                content.push_str(&format!("**Preferred Model**: {}\n", model));
            }
            if let Some(temp) = skill.temperature {
                content.push_str(&format!("**Temperature**: {}\n", temp));
            }
            if !skill.allowed_tools.is_empty() {
                content.push_str(&format!(
                    "**Allowed Tools**: {}\n",
                    skill
                        .allowed_tools
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if !skill.denied_tools.is_empty() {
                content.push_str(&format!(
                    "**Denied Tools**: {}\n",
                    skill
                        .denied_tools
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if !skill.tags.is_empty() {
                content.push_str(&format!("**Tags**: {}\n", skill.tags.join(", ")));
            }

            content.push_str(&format!(
                "\n**To use this skill**: Call `use_skill` with name=\"{}\"\n\n",
                skill.name
            ));
            content.push_str("---\n\n");
        }

        content
    }

    /// Get full content of a specific skill (for progressive disclosure via use_skill tool)
    pub fn get_skill_full_content(&self, skill_name: &str) -> Result<String> {
        let skill = self
            .skills
            .get(skill_name)
            .ok_or_else(|| anyhow!("Skill '{}' not found", skill_name))?;

        if !self.active_skills.contains(skill_name) {
            return Err(anyhow!(
                "Skill '{}' is not active. Only active skills can be loaded.",
                skill_name
            ));
        }

        let mut content = String::new();
        content.push_str(&format!("# Skill: {}\n\n", skill.name));
        content.push_str(&format!("**Description**: {}\n\n", skill.description));

        if let Some(model) = &skill.model {
            content.push_str(&format!("**Preferred Model**: {}\n", model));
        }
        if let Some(temp) = skill.temperature {
            content.push_str(&format!("**Temperature**: {}\n", temp));
        }
        if let Some(max_tokens) = skill.max_tokens {
            content.push_str(&format!("**Max Tokens**: {}\n", max_tokens));
        }
        if !skill.allowed_tools.is_empty() {
            content.push_str(&format!(
                "**Allowed Tools**: {}\n",
                skill
                    .allowed_tools
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !skill.denied_tools.is_empty() {
            content.push_str(&format!(
                "**Denied Tools**: {}\n",
                skill
                    .denied_tools
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !skill.tags.is_empty() {
            content.push_str(&format!("**Tags**: {}\n", skill.tags.join(", ")));
        }

        content.push_str("\n---\n\n");
        content.push_str("## Skill Content\n\n");
        content.push_str(&skill.content);

        Ok(content)
    }

    /// Get skill by name
    pub fn get_skill(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    /// Get mutable skill by name
    pub fn get_skill_mut(&mut self, name: &str) -> Option<&mut Skill> {
        self.skills.get_mut(name)
    }

    /// List all skills
    pub fn list_skills(&self) -> Vec<&Skill> {
        self.skills.values().collect()
    }

    /// Check if a skill is active
    pub fn is_skill_active(&self, name: &str) -> bool {
        self.active_skills.contains(name)
    }

    /// Load skill references (progressive disclosure)
    pub async fn load_skill_references(&mut self, skill_name: &str) -> Result<Vec<String>> {
        let skill = self
            .skills
            .get_mut(skill_name)
            .ok_or_else(|| anyhow!("Skill '{}' not found", skill_name))?;

        let mut loaded_content = Vec::new();
        let references_dir = self.skills_dir.join("references");

        for reference in &mut skill.references {
            if !reference.loaded {
                let ref_path = references_dir.join(&reference.path);

                if ref_path.exists() {
                    let content = fs::read_to_string(&ref_path).await?;
                    loaded_content.push(format!(
                        "## Reference: {}\n\n{}",
                        reference.description.as_deref().unwrap_or(&reference.path),
                        content
                    ));
                    reference.loaded = true;
                }
            }
        }

        Ok(loaded_content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // Helper function to create a test config
    fn create_test_config() -> Arc<RwLock<Config>> {
        let config = Config::default();
        Arc::new(RwLock::new(config))
    }

    fn create_test_manager(temp_dir: &TempDir) -> SkillManager {
        let config = create_test_config();
        let mut manager = SkillManager::new(config).unwrap();
        manager.skills_dir = temp_dir.path().to_path_buf();
        manager.config_path = temp_dir.path().join("config.toml");
        manager
    }

    // Helper function to create a basic skill
    fn create_test_skill(name: &str) -> Skill {
        Skill {
            name: name.to_string(),
            description: "Test skill".to_string(),
            content: "This is test content".to_string(),
            allowed_tools: HashSet::new(),
            denied_tools: HashSet::new(),
            model: None,
            temperature: None,
            max_tokens: None,
            tags: Vec::new(),
            references: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_skill_from_markdown_valid() {
        let markdown = r#"---
name: test-skill
description: A test skill
allowed_tools:
  - read_file
  - write_file
denied_tools:
  - bash
model: claude-3-sonnet
temperature: 0.7
max_tokens: 2048
tags:
  - testing
  - example
---

# Test Skill Content

This is the main content of the skill.

It can include multiple paragraphs and markdown formatting.
"#;

        let skill = Skill::from_markdown(markdown).expect("Failed to parse valid markdown");

        assert_eq!(skill.name, "test-skill");
        assert_eq!(skill.description, "A test skill");
        assert!(skill.allowed_tools.contains("read_file"));
        assert!(skill.allowed_tools.contains("write_file"));
        assert!(skill.denied_tools.contains("bash"));
        assert_eq!(skill.model, Some("claude-3-sonnet".to_string()));
        assert_eq!(skill.temperature, Some(0.7));
        assert_eq!(skill.max_tokens, Some(2048));
        assert_eq!(skill.tags, vec!["testing", "example"]);
        assert!(skill.content.contains("Test Skill Content"));
    }

    #[test]
    fn test_skill_from_markdown_minimal() {
        let markdown = r#"---
name: minimal
description: Minimal skill
---

Just content here.
"#;

        let skill = Skill::from_markdown(markdown).expect("Failed to parse minimal markdown");

        assert_eq!(skill.name, "minimal");
        assert_eq!(skill.description, "Minimal skill");
        assert!(skill.allowed_tools.is_empty());
        assert!(skill.denied_tools.is_empty());
        assert_eq!(skill.model, None);
        assert_eq!(skill.temperature, None);
        assert_eq!(skill.max_tokens, None);
        assert!(skill.tags.is_empty());
        assert_eq!(skill.content.trim(), "Just content here.");
    }

    #[test]
    fn test_skill_from_markdown_invalid_no_delimiters() {
        let markdown = "This is not valid markdown without frontmatter";

        let result = Skill::from_markdown(markdown);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("missing frontmatter delimiters"));
    }

    #[test]
    fn test_skill_from_markdown_invalid_yaml() {
        let markdown = r#"---
name: test
description: [invalid yaml structure without closing
---

Content here.
"#;

        let result = Skill::from_markdown(markdown);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to parse YAML frontmatter"));
    }

    #[test]
    fn test_skill_from_markdown_with_triple_dashes_in_content() {
        let markdown = r#"---
name: test
description: Test skill
---

# Content

Some content here.

---

More content after dashes.
"#;

        let skill = Skill::from_markdown(markdown).expect("Failed to parse markdown with dashes");

        assert!(skill.content.contains("---"));
        assert!(skill.content.contains("More content after dashes"));
    }

    #[test]
    fn test_skill_to_markdown() {
        let mut skill = create_test_skill("test");
        skill.allowed_tools.insert("read_file".to_string());
        skill.model = Some("claude-3-sonnet".to_string());
        skill.temperature = Some(0.5);
        skill.tags = vec!["test".to_string()];

        let markdown = skill.to_markdown();

        assert!(markdown.contains("---"));
        assert!(markdown.contains("name: test"));
        assert!(markdown.contains("description: Test skill"));
        assert!(markdown.contains("read_file"));
        assert!(markdown.contains("claude-3-sonnet"));
        assert!(markdown.contains("This is test content"));
    }

    #[test]
    fn test_skill_roundtrip() {
        let original = create_test_skill("roundtrip");
        let markdown = original.to_markdown();
        let parsed = Skill::from_markdown(&markdown).expect("Failed to parse roundtrip markdown");

        assert_eq!(parsed.name, original.name);
        assert_eq!(parsed.description, original.description);
        assert_eq!(parsed.content, original.content);
        assert_eq!(parsed.allowed_tools, original.allowed_tools);
        assert_eq!(parsed.denied_tools, original.denied_tools);
        assert_eq!(parsed.model, original.model);
        assert_eq!(parsed.tags, original.tags);
    }

    #[test]
    fn test_parse_references_single() {
        let content = "Check out @references/example.md for more info.";
        let references = Skill::parse_references(content);

        assert_eq!(references.len(), 1);
        assert_eq!(references[0].path, "example.md");
        assert!(!references[0].loaded);
    }

    #[test]
    fn test_parse_references_multiple() {
        let content = r#"
See @references/guide.md and @references/examples/advanced.md for details.
Also check @references/api.md
"#;
        let references = Skill::parse_references(content);

        assert_eq!(references.len(), 3);
        assert_eq!(references[0].path, "guide.md");
        assert_eq!(references[1].path, "examples/advanced.md");
        assert_eq!(references[2].path, "api.md");
    }

    #[test]
    fn test_parse_references_none() {
        let content = "This content has no references.";
        let references = Skill::parse_references(content);

        assert_eq!(references.len(), 0);
    }

    #[test]
    fn test_parse_references_in_parentheses() {
        let content = "See the documentation (@references/docs.md) for more.";
        let references = Skill::parse_references(content);

        assert_eq!(references.len(), 1);
        assert_eq!(references[0].path, "docs.md");
    }

    #[tokio::test]
    async fn test_skill_manager_new() {
        let config = create_test_config();
        let manager = SkillManager::new(config);
        assert!(manager.is_ok());
    }

    #[tokio::test]
    async fn test_skill_manager_create_and_get_skill() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&temp_dir);

        let skill = create_test_skill("test-create");

        manager
            .create_skill(skill.clone())
            .await
            .expect("Failed to create skill");

        let retrieved = manager.get_skill("test-create");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "test-create");
    }

    #[tokio::test]
    async fn test_skill_manager_create_duplicate_fails() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&temp_dir);

        let skill = create_test_skill("duplicate");

        manager.create_skill(skill.clone()).await.unwrap();
        let result = manager.create_skill(skill).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn test_skill_manager_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&temp_dir);

        let mut skill = create_test_skill("save-load");
        skill.description = "Updated description".to_string();

        manager.save_skill(&skill).await.expect("Failed to save");

        let skill_file = temp_dir.path().join("save-load").join("SKILL.md");
        assert!(skill_file.exists());

        let loaded = manager
            .load_skill_from_file(&skill_file)
            .await
            .expect("Failed to load");
        assert_eq!(loaded.name, "save-load");
        assert_eq!(loaded.description, "Updated description");
    }

    #[tokio::test]
    async fn test_skill_manager_update_skill() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&temp_dir);

        let skill = create_test_skill("update-test");
        manager.create_skill(skill).await.unwrap();

        let mut updated = manager.get_skill("update-test").unwrap().clone();
        updated.description = "New description".to_string();

        manager
            .update_skill(&updated)
            .await
            .expect("Failed to update");

        let retrieved = manager.get_skill("update-test").unwrap();
        assert_eq!(retrieved.description, "New description");
    }

    #[tokio::test]
    async fn test_skill_manager_update_nonexistent_fails() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&temp_dir);

        let skill = create_test_skill("nonexistent");
        let result = manager.update_skill(&skill).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[tokio::test]
    async fn test_skill_manager_delete_skill() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&temp_dir);

        let skill = create_test_skill("delete-test");
        manager.create_skill(skill).await.unwrap();

        assert!(manager.get_skill("delete-test").is_some());

        manager.delete_skill("delete-test").await.unwrap();

        assert!(manager.get_skill("delete-test").is_none());
        let skill_dir = temp_dir.path().join("delete-test");
        assert!(!skill_dir.exists());
    }

    #[tokio::test]
    async fn test_skill_manager_activate_skill() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&temp_dir);

        let skill = create_test_skill("activate-test");
        manager.create_skill(skill).await.unwrap();

        assert!(!manager.is_skill_active("activate-test"));

        manager.activate_skill("activate-test").await.unwrap();

        assert!(manager.is_skill_active("activate-test"));
        assert_eq!(manager.get_active_skills().len(), 1);
    }

    #[tokio::test]
    async fn test_skill_manager_activate_nonexistent_fails() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&temp_dir);

        let result = manager.activate_skill("nonexistent").await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[tokio::test]
    async fn test_skill_manager_deactivate_skill() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&temp_dir);

        let skill = create_test_skill("deactivate-test");
        manager.create_skill(skill).await.unwrap();
        manager.activate_skill("deactivate-test").await.unwrap();

        assert!(manager.is_skill_active("deactivate-test"));

        manager.deactivate_skill("deactivate-test").await.unwrap();

        assert!(!manager.is_skill_active("deactivate-test"));
    }

    #[tokio::test]
    async fn test_skill_manager_list_skills() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&temp_dir);

        manager
            .create_skill(create_test_skill("skill1"))
            .await
            .unwrap();
        manager
            .create_skill(create_test_skill("skill2"))
            .await
            .unwrap();
        manager
            .create_skill(create_test_skill("skill3"))
            .await
            .unwrap();

        let skills = manager.list_skills();
        assert_eq!(skills.len(), 3);
    }

    #[tokio::test]
    async fn test_skill_manager_get_active_skills_content() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&temp_dir);

        let mut skill = create_test_skill("content-test");
        skill.description = "A test skill description".to_string();
        manager.create_skill(skill).await.unwrap();
        manager.activate_skill("content-test").await.unwrap();

        let content = manager.get_active_skills_content();

        assert!(content.contains("# Active Skills"));
        assert!(content.contains("content-test"));
        assert!(content.contains("A test skill description"));
        assert!(content.contains("use_skill"));
    }

    #[tokio::test]
    async fn test_skill_manager_get_active_skills_content_empty() {
        let config = create_test_config();
        let manager = SkillManager::new(config).unwrap();

        let content = manager.get_active_skills_content();
        assert!(content.is_empty());
    }

    #[tokio::test]
    async fn test_skill_manager_get_skill_full_content() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&temp_dir);

        let mut skill = create_test_skill("full-content");
        skill.content = "Detailed instructions here.".to_string();
        skill.model = Some("claude-3-opus".to_string());
        manager.create_skill(skill).await.unwrap();
        manager.activate_skill("full-content").await.unwrap();

        let content = manager.get_skill_full_content("full-content").unwrap();

        assert!(content.contains("# Skill: full-content"));
        assert!(content.contains("Detailed instructions here."));
        assert!(content.contains("claude-3-opus"));
    }

    #[tokio::test]
    async fn test_skill_manager_get_skill_full_content_not_active() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&temp_dir);

        let skill = create_test_skill("inactive");
        manager.create_skill(skill).await.unwrap();

        let result = manager.get_skill_full_content("inactive");

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not active"));
    }

    #[tokio::test]
    async fn test_skill_manager_get_skill_full_content_nonexistent() {
        let config = create_test_config();
        let manager = SkillManager::new(config).unwrap();

        let result = manager.get_skill_full_content("nonexistent");

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_skill_manager_load_all_skills() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&temp_dir);

        // Create skills manually in the directory
        let skill1 = create_test_skill("auto-load-1");
        let skill2 = create_test_skill("auto-load-2");

        manager.save_skill(&skill1).await.unwrap();
        manager.save_skill(&skill2).await.unwrap();

        // Create a new manager and load all skills
        let config2 = create_test_config();
        let mut manager2 = SkillManager::new(config2).unwrap();
        manager2.skills_dir = temp_dir.path().to_path_buf();
        manager2.config_path = temp_dir.path().join("config.toml");

        manager2.load_all_skills().await.unwrap();

        assert_eq!(manager2.list_skills().len(), 2);
        assert!(manager2.get_skill("auto-load-1").is_some());
        assert!(manager2.get_skill("auto-load-2").is_some());
    }

    #[tokio::test]
    async fn test_skill_manager_load_skill_references() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&temp_dir);

        // Create a skill with references
        let content = "Check @references/guide.md for details.";
        let mut skill = create_test_skill("ref-test");
        skill.content = content.to_string();
        skill.references = Skill::parse_references(content);

        manager.create_skill(skill).await.unwrap();

        // Create the reference file
        let ref_dir = temp_dir.path().join("references");
        fs::create_dir_all(&ref_dir).await.unwrap();
        let ref_file = ref_dir.join("guide.md");
        fs::write(&ref_file, "# Guide\n\nThis is a guide.")
            .await
            .unwrap();

        // Load references
        let loaded = manager.load_skill_references("ref-test").await.unwrap();

        assert_eq!(loaded.len(), 1);
        assert!(loaded[0].contains("This is a guide"));

        // Verify reference is marked as loaded
        let skill = manager.get_skill("ref-test").unwrap();
        assert!(skill.references[0].loaded);
    }

    #[tokio::test]
    async fn test_skill_manager_load_nonexistent_references() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&temp_dir);

        let content = "@references/missing.md";
        let mut skill = create_test_skill("missing-ref");
        skill.content = content.to_string();
        skill.references = Skill::parse_references(content);

        manager.create_skill(skill).await.unwrap();

        let loaded = manager.load_skill_references("missing-ref").await.unwrap();

        // Should not fail, just return empty
        assert_eq!(loaded.len(), 0);
    }

    #[tokio::test]
    async fn test_skill_manager_get_skill_mut() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&temp_dir);

        let skill = create_test_skill("mutable-test");
        manager.create_skill(skill).await.unwrap();

        {
            let skill_mut = manager.get_skill_mut("mutable-test");
            assert!(skill_mut.is_some());
            skill_mut.unwrap().description = "Modified".to_string();
        }

        let skill = manager.get_skill("mutable-test").unwrap();
        assert_eq!(skill.description, "Modified");
    }

    #[test]
    fn test_skill_frontmatter_serialization() {
        let frontmatter = SkillFrontmatter {
            name: "test".to_string(),
            description: "Test description".to_string(),
            allowed_tools: {
                let mut set = HashSet::new();
                set.insert("read_file".to_string());
                set
            },
            denied_tools: HashSet::new(),
            model: Some("claude-3-sonnet".to_string()),
            temperature: Some(0.7),
            max_tokens: Some(1024),
            tags: vec!["test".to_string()],
            created_at: None,
            updated_at: None,
        };

        let yaml = serde_yaml::to_string(&frontmatter).unwrap();
        assert!(yaml.contains("name: test"));
        assert!(yaml.contains("description: Test description"));
        assert!(yaml.contains("read_file"));

        let deserialized: SkillFrontmatter = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(deserialized.name, "test");
        assert_eq!(deserialized.temperature, Some(0.7));
    }

    #[tokio::test]
    async fn test_skill_with_all_metadata_fields() {
        let markdown = r#"---
name: comprehensive
description: A comprehensive test
allowed_tools:
  - read_file
  - write_file
  - bash
denied_tools:
  - delete_file
model: claude-3-opus
temperature: 0.8
max_tokens: 4096
tags:
  - advanced
  - testing
  - production
---

# Comprehensive Skill

This skill has all possible metadata fields populated.

## Section 1
Content here.

## Section 2
More content.
"#;

        let skill = Skill::from_markdown(markdown).unwrap();

        assert_eq!(skill.name, "comprehensive");
        assert_eq!(skill.allowed_tools.len(), 3);
        assert_eq!(skill.denied_tools.len(), 1);
        assert_eq!(skill.model, Some("claude-3-opus".to_string()));
        assert_eq!(skill.temperature, Some(0.8));
        assert_eq!(skill.max_tokens, Some(4096));
        assert_eq!(skill.tags.len(), 3);
        assert!(skill.content.contains("Section 1"));
        assert!(skill.content.contains("Section 2"));
    }

    #[tokio::test]
    async fn test_multiple_skills_activation() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&temp_dir);

        manager
            .create_skill(create_test_skill("skill-a"))
            .await
            .unwrap();
        manager
            .create_skill(create_test_skill("skill-b"))
            .await
            .unwrap();
        manager
            .create_skill(create_test_skill("skill-c"))
            .await
            .unwrap();

        manager.activate_skill("skill-a").await.unwrap();
        manager.activate_skill("skill-b").await.unwrap();

        assert_eq!(manager.get_active_skills().len(), 2);
        assert!(manager.is_skill_active("skill-a"));
        assert!(manager.is_skill_active("skill-b"));
        assert!(!manager.is_skill_active("skill-c"));

        manager.deactivate_skill("skill-a").await.unwrap();

        assert_eq!(manager.get_active_skills().len(), 1);
        assert!(!manager.is_skill_active("skill-a"));
        assert!(manager.is_skill_active("skill-b"));
    }
}
