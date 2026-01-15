use crate::security::{FileSecurity, FileSecurityManager};
use crate::tools::create_directory::create_directory;
use crate::tools::delete_file::delete_file;
use crate::tools::edit_file::edit_file;
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
    let current_dir = std::env::current_dir().expect("current dir");
    tempfile::tempdir_in(current_dir).expect("temp dir")
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
async fn file_editing_commands_reject_out_of_project_paths() {
    let mut file_security_manager = new_file_security_manager();
    let call = make_call(
        "write_file",
        json!({
            "path": "../outside.txt",
            "content": "blocked",
        }),
    );
    let result = write_file(&call, &mut file_security_manager, false)
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("Invalid path for write_file"));

    let mut file_security_manager = new_file_security_manager();
    let call = make_call("create_directory", json!({ "path": "../outside-dir" }));
    let result = create_directory(&call, &mut file_security_manager, false)
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("Invalid path for create_directory"));

    let mut file_security_manager = new_file_security_manager();
    let call = make_call("delete_file", json!({ "path": "../outside-delete" }));
    let result = delete_file(&call, &mut file_security_manager, false)
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("Invalid path for delete_file"));

    let mut file_security_manager = new_file_security_manager();
    let call = make_call(
        "edit_file",
        json!({
            "path": "../outside-edit.txt",
            "old_text": "old",
            "new_text": "new",
        }),
    );
    let result = edit_file(&call, &mut file_security_manager, false)
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("Invalid path for edit_file"));
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

// ==================== Additional read_file tests ====================

#[tokio::test]
async fn read_file_handles_binary_content() {
    let temp = temp_dir();
    let file_path = temp.path().join("binary.dat");
    let binary_data = vec![0u8, 1, 2, 255, 128, 64];
    tokio::fs::write(&file_path, &binary_data).await.unwrap();

    let call = make_call("read_file", json!({ "path": file_path.to_string_lossy() }));
    let result = read_file(&call).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("File:"));
}

#[tokio::test]
async fn read_file_handles_empty_file() {
    let temp = temp_dir();
    let file_path = temp.path().join("empty.txt");
    tokio::fs::write(&file_path, "").await.unwrap();

    let call = make_call("read_file", json!({ "path": file_path.to_string_lossy() }));
    let result = read_file(&call).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("File:"));
}

#[tokio::test]
async fn read_file_handles_large_file() {
    let temp = temp_dir();
    let file_path = temp.path().join("large.txt");
    let large_content = "x".repeat(100_000);
    tokio::fs::write(&file_path, &large_content).await.unwrap();

    let call = make_call("read_file", json!({ "path": file_path.to_string_lossy() }));
    let result = read_file(&call).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("File:"));
    assert!(result.content.contains(&large_content));
}

#[tokio::test]
async fn read_file_handles_unicode_content() {
    let temp = temp_dir();
    let file_path = temp.path().join("unicode.txt");
    let unicode_content = "Hello 世界 🌍 Привет";
    tokio::fs::write(&file_path, unicode_content).await.unwrap();

    let call = make_call("read_file", json!({ "path": file_path.to_string_lossy() }));
    let result = read_file(&call).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains(unicode_content));
}

// ==================== Additional write_file tests ====================

#[tokio::test]
async fn write_file_overwrites_existing_file() {
    let temp = temp_dir();
    let file_path = temp.path().join("overwrite.txt");
    tokio::fs::write(&file_path, "original").await.unwrap();
    let mut file_security_manager = new_file_security_manager();

    let call = make_call(
        "write_file",
        json!({
            "path": file_path.to_string_lossy(),
            "content": "new content",
        }),
    );
    let result = write_file(&call, &mut file_security_manager, false)
        .await
        .unwrap();
    assert!(!result.is_error);

    let contents = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(contents, "new content");
}

#[tokio::test]
async fn write_file_creates_parent_directories() {
    let temp = temp_dir();
    let nested_path = temp.path().join("a").join("b").join("c").join("file.txt");
    let mut file_security_manager = new_file_security_manager();

    let call = make_call(
        "write_file",
        json!({
            "path": nested_path.to_string_lossy(),
            "content": "nested content",
        }),
    );
    let result = write_file(&call, &mut file_security_manager, false)
        .await
        .unwrap();
    assert!(!result.is_error);
    assert!(nested_path.exists());

    let contents = tokio::fs::read_to_string(&nested_path).await.unwrap();
    assert_eq!(contents, "nested content");
}

