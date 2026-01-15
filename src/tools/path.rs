use anyhow::{bail, Result};
use path_absolutize::*;
use std::path::{Component, Path, PathBuf};

pub fn resolve_project_path(path: &str) -> Result<PathBuf> {
    let expanded_path = shellexpand::tilde(path);
    let raw_path = Path::new(expanded_path.as_ref());

    if raw_path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("Path traversal ('..') is not allowed: {}", path);
    }

    let current_dir = std::env::current_dir()?;
    let project_root = current_dir.absolutize()?.to_path_buf();
    let absolute_path = if raw_path.is_absolute() {
        raw_path.absolutize()?.to_path_buf()
    } else {
        project_root.join(raw_path).absolutize()?.to_path_buf()
    };

    if !absolute_path.starts_with(&project_root) {
        bail!(
            "Path must be within the project directory: {}",
            project_root.display()
        );
    }

    Ok(absolute_path.to_path_buf())
}
