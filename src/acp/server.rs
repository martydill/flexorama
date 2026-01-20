use crate::acp::errors::AcpResult;
use crate::acp::handler::FlexoramaAcpHandler;
use crate::acp::transport::StdioTransport;
use crate::acp::types::{JsonRpcMessage, JsonRpcNotification, JsonRpcRequest};
use crate::agent::Agent;
use crate::config::Config;
use log::{debug, error, info, warn};

/// Run the ACP server
pub async fn run_acp_server(
    agent: Agent,
    config: Config,
    model: String,
    debug: bool,
) -> AcpResult<()> {
    info!("Starting ACP server (debug: {})", debug);
    info!("Model: {}", model);

    let mut transport = StdioTransport::new(debug);
    let mut handler = FlexoramaAcpHandler::new(agent, config, model, debug);

    info!("ACP server ready, waiting for messages...");

    loop {
        // Read message from stdin
        let message = match transport.read_message().await {
            Ok(msg) => msg,
            Err(e) => {
                if transport.is_closed().await {
                    info!("Client disconnected, shutting down");
                    break;
                }
                error!("Failed to read message: {}", e);
                continue;
            }
        };

        // Handle message
        match message {
            JsonRpcMessage::Request(request) => {
                debug!("Processing request: {}", request.method);

                // Handle exit specially
                if request.method == "exit" {
                    info!("Received exit request, shutting down");
                    break;
                }

                let response = handler.handle_request(request).await;

                // Send response
                if let Err(e) = transport.write_message(&JsonRpcMessage::Response(response)).await {
                    error!("Failed to write response: {}", e);
                    break;
                }
            }

            JsonRpcMessage::Notification(notification) => {
                debug!("Received notification: {}", notification.method);

                // Convert notification to request with null id for handling
                let request = JsonRpcRequest {
                    jsonrpc: notification.jsonrpc,
                    id: None,
                    method: notification.method,
                    params: notification.params,
                };

                // Handle notification (no response needed)
                let _result = handler.handle_request(request).await;
                // Don't send response for notifications
            }

            JsonRpcMessage::Response(response) => {
                warn!("Received unexpected response message: {:?}", response.id);
                // Servers shouldn't receive responses, only send them
            }
        }
    }

    info!("ACP server shut down");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_module_compiles() {
        // Basic compilation test
        assert!(true);
    }
}