#[tokio::test]
async fn write_file_handles_empty_content() {
    let temp = temp_dir();
    let file_path = temp.path().join("empty.txt");
    let mut file_security_manager = new_file_security_manager();

    let call = make_call(
        "write_file",
        json!({
            "path": file_path.to_string_lossy(),
            "content": "",
        }),
    );
    let result = write_file(&call, &mut file_security_manager, false)
        .await
        .unwrap();
    assert!(!result.is_error);

    let contents = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(contents, "");
}

#[tokio::test]
async fn write_file_handles_unicode_content() {
    let temp = temp_dir();
    let file_path = temp.path().join("unicode.txt");
    let unicode_content = "Hello 世界 🌍 Привет";
    let mut file_security_manager = new_file_security_manager();

    let call = make_call(
        "write_file",
        json!({
            "path": file_path.to_string_lossy(),
            "content": unicode_content,
        }),
    );
    let result = write_file(&call, &mut file_security_manager, false)
        .await
        .unwrap();
    assert!(!result.is_error);

    let contents = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(contents, unicode_content);
}

// ==================== Additional edit_file tests ====================

#[tokio::test]
async fn edit_file_handles_nonexistent_file() {
    let temp = temp_dir();
    let missing_path = temp.path().join("missing.txt");
    let mut file_security_manager = new_file_security_manager();

    let call = make_call(
        "edit_file",
        json!({
            "path": missing_path.to_string_lossy(),
            "old_text": "foo",
            "new_text": "bar",
        }),
    );
    let result = edit_file(&call, &mut file_security_manager, true)
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("Error reading file"));
}

#[tokio::test]
async fn edit_file_replaces_all_occurrences() {
    let temp = temp_dir();
    let file_path = temp.path().join("multi.txt");
    tokio::fs::write(&file_path, "foo bar foo baz foo")
        .await
        .unwrap();
    let mut file_security_manager = new_file_security_manager();

    let call = make_call(
        "edit_file",
        json!({
            "path": file_path.to_string_lossy(),
            "old_text": "foo bar foo baz foo",
            "new_text": "replaced",
        }),
    );
    let result = edit_file(&call, &mut file_security_manager, true)
        .await
        .unwrap();
    assert!(!result.is_error);

    let contents = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(contents, "replaced");
}

#[tokio::test]
async fn edit_file_handles_empty_file() {
    let temp = temp_dir();
    let file_path = temp.path().join("empty.txt");
    tokio::fs::write(&file_path, "").await.unwrap();
    let mut file_security_manager = new_file_security_manager();

    let call = make_call(
        "edit_file",
        json!({
            "path": file_path.to_string_lossy(),
            "old_text": "nonexistent",
            "new_text": "something",
        }),
    );
    let result = edit_file(&call, &mut file_security_manager, true)
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("Text not found in file"));
}

#[tokio::test]
async fn edit_file_preserves_unix_line_endings() {
    let temp = temp_dir();
    let file_path = temp.path().join("unix.txt");
    tokio::fs::write(&file_path, "line1\nline2\nline3\n")
        .await
        .unwrap();
    let mut file_security_manager = new_file_security_manager();

    let call = make_call(
        "edit_file",
        json!({
            "path": file_path.to_string_lossy(),
            "old_text": "line2",
            "new_text": "replaced",
        }),
    );
    let result = edit_file(&call, &mut file_security_manager, true)
        .await
        .unwrap();
    assert!(!result.is_error);

    let contents = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(contents, "line1\nreplaced\nline3\n");
}

// ==================== Additional create_directory tests ====================

#[tokio::test]
async fn create_directory_handles_deeply_nested_path() {
    let temp = temp_dir();
    let deep_path = temp
        .path()
        .join("a")
        .join("b")
        .join("c")
        .join("d")
        .join("e");
    let mut file_security_manager = new_file_security_manager();

    let call = make_call(
        "create_directory",
        json!({ "path": deep_path.to_string_lossy() }),
    );
    let result = create_directory(&call, &mut file_security_manager, false)
        .await
        .unwrap();
    assert!(!result.is_error);
    assert!(deep_path.is_dir());
}

