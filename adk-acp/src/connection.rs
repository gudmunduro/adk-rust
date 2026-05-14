//! ACP connection management.
//!
//! Wraps the `agent-client-protocol` SDK to manage the lifecycle of a connection
//! to an external ACP agent process.

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use agent_client_protocol::schema::{
    InitializeRequest, ProtocolVersion, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome,
};
use agent_client_protocol::{Agent, Client, ConnectionTo};
use agent_client_protocol_tokio::AcpAgent;
use tracing::{debug, info, warn};

use crate::error::{AcpError, Result};
use crate::permissions::{
    PermissionDecision, PermissionOption, PermissionPolicy, PermissionRequest,
};

/// Configuration for connecting to an ACP agent.
#[derive(Debug, Clone)]
pub struct AcpAgentConfig {
    /// Command to spawn the agent (e.g., "claude-code" or "codex --model o3").
    pub command: String,
    /// Working directory for the agent session.
    pub working_dir: PathBuf,
    /// Whether to auto-approve permission requests (YOLO mode).
    /// Used by `prompt_agent()`. For fine-grained control, use `prompt_agent_with_policy()`.
    pub auto_approve: bool,
}

impl AcpAgentConfig {
    /// Create a new config with a command string.
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            auto_approve: true,
        }
    }

    /// Set the working directory.
    pub fn working_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.working_dir = path.into();
        self
    }

    /// Set whether to auto-approve permission requests.
    pub fn auto_approve(mut self, approve: bool) -> Self {
        self.auto_approve = approve;
        self
    }
}

/// Send a single prompt to an ACP agent and return the response text.
///
/// Uses simple auto-approve/deny based on `config.auto_approve`.
/// For fine-grained permission control, use [`prompt_agent_with_policy`].
pub async fn prompt_agent(config: &AcpAgentConfig, prompt: &str) -> Result<String> {
    let policy =
        if config.auto_approve { PermissionPolicy::AutoApprove } else { PermissionPolicy::DenyAll };
    prompt_agent_with_policy(config, prompt, Arc::new(policy)).await
}

/// Send a single prompt to an ACP agent with a custom permission policy.
///
/// The policy is invoked for each `session/request_permission` message from the agent.
/// This enables HITL (human-in-the-loop) control over sensitive operations.
///
/// # Example
///
/// ```rust,ignore
/// use adk_acp::{AcpAgentConfig, PermissionPolicy, PermissionDecision};
/// use adk_acp::connection::prompt_agent_with_policy;
/// use std::sync::Arc;
///
/// let config = AcpAgentConfig::new("kiro-cli acp");
/// let policy = Arc::new(PermissionPolicy::Custom(Box::new(|req| {
///     if req.title.contains("delete") {
///         PermissionDecision::deny()
///     } else {
///         PermissionDecision::allow_once()
///     }
/// })));
///
/// let response = prompt_agent_with_policy(&config, "Refactor main.rs", policy).await?;
/// ```
pub async fn prompt_agent_with_policy(
    config: &AcpAgentConfig,
    prompt: &str,
    policy: Arc<PermissionPolicy>,
) -> Result<String> {
    info!(command = %config.command, cwd = %config.working_dir.display(), "spawning ACP agent");

    let agent = AcpAgent::from_str(&config.command).map_err(|e| {
        AcpError::InvalidConfig(format!("invalid command '{}': {e}", config.command))
    })?;

    let prompt_text = prompt.to_string();
    let working_dir = config.working_dir.clone();
    let policy_clone = policy.clone();

    let result: std::result::Result<String, agent_client_protocol::Error> = Client
        .builder()
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _cx: ConnectionTo<Agent>| {
                // Convert SDK permission request to our domain type
                let title = request
                    .options
                    .first()
                    .map(|o| o.name.to_string())
                    .unwrap_or_else(|| "Unknown operation".to_string());

                let perm_request = PermissionRequest {
                    title: title.clone(),
                    options: request
                        .options
                        .iter()
                        .map(|o| PermissionOption {
                            id: o.option_id.to_string(),
                            name: o.name.to_string(),
                        })
                        .collect(),
                };

                // Evaluate the policy
                let decision = policy_clone.decide(&perm_request);

                match &decision {
                    PermissionDecision::Allow(option_id) => {
                        debug!(title = %title, decision = %decision, "ACP permission granted");
                        responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                                option_id.clone(),
                            )),
                        ))
                    }
                    PermissionDecision::Deny => {
                        warn!(title = %title, "ACP permission DENIED by policy");
                        responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Cancelled,
                        ))
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
            // Initialize
            connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;

            // Create session, send prompt, and collect response
            let response_text = connection
                .build_session(&working_dir)
                .block_task()
                .run_until(async |mut session| {
                    session.send_prompt(&prompt_text)?;
                    let text = session.read_to_string().await?;
                    Ok(text)
                })
                .await?;

            Ok(response_text)
        })
        .await;

    result.map_err(|e| AcpError::Protocol(e.to_string()))
}
