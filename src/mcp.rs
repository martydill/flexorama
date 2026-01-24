use anyhow::Result;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info, warn};
use rand::RngCore;
use reqwest::{header::HeaderMap, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command as TokioCommand};
use tokio::sync::{Mutex, RwLock};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::{connect_async, tungstenite::Message};

// Re-export from config module to maintain compatibility
pub use crate::config::{
    McpAuthConfig, McpConfig, McpOAuthClientAuth, McpOAuthConfig, McpOAuthGrantType,
    McpServerConfig,
};

// PKCE (Proof Key for Code Exchange) support for OAuth 2.0
fn generate_code_verifier() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn generate_code_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    URL_SAFE_NO_PAD.encode(hash)
}

fn generate_state() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Create a reqwest HTTP client with a proper User-Agent header.
/// Some services (like GoDaddy) block requests without a User-Agent.
fn create_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("flexorama/0.1.0")
        .build()
        .unwrap_or_else(|_| create_http_client())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(flatten)]
    pub method: McpMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum McpMethod {
    #[serde(rename = "initialize", rename_all = "camelCase")]
    Initialize {
        protocol_version: String,
        capabilities: McpClientCapabilities,
        client_info: McpClientInfo,
    },
    #[serde(rename = "tools/list")]
    ListTools,
    #[serde(rename = "tools/call", rename_all = "camelCase")]
    CallTool {
        name: String,
        arguments: Option<Value>,
    },
    #[serde(rename = "resources/list", rename_all = "camelCase")]
    ListResources {
        #[serde(skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
    },
    #[serde(rename = "resources/read", rename_all = "camelCase")]
    ReadResource { uri: String },
    #[serde(rename = "prompts/list", rename_all = "camelCase")]
    ListPrompts {
        #[serde(skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
    },
    #[serde(rename = "prompts/get", rename_all = "camelCase")]
    GetPrompt {
        name: String,
        arguments: Option<Value>,
    },
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "notifications/initialized")]
    Initialized,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpClientCapabilities {
    pub tools: Option<McpToolsCapability>,
    pub resources: Option<McpResourcesCapability>,
    pub prompts: Option<McpPromptsCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolsCapability {
    pub list_changed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResourcesCapability {
    pub list_changed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpPromptsCapability {
    pub list_changed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: Option<String>,
    #[serde(alias = "inputSchema", rename = "input_schema")]
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResource {
    pub uri: String,
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(alias = "mimeType", rename = "mime_type")]
    pub mime_type: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPrompt {
    pub name: String,
    pub description: Option<String>,
    pub arguments: Option<Value>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Debug, Clone)]
struct OAuthTokenCacheEntry {
    header_value: String,
    expires_at: Option<Instant>,
}

async fn handle_mcp_response(
    name: &str,
    response: McpResponse,
    pending_requests: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<McpResponse>>>>,
    tools: Arc<RwLock<Vec<McpTool>>>,
    tools_version: Arc<RwLock<u64>>,
) {
    if let Some(id) = &response.id {
        let mut pending = pending_requests.lock().await;
        if let Some(sender) = pending.remove(id) {
            let _ = sender.send(response);
        }
    } else if let Some(result) = &response.result {
        if let Some(tools_list) = result.get("tools") {
            debug!(
                "Received tools via notification from {}: {}",
                name,
                serde_json::to_string_pretty(tools_list)
                    .unwrap_or_else(|_| "Invalid JSON".to_string())
            );

            match serde_json::from_value::<Vec<Value>>(tools_list.clone()) {
                Ok(raw_tools) => {
                    let mut parsed_tools = Vec::new();
                    for (i, raw_tool) in raw_tools.into_iter().enumerate() {
                        match serde_json::from_value::<McpTool>(raw_tool.clone()) {
                            Ok(tool) => {
                                debug!("Successfully parsed tool: {} from {}", tool.name, name);
                                parsed_tools.push(tool);
                            }
                            Err(e) => {
                                warn!("Failed to parse tool {} from server '{}' (index: {}): {}. Tool data: {}", 
                                      i, name, i, e, serde_json::to_string_pretty(&raw_tool).unwrap_or_else(|_| "Invalid JSON".to_string()));

                                if let Some(tool_name) =
                                    raw_tool.get("name").and_then(|v| v.as_str())
                                {
                                    let fallback_tool = McpTool {
                                        name: tool_name.to_string(),
                                        description: raw_tool
                                            .get("description")
                                            .and_then(|v| v.as_str())
                                            .map(|s| s.to_string()),
                                        input_schema: json!({
                                            "type": "object",
                                            "properties": {},
                                            "required": []
                                        }),
                                    };
                                    warn!("⚠️  Created fallback tool '{}' with default schema (original tool had null/invalid schema)", tool_name);
                                    parsed_tools.push(fallback_tool);
                                }
                            }
                        }
                    }
                    *tools.write().await = parsed_tools;
                    let mut version = tools_version.write().await;
                    *version += 1;
                    debug!(
                        "Updated {} tools from MCP server {} via notification",
                        tools.read().await.len(),
                        name
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to parse tools array from notification {}: {}. Raw response: {}",
                        name,
                        e,
                        serde_json::to_string_pretty(tools_list)
                            .unwrap_or_else(|_| "Invalid JSON".to_string())
                    );
                }
            }
        }
    }
}

fn is_oauth_token_expired(entry: &OAuthTokenCacheEntry) -> bool {
    match entry.expires_at {
        Some(expires_at) => Instant::now() >= expires_at,
        None => false,
    }
}

fn resolve_oauth_token_url(server_url: &str, oauth: &McpOAuthConfig) -> Result<String> {
    if let Some(token_url) = &oauth.token_url {
        return Ok(token_url.clone());
    }

    let mut url = Url::parse(server_url)?;
    let scheme = match url.scheme() {
        "ws" | "http" => "http",
        "wss" | "https" => "https",
        other => {
            return Err(anyhow::anyhow!(
                "Unsupported server URL scheme '{}' for OAuth token discovery",
                other
            ))
        }
    };

    url.set_scheme(scheme)
        .map_err(|_| anyhow::anyhow!("Invalid OAuth token URL scheme"))?;
    url.set_query(None);
    url.set_fragment(None);
    url.set_path("");

    let mut base = url.to_string();
    if base.ends_with('/') {
        base.pop();
    }

    Ok(format!("{}/token", base))
}

fn build_oauth_header_value(token_response: &OAuthTokenResponse) -> String {
    let token_type = token_response
        .token_type
        .as_deref()
        .unwrap_or("Bearer");
    let normalized = if token_type.eq_ignore_ascii_case("bearer") {
        "Bearer"
    } else {
        token_type
    };
    format!("{} {}", normalized, token_response.access_token)
}

fn extract_url_param(value: &str, key: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let key_lower = key.to_ascii_lowercase();
    let pattern = format!("{}=", key_lower);
    let idx = lower.find(&pattern)?;
    let after = &value[idx + pattern.len()..];
    let end = after
        .find(|c: char| c == ',' || c == ' ' || c == ';')
        .unwrap_or(after.len());
    let raw = after[..end].trim().trim_matches('"').trim_matches('\'');
    if raw.starts_with("http://") || raw.starts_with("https://") {
        Some(raw.to_string())
    } else {
        None
    }
}

fn extract_oauth_url_from_www_authenticate(value: &str) -> Option<String> {
    for key in [
        "authorization_url",
        "authorization_uri",
        "authorize_url",
        "auth_url",
        "login_url",
    ] {
        if let Some(url) = extract_url_param(value, key) {
            return Some(url);
        }
    }
    None
}

fn extract_oauth_url_from_json(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            for (key, val) in map {
                let key_lower = key.to_ascii_lowercase();
                if let Some(url) = val.as_str() {
                    let looks_like_auth = url.contains("authorize") || url.contains("oauth");
                    if [
                        "authorization_url",
                        "authorizationurl",
                        "authorization_uri",
                        "authorize_url",
                        "authorizeurl",
                        "auth_url",
                        "authurl",
                        "login_url",
                        "loginurl",
                    ]
                    .contains(&key_lower.as_str())
                        || (key_lower == "url" && looks_like_auth)
                    {
                        return Some(url.to_string());
                    }
                }
            }
            for val in map.values() {
                if let Some(url) = extract_oauth_url_from_json(val) {
                    return Some(url);
                }
            }
            None
        }
        Value::Array(items) => {
            for item in items {
                if let Some(url) = extract_oauth_url_from_json(item) {
                    return Some(url);
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_oauth_url_from_body(body: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(url) = extract_oauth_url_from_json(&value) {
            return Some(url);
        }
    }

    for token in body.split(|c: char| {
        c.is_whitespace() || c == '"' || c == '\'' || c == '<' || c == '>' || c == ')'
    }) {
        if (token.starts_with("http://") || token.starts_with("https://"))
            && (token.contains("oauth") || token.contains("authorize"))
        {
            return Some(token.to_string());
        }
    }

    None
}

fn truncate_for_log(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }
    let mut output = value[..max_len].to_string();
    output.push_str("...<truncated>");
    output
}

fn try_open_browser(url: &str) -> bool {
    if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .is_ok()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(url).spawn().is_ok()
    } else {
        Command::new("xdg-open").arg(url).spawn().is_ok()
    }
}

fn build_authorization_url(
    endpoint: &str,
    client_id: Option<&str>,
    scope: Option<&str>,
    audience: Option<&str>,
    extra_params: Option<&HashMap<String, String>>,
    redirect_uri: Option<&str>,
    state: Option<&str>,
    code_challenge: Option<&str>,
) -> String {
    let mut url = match Url::parse(endpoint) {
        Ok(url) => url,
        Err(_) => return endpoint.to_string(),
    };

    let existing: HashSet<String> = url.query_pairs().map(|(k, _)| k.to_string()).collect();

    {
        let mut pairs = url.query_pairs_mut();
        if let Some(client_id) = client_id {
            if !existing.contains("client_id") {
                pairs.append_pair("client_id", client_id);
            }
        }
        if let Some(scope) = scope {
            if !existing.contains("scope") {
                pairs.append_pair("scope", scope);
            }
        }
        if let Some(audience) = audience {
            if !existing.contains("audience") {
                pairs.append_pair("audience", audience);
            }
        }

        // Add redirect_uri for OAuth callback
        if let Some(redirect_uri) = redirect_uri {
            if !existing.contains("redirect_uri") {
                pairs.append_pair("redirect_uri", redirect_uri);
            }
        }

        // Add state for CSRF protection
        if let Some(state) = state {
            if !existing.contains("state") {
                pairs.append_pair("state", state);
            }
        }

        // Add PKCE code_challenge
        if let Some(code_challenge) = code_challenge {
            if !existing.contains("code_challenge") {
                pairs.append_pair("code_challenge", code_challenge);
                pairs.append_pair("code_challenge_method", "S256");
            }
        }

        let mut response_type = None;
        if let Some(extra) = extra_params {
            if let Some(value) = extra.get("response_type") {
                response_type = Some(value.as_str());
            }
        }
        if response_type.is_none() {
            response_type = Some("code");
        }
        if !existing.contains("response_type") {
            if let Some(value) = response_type {
                pairs.append_pair("response_type", value);
            }
        }

        if let Some(extra) = extra_params {
            for (key, value) in extra {
                if !existing.contains(key) {
                    pairs.append_pair(key, value);
                }
            }
        }
    }

    url.to_string()
}

fn warn_if_missing_redirect(url: &str) {
    if let Ok(parsed) = Url::parse(url) {
        let has_redirect = parsed.query_pairs().any(|(key, _)| {
            key == "redirect_uri" || key == "redirect_url"
        });
        if !has_redirect {
            warn!("OAuth authorization URL has no redirect_uri parameter.");
        }
    }
}

/// Start a local HTTP server to receive OAuth callback and return the authorization code
async fn start_oauth_callback_server() -> Result<(u16, tokio::sync::oneshot::Receiver<(String, String)>)> {
    // Try to bind to a random available port
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();

    let (tx, rx) = tokio::sync::oneshot::channel::<(String, String)>();
    let tx = Arc::new(Mutex::new(Some(tx)));

    tokio::spawn(async move {
        // Accept one connection
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buffer = [0u8; 4096];
            if let Ok(n) = tokio::io::AsyncReadExt::read(&mut stream, &mut buffer).await {
                let request = String::from_utf8_lossy(&buffer[..n]);

                // Parse the GET request to extract code and state
                if let Some(path_line) = request.lines().next() {
                    if let Some(path) = path_line.strip_prefix("GET ").and_then(|s| s.split(' ').next()) {
                        if let Ok(url) = Url::parse(&format!("http://localhost{}", path)) {
                            let mut code = None;
                            let mut state = None;

                            for (key, value) in url.query_pairs() {
                                match key.as_ref() {
                                    "code" => code = Some(value.to_string()),
                                    "state" => state = Some(value.to_string()),
                                    _ => {}
                                }
                            }

                            if let (Some(code), Some(state)) = (code, state) {
                                // Send success response to browser
                                let response = "HTTP/1.1 200 OK\r\n\
                                    Content-Type: text/html; charset=utf-8\r\n\
                                    Connection: close\r\n\r\n\
                                    <!DOCTYPE html><html><head><title>Authorization Complete</title></head>\
                                    <body style=\"font-family: system-ui, sans-serif; text-align: center; padding: 50px;\">\
                                    <h1>✓ Authorization Complete</h1>\
                                    <p>You can close this window and return to Flexorama.</p>\
                                    </body></html>";
                                let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await;

                                // Send code and state through channel
                                if let Some(tx) = tx.lock().await.take() {
                                    let _ = tx.send((code, state));
                                }
                            } else {
                                // Check for error response
                                let error = url.query_pairs()
                                    .find(|(k, _)| k == "error")
                                    .map(|(_, v)| v.to_string())
                                    .unwrap_or_else(|| "unknown_error".to_string());
                                let error_desc = url.query_pairs()
                                    .find(|(k, _)| k == "error_description")
                                    .map(|(_, v)| v.to_string())
                                    .unwrap_or_default();

                                let response = format!(
                                    "HTTP/1.1 200 OK\r\n\
                                    Content-Type: text/html; charset=utf-8\r\n\
                                    Connection: close\r\n\r\n\
                                    <!DOCTYPE html><html><head><title>Authorization Failed</title></head>\
                                    <body style=\"font-family: system-ui, sans-serif; text-align: center; padding: 50px;\">\
                                    <h1>✗ Authorization Failed</h1>\
                                    <p>Error: {} {}</p>\
                                    </body></html>",
                                    error, error_desc
                                );
                                let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await;
                            }
                        }
                    }
                }
            }
        }
    });

    Ok((port, rx))
}

/// Exchange authorization code for access token using PKCE
async fn exchange_code_for_token(
    token_url: &str,
    code: &str,
    redirect_uri: &str,
    client_id: &str,
    client_secret: &str,
    code_verifier: &str,
    client_auth: McpOAuthClientAuth,
) -> Result<OAuthTokenResponse> {
    let client = create_http_client();

    let mut form = HashMap::<String, String>::new();
    form.insert("grant_type".to_string(), "authorization_code".to_string());
    form.insert("code".to_string(), code.to_string());
    form.insert("redirect_uri".to_string(), redirect_uri.to_string());
    form.insert("code_verifier".to_string(), code_verifier.to_string());

    if client_auth == McpOAuthClientAuth::Body {
        form.insert("client_id".to_string(), client_id.to_string());
        form.insert("client_secret".to_string(), client_secret.to_string());
    }

    let mut request = client
        .post(token_url)
        .header("accept", "application/json")
        .header("content-type", "application/x-www-form-urlencoded");

    if client_auth == McpOAuthClientAuth::Basic {
        request = request.basic_auth(client_id, Some(client_secret));
    }

    let response = request.form(&form).send().await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "Token exchange failed (HTTP {}): {}",
            status,
            body
        ));
    }

    let token_response: OAuthTokenResponse = response.json().await?;
    Ok(token_response)
}

