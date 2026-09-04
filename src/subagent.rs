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

pub struct SubagentManager {
    agents_dir: PathBuf,
    active_subagent: Option<String>,
    subagents: HashMap<String, SubagentConfig>,
}

impl SubagentManager {
    pub fn new() -> Result<Self> {
        let agents_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".flexorama")
            .join("agents");
        Self::new_with_dir(agents_dir)
    }

    pub fn new_with_dir(agents_dir: PathBuf) -> Result<Self> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn temp_agents_dir() -> (tempfile::TempDir, PathBuf) {
        let current_dir = std::env::current_dir().expect("current dir");
        let temp = tempfile::tempdir_in(current_dir).expect("temp dir");
        let dir = temp.path().join("agents");
        (temp, dir)
    }

    fn sample_config(name: &str) -> SubagentConfig {
        let now = Utc::now();
        SubagentConfig {
            name: name.to_string(),
            system_prompt: "You are a test subagent.".to_string(),
            allowed_tools: ["read_file".to_string(), "glob".to_string()]
                .into_iter()
                .collect(),
            denied_tools: ["bash".to_string()].into_iter().collect(),
            max_tokens: Some(2048),
            temperature: Some(0.4),
            model: Some("claude-haiku-4-5".to_string()),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn new_with_dir_creates_missing_directory() {
        let (temp, dir) = temp_agents_dir();
        assert!(!dir.exists());

        SubagentManager::new_with_dir(dir.clone()).unwrap();

        assert!(dir.is_dir());
        drop(temp);
    }

    #[tokio::test]
    async fn create_subagent_persists_file_and_registers_it() {
        let (temp, dir) = temp_agents_dir();
        let mut manager = SubagentManager::new_with_dir(dir.clone()).unwrap();

        let config = manager
            .create_subagent("reviewer", "Review code", vec!["read_file".to_string()], vec![])
            .await
            .unwrap();

        assert!(dir.join("reviewer.md").exists());
        assert_eq!(manager.list_subagents().len(), 1);
        assert_eq!(manager.get_subagent("reviewer").unwrap().name, "reviewer");
        assert_eq!(config.system_prompt, "Review code");
        assert_eq!(config.allowed_tools, HashSet::from(["read_file".to_string()]));
        drop(temp);
    }

    #[tokio::test]
    async fn save_and_load_roundtrip_preserves_all_fields() {
        let (temp, dir) = temp_agents_dir();
        let manager = SubagentManager::new_with_dir(dir.clone()).unwrap();
        let original = sample_config("full");

        manager.save_subagent(&original).await.unwrap();

        let mut loader = SubagentManager::new_with_dir(dir.clone()).unwrap();
        loader.load_all_subagents().await.unwrap();

        let loaded = loader.get_subagent("full").expect("subagent loaded");
        assert_eq!(loaded.system_prompt, original.system_prompt);
        assert_eq!(loaded.allowed_tools, original.allowed_tools);
        assert_eq!(loaded.denied_tools, original.denied_tools);
        assert_eq!(loaded.max_tokens, original.max_tokens);
        assert_eq!(loaded.temperature, original.temperature);
        assert_eq!(loaded.model, original.model);
        assert_eq!(loaded.created_at, original.created_at);
        assert_eq!(loaded.updated_at, original.updated_at);
        drop(temp);
    }

    #[tokio::test]
    async fn load_all_subagents_ignores_non_markdown_and_malformed_files() {
        let (temp, dir) = temp_agents_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("good.md"), "---\nname: good\nallowed_tools: []\ndenied_tools: []\nmax_tokens: ~\ntemperature: ~\nmodel: ~\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\n---\nPrompt body\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "not a subagent").unwrap();
        std::fs::write(dir.join("broken.md"), "no frontmatter here").unwrap();
        std::fs::write(
            dir.join("unclosed.md"),
            "---\nname: unclosed\nallowed_tools: []\ndenied_tools: []\n",
        )
        .unwrap();

        let mut manager = SubagentManager::new_with_dir(dir.clone()).unwrap();
        manager.load_all_subagents().await.unwrap();

        assert_eq!(manager.list_subagents().len(), 1);
        assert!(manager.get_subagent("good").is_some());
        assert!(manager.get_subagent("unclosed").is_none());
        drop(temp);
    }

    #[tokio::test]
    async fn load_all_subagents_normalizes_crlf_and_bom() {
        let (temp, dir) = temp_agents_dir();
        std::fs::create_dir_all(&dir).unwrap();

        let frontmatter = "name: windows\r\nallowed_tools: []\r\ndenied_tools: []\r\nmax_tokens: ~\r\ntemperature: ~\r\nmodel: ~\r\ncreated_at: 2026-01-01T00:00:00Z\r\nupdated_at: 2026-01-01T00:00:00Z\r\n";
        let crlf_file = format!("---\r\n{}\r\n---\r\nWindows prompt", frontmatter);
        std::fs::write(dir.join("windows.md"), crlf_file).unwrap();

        let bom_file = format!(
            "\u{FEFF}---\nname: bommy\nallowed_tools: []\ndenied_tools: []\nmax_tokens: ~\ntemperature: ~\nmodel: ~\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\n---\nBOM prompt"
        );
        std::fs::write(dir.join("bommy.md"), bom_file).unwrap();

        let mut manager = SubagentManager::new_with_dir(dir.clone()).unwrap();
        manager.load_all_subagents().await.unwrap();

        assert_eq!(manager.list_subagents().len(), 2);
        assert_eq!(
            manager.get_subagent("windows").unwrap().system_prompt,
            "Windows prompt"
        );
        assert_eq!(
            manager.get_subagent("bommy").unwrap().system_prompt,
            "BOM prompt"
        );
        drop(temp);
    }

    #[tokio::test]
    async fn load_all_subagents_on_missing_directory_creates_it_and_yields_empty() {
        let (temp, dir) = temp_agents_dir();

        let mut manager = SubagentManager::new_with_dir(dir.clone()).unwrap();
        manager.load_all_subagents().await.unwrap();

        assert!(dir.exists());
        assert!(manager.list_subagents().is_empty());
        drop(temp);
    }

    #[tokio::test]
    async fn delete_subagent_removes_file_entry_and_active_pointer() {
        let (temp, dir) = temp_agents_dir();
        let mut manager = SubagentManager::new_with_dir(dir.clone()).unwrap();

        manager
            .create_subagent("doomed", "bye", vec![], vec![])
            .await
            .unwrap();
        manager.set_active_subagent(Some("doomed".to_string()));

        manager.delete_subagent("doomed").await.unwrap();

        assert!(!dir.join("doomed.md").exists());
        assert!(manager.get_subagent("doomed").is_none());
        assert_eq!(manager.active_subagent, None);
        drop(temp);
    }

    #[tokio::test]
    async fn delete_missing_subagent_errors() {
        let (temp, dir) = temp_agents_dir();
        let mut manager = SubagentManager::new_with_dir(dir.clone()).unwrap();

        assert!(manager.delete_subagent("ghost").await.is_err());
        drop(temp);
    }

    #[tokio::test]
    async fn update_subagent_persists_changes_and_refreshes_timestamp() {
        let (temp, dir) = temp_agents_dir();
        let mut manager = SubagentManager::new_with_dir(dir.clone()).unwrap();

        let mut config = manager
            .create_subagent("evolving", "v1", vec![], vec![])
            .await
            .unwrap();
        config.system_prompt = "v2".to_string();
        config.max_tokens = Some(99);

        manager.update_subagent(&config).await.unwrap();

        let updated = manager.get_subagent("evolving").unwrap();
        assert_eq!(updated.system_prompt, "v2");
        assert_eq!(updated.max_tokens, Some(99));
        assert!(updated.updated_at >= config.created_at);

        let mut loader = SubagentManager::new_with_dir(dir.clone()).unwrap();
        loader.load_all_subagents().await.unwrap();
        assert_eq!(loader.get_subagent("evolving").unwrap().system_prompt, "v2");
        drop(temp);
    }

    #[tokio::test]
    async fn created_subagent_has_no_model_or_limits() {
        let (temp, dir) = temp_agents_dir();
        let mut manager = SubagentManager::new_with_dir(dir.clone()).unwrap();

        let config = manager
            .create_subagent("plain", "prompt", vec![], vec![])
            .await
            .unwrap();

        assert_eq!(config.model, None);
        assert_eq!(config.max_tokens, None);
        assert_eq!(config.temperature, None);
        drop(temp);
    }
}
