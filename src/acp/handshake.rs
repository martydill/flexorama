use serde::{Deserialize, Serialize};
use serde_json::Value;

/// ACP Protocol handshake - sent by client on connection
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpHandshakeRequest {
    pub protocol_version: u32,
    pub client_capabilities: ClientCapabilities,
    pub client_info: ClientInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fs: Option<FsCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<bool>,
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_text_file: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_text_file: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub title: String,
    pub version: String,
}

/// ACP Protocol handshake response - sent by server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpHandshakeResponse {
    pub protocol_version: u32,
    pub server_capabilities: ServerCapabilities,
    pub server_info: ServerInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fs: Option<FsServerCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsServerCapabilities {
    pub read_text_file: bool,
    pub write_text_file: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

impl AcpHandshakeResponse {
    pub fn new(request: &AcpHandshakeRequest, plan_mode: bool) -> Self {
        Self {
            protocol_version: request.protocol_version,
            server_capabilities: ServerCapabilities {
                fs: Some(FsServerCapabilities {
                    read_text_file: true,
                    write_text_file: !plan_mode,
                }),
                terminal: Some(!plan_mode),
            },
            server_info: ServerInfo {
                name: "Flexorama".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handshake_request_deserialization() {
        let json = r#"{
            "protocolVersion": 1,
            "clientCapabilities": {
                "fs": {
                    "readTextFile": true,
                    "writeTextFile": true
                },
                "terminal": true
            },
            "clientInfo": {
                "name": "zed",
                "title": "Zed",
                "version": "0.219.5"
            }
        }"#;

        let request: AcpHandshakeRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.protocol_version, 1);
        assert_eq!(request.client_info.name, "zed");
        assert!(request.client_capabilities.fs.is_some());
    }

    #[test]
    fn test_handshake_response_serialization() {
        let request = AcpHandshakeRequest {
            protocol_version: 1,
            client_capabilities: ClientCapabilities {
                fs: Some(FsCapabilities {
                    read_text_file: Some(true),
                    write_text_file: Some(true),
                }),
                terminal: Some(true),
                meta: None,
            },
            client_info: ClientInfo {
                name: "test".to_string(),
                title: "Test".to_string(),
                version: "1.0".to_string(),
            },
        };

        let response = AcpHandshakeResponse::new(&request, false);
        assert_eq!(response.protocol_version, 1);
        assert_eq!(response.server_info.name, "Flexorama");

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("protocolVersion"));
        assert!(json.contains("serverCapabilities"));
    }

    #[test]
    fn test_handshake_response_plan_mode() {
        let request = AcpHandshakeRequest {
            protocol_version: 1,
            client_capabilities: ClientCapabilities {
                fs: None,
                terminal: None,
                meta: None,
            },
            client_info: ClientInfo {
                name: "test".to_string(),
                title: "Test".to_string(),
                version: "1.0".to_string(),
            },
        };

        let response = AcpHandshakeResponse::new(&request, true);
        let fs_caps = response.server_capabilities.fs.unwrap();
        assert!(fs_caps.read_text_file);
        assert!(!fs_caps.write_text_file); // Plan mode = read-only
        assert_eq!(response.server_capabilities.terminal, Some(false));
    }
}
