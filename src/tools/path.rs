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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_project_path_rejects_parent_traversal() {
        let result = resolve_project_path("../outside");
        assert!(result.is_err());
    }

    #[test]
    fn resolve_project_path_rejects_absolute_outside_project() {
        let outside_path = std::env::temp_dir().join("flexorama-outside");
        let result = resolve_project_path(&outside_path.to_string_lossy());
        assert!(result.is_err());
    }

    #[test]
    fn resolve_project_path_accepts_relative_inside_project() {
        let result = resolve_project_path("src");
        assert!(result.is_ok());
        let resolved = result.expect("resolved path");
        assert!(resolved.ends_with("src"));
    }

    #[test]
    fn resolve_project_path_accepts_absolute_inside_project() {
        let project_root = std::env::current_dir().expect("current dir");
        let absolute = project_root.join("Cargo.toml");
        let result = resolve_project_path(&absolute.to_string_lossy());
        assert!(result.is_ok());
    }
}