/// Perform the full OAuth authorization code flow with PKCE
async fn perform_oauth_authorization_flow(
    name: &str,
    authorization_url: &str,
    token_url: &str,
    client_id: &str,
    client_secret: &str,
    client_auth: McpOAuthClientAuth,
    scope: Option<&str>,
    audience: Option<&str>,
    extra_params: Option<&HashMap<String, String>>,
) -> Result<OAuthTokenCacheEntry> {
    // Start callback server
    let (port, callback_rx) = start_oauth_callback_server().await?;
    let redirect_uri = format!("http://127.0.0.1:{}/callback", port);

    // Generate PKCE parameters
    let code_verifier = generate_code_verifier();
    let code_challenge = generate_code_challenge(&code_verifier);
    let state = generate_state();

    // Build authorization URL with all required parameters
    let auth_url = build_authorization_url(
        authorization_url,
        Some(client_id),
        scope,
        audience,
        extra_params,
        Some(&redirect_uri),
        Some(&state),
        Some(&code_challenge),
    );

    info!(
        "MCP server '{}' requires OAuth authorization. Opening browser...",
        name
    );
    debug!("OAuth authorization URL: {}", auth_url);

    // Open browser
    if try_open_browser(&auth_url) {
        info!("Opened browser for OAuth authorization.");
        info!("Waiting for authorization callback on port {}...", port);
    } else {
        warn!("Unable to open browser. Please visit this URL to authorize:");
        warn!("{}", auth_url);
    }

    // Wait for callback with timeout
    let timeout = tokio::time::timeout(Duration::from_secs(300), callback_rx).await;

    let (code, received_state) = match timeout {
        Ok(Ok((code, state))) => (code, state),
        Ok(Err(_)) => {
            return Err(anyhow::anyhow!(
                "OAuth callback channel closed unexpectedly"
            ));
        }
        Err(_) => {
            return Err(anyhow::anyhow!(
                "OAuth authorization timed out after 5 minutes"
            ));
        }
    };

    // Verify state to prevent CSRF
    if received_state != state {
        return Err(anyhow::anyhow!(
            "OAuth state mismatch - possible CSRF attack"
        ));
    }

    info!("Received authorization code, exchanging for access token...");

    // Exchange code for token
    let token_response = exchange_code_for_token(
        token_url,
        &code,
        &redirect_uri,
        client_id,
        client_secret,
        &code_verifier,
        client_auth,
    )
    .await?;

    info!("Successfully obtained access token for MCP server '{}'", name);

    let header_value = build_oauth_header_value(&token_response);
    let expires_at = token_response
        .expires_in
        .map(|secs| Instant::now() + Duration::from_secs(secs.saturating_sub(30)));

    Ok(OAuthTokenCacheEntry {
        header_value,
        expires_at,
    })
}

/// Result of OAuth handling - either got a token or just showed instructions
enum OAuthHandleResult {
    /// Successfully obtained an access token via PKCE flow
    Token(OAuthTokenCacheEntry),
    /// No token obtained (showed instructions to user or no OAuth detected)
    NoToken,
}

