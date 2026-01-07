use crate::config::Config;
use crate::skill;
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
            return Err(anyhow!("Invalid SKILL.md format: missing frontmatter delimiters"));
        }

        // Parse YAML frontmatter (second part, index 1)
        let frontmatter_str = parts[1].trim();
        let frontmatter: SkillFrontmatter = serde_yaml::from_str(frontmatter_str)
            .context("Failed to parse YAML frontmatter")?;

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

        let yaml_str = serde_yaml::to_string(&frontmatter)
            .unwrap_or_else(|_| String::from("name: error\ndescription: Failed to serialize frontmatter"));

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
}

impl SkillManager {
    /// Initialize SkillManager from ~/.aixplosion/skills/ directory
    pub fn new(config: Arc<RwLock<Config>>) -> Result<Self> {
        // Determine skills directory
        let skills_dir = Self::get_skills_dir()?;

        Ok(SkillManager {
            skills_dir,
            skills: HashMap::new(),
            active_skills: HashSet::new(),
            config,
        })
    }

    /// Get the ~/.aixplosion/skills/ directory path
    fn get_skills_dir() -> Result<PathBuf> {
        let skills_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".aixplosion")
            .join("skills");
        Ok(skills_dir)
    }

    /// Load all skills from ~/.aixplosion/skills/ directory
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
                if candidate.is_file() { Some(candidate) } else { None }
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
        config.save(None).await?;

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
        config.save(None).await?;

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
        config.save(None).await?;

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
        content.push_str("Each skill contains detailed instructions, best practices, and guidelines.\n\n");

        content.push_str("## How to Use Skills\n\n");
        content.push_str("- Skills are listed below with their descriptions and metadata\n");
        content.push_str("- **IMPORTANT**: Use the `use_skill` tool to load the full content of a skill when you need its detailed instructions\n");
        content.push_str("- Call `use_skill` with the skill name when the user's request matches a skill's domain\n");
        content.push_str("- Skills may restrict which tools you can use (check allowed_tools and denied_tools)\n");
        content.push_str("- Multiple skills can be active simultaneously - load and combine their knowledge as needed\n\n");

        content.push_str(&format!("## Currently Active Skills ({})\n\n", active.len()));

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
                content.push_str(&format!("**Allowed Tools**: {}\n", skill.allowed_tools.iter().cloned().collect::<Vec<_>>().join(", ")));
            }
            if !skill.denied_tools.is_empty() {
                content.push_str(&format!("**Denied Tools**: {}\n", skill.denied_tools.iter().cloned().collect::<Vec<_>>().join(", ")));
            }
            if !skill.tags.is_empty() {
                content.push_str(&format!("**Tags**: {}\n", skill.tags.join(", ")));
            }

            content.push_str(&format!("\n**To use this skill**: Call `use_skill` with name=\"{}\"\n\n", skill.name));
            content.push_str("---\n\n");
        }

        content
    }

    /// Get full content of a specific skill (for progressive disclosure via use_skill tool)
    pub fn get_skill_full_content(&self, skill_name: &str) -> Result<String> {
        let skill = self.skills.get(skill_name)
            .ok_or_else(|| anyhow!("Skill '{}' not found", skill_name))?;

        if !self.active_skills.contains(skill_name) {
            return Err(anyhow!("Skill '{}' is not active. Only active skills can be loaded.", skill_name));
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
            content.push_str(&format!("**Allowed Tools**: {}\n", skill.allowed_tools.iter().cloned().collect::<Vec<_>>().join(", ")));
        }
        if !skill.denied_tools.is_empty() {
            content.push_str(&format!("**Denied Tools**: {}\n", skill.denied_tools.iter().cloned().collect::<Vec<_>>().join(", ")));
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
        let skill = self.skills.get_mut(skill_name)
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
