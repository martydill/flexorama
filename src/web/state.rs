use crate::agent::Agent;
use crate::config;
use crate::csrf::CsrfManager;
use crate::database::DatabaseManager;
use crate::mcp::McpManager;
use crate::security::{PermissionHandler, PermissionKind, PermissionPrompt};
use crate::skill::SkillManager;
use crate::subagent::SubagentManager;
use anyhow::Result;
use bytes::Bytes;
use chrono::Utc;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};
use uuid::Uuid;

use super::permissions::PermissionRequestDto;

#[derive(Clone)]
pub struct WebState {
    pub agent: Arc<Mutex<Agent>>,
    pub database: Arc<DatabaseManager>,
    pub mcp_manager: Arc<McpManager>,
    pub subagent_manager: Arc<Mutex<SubagentManager>>,
    pub permission_hub: Arc<PermissionHub>,
    pub skill_manager: Arc<Mutex<SkillManager>>,
    pub conversation_agents: Arc<Mutex<HashMap<String, Arc<Mutex<Agent>>>>>,
    pub csrf_manager: Arc<CsrfManager>,
}

pub struct PermissionHub {
    pending: Mutex<HashMap<String, PermissionRequestDto>>,
    responders: Mutex<HashMap<String, oneshot::Sender<Option<usize>>>>,
}

impl PermissionHub {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            responders: Mutex::new(HashMap::new()),
        }
    }

    pub async fn create_request(
        &self,
        request: PermissionRequestDto,
    ) -> oneshot::Receiver<Option<usize>> {
        let (tx, rx) = oneshot::channel();
        let request_id = request.id.clone();
        self.pending
            .lock()
            .await
            .insert(request_id.clone(), request);
        self.responders.lock().await.insert(request_id, tx);
        rx
    }

    pub async fn list_pending(&self, conversation_id: Option<&str>) -> Vec<PermissionRequestDto> {
        let pending = self.pending.lock().await;
        pending
            .values()
            .filter(|req| {
                if let Some(cid) = conversation_id {
                    req.conversation_id.as_deref() == Some(cid)
                } else {
                    true
                }
            })
            .cloned()
            .collect()
    }

    pub async fn resolve(&self, id: &str, selection: Option<usize>) -> bool {
        let sender = self.responders.lock().await.remove(id);
        self.pending.lock().await.remove(id);
        if let Some(sender) = sender {
            let _ = sender.send(selection);
            true
        } else {
            false
        }
    }
}

/// Get or create an agent for a specific conversation
/// This allows multiple conversations to be processed concurrently without blocking
pub async fn get_or_create_conversation_agent(
    state: &WebState,
    conversation_id: &str,
) -> Result<Arc<Mutex<Agent>>> {
    // Check if agent already exists for this conversation
    {
        let agents = state.conversation_agents.lock().await;
        if let Some(agent) = agents.get(conversation_id) {
            return Ok(agent.clone());
        }
    }

    // Create a new agent for this conversation
    // Load config and get settings from template agent
    let config = config::Config::load(None).await?;
    let (model, plan_mode) = {
        let template_agent = state.agent.lock().await;
        (
            template_agent.model().to_string(),
            template_agent.plan_mode(),
        )
    };

    // Create new agent with same configuration (yolo_mode always false for web)
    let mut new_agent = Agent::new_with_plan_mode(config, model, false, plan_mode)
        .await
        .with_database_manager(state.database.clone())
        .with_skill_manager(state.skill_manager.clone());

    // Resume the conversation in the new agent
    new_agent.resume_conversation(conversation_id).await?;

    let agent_arc = Arc::new(Mutex::new(new_agent));

    // Store in pool
    {
        let mut agents = state.conversation_agents.lock().await;
        agents.insert(conversation_id.to_string(), agent_arc.clone());
    }

    Ok(agent_arc)
}

fn permission_kind_label(kind: &PermissionKind) -> &'static str {
    match kind {
        PermissionKind::Bash => "bash",
        PermissionKind::File => "file",
    }
}

pub fn build_permission_handler(
    hub: Arc<PermissionHub>,
    conversation_id: Option<String>,
    stream_sender: Option<mpsc::Sender<Result<Bytes, Infallible>>>,
) -> PermissionHandler {
    Arc::new(move |prompt: PermissionPrompt| {
        let hub = hub.clone();
        let conversation_id = conversation_id.clone();
        let stream_sender = stream_sender.clone();
        Box::pin(async move {
            let request_id = Uuid::new_v4().to_string();
            let request_conversation_id = conversation_id.clone();
            let request = PermissionRequestDto {
                id: request_id.clone(),
                kind: permission_kind_label(&prompt.kind).to_string(),
                title: prompt.summary,
                detail: prompt.detail,
                options: prompt.options,
                conversation_id: request_conversation_id,
                created_at: Utc::now().to_rfc3339(),
            };

            let pending = request.clone();
            let receiver = hub.create_request(request).await;

            if let Some(sender) = &stream_sender {
                if let Ok(text) = serde_json::to_string(&serde_json::json!({
                    "type": "permission_request",
                    "id": pending.id,
                    "kind": pending.kind,
                    "title": pending.title,
                    "detail": pending.detail,
                    "options": pending.options,
                    "conversation_id": pending.conversation_id,
                    "created_at": pending.created_at,
                })) {
                    let _ = sender.try_send(Ok(Bytes::from(text + "\n")));
                }
            }

            match receiver.await {
                Ok(selection) => selection,
                Err(_) => {
                    let _ = hub.resolve(&request_id, None).await;
                    None
                }
            }
        })
    })
}