async fn maybe_handle_oauth_required(
    name: &str,
    status: StatusCode,
    headers: &HeaderMap,
    body: &str,
    fallback_url: Option<&str>,
    server_url: Option<&str>,
    client: Option<&reqwest::Client>,
    client_id: Option<&str>,
    scope: Option<&str>,
    audience: Option<&str>,
    extra_params: Option<&HashMap<String, String>>,
    token_url: Option<&str>,
) -> OAuthHandleResult {
    if status != StatusCode::UNAUTHORIZED && status != StatusCode::FORBIDDEN {
        return OAuthHandleResult::NoToken;
    }

    let www_auth = headers
        .get(reqwest::header::WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok());
    debug!(
        "MCP OAuth debug for '{}': status={}, fallback_url={:?}",
        name, status, fallback_url
    );
    if let Some(www_auth) = www_auth {
        debug!(
            "MCP OAuth debug for '{}': WWW-Authenticate: {}",
            name, www_auth
        );
    } else {
        debug!(
            "MCP OAuth debug for '{}': WWW-Authenticate header missing",
            name
        );
    }
    if !headers.is_empty() {
        debug!(
            "MCP OAuth debug for '{}': response headers: {:?}",
            name, headers
        );
    }
    if !body.trim().is_empty() {
        debug!(
            "MCP OAuth debug for '{}': body: {}",
            name,
            truncate_for_log(body, 2000)
        );
    } else {
        debug!("MCP OAuth debug for '{}': empty body", name);
    }

    let mut url = www_auth
        .and_then(extract_oauth_url_from_www_authenticate)
        .or_else(|| extract_oauth_url_from_body(body))
        .or_else(|| fallback_url.map(|value| value.to_string()));
    let wants_discovery = www_auth
        .map(|value| value.to_ascii_lowercase().contains("bearer realm=\"oauth\""))
        .unwrap_or(false);
    // Try OAuth discovery if we need the auth URL or want to get more metadata
    let mut discovered_token_url: Option<String> = None;
    if wants_discovery || url.is_none() {
        if let (Some(server_url), Some(client)) = (server_url, client) {
            if let Ok(mut base) = Url::parse(server_url) {
                base.set_query(None);
                base.set_fragment(None);
                base.set_path("/.well-known/oauth-authorization-server");
                let discovery_url = base.to_string();
                debug!(
                    "MCP OAuth debug for '{}': fetching discovery document: {}",
                    name, discovery_url
                );
                match client.get(&discovery_url).send().await {
                    Ok(response) => {
                        let status = response.status();
                        let body = response.text().await.unwrap_or_default();
                        debug!(
                            "MCP OAuth debug for '{}': discovery response status={} body={}",
                            name,
                            status,
                            truncate_for_log(&body, 2000)
                        );
                        if status.is_success() {
                            if let Ok(value) = serde_json::from_str::<Value>(&body) {
                                // Extract authorization endpoint
                                if url.is_none() {
                                    if let Some(endpoint) = value
                                        .get("authorization_endpoint")
                                        .and_then(|v| v.as_str())
                                    {
                                        url = Some(endpoint.to_string());
                                    } else if let Some(endpoint) =
                                        extract_oauth_url_from_json(&value)
                                    {
                                        url = Some(endpoint);
                                    }
                                }
                                // Extract token endpoint
                                if let Some(token_ep) = value
                                    .get("token_endpoint")
                                    .and_then(|v| v.as_str())
                                {
                                    discovered_token_url = Some(token_ep.to_string());
                                    debug!(
                                        "MCP OAuth debug for '{}': discovered token_endpoint: {}",
                                        name, token_ep
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        debug!(
                            "MCP OAuth debug for '{}': discovery request failed: {}",
                            name, e
                        );
                    }
                }
            }
        }
    }
    debug!(
        "MCP OAuth debug for '{}': detected authorization url: {:?}",
        name, url
    );

    let body_lower = body.to_ascii_lowercase();
    let mentions_oauth = body_lower.contains("oauth")
        || body_lower.contains("invalid_token")
        || body_lower.contains("unauthorized")
        || www_auth.is_some();

    if let Some(auth_url) = url {
        // Try to extract client_id from the authorization URL if not provided
        let resolved_client_id = client_id.map(|s| s.to_string()).or_else(|| {
            Url::parse(&auth_url).ok().and_then(|u| {
                u.query_pairs()
                    .find(|(k, _)| k == "client_id")
                    .map(|(_, v)| v.to_string())
            })
        });

        // Try to derive token_url if not provided
        let resolved_token_url = token_url
            .map(|s| s.to_string())
            // First use discovered token_endpoint from well-known document
            .or(discovered_token_url.clone())
            // Then try to derive from authorization URL host
            .or_else(|| {
                Url::parse(&auth_url).ok().and_then(|u| {
                    u.host_str().map(|host| {
                        let scheme = u.scheme();
                        format!("{}://{}/oauth/token", scheme, host)
                    })
                })
            })
            // Fall back to deriving from server URL
            .or_else(|| {
                server_url.and_then(|server| {
                    Url::parse(server).ok().map(|mut u| {
                        let scheme = match u.scheme() {
                            "ws" | "http" => "http",
                            _ => "https",
                        };
                        let _ = u.set_scheme(scheme);
                        u.set_query(None);
                        u.set_fragment(None);
                        u.set_path("/oauth/token");
                        u.to_string()
                    })
                })
            });

        // If we have client_id and token_url, use the full PKCE flow
        if let (Some(client_id), Some(token_url)) = (resolved_client_id.as_deref(), resolved_token_url) {
            info!(
                "MCP server '{}' requires OAuth. Starting authorization flow...",
                name
            );

            match perform_oauth_authorization_flow(
                name,
                &auth_url,
                &token_url,
                client_id,
                "", // No client secret for public PKCE flow
                McpOAuthClientAuth::Body,
                scope,
                audience,
                extra_params,
            )
            .await
            {
                Ok(token_entry) => {
                    info!(
                        "Successfully obtained OAuth token for MCP server '{}'",
                        name
                    );
                    return OAuthHandleResult::Token(token_entry);
                }
                Err(e) => {
                    warn!(
                        "OAuth flow failed for MCP server '{}': {}. You may need to configure OAuth manually.",
                        name, e
                    );
                }
            }
        } else {
            // Fall back to just showing the URL with whatever params we have
            let url = build_authorization_url(
                auth_url.as_str(),
                resolved_client_id.as_deref(),
                scope,
                audience,
                extra_params,
                None, // redirect_uri
                None, // state
                None, // code_challenge
            );
            debug!(
                "MCP OAuth debug for '{}': authorization url with params: {}",
                name, url
            );
            warn_if_missing_redirect(&url);
            warn!(
                "MCP server '{}' requires OAuth authorization.",
                name
            );
            if resolved_client_id.is_none() {
                warn!(
                    "Could not determine client_id. Please configure OAuth with client_id in your MCP server config."
                );
                warn!(
                    "Example: auth = {{ type = \"oauth\", grant_type = \"authorization_code\", authorization_url = \"...\", client_id = \"your-client-id\" }}"
                );
            }
            warn!("Authorization URL: {}", url);
            if try_open_browser(&url) {
                info!("Opened browser for OAuth authorization URL.");
            } else {
                warn!("Unable to open a browser. Please open the URL manually.");
            }
        }
    } else if mentions_oauth {
        warn!(
            "MCP server '{}' requires OAuth. Configure an authorization URL or provide a valid access token.",
            name
        );
    }

    OAuthHandleResult::NoToken
}

async fn start_http_sse(
    url: &str,
    client: &reqwest::Client,
    auth_header: Option<&str>,
    name: String,
    pending_requests: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<McpResponse>>>>,
    tools: Arc<RwLock<Vec<McpTool>>>,
    tools_version: Arc<RwLock<u64>>,
    oauth_authorization_url: Option<&str>,
    oauth_client_id: Option<&str>,
    oauth_scope: Option<&str>,
    oauth_audience: Option<&str>,
    oauth_extra_params: Option<&HashMap<String, String>>,
) -> Result<Option<tokio::sync::oneshot::Sender<()>>> {
    let mut request = client.get(url).header("accept", "text/event-stream, application/json");
    if let Some(header_value) = auth_header {
        request = request.header("authorization", header_value);
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(e) => {
            warn!("Failed to start MCP SSE stream for {}: {}", name, e);
            return Ok(None);
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.text().await.unwrap_or_default();
        let _ = maybe_handle_oauth_required(
            &name,
            status,
            &headers,
            &body,
            oauth_authorization_url,
            Some(url),
            Some(client),
            oauth_client_id,
            oauth_scope,
            oauth_audience,
            oauth_extra_params,
            None, // token_url - will be derived from server_url
        )
        .await;
        warn!(
            "MCP SSE stream for {} returned HTTP {}: {}",
            name, status, body
        );
        return Ok(None);
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !content_type.contains("text/event-stream") {
        warn!(
            "MCP SSE stream for {} returned unexpected content-type '{}'",
            name, content_type
        );
        return Ok(None);
    }

    let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
    let mut stream = response.bytes_stream();

    tokio::spawn(async move {
        let mut buffer = String::new();
        loop {
            tokio::select! {
                _ = &mut cancel_rx => {
                    debug!("MCP SSE stream for {} cancelled", name);
                    break;
                }
                chunk = stream.next() => {
                    match chunk {
                        Some(Ok(bytes)) => {
                            let text = String::from_utf8_lossy(&bytes);
                            buffer.push_str(&text);
                            if buffer.contains("\r\n") {
                                buffer = buffer.replace("\r\n", "\n");
                            }

                            while let Some(idx) = buffer.find("\n\n") {
                                let raw_event = buffer[..idx].to_string();
                                buffer = buffer[idx + 2..].to_string();
                                if let Some(data) = extract_sse_data(&raw_event) {
                                    if data.trim().is_empty() {
                                        continue;
                                    }
                                    match serde_json::from_str::<McpResponse>(data.trim()) {
                                        Ok(response) => {
                                            handle_mcp_response(
                                                &name,
                                                response,
                                                pending_requests.clone(),
                                                tools.clone(),
                                                tools_version.clone(),
                                            ).await;
                                        }
                                        Err(e) => {
                                            warn!(
                                                "Failed to parse MCP SSE message from {}: {}. Data: {}",
                                                name, e, data.trim()
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        Some(Err(e)) => {
                            warn!("Error reading MCP SSE stream for {}: {}", name, e);
                            break;
                        }
                        None => {
                            debug!("MCP SSE stream for {} closed", name);
                            break;
                        }
                    }
                }
            }
        }
    });

    Ok(Some(cancel_tx))
}

fn extract_sse_data(event: &str) -> Option<String> {
    let mut data_lines = Vec::new();
    for line in event.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(data) = trimmed.strip_prefix("data:") {
            data_lines.push(data.trim_start().to_string());
        }
    }
    if data_lines.is_empty() {
        None
    } else {
        Some(data_lines.join("\n"))
    }
}

#[derive(Debug)]
pub struct McpConnection {
    pub name: String,
    pub process: Option<Child>,
    pub websocket:
        Option<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>>,
    pub http_url: Option<String>,
    pub http_client: Option<reqwest::Client>,
    pub http_auth_header: Option<String>,
    pub http_session_id: Option<String>,
    pub oauth_authorization_url: Option<String>,
    pub oauth_client_id: Option<String>,
    pub oauth_scope: Option<String>,
    pub oauth_audience: Option<String>,
    pub oauth_extra_params: Option<HashMap<String, String>>,
    pub sse_enabled: bool,
    pub sse_cancel: Option<tokio::sync::oneshot::Sender<()>>,
    pub reader: Option<BufReader<tokio::process::ChildStdout>>,
    pub writer: Option<tokio::process::ChildStdin>,
    pub request_id: u64,
    pub pending_requests: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<McpResponse>>>>,
    pub tools: Arc<RwLock<Vec<McpTool>>>,
    pub tools_version: Arc<RwLock<u64>>,
}

impl McpConnection {
    pub fn new(name: String) -> Self {
        Self {
            name,
            process: None,
            websocket: None,
            http_url: None,
            http_client: None,
            http_auth_header: None,
            http_session_id: None,
            oauth_authorization_url: None,
            oauth_client_id: None,
            oauth_scope: None,
            oauth_audience: None,
            oauth_extra_params: None,
            sse_enabled: false,
            sse_cancel: None,
            reader: None,
            writer: None,
            request_id: 1,
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            tools: Arc::new(RwLock::new(Vec::new())),
            tools_version: Arc::new(RwLock::new(0)),
        }
    }

    /// Log detailed information about a tool and its parameters
    fn log_tool_details(&self, tool: &McpTool) {
        debug!("📋 Tool Details:");
        debug!("   Name: {}", tool.name);

        if let Some(description) = &tool.description {
            debug!("   Description: {}", description);
        } else {
            debug!("   Description: <No description provided>");
        }

        // Log input schema details
        if let Some(schema_obj) = tool.input_schema.as_object() {
            if let Some(properties) = schema_obj.get("properties").and_then(|p| p.as_object()) {
                if properties.is_empty() {
                    debug!("   Parameters: <No parameters>");
                } else {
                    debug!("   Parameters ({}):", properties.len());
                    for (param_name, param_schema) in properties {
                        let param_type = param_schema
                            .get("type")
                            .and_then(|t| t.as_str())
                            .unwrap_or("unknown");
                        let param_desc = param_schema
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("<No description>");
                        let required = schema_obj
                            .get("required")
                            .and_then(|r| r.as_array())
                            .map(|reqs| reqs.iter().any(|req| req.as_str() == Some(param_name)))
                            .unwrap_or(false);

                        let required_marker = if required { " (required)" } else { "" };
                        debug!(
                            "     • {} [{}]{}: {}",
                            param_name, param_type, required_marker, param_desc
                        );
                    }
                }
            } else {
                debug!("   Parameters: <No parameters defined>");
            }
        } else {
            debug!("   Parameters: <Invalid schema>");
        }

        debug!(
            "   Raw Schema: {}",
            serde_json::to_string(&tool.input_schema)
                .unwrap_or_else(|_| "<Invalid JSON>".to_string())
        );
    }

    pub async fn connect_stdio(
        &mut self,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<()> {
        debug!("Starting MCP server: {} {}", command, args.join(" "));
        debug!("MCP server details:");
        debug!("  Command: {}", command);
        debug!("  Args: {:?}", args);
        debug!("  Environment variables: {}", env.len());

        // Handle Windows-specific command resolution
        let (cmd, cmd_args) = if cfg!(target_os = "windows") {
            // On Windows, try to resolve the command properly
            if command == "npx" {
                // Try to find npx in common locations
                let npx_path = self.find_npx_on_windows().await?;
                (npx_path, args.to_vec())
            } else {
                // For other commands, try to find them in PATH
                match which::which(command) {
                    Ok(path) => (path.to_string_lossy().to_string(), args.to_vec()),
                    Err(_) => (command.to_string(), args.to_vec()),
                }
            }
        } else {
            (command.to_string(), args.to_vec())
        };

        let mut cmd_process = TokioCommand::new(&cmd);
        cmd_process
            .args(&cmd_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        for (key, value) in env {
            cmd_process.env(key, value);
        }

        // Add more detailed error logging for debugging
        debug!("Executing command: {} with args: {:?}", cmd, cmd_args);

        let mut child = cmd_process.spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn MCP server process '{}': {}\nPlease ensure:\n1. The command exists and is executable\n2. All required dependencies are installed\n3. The command is in your PATH\n4. On Windows: Node.js and npm are properly installed", cmd, e))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get stdin from child process"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get stdout from child process"))?;

        self.reader = Some(BufReader::new(stdout));
        self.writer = Some(stdin);
        self.process = Some(child);

        // Start message handling loop
        let pending_requests = self.pending_requests.clone();
        let tools = self.tools.clone();
        let tools_version = self.tools_version.clone();
        let name = self.name.clone();
        let mut reader = self.reader.take().unwrap();

        tokio::spawn(async move {
            let mut buffer = String::new();
            loop {
                match reader.read_line(&mut buffer).await {
                    Ok(0) => {
                        debug!("MCP server {} closed connection", name);
                        break;
                    }
                    Ok(_) => {
                        if buffer.trim().is_empty() {
                            buffer.clear();
                            continue;
                        }

                        debug!("Received from MCP server {}: {}", name, buffer.trim());

                        match serde_json::from_str::<McpResponse>(&buffer.trim()) {
                            Ok(response) => {
                                handle_mcp_response(
                                    &name,
                                    response,
                                    pending_requests.clone(),
                                    tools.clone(),
                                    tools_version.clone(),
                                )
                                .await;
                            }
                            Err(e) => {
                                warn!(
                                    "Failed to parse MCP response from {}: {}. Response: {}",
                                    name,
                                    e,
                                    buffer.trim()
                                );
                                error!(
                                    "MCP server '{}' sent invalid JSON data. This may indicate:",
                                    name
                                );
                                error!("1. Server is not following MCP protocol correctly");
                                error!(
                                    "2. Server process is crashing or outputting error messages"
                                );
                                error!("3. Version mismatch between client and server");
                                debug!("Raw response that failed to parse: {}", buffer.trim());
                            }
                        }
                        buffer.clear();
                    }
                    Err(e) => {
                        error!("Error reading from MCP server {}: {}", name, e);
                        error!(
                            "MCP server {} connection broken - tools may be unavailable",
                            name
                        );
                        break;
                    }
                }
            }
        });

        // Initialize connection
        match self.initialize().await {
            Ok(_) => {
                debug!(
                    "MCP server '{}' initialization completed successfully",
                    self.name
                );

                // Verify the process is still running after initialization
                if let Some(ref mut process) = self.process {
                    match process.try_wait() {
                        Ok(Some(status)) => {
                            error!(
                                "MCP server '{}' process exited unexpectedly with status: {}",
                                self.name, status
                            );
                            return Err(anyhow::anyhow!(
                                "MCP server '{}' process exited during initialization",
                                self.name
                            ));
                        }
                        Ok(None) => {
                            debug!("MCP server '{}' process is running normally", self.name);
                        }
                        Err(e) => {
                            warn!("Failed to check MCP server '{}' status: {}", self.name, e);
                        }
                    }
                }
                Ok(())
            }
            Err(e) => {
                error!("MCP server '{}' initialization failed: {}", self.name, e);

                // Check if the process is still running
                if let Some(ref mut process) = self.process {
                    match process.try_wait() {
                        Ok(Some(status)) => {
                            error!(
                                "MCP server '{}' process exited with status: {}",
                                self.name, status
                            );
                        }
                        Ok(None) => {
                            debug!("MCP server '{}' process is still running but initialization failed", self.name);
                        }
                        Err(_) => {}
                    }
                }

                Err(e)
            }
        }
    }

    /// Get the current tools version
    pub async fn get_tools_version(&self) -> u64 {
        *self.tools_version.read().await
    }

    /// Find npx executable on Windows
    async fn find_npx_on_windows(&self) -> Result<String> {
        // Try common Node.js installation paths on Windows
        let common_paths = vec![
            r"C:\Program Files\nodejs\npx.cmd",
            r"C:\Program Files (x86)\nodejs\npx.cmd",
            r"%APPDATA%\npm\npx.cmd",
        ];

        // First try to find npx in PATH
        if let Ok(npx_path) = which::which("npx") {
            return Ok(npx_path.to_string_lossy().to_string());
        }

        // Try common installation paths
        for path in &common_paths {
            let expanded_path = env::var("APPDATA").unwrap_or_default();
            let full_path = path.replace("%APPDATA%", &expanded_path);

            if Path::new(&full_path).exists() {
                debug!("Found npx at: {}", full_path);
                return Ok(full_path);
            }
        }

        // Try to find Node.js and use npx from there
        if let Ok(node_path) = which::which("node") {
            if let Some(parent) = Path::new(&node_path).parent() {
                let npx_path = parent.join("npx.cmd");
                if npx_path.exists() {
                    debug!("Found npx at: {}", npx_path.display());
                    return Ok(npx_path.to_string_lossy().to_string());
                }
            }
        }

        Err(anyhow::anyhow!(
            "npx not found. Please install Node.js and npm from https://nodejs.org/"
        ))
    }

    pub async fn connect_websocket(&mut self, url: &str, auth_header: Option<String>) -> Result<()> {
        debug!("Connecting to MCP server via WebSocket: {}", url);

        let mut request = url.into_client_request()?;
        if let Some(header_value) = auth_header {
            let parsed_header = header_value
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid OAuth authorization header"))?;
            request.headers_mut().insert(AUTHORIZATION, parsed_header);
        }

        let (ws_stream, _) = connect_async(request).await?;
        self.websocket = Some(ws_stream);

        // Initialize connection
        self.initialize().await?;
        Ok(())
    }

    pub async fn connect_http(&mut self, url: &str, auth_header: Option<String>) -> Result<()> {
        debug!("Connecting to MCP server via HTTP: {}", url);

        let client = create_http_client();
        self.http_url = Some(url.to_string());
        self.http_client = Some(client.clone());
        self.http_auth_header = auth_header.clone();

        if let Some(response) = start_http_sse(
            url,
            &client,
            auth_header.as_deref(),
            self.name.clone(),
            self.pending_requests.clone(),
            self.tools.clone(),
            self.tools_version.clone(),
            self.oauth_authorization_url.as_deref(),
            self.oauth_client_id.as_deref(),
            self.oauth_scope.as_deref(),
            self.oauth_audience.as_deref(),
            self.oauth_extra_params.as_ref(),
        )
        .await?
        {
            self.sse_enabled = true;
            self.sse_cancel = Some(response);
        } else {
            self.sse_enabled = false;
        }

        self.initialize().await?;
        Ok(())
    }

    async fn initialize(&mut self) -> Result<()> {
        let init_request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(self.next_id()),
            method: McpMethod::Initialize {
                protocol_version: "2024-11-05".to_string(),
                capabilities: McpClientCapabilities {
                    tools: Some(McpToolsCapability {
                        list_changed: Some(true),
                    }),
                    resources: Some(McpResourcesCapability {
                        list_changed: Some(true),
                    }),
                    prompts: Some(McpPromptsCapability {
                        list_changed: Some(true),
                    }),
                },
                client_info: McpClientInfo {
                    name: "flexorama".to_string(),
                    version: "0.1.0".to_string(),
                },
            },
        };

        let response = self.send_request(init_request).await?;

        if response.error.is_some() {
            return Err(anyhow::anyhow!(
                "MCP initialization failed: {:?}",
                response.error
            ));
        }

        // Send initialized notification
        let initialized = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: McpMethod::Initialized,
        };

        self.send_notification(initialized).await?;

        // Load tools
        self.load_tools().await?;

        debug!("MCP server {} initialized successfully", self.name);
        Ok(())
    }

    async fn load_tools(&mut self) -> Result<()> {
        debug!("Loading tools from MCP server '{}'...", self.name);

        let tools_request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(self.next_id()),
            method: McpMethod::ListTools,
        };

        // Add timeout for tools loading to prevent hanging
        let response_result = tokio::time::timeout(
            std::time::Duration::from_secs(10), // 10 second timeout for tools loading
            self.send_request(tools_request),
        )
        .await;

        let response = match response_result {
            Ok(Ok(response)) => response,
            Ok(Err(e)) => {
                error!(
                    "Failed to send tools request to MCP server '{}': {}",
                    self.name, e
                );
                return Err(e);
            }
            Err(_) => {
                error!(
                    "MCP server '{}' tools loading timed out after 10 seconds",
                    self.name
                );
                return Err(anyhow::anyhow!(
                    "Tools loading timed out for MCP server '{}'",
                    self.name
                ));
            }
        };

        if let Some(error) = response.error {
            error!(
                "Failed to list tools from MCP server '{}': {:?}",
                self.name, error
            );
            return Err(anyhow::anyhow!("Failed to list tools: {:?}", error));
        }

        if let Some(result) = response.result {
            if let Some(tools_value) = result.get("tools") {
                debug!(
                    "Raw tools response from {}: {}",
                    self.name,
                    serde_json::to_string_pretty(tools_value)?
                );

                // Try to parse tools with better error handling
                match serde_json::from_value::<Vec<Value>>(tools_value.clone()) {
                    Ok(raw_tools) => {
                        debug!(
                            "MCP server '{}' returned {} tools",
                            self.name,
                            raw_tools.len()
                        );

                        let mut parsed_tools = Vec::new();
                        for (i, raw_tool) in raw_tools.into_iter().enumerate() {
                            // Log the raw tool data before parsing
                            debug!(
                                "Raw tool data {} from server '{}': {}",
                                i,
                                self.name,
                                serde_json::to_string_pretty(&raw_tool)
                                    .unwrap_or_else(|_| "Invalid JSON".to_string())
                            );

                            // Check if inputSchema exists and what its value is
                            if let Some(input_schema) = raw_tool.get("inputSchema") {
                                debug!(
                                    "  inputSchema field found: {}",
                                    serde_json::to_string_pretty(input_schema)
                                        .unwrap_or_else(|_| "Invalid JSON".to_string())
                                );
                                if input_schema.is_null() {
                                    debug!("  inputSchema is null - will need fallback");
                                } else {
                                    debug!("  inputSchema has valid data");
                                }
                            } else if let Some(input_schema) = raw_tool.get("input_schema") {
                                debug!(
                                    "  input_schema field found (snake_case): {}",
                                    serde_json::to_string_pretty(input_schema)
                                        .unwrap_or_else(|_| "Invalid JSON".to_string())
                                );
                                if input_schema.is_null() {
                                    debug!("  input_schema is null - will need fallback");
                                } else {
                                    debug!("  input_schema has valid data");
                                }
                            } else {
                                debug!("  No input schema field found - will use serde default");
                            }

                            match serde_json::from_value::<McpTool>(raw_tool.clone()) {
                                Ok(tool) => {
                                    debug!(
                                        "Successfully parsed tool: {} from {}",
                                        tool.name, self.name
                                    );
                                    debug!(
                                        "  Parsed schema: {}",
                                        serde_json::to_string(&tool.input_schema)
                                            .unwrap_or_else(|_| "Invalid JSON".to_string())
                                    );
                                    debug!(
                                        "✓ Loaded tool: {} from server '{}'",
                                        tool.name, self.name
                                    );
                                    self.log_tool_details(&tool);
                                    parsed_tools.push(tool);
                                }
                                Err(e) => {
                                    warn!("Failed to parse tool {} from server '{}' (index: {}): {}. Tool data: {}", 
                                          i, self.name, i, e, serde_json::to_string_pretty(&raw_tool).unwrap_or_else(|_| "Invalid JSON".to_string()));

                                    // Try to create a minimal tool with the available data
                                    if let Some(name) =
                                        raw_tool.get("name").and_then(|v| v.as_str())
                                    {
                                        let fallback_tool = McpTool {
                                            name: name.to_string(),
                                            description: raw_tool
                                                .get("description")
                                                .and_then(|v| v.as_str())
                                                .map(|s| s.to_string()),
                                            input_schema: json!({
                                                "type": "object",
                                                "properties": {},
                                                "required": []
                                            }),
                                        };
                                        warn!("⚠️  Created fallback tool '{}' with default schema (original tool had null/invalid schema)", name);
                                        self.log_tool_details(&fallback_tool);
                                        parsed_tools.push(fallback_tool);
                                    }
                                }
                            }
                        }

                        *self.tools.write().await = parsed_tools;
                        // Increment version for this connection
                        let mut version = self.tools_version.write().await;
                        *version += 1;

                        debug!(
                            "Successfully loaded {} tools from MCP server '{}'",
                            self.tools.read().await.len(),
                            self.name
                        );
                    }
                    Err(e) => {
                        warn!(
                            "Failed to parse tools array from {}: {}. Raw response: {}",
                            self.name,
                            e,
                            serde_json::to_string_pretty(tools_value)?
                        );
                        return Err(anyhow::anyhow!(
                            "Invalid tools response format from MCP server '{}': {}",
                            self.name,
                            e
                        ));
                    }
                }
            } else {
                warn!(
                    "No 'tools' field found in response from MCP server '{}'",
                    self.name
                );
            }
        } else {
            warn!(
                "No result found in tools response from MCP server '{}'",
                self.name
            );
        }

        Ok(())
    }

    pub async fn call_tool(&mut self, name: &str, arguments: Option<Value>) -> Result<Value> {
        // Check if connection is still alive
        if let Some(ref mut process) = self.process {
            match process.try_wait() {
                Ok(Some(_)) => {
                    return Err(anyhow::anyhow!(
                        "MCP server '{}' process has terminated",
                        self.name
                    ));
                }
                Ok(None) => {
                    // Process is still running, good
                }
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "Failed to check MCP server '{}' status: {}",
                        self.name,
                        e
                    ));
                }
            }
        }

        // Log the tool call with details
        debug!("🔧 Calling MCP tool '{}' on server '{}'", name, self.name);

        if let Some(ref args) = arguments {
            if !args.is_null() {
                debug!(
                    "   Arguments: {}",
                    serde_json::to_string_pretty(args)
                        .unwrap_or_else(|_| "<Invalid JSON>".to_string())
                );
            } else {
                debug!("   Arguments: <No arguments>");
            }
        } else {
            debug!("   Arguments: <No arguments>");
        }

        let tool_request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(self.next_id()),
            method: McpMethod::CallTool {
                name: name.to_string(),
                arguments,
            },
        };

        let response = self.send_request(tool_request).await?;

        if let Some(error) = response.error {
            error!(
                "❌ MCP tool '{}' failed on server '{}': {:?}",
                name, self.name, error
            );
            return Err(anyhow::anyhow!("Tool call failed: {:?}", error));
        }

        debug!(
            "MCP tool '{}' completed successfully on server '{}'",
            name, self.name
        );

        if let Some(ref result) = response.result {
            debug!(
                "   Result: {}",
                serde_json::to_string_pretty(result)
                    .unwrap_or_else(|_| "<Invalid JSON>".to_string())
            );
        }

        Ok(response.result.unwrap_or(json!({})))
    }

    pub async fn list_resources(&mut self) -> Result<Vec<McpResource>> {
        let request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(self.next_id()),
            method: McpMethod::ListResources { cursor: None },
        };

        let response = self.send_request(request).await?;

        if let Some(error) = response.error {
            return Err(anyhow::anyhow!(
                "Failed to list MCP resources from '{}': {:?}",
                self.name,
                error
            ));
        }

        let result = response
            .result
            .ok_or_else(|| anyhow::anyhow!("No result returned for MCP resources list"))?;
        let resources_value = result
            .get("resources")
            .cloned()
            .unwrap_or_else(|| json!([]));
        let resources: Vec<McpResource> = serde_json::from_value(resources_value)?;
        Ok(resources)
    }

    pub async fn read_resource(&mut self, uri: &str) -> Result<Value> {
        let request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(self.next_id()),
            method: McpMethod::ReadResource {
                uri: uri.to_string(),
            },
        };

        let response = self.send_request(request).await?;

        if let Some(error) = response.error {
            return Err(anyhow::anyhow!(
                "Failed to read MCP resource '{}' from '{}': {:?}",
                uri,
                self.name,
                error
            ));
        }

        Ok(response.result.unwrap_or(json!({})))
    }

    pub async fn list_prompts(&mut self) -> Result<Vec<McpPrompt>> {
        let request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(self.next_id()),
            method: McpMethod::ListPrompts { cursor: None },
        };

        let response = self.send_request(request).await?;

        if let Some(error) = response.error {
            return Err(anyhow::anyhow!(
                "Failed to list MCP prompts from '{}': {:?}",
                self.name,
                error
            ));
        }

        let result = response
            .result
            .ok_or_else(|| anyhow::anyhow!("No result returned for MCP prompts list"))?;
        let prompts_value = result.get("prompts").cloned().unwrap_or_else(|| json!([]));
        let prompts: Vec<McpPrompt> = serde_json::from_value(prompts_value)?;
        Ok(prompts)
    }

    pub async fn get_prompt(&mut self, name: &str, arguments: Option<Value>) -> Result<Value> {
        let request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(self.next_id()),
            method: McpMethod::GetPrompt {
                name: name.to_string(),
                arguments,
            },
        };

        let response = self.send_request(request).await?;

        if let Some(error) = response.error {
            return Err(anyhow::anyhow!(
                "Failed to get MCP prompt '{}' from '{}': {:?}",
                name,
                self.name,
                error
            ));
        }

        Ok(response.result.unwrap_or(json!({})))
    }

    async fn send_request(&mut self, request: McpRequest) -> Result<McpResponse> {
        let id = request.id.clone().unwrap();
        let request_json = serde_json::to_string(&request)?;

        debug!("Sending MCP request to {}: {}", self.name, request_json);

        if let Some(http_url) = &self.http_url {
            let client = self
                .http_client
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("HTTP client not initialized"))?;
            let mut http_request = client
                .post(http_url)
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .body(request_json.clone());
            if let Some(auth_header) = &self.http_auth_header {
                http_request = http_request.header("authorization", auth_header);
            }
            if let Some(session_id) = &self.http_session_id {
                http_request = http_request.header("mcp-session-id", session_id);
            }

            if self.sse_enabled {
                // Create response channel for SSE responses
                let (tx, rx) = tokio::sync::oneshot::channel();
                self.pending_requests.lock().await.insert(id.clone(), tx);

                let response = match http_request.send().await {
                    Ok(response) => response,
                    Err(e) => {
                        self.pending_requests.lock().await.remove(&id);
                        return Err(e.into());
                    }
                };
                if !response.status().is_success() {
                    let status = response.status();
                    let headers = response.headers().clone();
                    let body = response.text().await.unwrap_or_default();
                    let _ = maybe_handle_oauth_required(
                        &self.name,
                        status,
                        &headers,
                        &body,
                        self.oauth_authorization_url.as_deref(),
                        self.http_url.as_deref(),
                        self.http_client.as_ref(),
                        self.oauth_client_id.as_deref(),
                        self.oauth_scope.as_deref(),
                        self.oauth_audience.as_deref(),
                        self.oauth_extra_params.as_ref(),
                        None, // token_url - will be derived from server_url
                    )
                    .await;
                    self.pending_requests.lock().await.remove(&id);
                    return Err(anyhow::anyhow!(
                        "MCP HTTP request failed for '{}' (HTTP {}): {}",
                        self.name,
                        status,
                        body
                    ));
                }

                // Extract session ID from response headers if present
                if let Some(session_id) = response
                    .headers()
                    .get("mcp-session-id")
                    .and_then(|v| v.to_str().ok())
                {
                    debug!("Received MCP session ID for '{}': {}", self.name, session_id);
                    self.http_session_id = Some(session_id.to_string());
                }

                let response = tokio::time::timeout(std::time::Duration::from_secs(30), rx).await;
                let response = match response {
                    Ok(Ok(response)) => response,
                    Ok(Err(_)) => {
                        self.pending_requests.lock().await.remove(&id);
                        return Err(anyhow::anyhow!(
                            "MCP server '{}' response channel was dropped",
                            self.name
                        ));
                    }
                    Err(_) => {
                        self.pending_requests.lock().await.remove(&id);
                        return Err(anyhow::anyhow!(
                            "MCP server '{}' timed out after 30 seconds",
                            self.name
                        ));
                    }
                };

                return Ok(response);
            } else {
                let response = http_request.send().await?;
                if !response.status().is_success() {
                    let status = response.status();
                    let headers = response.headers().clone();
                    let body = response.text().await.unwrap_or_default();
                    let _ = maybe_handle_oauth_required(
                        &self.name,
                        status,
                        &headers,
                        &body,
                        self.oauth_authorization_url.as_deref(),
                        self.http_url.as_deref(),
                        self.http_client.as_ref(),
                        self.oauth_client_id.as_deref(),
                        self.oauth_scope.as_deref(),
                        self.oauth_audience.as_deref(),
                        self.oauth_extra_params.as_ref(),
                        None, // token_url - will be derived from server_url
                    )
                    .await;
                    return Err(anyhow::anyhow!(
                        "MCP HTTP request failed for '{}' (HTTP {}): {}",
                        self.name,
                        status,
                        body
                    ));
                }

                // Extract session ID from response headers if present
                if let Some(session_id) = response
                    .headers()
                    .get("mcp-session-id")
                    .and_then(|v| v.to_str().ok())
                {
                    debug!("Received MCP session ID for '{}': {}", self.name, session_id);
                    self.http_session_id = Some(session_id.to_string());
                }

                // Check Content-Type to determine how to parse the response
                let content_type = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");

                if content_type.contains("text/event-stream") {
                    // Server returned SSE stream - read and parse events
                    let response_text = response.text().await?;
                    debug!(
                        "MCP SSE response from '{}': {}",
                        self.name,
                        truncate_for_log(&response_text, 500)
                    );

                    // Parse SSE events to find the JSON-RPC response
                    for line in response_text.lines() {
                        if let Some(data) = line.strip_prefix("data:") {
                            let data = data.trim();
                            if data.is_empty() || data == "[DONE]" {
                                continue;
                            }
                            if let Ok(response) = serde_json::from_str::<McpResponse>(data) {
                                return Ok(response);
                            }
                        }
                    }

                    // If no valid response found in SSE, return error
                    return Err(anyhow::anyhow!(
                        "MCP server '{}' returned SSE stream but no valid JSON-RPC response found",
                        self.name
                    ));
                } else {
                    // Regular JSON response
                    let response_json = response.text().await?;
                    if response_json.trim().is_empty() {
                        return Err(anyhow::anyhow!(
                            "MCP server '{}' returned empty response",
                            self.name
                        ));
                    }
                    debug!(
                        "MCP JSON response from '{}': {}",
                        self.name,
                        truncate_for_log(&response_json, 500)
                    );
                    let response: McpResponse = serde_json::from_str(&response_json)
                        .map_err(|e| anyhow::anyhow!(
                            "Failed to parse MCP response from '{}': {}. Response: {}",
                            self.name,
                            e,
                            truncate_for_log(&response_json, 200)
                        ))?;
                    return Ok(response);
                }
            }
        }

        // Create response channel
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending_requests.lock().await.insert(id.clone(), tx);

        // Send request
        if let Some(writer) = &mut self.writer {
            writer.write_all(request_json.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        } else if let Some(websocket) = &mut self.websocket {
            websocket.send(Message::Text(request_json)).await?;
        } else {
            return Err(anyhow::anyhow!("No connection available"));
        }

        // Wait for response with timeout
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(30), // 30 second timeout
            rx,
        )
        .await
        .map_err(|_| anyhow::anyhow!("MCP server '{}' timed out after 30 seconds", self.name))?
        .map_err(|_| anyhow::anyhow!("MCP server '{}' response channel was dropped", self.name))?;

        Ok(response)
    }

    async fn send_notification(&mut self, notification: McpRequest) -> Result<()> {
        let notification_json = serde_json::to_string(&notification)?;

        debug!(
            "Sending MCP notification to {}: {}",
            self.name, notification_json
        );

        if let Some(http_url) = &self.http_url {
            let client = self
                .http_client
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("HTTP client not initialized"))?;
            let mut http_request = client
                .post(http_url)
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .body(notification_json);
            if let Some(auth_header) = &self.http_auth_header {
                http_request = http_request.header("authorization", auth_header);
            }
            if let Some(session_id) = &self.http_session_id {
                http_request = http_request.header("mcp-session-id", session_id);
            }
            let response = http_request.send().await?;
            if !response.status().is_success() {
                let status = response.status();
                let headers = response.headers().clone();
                let body = response.text().await.unwrap_or_default();
                let _ = maybe_handle_oauth_required(
                    &self.name,
                    status,
                    &headers,
                    &body,
                    self.oauth_authorization_url.as_deref(),
                    self.http_url.as_deref(),
                    self.http_client.as_ref(),
                    self.oauth_client_id.as_deref(),
                    self.oauth_scope.as_deref(),
                    self.oauth_audience.as_deref(),
                    self.oauth_extra_params.as_ref(),
                    None, // token_url - will be derived from server_url
                )
                .await;
                return Err(anyhow::anyhow!(
                    "MCP HTTP notification failed for '{}' (HTTP {}): {}",
                    self.name,
                    status,
                    body
                ));
            }
            return Ok(());
        }

        if let Some(writer) = &mut self.writer {
            writer.write_all(notification_json.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        } else if let Some(websocket) = &mut self.websocket {
            websocket.send(Message::Text(notification_json)).await?;
        } else {
            return Err(anyhow::anyhow!("No connection available"));
        }

        Ok(())
    }

    pub async fn get_tools(&self) -> Vec<McpTool> {
        self.tools.read().await.clone()
    }

    fn next_id(&mut self) -> String {
        let id = self.request_id.to_string();
        self.request_id += 1;
        id
    }

    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(mut process) = self.process.take() {
            let _ = process.kill().await;
        }

        if let Some(mut websocket) = self.websocket.take() {
            let _ = websocket.close(None).await;
        }

        if let Some(cancel) = self.sse_cancel.take() {
            let _ = cancel.send(());
        }

        self.http_url = None;
        self.http_client = None;
        self.http_auth_header = None;
        self.http_session_id = None;
        self.sse_enabled = false;

        debug!("Disconnected from MCP server {}", self.name);
        Ok(())
    }
}

