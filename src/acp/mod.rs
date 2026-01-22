/// Agent Client Protocol (ACP) implementation for Flexorama
///
/// This module provides a JSON-RPC 2.0 over stdio server that implements
/// the Agent Client Protocol, allowing code editors to communicate with
/// Flexorama as an AI coding agent.
///
/// # Architecture
///
/// - `server`: Main server loop handling stdio communication
/// - `handler`: Request handling and agent operations
/// - `transport`: Stdio transport layer for JSON-RPC messages
/// - `capabilities`: Capability negotiation with clients
/// - `types`: JSON-RPC and ACP type definitions
/// - `errors`: Error types and conversions
///
/// # Usage
///
/// ```bash
/// flexorama --acp
/// ```

pub mod capabilities;
pub mod errors;
pub mod filesystem;
pub mod handler;
pub mod server;
pub mod session;
pub mod transport;
pub mod types;

// Re-export main entry point
pub use server::run_acp_server;