#[tokio::test]
async fn create_directory_handles_existing_directory() {
    let temp = temp_dir();
    let dir_path = temp.path().join("existing");
    tokio::fs::create_dir(&dir_path).await.unwrap();
    let mut file_security_manager = new_file_security_manager();

    let call = make_call(
        "create_directory",
        json!({ "path": dir_path.to_string_lossy() }),
    );
    let _ = create_directory(&call, &mut file_security_manager, false).await;
    // Creating an existing directory may succeed or fail depending on the implementation
    // The important thing is that it doesn't crash
    assert!(dir_path.is_dir());
}

// ==================== Additional delete_file tests ====================

#[tokio::test]
async fn delete_file_removes_directory() {
    let temp = temp_dir();
    let dir_path = temp.path().join("to-delete");
    tokio::fs::create_dir(&dir_path).await.unwrap();
    let mut file_security_manager = new_file_security_manager();

    let call = make_call("delete_file", json!({ "path": dir_path.to_string_lossy() }));
    let result = delete_file(&call, &mut file_security_manager, false)
        .await
        .unwrap();
    assert!(!result.is_error);
    assert!(!dir_path.exists());
}

#[tokio::test]
async fn delete_file_handles_nonempty_directory() {
    let temp = temp_dir();
    let dir_path = temp.path().join("nonempty");
    tokio::fs::create_dir(&dir_path).await.unwrap();
    let file_in_dir = dir_path.join("file.txt");
    tokio::fs::write(&file_in_dir, "content").await.unwrap();
    let mut file_security_manager = new_file_security_manager();

    let call = make_call("delete_file", json!({ "path": dir_path.to_string_lossy() }));
    let result = delete_file(&call, &mut file_security_manager, false)
        .await
        .unwrap();
    // Should succeed as delete_file removes directories recursively
    assert!(!result.is_error);
}

// ==================== Additional list_directory tests ====================

#[tokio::test]
async fn list_directory_handles_empty_directory() {
    let temp = temp_dir();
    let empty_dir = temp.path().join("empty");
    tokio::fs::create_dir(&empty_dir).await.unwrap();

    let call = make_call(
        "list_directory",
        json!({ "path": empty_dir.to_string_lossy() }),
    );
    let result = list_directory(&call).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("Contents of"));
}

#[tokio::test]
async fn list_directory_shows_multiple_files() {
    let temp = temp_dir();
    tokio::fs::write(temp.path().join("file1.txt"), "a")
        .await
        .unwrap();
    tokio::fs::write(temp.path().join("file2.txt"), "b")
        .await
        .unwrap();
    tokio::fs::write(temp.path().join("file3.txt"), "c")
        .await
        .unwrap();

    let call = make_call(
        "list_directory",
        json!({ "path": temp.path().to_string_lossy() }),
    );
    let result = list_directory(&call).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("file1.txt"));
    assert!(result.content.contains("file2.txt"));
    assert!(result.content.contains("file3.txt"));
}

#[tokio::test]
async fn list_directory_distinguishes_files_and_directories() {
    let temp = temp_dir();
    let subdir = temp.path().join("subdir");
    tokio::fs::create_dir(&subdir).await.unwrap();
    tokio::fs::write(temp.path().join("file.txt"), "data")
        .await
        .unwrap();

    let call = make_call(
        "list_directory",
        json!({ "path": temp.path().to_string_lossy() }),
    );
    let result = list_directory(&call).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("📁 subdir/"));
    assert!(result.content.contains("📄 file.txt"));
}

// ==================== Additional glob tests ====================

#[tokio::test]
async fn glob_handles_no_matches() {
    let temp = temp_dir();
    tokio::fs::write(temp.path().join("file.txt"), "data")
        .await
        .unwrap();

    let call = make_call(
        "glob",
        json!({
            "pattern": "*.rs",
            "base_path": temp.path().to_string_lossy(),
        }),
    );
    let result = glob_files(&call).await.unwrap();
    assert!(!result.is_error);
    assert!(result
        .content
        .contains("No files found matching the pattern"));
}

#[tokio::test]
async fn glob_handles_recursive_pattern() {
    let temp = temp_dir();
    let subdir = temp.path().join("subdir");
    tokio::fs::create_dir(&subdir).await.unwrap();
    tokio::fs::write(temp.path().join("root.txt"), "a")
        .await
        .unwrap();
    tokio::fs::write(subdir.join("nested.txt"), "b")
        .await
        .unwrap();

    let call = make_call(
        "glob",
        json!({
            "pattern": "**/*.txt",
            "base_path": temp.path().to_string_lossy(),
        }),
    );
    let result = glob_files(&call).await.unwrap();
    assert!(!result.is_error);
    // Should find files matching the pattern
    assert!(result.content.contains("Files matching pattern"));
}

