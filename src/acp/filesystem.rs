use crate::acp::errors::{AcpError, AcpResult};
use crate::security::FileSecurityManager;
use crate::tools::ToolCall;
use log::debug;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

/// File system operations handler for ACP
pub struct FileSystemHandler {
    file_security: Arc<RwLock<FileSecurityManager>>,
    workspace_root: Option<PathBuf>,
    yolo_mode: bool,
}

impl FileSystemHandler {
    pub fn new(
        file_security: Arc<RwLock<FileSecurityManager>>,
        workspace_root: Option<PathBuf>,
        yolo_mode: bool,
    ) -> Self {
        Self {
            file_security,
            workspace_root,
            yolo_mode,
        }
    }

    /// Set workspace root
    pub fn set_workspace_root(&mut self, root: PathBuf) {
        self.workspace_root = Some(root);
    }

    /// Resolve path relative to workspace if it's relative
    pub fn resolve_path(&self, path: &str) -> AcpResult<PathBuf> {
        let path_buf = PathBuf::from(path);

        if path_buf.is_absolute() {
            Ok(path_buf)
        } else if let Some(workspace) = &self.workspace_root {
            Ok(workspace.join(path))
        } else {
            // No workspace, use current directory
            Ok(std::env::current_dir()
                .map_err(|e| AcpError::Io(e))?
                .join(path))
        }
    }

    /// Validate path is within workspace (if workspace is set)
    pub fn validate_path(&self, path: &Path) -> AcpResult<()> {
        if let Some(workspace) = &self.workspace_root {
            let canonical_path = path
                .canonicalize()
                .map_err(|_| AcpError::FileNotFound(path.display().to_string()))?;

            let canonical_workspace = workspace
                .canonicalize()
                .map_err(|_| AcpError::InvalidPath(workspace.display().to_string()))?;

            if !canonical_path.starts_with(&canonical_workspace) {
                return Err(AcpError::PermissionDenied(format!(
                    "Path {} is outside workspace {}",
                    path.display(),
                    workspace.display()
                )));
            }
        }
        Ok(())
    }

    /// Read a file
    pub async fn read_file(&self, path: &str) -> AcpResult<String> {
        let resolved_path = self.resolve_path(path)?;
        debug!("Reading file: {}", resolved_path.display());

        // Validate path is within workspace
        self.validate_path(&resolved_path)?;

        // Use read_file tool
        let call = ToolCall {
            id: "acp-read".to_string(),
            name: "read_file".to_string(),
            arguments: json!({
                "file_path": resolved_path.to_str().ok_or_else(|| {
                    AcpError::InvalidPath(resolved_path.display().to_string())
                })?
            }),
        };

        let result = crate::tools::read_file::read_file(&call).await?;

        if result.is_error {
            Err(AcpError::Agent(anyhow::anyhow!(result.content)))
        } else {
            Ok(result.content)
        }
    }

    /// Write a file
    pub async fn write_file(&self, path: &str, content: &str) -> AcpResult<()> {
        let resolved_path = self.resolve_path(path)?;
        debug!("Writing file: {}", resolved_path.display());

        // Validate path is within workspace
        self.validate_path(&resolved_path)?;

        // Use write_file tool with security manager
        let call = ToolCall {
            id: "acp-write".to_string(),
            name: "write_file".to_string(),
            arguments: json!({
                "file_path": resolved_path.to_str().ok_or_else(|| {
                    AcpError::InvalidPath(resolved_path.display().to_string())
                })?,
                "content": content
            }),
        };

        let mut security_manager = self.file_security.write().await;
        let result = crate::tools::write_file::write_file(&call, &mut *security_manager, self.yolo_mode).await?;

        if result.is_error {
            Err(AcpError::Agent(anyhow::anyhow!(result.content)))
        } else {
            Ok(())
        }
    }

