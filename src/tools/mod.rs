// Tools module - re-exports all tool functionality

pub mod bash;
pub mod builtin;
pub mod create_directory;
pub mod delete_file;
pub mod edit_file;
pub mod glob;
pub mod list_directory;
pub mod mcp;
pub mod read_file;
pub mod search_in_files;
pub mod types;
pub mod write_file;

// New display system modules
pub mod display;
pub mod registry;

// Re-export all public types and functions for backward compatibility
pub use builtin::*;
pub use mcp::*;
pub use types::*;

// Re-export new display system
pub use registry::*;

// Re-export tool creation functions for security manager integration
pub use bash::{bash, create_bash_tool};
pub use create_directory::{create_create_directory_tool, create_directory};
pub use delete_file::{create_delete_file_tool, delete_file};
pub use edit_file::{create_edit_file_tool, edit_file};
pub use write_file::{create_write_file_tool, write_file};
