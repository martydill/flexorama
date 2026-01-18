use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub(crate) struct FileAutocompleteQuery {
    prefix: String,
}

#[derive(Serialize)]
struct FileAutocompleteResponse {
    files: Vec<FileAutocompleteItem>,
}

#[derive(Serialize)]
struct FileAutocompleteItem {
    path: String,
    is_directory: bool,
}

pub async fn get_file_autocomplete(
    axum::extract::Query(params): axum::extract::Query<FileAutocompleteQuery>,
) -> impl IntoResponse {
    use std::path::{Component, Path, PathBuf};

    let prefix = params.prefix.trim();
    let root = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(_) => {
            return Json(FileAutocompleteResponse { files: Vec::new() }).into_response();
        }
    };

    fn resolve_search_dir(root: &Path, dir_part: &str) -> Option<PathBuf> {
        if dir_part.is_empty() || dir_part == "." {
            return Some(root.to_path_buf());
        }

        let mut resolved = root.to_path_buf();
        let base_depth = resolved.components().count();
        let rel_path = Path::new(dir_part);

        for component in rel_path.components() {
            match component {
                Component::CurDir => {}
                Component::Normal(part) => resolved.push(part),
                Component::ParentDir => {
                    if resolved.components().count() <= base_depth {
                        return None;
                    }
                    resolved.pop();
                }
                Component::RootDir | Component::Prefix(_) => return None,
            }
        }

        Some(resolved)
    }

    // Determine the search directory and filter pattern
    let (search_dir, filter_prefix) = if prefix.is_empty() {
        (".", "")
    } else if prefix.ends_with('/') || prefix.ends_with('\\') {
        // If prefix ends with separator, list contents of that directory
        (prefix, "")
    } else {
        // Split the prefix into directory and filename parts
        let path = Path::new(prefix);
        if let Some(parent) = path.parent() {
            let parent_str = if parent.as_os_str().is_empty() {
                "."
            } else {
                parent.to_str().unwrap_or(".")
            };
            let file_part = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
            (parent_str, file_part)
        } else {
            (".", prefix)
        }
    };

    let search_dir_path = match resolve_search_dir(&root, search_dir) {
        Some(path) => path,
        None => {
            return Json(FileAutocompleteResponse { files: Vec::new() }).into_response();
        }
    };

    let entries = match std::fs::read_dir(&search_dir_path) {
        Ok(entries) => entries,
        Err(_) => {
            return Json(FileAutocompleteResponse { files: Vec::new() }).into_response();
        }
    };

    let mut files: Vec<FileAutocompleteItem> = Vec::new();
    let filter_lower = filter_prefix.to_lowercase();

    for entry in entries {
        if let Ok(entry) = entry {
            let path = entry.path();
            let filename = path.file_name().and_then(|f| f.to_str()).unwrap_or("");

            if filter_prefix.is_empty() || filename.to_lowercase().starts_with(&filter_lower) {
                let relative_path = match path.strip_prefix(&root) {
                    Ok(rel_path) => rel_path,
                    Err(_) => continue,
                };

                files.push(FileAutocompleteItem {
                    path: relative_path.to_string_lossy().to_string(),
                    is_directory: path.is_dir(),
                });

                if files.len() >= 50 {
                    break;
                }
            }
        }
    }

    files.sort_by(|a, b| match (a.is_directory, b.is_directory) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.path.to_lowercase().cmp(&b.path.to_lowercase()),
    });

    Json(FileAutocompleteResponse { files }).into_response()
}