    /// List directory contents
    pub async fn list_directory(&self, path: &str) -> AcpResult<Vec<FileEntry>> {
        let resolved_path = self.resolve_path(path)?;
        debug!("Listing directory: {}", resolved_path.display());

        // Validate path is within workspace
        self.validate_path(&resolved_path)?;

        // Use list_directory tool
        let call = ToolCall {
            id: "acp-list".to_string(),
            name: "list_directory".to_string(),
            arguments: json!({
                "path": resolved_path.to_str().ok_or_else(|| {
                    AcpError::InvalidPath(resolved_path.display().to_string())
                })?
            }),
        };

        let result = crate::tools::list_directory::list_directory(&call).await?;

        if result.is_error {
            return Err(AcpError::Agent(anyhow::anyhow!(result.content)));
        }

        // Parse result content as file list
        // The list_directory tool returns a formatted string, we need to parse it
        let entries: Vec<FileEntry> = result
            .content
            .lines()
            .filter_map(|line| {
                if line.trim().is_empty() || line.starts_with("Directory") {
                    None
                } else {
                    // Simple parsing - may need refinement
                    Some(FileEntry {
                        name: line.trim().to_string(),
                        is_directory: line.ends_with('/'),
                        path: resolved_path.join(line.trim()).display().to_string(),
                    })
                }
            })
            .collect();

        Ok(entries)
    }

    /// Search for files matching a glob pattern
    pub async fn glob(&self, pattern: &str) -> AcpResult<Vec<String>> {
        debug!("Globbing pattern: {}", pattern);

        // Use glob tool
        let call = ToolCall {
            id: "acp-glob".to_string(),
            name: "glob".to_string(),
            arguments: json!({
                "pattern": pattern
            }),
        };

        let result = crate::tools::glob::glob_files(&call).await?;

        if result.is_error {
            return Err(AcpError::Agent(anyhow::anyhow!(result.content)));
        }

        // Parse result
        let files: Vec<String> = result
            .content
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with("Found") {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
            .collect();

        Ok(files)
    }

    /// Delete a file or directory
    pub async fn delete(&self, path: &str) -> AcpResult<()> {
        let resolved_path = self.resolve_path(path)?;
        debug!("Deleting: {}", resolved_path.display());

        // Validate path is within workspace
        self.validate_path(&resolved_path)?;

        // Use delete_file tool with security manager
        let call = ToolCall {
            id: "acp-delete".to_string(),
            name: "delete_file".to_string(),
            arguments: json!({
                "path": resolved_path.to_str().ok_or_else(|| {
                    AcpError::InvalidPath(resolved_path.display().to_string())
                })?
            }),
        };

        let mut security_manager = self.file_security.write().await;
        let result = crate::tools::delete_file::delete_file(&call, &mut *security_manager, self.yolo_mode).await?;

        if result.is_error {
            Err(AcpError::Agent(anyhow::anyhow!(result.content)))
        } else {
            Ok(())
        }
    }

    /// Create a directory
    pub async fn create_directory(&self, path: &str) -> AcpResult<()> {
        let resolved_path = self.resolve_path(path)?;
        debug!("Creating directory: {}", resolved_path.display());

        // Validate path is within workspace
        self.validate_path(&resolved_path)?;

        // Use create_directory tool with security manager
        let call = ToolCall {
            id: "acp-mkdir".to_string(),
            name: "create_directory".to_string(),
            arguments: json!({
                "path": resolved_path.to_str().ok_or_else(|| {
                    AcpError::InvalidPath(resolved_path.display().to_string())
                })?
            }),
        };

        let mut security_manager = self.file_security.write().await;
        let result = crate::tools::create_directory::create_directory(&call, &mut *security_manager, self.yolo_mode).await?;

        if result.is_error {
            Err(AcpError::Agent(anyhow::anyhow!(result.content)))
        } else {
            Ok(())
        }
    }
}

/// File entry for directory listings
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub is_directory: bool,
    pub path: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::FileSecurity;

    #[test]
    fn test_resolve_path_absolute() {
        let security = Arc::new(RwLock::new(FileSecurityManager::new(FileSecurity::default())));
        let handler = FileSystemHandler::new(security, None, false);

        #[cfg(windows)]
        {
            let result = handler.resolve_path("C:\\absolute\\path").unwrap();
            assert_eq!(result, PathBuf::from("C:\\absolute\\path"));
        }

        #[cfg(not(windows))]
        {
            let result = handler.resolve_path("/absolute/path").unwrap();
            assert_eq!(result, PathBuf::from("/absolute/path"));
        }
    }

    #[test]
    fn test_resolve_path_relative_with_workspace() {
        let security = Arc::new(RwLock::new(FileSecurityManager::new(FileSecurity::default())));
        let mut handler = FileSystemHandler::new(security, None, false);
        handler.set_workspace_root(PathBuf::from("/workspace"));

        let result = handler.resolve_path("relative/path").unwrap();
        assert_eq!(result, PathBuf::from("/workspace/relative/path"));
    }
}