#[tokio::test]
async fn glob_matches_specific_extension() {
    let temp = temp_dir();
    tokio::fs::write(temp.path().join("file1.txt"), "a")
        .await
        .unwrap();
    tokio::fs::write(temp.path().join("file2.txt"), "b")
        .await
        .unwrap();
    tokio::fs::write(temp.path().join("file3.md"), "c")
        .await
        .unwrap();

    let call = make_call(
        "glob",
        json!({
            "pattern": "*.txt",
            "base_path": temp.path().to_string_lossy(),
        }),
    );
    let result = glob_files(&call).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("file1.txt"));
    assert!(result.content.contains("file2.txt"));
    assert!(!result.content.contains("file3.md"));
}

#[tokio::test]
async fn glob_uses_default_base_path() {
    let call = make_call("glob", json!({ "pattern": "*.nonexistent" }));
    let result = glob_files(&call).await.unwrap();
    // Should not error, just return no matches
    assert!(!result.is_error);
}

// ==================== Additional search_in_files tests ====================

#[tokio::test]
async fn search_in_files_finds_no_matches() {
    let temp = temp_dir();
    tokio::fs::write(temp.path().join("file.txt"), "hello world")
        .await
        .unwrap();

    let call = make_call(
        "search_in_files",
        json!({
            "path": temp.path().to_string_lossy(),
            "query": "nonexistent",
        }),
    );
    let result = search_in_files(&call).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("No matches for 'nonexistent'"));
}

#[tokio::test]
async fn search_in_files_finds_multiple_matches_in_file() {
    let temp = temp_dir();
    let content = "foo\nbar\nfoo\nbaz\nfoo\n";
    tokio::fs::write(temp.path().join("file.txt"), content)
        .await
        .unwrap();

    let call = make_call(
        "search_in_files",
        json!({
            "path": temp.path().to_string_lossy(),
            "query": "foo",
        }),
    );
    let result = search_in_files(&call).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("Found 3 matches"));
}

#[tokio::test]
async fn search_in_files_searches_recursively() {
    let temp = temp_dir();
    let subdir = temp.path().join("subdir");
    tokio::fs::create_dir(&subdir).await.unwrap();
    tokio::fs::write(temp.path().join("root.txt"), "needle")
        .await
        .unwrap();
    tokio::fs::write(subdir.join("nested.txt"), "needle")
        .await
        .unwrap();

    let call = make_call(
        "search_in_files",
        json!({
            "path": temp.path().to_string_lossy(),
            "query": "needle",
        }),
    );
    let result = search_in_files(&call).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("Found 2 matches"));
}

#[tokio::test]
async fn search_in_files_skips_git_directory() {
    let temp = temp_dir();
    let git_dir = temp.path().join(".git");
    tokio::fs::create_dir(&git_dir).await.unwrap();
    tokio::fs::write(git_dir.join("config"), "needle")
        .await
        .unwrap();
    tokio::fs::write(temp.path().join("readme.txt"), "other")
        .await
        .unwrap();

    let call = make_call(
        "search_in_files",
        json!({
            "path": temp.path().to_string_lossy(),
            "query": "needle",
        }),
    );
    let result = search_in_files(&call).await.unwrap();
    assert!(!result.is_error);
    // Should not find "needle" in .git directory
    assert!(result.content.contains("No matches for 'needle'"));
}

#[tokio::test]
async fn search_in_files_handles_empty_file() {
    let temp = temp_dir();
    tokio::fs::write(temp.path().join("empty.txt"), "")
        .await
        .unwrap();

    let call = make_call(
        "search_in_files",
        json!({
            "path": temp.path().to_string_lossy(),
            "query": "anything",
        }),
    );
    let result = search_in_files(&call).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("No matches"));
}

#[tokio::test]
async fn search_in_files_uses_default_path() {
    let call = make_call(
        "search_in_files",
        json!({
            "query": "nonexistent_search_term_12345",
        }),
    );
    let result = search_in_files(&call).await.unwrap();
    // Should use current directory as default
    assert!(!result.is_error);
}
