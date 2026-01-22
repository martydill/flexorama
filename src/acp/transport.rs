use crate::acp::errors::{AcpError, AcpResult};
use crate::acp::types::JsonRpcMessage;
use log::{debug, error, trace};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Stdio transport for JSON-RPC 2.0 messages
pub struct StdioTransport {
    stdin: BufReader<tokio::io::Stdin>,
    stdout: tokio::io::Stdout,
    debug: bool,
}

impl StdioTransport {
    pub fn new(debug: bool) -> Self {
        Self {
            stdin: BufReader::new(tokio::io::stdin()),
            stdout: tokio::io::stdout(),
            debug,
        }
    }

    /// Read a single JSON-RPC message from stdin
    /// Messages are newline-delimited JSON
    pub async fn read_message(&mut self) -> AcpResult<JsonRpcMessage> {
        let mut line = String::new();

        match self.stdin.read_line(&mut line).await {
            Ok(0) => {
                debug!("EOF on stdin, client disconnected");
                return Err(AcpError::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Client disconnected",
                )));
            }
            Ok(n) => {
                trace!("Read {} bytes from stdin", n);
            }
            Err(e) => {
                error!("Failed to read from stdin: {}", e);
                return Err(AcpError::Io(e));
            }
        }

        let line = line.trim();
        if line.is_empty() {
            return Err(AcpError::InvalidMessage("Empty message".to_string()));
        }

        if self.debug {
            eprintln!("[ACP RX] {}", line);
        }

        match serde_json::from_str(line) {
            Ok(msg) => Ok(msg),
            Err(e) => {
                error!("Failed to parse JSON-RPC message: {}", e);
                Err(AcpError::InvalidMessage(format!("Invalid JSON: {}", e)))
            }
        }
    }

    /// Write a JSON-RPC message to stdout
    /// Each message is followed by a newline
    pub async fn write_message(&mut self, msg: &JsonRpcMessage) -> AcpResult<()> {
        let json = serde_json::to_string(msg).map_err(|e| AcpError::Json(e))?;

        if self.debug {
            eprintln!("[ACP TX] {}", json);
        }

        self.stdout
            .write_all(json.as_bytes())
            .await
            .map_err(|e| AcpError::Io(e))?;

        self.stdout
            .write_all(b"\n")
            .await
            .map_err(|e| AcpError::Io(e))?;

        self.stdout.flush().await.map_err(|e| AcpError::Io(e))?;

        trace!("Wrote {} bytes to stdout", json.len() + 1);

        Ok(())
    }

    /// Check if stdin is closed
    pub async fn is_closed(&mut self) -> bool {
        // Try to peek at the next byte without consuming it
        match self.stdin.fill_buf().await {
            Ok(buf) => buf.is_empty(),
            Err(_) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::types::{JsonRpcRequest, JsonRpcResponse};
    use serde_json::json;

    #[test]
    fn test_message_serialization() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "initialize".to_string(),
            params: Some(json!({"workspaceRoot": "/path/to/workspace"})),
        };

        let msg = JsonRpcMessage::Request(request);
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"initialize\""));
    }

    #[test]
    fn test_response_serialization() {
        let response = JsonRpcResponse::success(Some(json!(1)), json!({"capabilities": {}}));

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"result\""));
    }
}
