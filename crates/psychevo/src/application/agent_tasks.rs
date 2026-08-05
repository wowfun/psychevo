use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use super::Client;
use crate::agents::{AgentRunRecord, AgentSupervisor};
use crate::extensions::SelectedCapabilityRoot;
use crate::types::{ApprovalHandler, McpServerInput, PermissionMode, RunMode};
use crate::{Result, run};

#[derive(Clone)]
pub struct StartAgentTaskRequest {
    pub cwd: PathBuf,
    pub parent_thread_id: Option<String>,
    pub prompt: String,
    pub agent: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub mode: RunMode,
    pub permission_mode: Option<PermissionMode>,
    pub approval_handler: Option<Arc<dyn ApprovalHandler>>,
    pub inherited_env: Option<BTreeMap<String, String>>,
    pub selected_parent_agent: Option<String>,
    pub no_skills: bool,
    pub selected_capability_roots: Vec<SelectedCapabilityRoot>,
    pub skill_inputs: Vec<String>,
    pub mcp_servers: Vec<McpServerInput>,
}

impl StartAgentTaskRequest {
    pub fn new(
        cwd: impl Into<PathBuf>,
        agent: impl Into<String>,
        prompt: impl Into<String>,
    ) -> Self {
        Self {
            cwd: cwd.into(),
            parent_thread_id: None,
            prompt: prompt.into(),
            agent: agent.into(),
            model: None,
            reasoning_effort: None,
            mode: RunMode::Default,
            permission_mode: None,
            approval_handler: None,
            inherited_env: None,
            selected_parent_agent: None,
            no_skills: false,
            selected_capability_roots: Vec::new(),
            skill_inputs: Vec::new(),
            mcp_servers: Vec::new(),
        }
    }
}

impl fmt::Debug for StartAgentTaskRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StartAgentTaskRequest")
            .field("cwd", &self.cwd)
            .field("parent_thread_id", &self.parent_thread_id)
            .field("agent", &self.agent)
            .field("model", &self.model)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("mode", &self.mode)
            .field("permission_mode", &self.permission_mode)
            .field("has_approval_handler", &self.approval_handler.is_some())
            .field("has_inherited_env", &self.inherited_env.is_some())
            .field("selected_parent_agent", &self.selected_parent_agent)
            .field("no_skills", &self.no_skills)
            .field(
                "selected_capability_root_count",
                &self.selected_capability_roots.len(),
            )
            .field("skill_input_count", &self.skill_inputs.len())
            .field("mcp_server_count", &self.mcp_servers.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct AgentTaskReceipt {
    pub thread_id: String,
    pub agent: AgentRunRecord,
}

impl Client {
    pub async fn start_agent_task(
        &self,
        mut request: StartAgentTaskRequest,
    ) -> Result<AgentTaskReceipt> {
        self.ensure_open()?;
        request.inherited_env = Some(self.application_environment(request.inherited_env.take()));
        run::start_agent_task(
            self.inner.state.clone(),
            AgentSupervisor::clone(&self.inner.runtime.agent_supervisor),
            self.inner.config_path.clone(),
            request,
        )
        .await
    }
}