#[derive(Debug)]
pub struct McpManager {
    connections: Arc<RwLock<HashMap<String, McpConnection>>>,
    config: Arc<RwLock<McpConfig>>,
    config_path: Option<PathBuf>,
    oauth_tokens: Arc<Mutex<HashMap<String, OAuthTokenCacheEntry>>>,
}

impl McpManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            config: Arc::new(RwLock::new(McpConfig::default())),
            config_path: None,
            oauth_tokens: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a new MCP manager with a specific config path (useful for testing)
    pub fn new_with_config_path(config_path: PathBuf) -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            config: Arc::new(RwLock::new(McpConfig::default())),
            config_path: Some(config_path),
            oauth_tokens: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Initialize with MCP configuration from unified config
    pub async fn initialize(&self, mcp_config: McpConfig) -> Result<()> {
        *self.config.write().await = mcp_config;
        debug!(
            "MCP manager initialized with {} servers",
            self.config.read().await.servers.len()
        );
        Ok(())
    }

    /// Save current MCP configuration to unified config file
    pub async fn save_to_config_file(&self) -> Result<()> {
        use crate::config::Config;

        let config_path = self
            .config_path
            .as_ref()
            .map(|path| path.to_string_lossy().to_string());

        // Load existing unified config to preserve other settings
        let mut unified_config = Config::load(config_path.as_deref()).await?;

        // Update MCP configuration
        let current_mcp_config = self.config.read().await.clone();
        unified_config.mcp = current_mcp_config;

        // Save unified config
        unified_config.save(config_path.as_deref()).await?;
        debug!("Saved MCP configuration to unified config file");
        Ok(())
    }

    pub async fn load_config(&self) -> Result<McpConfig> {
        Ok(self.config.read().await.clone())
    }

    pub async fn get_server(&self, name: &str) -> Option<McpServerConfig> {
        let config = self.config.read().await;
        config.servers.get(name).cloned()
    }

    pub async fn add_server(&self, name: &str, server_config: McpServerConfig) -> Result<()> {
        let mut config = self.config.write().await;
        config.servers.insert(name.to_string(), server_config);
        drop(config);
        self.save_to_config_file().await?;
        Ok(())
    }

    pub async fn remove_server(&self, name: &str) -> Result<()> {
        let mut config = self.config.write().await;
        config.servers.remove(name);
        drop(config);
        self.save_to_config_file().await?;

        // Disconnect if connected
        let mut connections = self.connections.write().await;
        if let Some(mut connection) = connections.remove(name) {
            let _ = connection.disconnect().await;
        }
        self.oauth_tokens.lock().await.remove(name);

        Ok(())
    }

    pub async fn upsert_server(&self, name: &str, server_config: McpServerConfig) -> Result<()> {
        {
            let mut config = self.config.write().await;
            config
                .servers
                .insert(name.to_string(), server_config.clone());
        }
        self.oauth_tokens.lock().await.remove(name);
        self.save_to_config_file().await?;

        // Restart the connection based on the new config
        let _ = self.disconnect_server(name).await;
        if server_config.enabled {
            let _ = self.connect_server(name).await?;
        }

        Ok(())
    }

    pub async fn connect_server(&self, name: &str) -> Result<()> {
        let config = self.load_config().await?;
        let server_config = config
            .servers
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Server '{}' not found in configuration", name))?;

        if !server_config.enabled {
            return Err(anyhow::anyhow!("Server '{}' is disabled", name));
        }

        debug!("🔌 Connecting to MCP server: {}", name);
        debug!("   Configuration:");

        if let Some(url) = &server_config.url {
            if url.starts_with("ws://") || url.starts_with("wss://") {
                debug!("     Type: WebSocket");
            } else if url.starts_with("http://") || url.starts_with("https://") {
                debug!("     Type: HTTP");
            } else {
                debug!("     Type: URL");
            }
            debug!("     URL: {}", url);
        } else if let Some(command) = &server_config.command {
            debug!("     Type: STDIO");
            debug!("     Command: {}", command);
            if let Some(args) = &server_config.args {
                debug!("     Args: {}", args.join(" "));
            }
            if let Some(env_vars) = &server_config.env {
                debug!("     Environment Variables: {}", env_vars.len());
                for (key, value) in env_vars {
                    debug!("       {}={}", key, value);
                }
            }
        }
        if let Some(auth) = &server_config.auth {
            match auth {
                McpAuthConfig::OAuth(oauth) => {
                    debug!(
                        "     Auth: OAuth client_credentials (client_auth: {:?})",
                        oauth.client_auth
                    );
                    if let Some(auth_url) = &oauth.authorization_url {
                        debug!("     OAuth Authorization URL: {}", auth_url);
                    }
                    if let Some(token_url) = &oauth.token_url {
                        debug!("     OAuth Token URL: {}", token_url);
                    } else {
                        debug!("     OAuth Token URL: <derived from server URL>");
                    }
                }
            }
        }
        debug!("     Enabled: {}", server_config.enabled);

        let mut connection = McpConnection::new(name.to_string());
        if let Some(McpAuthConfig::OAuth(oauth)) = &server_config.auth {
            connection.oauth_authorization_url = oauth.authorization_url.clone();
            connection.oauth_client_id = Some(oauth.client_id.clone());
            connection.oauth_scope = oauth.scope.clone();
            connection.oauth_audience = oauth.audience.clone();
            connection.oauth_extra_params = oauth.extra_params.clone();
        }

        let auth_header = if server_config.auth.is_some() {
            if server_config.url.is_none() {
                warn!(
                    "OAuth auth configured for MCP server '{}' but it is not a WebSocket server - ignoring auth",
                    name
                );
                None
            } else {
                self.oauth_header_for_server(name, server_config).await?
            }
        } else {
            None
        };

        if let Some(url) = &server_config.url {
            if url.starts_with("ws://") || url.starts_with("wss://") {
                connection.connect_websocket(url, auth_header).await?;
            } else if url.starts_with("http://") || url.starts_with("https://") {
                connection.connect_http(url, auth_header).await?;
            } else {
                return Err(anyhow::anyhow!(
                    "Server '{}' URL must start with ws://, wss://, http://, or https://",
                    name
                ));
            }
        } else if let Some(command) = &server_config.command {
            // Connect via stdio
            let args = server_config.args.as_deref().unwrap_or(&[]);
            let env_vars = server_config.env.as_ref().cloned().unwrap_or_default();
            connection.connect_stdio(command, args, &env_vars).await?;
        } else {
            return Err(anyhow::anyhow!(
                "Server '{}' has no command or URL configured",
                name
            ));
        }

        self.connections
            .write()
            .await
            .insert(name.to_string(), connection);
        info!("✅ Successfully connected to MCP server: {}", name);

        // Log summary of available tools (debug level)
        if let Some(connection) = self.connections.read().await.get(name) {
            let tools = connection.get_tools().await;
            debug!("📊 Available tools from '{}': {}", name, tools.len());
            for tool in &tools {
                debug!(
                    "   • {} - {}",
                    tool.name,
                    tool.description.as_deref().unwrap_or("<No description>")
                );
            }
        }

        Ok(())
    }

    async fn oauth_header_for_server(
        &self,
        server_name: &str,
        server_config: &McpServerConfig,
    ) -> Result<Option<String>> {
        let auth = match &server_config.auth {
            Some(auth) => auth,
            None => return Ok(None),
        };

        let oauth = match auth {
            McpAuthConfig::OAuth(oauth) => oauth,
        };

        let server_url = server_config.url.as_deref().ok_or_else(|| {
            anyhow::anyhow!("OAuth auth configured but server '{}' has no URL", server_name)
        })?;

        {
            let cache = self.oauth_tokens.lock().await;
            if let Some(entry) = cache.get(server_name) {
                if !is_oauth_token_expired(entry) {
                    return Ok(Some(entry.header_value.clone()));
                }
            }
        }

        let entry = self.fetch_oauth_token(server_name, server_url, oauth).await?;
        let mut cache = self.oauth_tokens.lock().await;
        cache.insert(server_name.to_string(), entry.clone());
        Ok(Some(entry.header_value))
    }

    async fn fetch_oauth_token(
        &self,
        server_name: &str,
        server_url: &str,
        oauth: &McpOAuthConfig,
    ) -> Result<OAuthTokenCacheEntry> {
        use crate::config::McpOAuthGrantType;

        match oauth.grant_type {
            McpOAuthGrantType::AuthorizationCode => {
                // Use authorization code flow with PKCE
                let authorization_url = oauth.authorization_url.as_deref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "OAuth authorization_url is required for authorization_code flow on server '{}'",
                        server_name
                    )
                })?;

                let token_url = resolve_oauth_token_url(server_url, oauth)?;

                perform_oauth_authorization_flow(
                    server_name,
                    authorization_url,
                    &token_url,
                    &oauth.client_id,
                    oauth.client_secret.as_deref().unwrap_or(""),
                    oauth.client_auth,
                    oauth.scope.as_deref(),
                    oauth.audience.as_deref(),
                    oauth.extra_params.as_ref(),
                )
                .await
            }
            McpOAuthGrantType::ClientCredentials => {
                // Use client credentials flow (machine-to-machine)
                let client_secret = oauth.client_secret.as_ref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "OAuth client_secret is required for client_credentials flow on server '{}'",
                        server_name
                    )
                })?;

                let token_url = resolve_oauth_token_url(server_url, oauth)?;
                let mut form = HashMap::<String, String>::new();
                form.insert("grant_type".to_string(), "client_credentials".to_string());

                if oauth.client_auth == McpOAuthClientAuth::Body {
                    form.insert("client_id".to_string(), oauth.client_id.clone());
                    form.insert("client_secret".to_string(), client_secret.clone());
                }

                if let Some(scope) = &oauth.scope {
                    form.insert("scope".to_string(), scope.clone());
                }
                if let Some(audience) = &oauth.audience {
                    form.insert("audience".to_string(), audience.clone());
                }
                if let Some(extra_params) = &oauth.extra_params {
                    for (key, value) in extra_params {
                        if form.contains_key(key) {
                            warn!(
                                "Skipping OAuth extra param '{}' for server '{}' because it conflicts with a standard parameter",
                                key, server_name
                            );
                            continue;
                        }
                        form.insert(key.clone(), value.clone());
                    }
                }

                let client = create_http_client();
                let mut request = client.post(&token_url).header("accept", "application/json");
                if oauth.client_auth == McpOAuthClientAuth::Basic {
                    request = request.basic_auth(&oauth.client_id, Some(client_secret));
                }

                let response = request.form(&form).send().await?;
                if !response.status().is_success() {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    return Err(anyhow::anyhow!(
                        "OAuth token request failed for server '{}' (HTTP {}): {}",
                        server_name,
                        status,
                        body
                    ));
                }

                let token_response: OAuthTokenResponse = response.json().await?;
                let header_value = build_oauth_header_value(&token_response);
                let expires_at = token_response
                    .expires_in
                    .map(|secs| Instant::now() + Duration::from_secs(secs.saturating_sub(30)));

                Ok(OAuthTokenCacheEntry {
                    header_value,
                    expires_at,
                })
            }
        }
    }

    pub async fn disconnect_server(&self, name: &str) -> Result<()> {
        // First, try to remove and disconnect the connection
        let mut connections = self.connections.write().await;
        if let Some(mut connection) = connections.remove(name) {
            connection.disconnect().await?;
            debug!("Disconnected from MCP server: {}", name);
            Ok(())
        } else {
            // Only check configuration if there's no active connection
            let config = self.load_config().await?;
            if !config.servers.contains_key(name) {
                return Err(anyhow::anyhow!("Server '{}' not found", name));
            }
            // Server exists in config but isn't connected, which is fine
            Ok(())
        }
    }

    pub async fn reconnect_server(&self, name: &str) -> Result<()> {
        self.disconnect_server(name).await?;
        self.connect_server(name).await?;
        Ok(())
    }

    pub async fn list_servers(&self) -> Result<Vec<(String, McpServerConfig, bool)>> {
        let config = self.config.read().await;
        let connections = self.connections.read().await;

        let mut servers = Vec::new();
        for (name, server_config) in config.servers.iter() {
            let connected = connections.contains_key(name);
            servers.push((name.clone(), server_config.clone(), connected));
        }

        Ok(servers)
    }

    pub async fn get_all_tools(&self) -> Result<Vec<(String, McpTool)>> {
        let connections = self.connections.read().await;
        let mut all_tools = Vec::new();

        for (name, connection) in connections.iter() {
            let tools = connection.get_tools().await;
            for tool in tools {
                all_tools.push((name.clone(), tool));
            }
        }

        Ok(all_tools)
    }

    pub async fn disconnect_all(&self) -> Result<()> {
        let connections = self.connections.read().await;
        let server_names: Vec<String> = connections.keys().cloned().collect();
        drop(connections);

        for name in server_names {
            let _ = self.disconnect_server(&name).await;
        }

        Ok(())
    }

    pub async fn is_connected(&self, name: &str) -> bool {
        let connections = self.connections.read().await;
        connections.contains_key(name)
    }

    /// Get the global tools version (sum of all connection versions)
    pub async fn get_tools_version(&self) -> u64 {
        let connections = self.connections.read().await;
        let mut total_version = 0u64;

        for connection in connections.values() {
            total_version = total_version.wrapping_add(connection.get_tools_version().await);
        }

        total_version
    }

    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Option<Value>,
    ) -> Result<Value> {
        let mut connections = self.connections.write().await;
        if let Some(connection) = connections.get_mut(server_name) {
            connection.call_tool(tool_name, arguments).await
        } else {
            Err(anyhow::anyhow!("Server '{}' is not connected", server_name))
        }
    }

    pub async fn list_resources(&self, server_name: &str) -> Result<Vec<McpResource>> {
        let mut connections = self.connections.write().await;
        if let Some(connection) = connections.get_mut(server_name) {
            connection.list_resources().await
        } else {
            Err(anyhow::anyhow!("Server '{}' is not connected", server_name))
        }
    }

    pub async fn read_resource(&self, server_name: &str, uri: &str) -> Result<Value> {
        let mut connections = self.connections.write().await;
        if let Some(connection) = connections.get_mut(server_name) {
            connection.read_resource(uri).await
        } else {
            Err(anyhow::anyhow!("Server '{}' is not connected", server_name))
        }
    }

    pub async fn list_prompts(&self, server_name: &str) -> Result<Vec<McpPrompt>> {
        let mut connections = self.connections.write().await;
        if let Some(connection) = connections.get_mut(server_name) {
            connection.list_prompts().await
        } else {
            Err(anyhow::anyhow!("Server '{}' is not connected", server_name))
        }
    }

    pub async fn get_prompt(
        &self,
        server_name: &str,
        name: &str,
        arguments: Option<Value>,
    ) -> Result<Value> {
        let mut connections = self.connections.write().await;
        if let Some(connection) = connections.get_mut(server_name) {
            connection.get_prompt(name, arguments).await
        } else {
            Err(anyhow::anyhow!("Server '{}' is not connected", server_name))
        }
    }

    pub async fn connect_all_enabled(&self) -> Result<()> {
        let config = self.config.read().await;
        let mut connected_count = 0;
        let mut failed_count = 0;

        debug!("🌐 Connecting to all enabled MCP servers...");
        debug!("   Total servers configured: {}", config.servers.len());

        for (name, server_config) in config.servers.iter() {
            if server_config.enabled {
                debug!("   Attempting to connect to: {}", name);

                // Add timeout for individual server connections
                let connect_result = tokio::time::timeout(
                    std::time::Duration::from_secs(10), // 10 second timeout per server
                    self.connect_server(name),
                )
                .await;

                match connect_result {
                    Ok(Ok(_)) => {
                        connected_count += 1;
                        debug!("✅ Connected to MCP server: {}", name);
                    }
                    Ok(Err(e)) => {
                        failed_count += 1;
                        warn!("❌ Failed to connect to MCP server '{}': {}", name, e);
                    }
                    Err(_) => {
                        failed_count += 1;
                        warn!(
                            "⏰ MCP server '{}' connection timed out after 10 seconds",
                            name
                        );
                    }
                }
            } else {
                debug!("⏭️  Skipping disabled server: {}", name);
            }
        }

        debug!("📊 MCP Connection Summary:");
        debug!("   Successfully connected: {}", connected_count);
        debug!("   Failed connections: {}", failed_count);
        debug!(
            "   Skipped (disabled): {}",
            config.servers.len() - connected_count - failed_count
        );

        if connected_count > 0 {
            // Log total tools available across all servers (debug level)
            let all_tools = self.get_all_tools().await?;
            debug!(
                "🛠️  Total tools available across all MCP servers: {}",
                all_tools.len()
            );

            // Group tools by server for better organization
            let mut tools_by_server = std::collections::HashMap::new();
            for (server_name, tool) in all_tools {
                tools_by_server
                    .entry(server_name)
                    .or_insert_with(Vec::new)
                    .push(tool);
            }

            for (server_name, tools) in tools_by_server {
                debug!("   Server '{}': {} tools", server_name, tools.len());
            }
        }

        Ok(())
    }
}

