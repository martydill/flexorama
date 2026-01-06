use crate::security::{FileSecurity, FileSecurityManager};
use crate::tools::create_directory::create_directory;
use crate::tools::delete_file::delete_file;
use crate::tools::glob::glob_files;
use crate::tools::list_directory::list_directory;
use crate::tools::read_file::read_file;
use crate::tools::search_in_files::search_in_files;
use crate::tools::types::ToolCall;
use crate::tools::write_file::write_file;
use serde_json::json;
use tempfile::TempDir;

fn make_call(name: &str, arguments: serde_json::Value) -> ToolCall {
    ToolCall {
        id: "test-id".to_string(),
        name: name.to_string(),
        arguments,
    }
}

fn temp_dir() -> TempDir {
    tempfile::tempdir().expect("temp dir")
}

fn new_file_security_manager() -> FileSecurityManager {
    FileSecurityManager::new(FileSecurity {
        ask_for_permission: false,
        enabled: true,
        allow_all_session: false,
    })
}

#[tokio::test]
async fn read_file_success_and_error() {
    let temp = temp_dir();
    let file_path = temp.path().join("example.txt");
    tokio::fs::write(&file_path, "hello world").await.unwrap();

    let call = make_call("read_file", json!({ "path": file_path.to_string_lossy() }));
    let result = read_file(&call).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("File:"));
    assert!(result.content.contains("hello world"));

    let missing_path = temp.path().join("missing.txt");
    let call = make_call(
        "read_file",
        json!({ "path": missing_path.to_string_lossy() }),
    );
    let result = read_file(&call).await.unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("Error opening file"));
}

#[tokio::test]
async fn write_file_success_and_error() {
    let temp = temp_dir();
    let file_path = temp.path().join("output.txt");
    let mut file_security_manager = new_file_security_manager();

    let call = make_call(
        "write_file",
        json!({
            "path": file_path.to_string_lossy(),
            "content": "file contents",
        }),
    );
    let result = write_file(&call, &mut file_security_manager, false)
        .await
        .unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("Successfully wrote to file"));
    let contents = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(contents, "file contents");

    let mut file_security_manager = new_file_security_manager();
    let call = make_call(
        "write_file",
        json!({
            "path": temp.path().to_string_lossy(),
            "content": "should fail",
        }),
    );
    let result = write_file(&call, &mut file_security_manager, false)
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("Error writing to file"));
}

#[tokio::test]
async fn create_directory_success_and_error() {
    let temp = temp_dir();
    let dir_path = temp.path().join("nested");
    let mut file_security_manager = new_file_security_manager();

    let call = make_call(
        "create_directory",
        json!({ "path": dir_path.to_string_lossy() }),
    );
    let result = create_directory(&call, &mut file_security_manager, false)
        .await
        .unwrap();
    assert!(!result.is_error);
    assert!(dir_path.is_dir());

    let file_path = temp.path().join("occupied");
    tokio::fs::write(&file_path, "data").await.unwrap();
    let mut file_security_manager = new_file_security_manager();
    let call = make_call(
        "create_directory",
        json!({ "path": file_path.to_string_lossy() }),
    );
    let result = create_directory(&call, &mut file_security_manager, false)
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("Error creating directory"));
}

#[tokio::test]
async fn delete_file_success_and_error() {
    let temp = temp_dir();
    let file_path = temp.path().join("delete-me.txt");
    tokio::fs::write(&file_path, "remove").await.unwrap();
    let mut file_security_manager = new_file_security_manager();

    let call = make_call(
        "delete_file",
        json!({ "path": file_path.to_string_lossy() }),
    );
    let result = delete_file(&call, &mut file_security_manager, false)
        .await
        .unwrap();
    assert!(!result.is_error);
    assert!(!file_path.exists());

    let missing_path = temp.path().join("missing.txt");
    let mut file_security_manager = new_file_security_manager();
    let call = make_call(
        "delete_file",
        json!({ "path": missing_path.to_string_lossy() }),
    );
    let result = delete_file(&call, &mut file_security_manager, false)
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("Error accessing path"));
}

#[tokio::test]
async fn list_directory_formatting_and_error() {
    let temp = temp_dir();
    let subdir_path = temp.path().join("subdir");
    tokio::fs::create_dir_all(&subdir_path).await.unwrap();
    let file_path = temp.path().join("notes.txt");
    tokio::fs::write(&file_path, "abc").await.unwrap();

    let call = make_call(
        "list_directory",
        json!({ "path": temp.path().to_string_lossy() }),
    );
    let result = list_directory(&call).await.unwrap();
    assert!(!result.is_error);
    let expected_header = format!("Contents of '{}':", temp.path().display());
    assert!(result.content.contains(&expected_header));
    assert!(result.content.contains("📁 subdir/"));
    assert!(result.content.contains("📄 notes.txt (3 bytes)"));

    let missing_path = temp.path().join("missing-dir");
    let call = make_call(
        "list_directory",
        json!({ "path": missing_path.to_string_lossy() }),
    );
    let result = list_directory(&call).await.unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("Error reading directory"));
}

#[tokio::test]
async fn glob_and_search_in_files_match_expected_entries() {
    let temp = temp_dir();
    let subdir_path = temp.path().join("sub");
    tokio::fs::create_dir_all(&subdir_path).await.unwrap();

    let alpha_path = temp.path().join("alpha.txt");
    let beta_path = temp.path().join("beta.log");
    let gamma_path = subdir_path.join("gamma.txt");
    tokio::fs::write(&alpha_path, "needle one").await.unwrap();
    tokio::fs::write(&beta_path, "no match").await.unwrap();
    tokio::fs::write(&gamma_path, "needle two").await.unwrap();

    let call = make_call(
        "glob",
        json!({
            "pattern": "*.txt",
            "base_path": temp.path().to_string_lossy(),
        }),
    );
    let result = glob_files(&call).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("Files matching pattern '*.txt'"));
    assert!(result
        .content
        .contains(alpha_path.to_string_lossy().as_ref()));
    assert!(!result
        .content
        .contains(beta_path.to_string_lossy().as_ref()));

    let call = make_call(
        "search_in_files",
        json!({
            "path": temp.path().to_string_lossy(),
            "query": "needle",
        }),
    );
    let result = search_in_files(&call).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("Found 2 matches for 'needle'"));
    assert!(result
        .content
        .contains(alpha_path.to_string_lossy().as_ref()));
    assert!(result
        .content
        .contains(gamma_path.to_string_lossy().as_ref()));
}
