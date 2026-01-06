use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tokio::fs;

// Frontmatter structure - excludes system_prompt since it's the markdown content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentFrontmatter {
    pub name: String,
    pub allowed_tools: HashSet<String>,
    pub denied_tools: HashSet<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub model: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// Full config structure - includes system_prompt for internal use
#[derive(Debug, Clone)]
pub struct SubagentConfig {
    pub name: String,
    pub system_prompt: String,
    pub allowed_tools: HashSet<String>,
    pub denied_tools: HashSet<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub model: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Subagent {
    pub config: SubagentConfig,
    // Note: Agent and ConversationManager don't implement Clone/Debug
    // We'll handle this differently when needed
}

pub struct SubagentManager {
    agents_dir: PathBuf,
    active_subagent: Option<String>,
    subagents: HashMap<String, SubagentConfig>,
}

impl SubagentManager {
    pub fn new() -> Result<Self> {
        let agents_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".aixplosion")
            .join("agents");

        // Create directory if it doesn't exist
        std::fs::create_dir_all(&agents_dir)?;

        Ok(Self {
            agents_dir,
            active_subagent: None,
            subagents: HashMap::new(),
        })
    }

    pub async fn load_all_subagents(&mut self) -> Result<()> {
        let mut subagents = HashMap::new();

        // Check if directory exists
        if !self.agents_dir.exists() {
            std::fs::create_dir_all(&self.agents_dir)?;
            self.subagents = subagents;
            return Ok(());
        }

        let entries = match fs::read_dir(&self.agents_dir).await {
            Ok(entries) => entries,
            Err(e) => {
                log::warn!("Failed to read agents directory: {}", e);
                self.subagents = subagents;
                return Ok(());
            }
        };

        let mut entries_vec = Vec::new();
        let mut entry_stream = entries;
        while let Some(entry) = entry_stream.next_entry().await? {
            entries_vec.push(entry);
        }

        for entry in entries_vec {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("md") {
                match self.load_subagent_from_file(&path).await {
                    Ok(config) => {
                        subagents.insert(config.name.clone(), config);
                    }
                    Err(e) => {
                        log::warn!("Failed to load subagent from {}: {}", path.display(), e);
                    }
                }
            }
        }

        self.subagents = subagents;
        log::info!("Loaded {} subagents", self.subagents.len());
        Ok(())
    }

    async fn load_subagent_from_file(&self, path: &Path) -> Result<SubagentConfig> {
        let content = fs::read_to_string(path).await?;

        // Normalize line endings to handle both Windows (\r\n) and Unix (\n)
        let normalized_content = content.replace("\r\n", "\n");

        // Remove BOM if present
        let cleaned_content = normalized_content.trim_start_matches('\u{FEFF}');

        // Parse frontmatter using a more straightforward approach
        if !cleaned_content.starts_with("---\n") {
            return Err(anyhow!(
                "Invalid subagent file format: must start with ---. File: {}",
                path.display()
            ));
        }

        // Find the end of the frontmatter
        let frontmatter_end = cleaned_content.find("\n---\n").ok_or_else(|| {
            anyhow!(
                "Invalid subagent file format: missing closing ---. File: {}",
                path.display()
            )
        })?;

        // Extract frontmatter and content
        let frontmatter_str = &cleaned_content[4..frontmatter_end]; // Skip opening "---\n"
        let system_prompt = cleaned_content[frontmatter_end + 5..].to_string(); // Skip closing "\n---\n"

        // Parse YAML frontmatter as SubagentFrontmatter (without system_prompt field)
        let frontmatter: SubagentFrontmatter = serde_yaml::from_str(frontmatter_str)
            .map_err(|e| anyhow!("Failed to parse frontmatter YAML: {}", e))?;

        // Create full config by combining frontmatter with system_prompt
        let config = SubagentConfig {
            name: frontmatter.name,
            system_prompt,
            allowed_tools: frontmatter.allowed_tools,
            denied_tools: frontmatter.denied_tools,
            max_tokens: frontmatter.max_tokens,
            temperature: frontmatter.temperature,
            model: frontmatter.model,
            created_at: frontmatter.created_at,
            updated_at: frontmatter.updated_at,
        };

        Ok(config)
    }

    pub async fn save_subagent(&self, config: &SubagentConfig) -> Result<()> {
        let file_path = self.agents_dir.join(format!("{}.md", config.name));

        // Create frontmatter structure without system_prompt
        let frontmatter = SubagentFrontmatter {
            name: config.name.clone(),
            allowed_tools: config.allowed_tools.clone(),
            denied_tools: config.denied_tools.clone(),
            max_tokens: config.max_tokens,
            temperature: config.temperature,
            model: config.model.clone(),
            created_at: config.created_at,
            updated_at: config.updated_at,
        };

        let frontmatter_yaml = serde_yaml::to_string(&frontmatter)?;

        let content = format!(
            "---\n{}\n---\n{}",
            frontmatter_yaml.trim(),
            config.system_prompt
        );

        fs::write(&file_path, content).await?;
        log::info!("Saved subagent config to: {}", file_path.display());
        Ok(())
    }

    pub async fn create_subagent(
        &mut self,
        name: &str,
        system_prompt: &str,
        allowed_tools: Vec<String>,
        denied_tools: Vec<String>,
    ) -> Result<SubagentConfig> {
        let now = Utc::now();
        let config = SubagentConfig {
            name: name.to_string(),
            system_prompt: system_prompt.to_string(),
            allowed_tools: allowed_tools.into_iter().collect(),
            denied_tools: denied_tools.into_iter().collect(),
            max_tokens: None,
            temperature: None,
            model: None,
            created_at: now,
            updated_at: now,
        };

        self.save_subagent(&config).await?;
        self.subagents.insert(name.to_string(), config.clone());
        Ok(config)
    }

    pub fn list_subagents(&self) -> Vec<&SubagentConfig> {
        self.subagents.values().collect()
    }

    pub fn get_subagent(&self, name: &str) -> Option<&SubagentConfig> {
        self.subagents.get(name)
    }

    pub async fn delete_subagent(&mut self, name: &str) -> Result<()> {
        let file_path = self.agents_dir.join(format!("{}.md", name));
        fs::remove_file(&file_path).await?;
        self.subagents.remove(name);

        // If this was the active subagent, deactivate it
        if self.active_subagent.as_ref() == Some(&name.to_string()) {
            self.active_subagent = None;
        }

        log::info!("Deleted subagent: {}", name);
        Ok(())
    }

    pub async fn update_subagent(&mut self, config: &SubagentConfig) -> Result<()> {
        let mut updated_config = config.clone();
        updated_config.updated_at = Utc::now();

        self.save_subagent(&updated_config).await?;
        self.subagents.insert(config.name.clone(), updated_config);
        Ok(())
    }

    pub fn set_active_subagent(&mut self, name: Option<String>) {
        self.active_subagent = name;
    }

    pub fn get_active_subagent(&self) -> Option<&String> {
        self.active_subagent.as_ref()
    }

    pub fn get_agents_dir(&self) -> &PathBuf {
        &self.agents_dir
    }
}