impl Default for McpManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;
    use tempfile::TempDir;

    // Helper function to create a temporary config directory
    fn temp_config_dir() -> TempDir {
        tempfile::tempdir().expect("Failed to create temp dir")
    }

    // Helper function to create a test McpServerConfig
    fn test_server_config() -> McpServerConfig {
        McpServerConfig {
            name: "test-server".to_string(),
            command: Some("echo".to_string()),
            args: Some(vec!["test".to_string()]),
            env: Some(HashMap::new()),
            url: None,
            auth: None,
            enabled: true,
        }
    }

    // Debug test to see actual serialization
    #[test]
    fn test_print_serialization() {
        let request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some("1".to_string()),
            method: McpMethod::Initialize {
                protocol_version: "2024-11-05".to_string(),
                capabilities: McpClientCapabilities {
                    tools: Some(McpToolsCapability {
                        list_changed: Some(true),
                    }),
                    resources: None,
                    prompts: None,
                },
                client_info: McpClientInfo {
                    name: "flexorama".to_string(),
                    version: "0.1.0".to_string(),
                },
            },
        };

        let json = serde_json::to_string_pretty(&request).unwrap();
        println!("Actual serialized JSON:\n{}", json);

        // Check the structure
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        println!("Keys: {:?}", value.as_object().unwrap().keys().collect::<Vec<_>>());
    }

    // Tests for McpRequest serialization
    #[test]
    fn test_mcp_request_initialize_serialization() {
        let request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some("1".to_string()),
            method: McpMethod::Initialize {
                protocol_version: "2024-11-05".to_string(),
                capabilities: McpClientCapabilities {
                    tools: Some(McpToolsCapability {
                        list_changed: Some(true),
                    }),
                    resources: Some(McpResourcesCapability {
                        list_changed: Some(true),
                    }),
                    prompts: Some(McpPromptsCapability {
                        list_changed: Some(true),
                    }),
                },
                client_info: McpClientInfo {
                    name: "flexorama".to_string(),
                    version: "0.1.0".to_string(),
                },
            },
        };

        let serialized = serde_json::to_value(&request).unwrap();
        assert_eq!(serialized["jsonrpc"], "2.0");
        assert_eq!(serialized["id"], "1");
        assert_eq!(serialized["method"], "initialize");
        assert!(
            serialized["params"]["capabilities"]["tools"]["listChanged"]
                .as_bool()
                .unwrap()
        );
        // Verify camelCase field names
        assert!(serialized["params"]["protocolVersion"].is_string());
        assert!(serialized["params"]["clientInfo"].is_object());
    }

    #[test]
    fn test_mcp_request_list_tools_serialization() {
        let request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some("2".to_string()),
            method: McpMethod::ListTools,
        };

        let serialized = serde_json::to_value(&request).unwrap();
        assert_eq!(serialized["jsonrpc"], "2.0");
        assert_eq!(serialized["id"], "2");
        assert_eq!(serialized["method"], "tools/list");
    }

    #[test]
    fn test_mcp_request_call_tool_serialization() {
        let arguments = json!({
            "path": "/test/path",
            "content": "test content"
        });

        let request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some("3".to_string()),
            method: McpMethod::CallTool {
                name: "write_file".to_string(),
                arguments: Some(arguments.clone()),
            },
        };

        let serialized = serde_json::to_value(&request).unwrap();
        assert_eq!(serialized["jsonrpc"], "2.0");
        assert_eq!(serialized["id"], "3");
        assert_eq!(serialized["method"], "tools/call");
        assert_eq!(serialized["params"]["name"], "write_file");
        assert_eq!(serialized["params"]["arguments"]["path"], "/test/path");
    }

    #[test]
    fn test_mcp_request_list_resources_serialization() {
        let request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some("4".to_string()),
            method: McpMethod::ListResources { cursor: None },
        };

        let serialized = serde_json::to_value(&request).unwrap();
        assert_eq!(serialized["jsonrpc"], "2.0");
        assert_eq!(serialized["id"], "4");
        assert_eq!(serialized["method"], "resources/list");
    }

    #[test]
    fn test_mcp_request_read_resource_serialization() {
        let request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some("5".to_string()),
            method: McpMethod::ReadResource {
                uri: "file:///tmp/test.txt".to_string(),
            },
        };

        let serialized = serde_json::to_value(&request).unwrap();
        assert_eq!(serialized["jsonrpc"], "2.0");
        assert_eq!(serialized["id"], "5");
        assert_eq!(serialized["method"], "resources/read");
        assert_eq!(serialized["params"]["uri"], "file:///tmp/test.txt");
    }

    #[test]
    fn test_mcp_request_list_prompts_serialization() {
        let request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some("6".to_string()),
            method: McpMethod::ListPrompts { cursor: None },
        };

        let serialized = serde_json::to_value(&request).unwrap();
        assert_eq!(serialized["jsonrpc"], "2.0");
        assert_eq!(serialized["id"], "6");
        assert_eq!(serialized["method"], "prompts/list");
    }

    #[test]
    fn test_mcp_request_get_prompt_serialization() {
        let arguments = json!({
            "topic": "greeting"
        });

        let request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some("7".to_string()),
            method: McpMethod::GetPrompt {
                name: "hello".to_string(),
                arguments: Some(arguments.clone()),
            },
        };

        let serialized = serde_json::to_value(&request).unwrap();
        assert_eq!(serialized["jsonrpc"], "2.0");
        assert_eq!(serialized["id"], "7");
        assert_eq!(serialized["method"], "prompts/get");
        assert_eq!(serialized["params"]["name"], "hello");
        assert_eq!(serialized["params"]["arguments"]["topic"], "greeting");
    }

    #[test]
    fn test_mcp_request_ping_serialization() {
        let request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some("8".to_string()),
            method: McpMethod::Ping,
        };

        let serialized = serde_json::to_value(&request).unwrap();
        assert_eq!(serialized["jsonrpc"], "2.0");
        assert_eq!(serialized["id"], "8");
        assert_eq!(serialized["method"], "ping");
    }

    #[test]
    fn test_mcp_request_initialized_serialization() {
        let request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: McpMethod::Initialized,
        };

        let serialized = serde_json::to_value(&request).unwrap();
        assert_eq!(serialized["jsonrpc"], "2.0");
        assert!(serialized["id"].is_null());
        assert_eq!(serialized["method"], "notifications/initialized");
    }

    // Tests for McpResponse deserialization
    #[test]
    fn test_mcp_response_success_deserialization() {
        let json_str = r#"{
            "jsonrpc": "2.0",
            "id": "1",
            "result": {
                "tools": []
            }
        }"#;

        let response: McpResponse = serde_json::from_str(json_str).unwrap();
        assert_eq!(response.jsonrpc, "2.0");
        assert_eq!(response.id.unwrap(), "1");
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[test]
    fn test_mcp_response_error_deserialization() {
        let json_str = r#"{
            "jsonrpc": "2.0",
            "id": "1",
            "error": {
                "code": -32601,
                "message": "Method not found"
            }
        }"#;

        let response: McpResponse = serde_json::from_str(json_str).unwrap();
        assert_eq!(response.jsonrpc, "2.0");
        assert_eq!(response.id.unwrap(), "1");
        assert!(response.result.is_none());
        assert!(response.error.is_some());

        let error = response.error.unwrap();
        assert_eq!(error.code, -32601);
        assert_eq!(error.message, "Method not found");
    }

    #[test]
    fn test_mcp_error_with_data() {
        let json_str = r#"{
            "jsonrpc": "2.0",
            "id": "1",
            "error": {
                "code": -32600,
                "message": "Invalid Request",
                "data": {
                    "details": "Missing required parameter"
                }
            }
        }"#;

        let response: McpResponse = serde_json::from_str(json_str).unwrap();
        let error = response.error.unwrap();
        assert_eq!(error.code, -32600);
        assert!(error.data.is_some());
        assert_eq!(error.data.unwrap()["details"], "Missing required parameter");
    }

    // Tests for McpTool
    #[test]
    fn test_mcp_tool_deserialization_camel_case() {
        let json_str = r#"{
            "name": "read_file",
            "description": "Read a file",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path"
                    }
                },
                "required": ["path"]
            }
        }"#;

        let tool: McpTool = serde_json::from_str(json_str).unwrap();
        assert_eq!(tool.name, "read_file");
        assert_eq!(tool.description.unwrap(), "Read a file");
        assert!(tool.input_schema.is_object());
    }

    #[test]
    fn test_mcp_tool_deserialization_snake_case() {
        let json_str = r#"{
            "name": "write_file",
            "description": "Write a file",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string"
                    },
                    "content": {
                        "type": "string"
                    }
                }
            }
        }"#;

        let tool: McpTool = serde_json::from_str(json_str).unwrap();
        assert_eq!(tool.name, "write_file");
        assert_eq!(tool.description.unwrap(), "Write a file");
        assert!(tool.input_schema.is_object());
    }

    #[test]
    fn test_mcp_tool_no_description() {
        let json_str = r#"{
            "name": "test_tool",
            "input_schema": {
                "type": "object",
                "properties": {}
            }
        }"#;

        let tool: McpTool = serde_json::from_str(json_str).unwrap();
        assert_eq!(tool.name, "test_tool");
        assert!(tool.description.is_none());
    }

    // Tests for McpConnection
    #[test]
    fn test_mcp_connection_new() {
        let connection = McpConnection::new("test-server".to_string());
        assert_eq!(connection.name, "test-server");
        assert_eq!(connection.request_id, 1);
        assert!(connection.process.is_none());
        assert!(connection.websocket.is_none());
        assert!(connection.reader.is_none());
        assert!(connection.writer.is_none());
    }

    #[test]
    fn test_mcp_connection_next_id() {
        let mut connection = McpConnection::new("test".to_string());
        assert_eq!(connection.next_id(), "1");
        assert_eq!(connection.next_id(), "2");
        assert_eq!(connection.next_id(), "3");
        assert_eq!(connection.request_id, 4);
    }

    #[tokio::test]
    async fn test_mcp_connection_get_tools_empty() {
        let connection = McpConnection::new("test".to_string());
        let tools = connection.get_tools().await;
        assert_eq!(tools.len(), 0);
    }

    #[tokio::test]
    async fn test_mcp_connection_get_tools_version() {
        let connection = McpConnection::new("test".to_string());
        let version = connection.get_tools_version().await;
        assert_eq!(version, 0);
    }

    #[tokio::test]
    async fn test_mcp_connection_tools_version_increment() {
        let connection = McpConnection::new("test".to_string());

        // Initial version should be 0
        assert_eq!(connection.get_tools_version().await, 0);

        // Simulate version increment
        {
            let mut version = connection.tools_version.write().await;
            *version += 1;
        }

        assert_eq!(connection.get_tools_version().await, 1);
    }

    #[test]
    fn test_log_tool_details() {
        let connection = McpConnection::new("test".to_string());
        let tool = McpTool {
            name: "test_tool".to_string(),
            description: Some("A test tool".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "arg1": {
                        "type": "string",
                        "description": "First argument"
                    },
                    "arg2": {
                        "type": "number",
                        "description": "Second argument"
                    }
                },
                "required": ["arg1"]
            }),
        };

        // This should not panic
        connection.log_tool_details(&tool);
    }

    #[test]
    fn test_log_tool_details_no_description() {
        let connection = McpConnection::new("test".to_string());
        let tool = McpTool {
            name: "minimal_tool".to_string(),
            description: None,
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        };

        // This should not panic
        connection.log_tool_details(&tool);
    }

    #[test]
    fn test_log_tool_details_invalid_schema() {
        let connection = McpConnection::new("test".to_string());
        let tool = McpTool {
            name: "invalid_tool".to_string(),
            description: Some("Tool with invalid schema".to_string()),
            input_schema: json!("not an object"),
        };

        // This should not panic
        connection.log_tool_details(&tool);
    }

    // Tests for McpManager
    #[test]
    fn test_mcp_manager_new() {
        let manager = McpManager::new();
        assert!(manager.config_path.is_none());
    }

    #[test]
    fn test_mcp_manager_new_with_config_path() {
        let temp_dir = temp_config_dir();
        let config_path = temp_dir.path().join("config.toml");

        let manager = McpManager::new_with_config_path(config_path.clone());
        assert_eq!(manager.config_path, Some(config_path));
    }

    #[test]
    fn test_mcp_manager_default() {
        let manager = McpManager::default();
        assert!(manager.config_path.is_none());
    }

    #[tokio::test]
    async fn test_mcp_manager_initialize() {
        let manager = McpManager::new();
        let mut config = McpConfig::default();
        config
            .servers
            .insert("test-server".to_string(), test_server_config());

        let result = manager.initialize(config.clone()).await;
        assert!(result.is_ok());

        let loaded_config = manager.load_config().await.unwrap();
        assert_eq!(loaded_config.servers.len(), 1);
        assert!(loaded_config.servers.contains_key("test-server"));
    }

    #[tokio::test]
    async fn test_mcp_manager_get_server() {
        let manager = McpManager::new();
        let mut config = McpConfig::default();
        config
            .servers
            .insert("test-server".to_string(), test_server_config());

        manager.initialize(config).await.unwrap();

        let server = manager.get_server("test-server").await;
        assert!(server.is_some());
        assert_eq!(server.unwrap().command, Some("echo".to_string()));

        let missing_server = manager.get_server("nonexistent").await;
        assert!(missing_server.is_none());
    }

    #[tokio::test]
    async fn test_mcp_manager_is_connected() {
        let manager = McpManager::new();

        // Initially not connected
        assert!(!manager.is_connected("test-server").await);

        // Manually add a connection for testing
        {
            let mut connections = manager.connections.write().await;
            connections.insert(
                "test-server".to_string(),
                McpConnection::new("test-server".to_string()),
            );
        }

        // Now should be connected
        assert!(manager.is_connected("test-server").await);
    }

    #[tokio::test]
    async fn test_mcp_manager_get_tools_version_empty() {
        let manager = McpManager::new();
        let version = manager.get_tools_version().await;
        assert_eq!(version, 0);
    }

    #[tokio::test]
    async fn test_mcp_manager_get_tools_version_with_connections() {
        let manager = McpManager::new();

        // Add connections with different versions
        {
            let mut connections = manager.connections.write().await;

            let conn1 = McpConnection::new("server1".to_string());
            {
                let mut version = conn1.tools_version.write().await;
                *version = 5;
            }
            connections.insert("server1".to_string(), conn1);

            let conn2 = McpConnection::new("server2".to_string());
            {
                let mut version = conn2.tools_version.write().await;
                *version = 3;
            }
            connections.insert("server2".to_string(), conn2);
        }

        // Total version should be sum of all connection versions
        let version = manager.get_tools_version().await;
        assert_eq!(version, 8);
    }

    #[tokio::test]
    async fn test_mcp_manager_list_servers_empty() {
        let manager = McpManager::new();
        let servers = manager.list_servers().await.unwrap();
        assert_eq!(servers.len(), 0);
    }

    #[tokio::test]
    async fn test_mcp_manager_list_servers() {
        let manager = McpManager::new();
        let mut config = McpConfig::default();

        config
            .servers
            .insert("server1".to_string(), test_server_config());

        let mut server2_config = test_server_config();
        server2_config.name = "server2".to_string();
        server2_config.enabled = false;
        config.servers.insert("server2".to_string(), server2_config);

        manager.initialize(config).await.unwrap();

        let servers = manager.list_servers().await.unwrap();
        assert_eq!(servers.len(), 2);

        // Find server1 and server2
        let server1 = servers.iter().find(|(name, _, _)| name == "server1");
        let server2 = servers.iter().find(|(name, _, _)| name == "server2");

        assert!(server1.is_some());
        assert!(server2.is_some());

        let (_, server1_config, server1_connected) = server1.unwrap();
        let (_, server2_config, server2_connected) = server2.unwrap();

        assert!(server1_config.enabled);
        assert!(!server2_config.enabled);
        assert!(!server1_connected);
        assert!(!server2_connected);
    }

    #[tokio::test]
    async fn test_mcp_manager_get_all_tools_empty() {
        let manager = McpManager::new();
        let tools = manager.get_all_tools().await.unwrap();
        assert_eq!(tools.len(), 0);
    }

    #[tokio::test]
    async fn test_mcp_manager_get_all_tools() {
        let manager = McpManager::new();

        // Add connections with tools
        {
            let mut connections = manager.connections.write().await;

            let conn1 = McpConnection::new("server1".to_string());
            {
                let mut tools = conn1.tools.write().await;
                tools.push(McpTool {
                    name: "tool1".to_string(),
                    description: Some("First tool".to_string()),
                    input_schema: json!({"type": "object"}),
                });
                tools.push(McpTool {
                    name: "tool2".to_string(),
                    description: Some("Second tool".to_string()),
                    input_schema: json!({"type": "object"}),
                });
            }
            connections.insert("server1".to_string(), conn1);

            let conn2 = McpConnection::new("server2".to_string());
            {
                let mut tools = conn2.tools.write().await;
                tools.push(McpTool {
                    name: "tool3".to_string(),
                    description: Some("Third tool".to_string()),
                    input_schema: json!({"type": "object"}),
                });
            }
            connections.insert("server2".to_string(), conn2);
        }

        let all_tools = manager.get_all_tools().await.unwrap();
        assert_eq!(all_tools.len(), 3);

        // Check that tools are properly associated with servers
        let tool1 = all_tools.iter().find(|(_, tool)| tool.name == "tool1");
        let tool2 = all_tools.iter().find(|(_, tool)| tool.name == "tool2");
        let tool3 = all_tools.iter().find(|(_, tool)| tool.name == "tool3");

        assert!(tool1.is_some());
        assert!(tool2.is_some());
        assert!(tool3.is_some());

        assert_eq!(tool1.unwrap().0, "server1");
        assert_eq!(tool2.unwrap().0, "server1");
        assert_eq!(tool3.unwrap().0, "server2");
    }

    #[tokio::test]
    async fn test_mcp_manager_disconnect_all() {
        let manager = McpManager::new();

        // Add some connections
        {
            let mut connections = manager.connections.write().await;
            connections.insert(
                "server1".to_string(),
                McpConnection::new("server1".to_string()),
            );
            connections.insert(
                "server2".to_string(),
                McpConnection::new("server2".to_string()),
            );
        }

        assert!(manager.is_connected("server1").await);
        assert!(manager.is_connected("server2").await);

        // Disconnect all
        let result = manager.disconnect_all().await;
        assert!(result.is_ok());

        // Should no longer be connected
        assert!(!manager.is_connected("server1").await);
        assert!(!manager.is_connected("server2").await);
    }

    #[tokio::test]
    async fn test_mcp_manager_call_tool_not_connected() {
        let manager = McpManager::new();

        let result = manager.call_tool("nonexistent", "test_tool", None).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("is not connected"));
    }

    // Test McpClientCapabilities serialization
    #[test]
    fn test_mcp_client_capabilities_serialization() {
        let capabilities = McpClientCapabilities {
            tools: Some(McpToolsCapability {
                list_changed: Some(true),
            }),
            resources: Some(McpResourcesCapability {
                list_changed: Some(true),
            }),
            prompts: Some(McpPromptsCapability {
                list_changed: Some(true),
            }),
        };

        let serialized = serde_json::to_value(&capabilities).unwrap();
        assert!(serialized["tools"]["listChanged"].as_bool().unwrap());
        assert!(serialized["resources"]["listChanged"].as_bool().unwrap());
        assert!(serialized["prompts"]["listChanged"].as_bool().unwrap());
    }

    #[test]
    fn test_mcp_client_capabilities_no_tools() {
        let capabilities = McpClientCapabilities {
            tools: None,
            resources: None,
            prompts: None,
        };

        let serialized = serde_json::to_value(&capabilities).unwrap();
        assert!(serialized["tools"].is_null());
        assert!(serialized["resources"].is_null());
        assert!(serialized["prompts"].is_null());
    }

    // Test McpClientInfo serialization
    #[test]
    fn test_mcp_client_info_serialization() {
        let client_info = McpClientInfo {
            name: "test-client".to_string(),
            version: "1.0.0".to_string(),
        };

        let serialized = serde_json::to_value(&client_info).unwrap();
        assert_eq!(serialized["name"], "test-client");
        assert_eq!(serialized["version"], "1.0.0");
    }

    // Test edge cases for tool schema parsing
    #[test]
    fn test_mcp_tool_empty_properties() {
        let json_str = r#"{
            "name": "no_args_tool",
            "description": "Tool with no arguments",
            "input_schema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }"#;

        let tool: McpTool = serde_json::from_str(json_str).unwrap();
        assert_eq!(tool.name, "no_args_tool");
        assert!(tool.input_schema["properties"]
            .as_object()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_mcp_tool_complex_schema() {
        let json_str = r#"{
            "name": "complex_tool",
            "input_schema": {
                "type": "object",
                "properties": {
                    "nested": {
                        "type": "object",
                        "properties": {
                            "value": {
                                "type": "string"
                            }
                        }
                    },
                    "array": {
                        "type": "array",
                        "items": {
                            "type": "number"
                        }
                    }
                }
            }
        }"#;

        let tool: McpTool = serde_json::from_str(json_str).unwrap();
        assert_eq!(tool.name, "complex_tool");
        assert!(tool.input_schema["properties"]["nested"].is_object());
        assert!(tool.input_schema["properties"]["array"].is_object());
    }

    // Test McpError serialization
    #[test]
    fn test_mcp_error_serialization() {
        let error = McpError {
            code: -32600,
            message: "Invalid Request".to_string(),
            data: Some(json!({"detail": "Missing parameter"})),
        };

        let serialized = serde_json::to_value(&error).unwrap();
        assert_eq!(serialized["code"], -32600);
        assert_eq!(serialized["message"], "Invalid Request");
        assert_eq!(serialized["data"]["detail"], "Missing parameter");
    }

    #[test]
    fn test_mcp_error_no_data() {
        let error = McpError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        };

        let serialized = serde_json::to_value(&error).unwrap();
        assert_eq!(serialized["code"], -32601);
        assert!(serialized["data"].is_null());
    }

    // Test McpResponse serialization
    #[test]
    fn test_mcp_response_success_serialization() {
        let response = McpResponse {
            jsonrpc: "2.0".to_string(),
            id: Some("1".to_string()),
            result: Some(json!({"status": "ok"})),
            error: None,
        };

        let serialized = serde_json::to_value(&response).unwrap();
        assert_eq!(serialized["jsonrpc"], "2.0");
        assert_eq!(serialized["id"], "1");
        assert_eq!(serialized["result"]["status"], "ok");
        assert!(serialized["error"].is_null());
    }

    #[test]
    fn test_mcp_response_error_serialization() {
        let response = McpResponse {
            jsonrpc: "2.0".to_string(),
            id: Some("1".to_string()),
            result: None,
            error: Some(McpError {
                code: -32600,
                message: "Invalid Request".to_string(),
                data: None,
            }),
        };

        let serialized = serde_json::to_value(&response).unwrap();
        assert_eq!(serialized["jsonrpc"], "2.0");
        assert_eq!(serialized["id"], "1");
        assert!(serialized["result"].is_null());
        assert_eq!(serialized["error"]["code"], -32600);
    }

    // Test request ID generation
    #[test]
    fn test_next_id_sequential() {
        let mut connection = McpConnection::new("test".to_string());

        let ids: Vec<String> = (0..10).map(|_| connection.next_id()).collect();

        for (i, id) in ids.iter().enumerate() {
            assert_eq!(id, &(i + 1).to_string());
        }
    }

    // Test McpConnection disconnect
    #[tokio::test]
    async fn test_mcp_connection_disconnect_no_process() {
        let mut connection = McpConnection::new("test".to_string());
        let result = connection.disconnect().await;
        assert!(result.is_ok());
    }
}
