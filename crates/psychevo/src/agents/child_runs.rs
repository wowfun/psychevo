mod lifecycle;
mod policy;

pub(crate) use lifecycle::{
    ChildRun, SpawnAgentArgs, spawn_child_agent_background, spawn_subagent,
};
#[cfg(test)]
pub(crate) use lifecycle::{
    resolve_agent_tool_name, resolved_child_spawn_depth_remaining, validate_task_name,
};
pub(crate) use policy::{
    AGENT_NOTIFICATION_METADATA_KEY, bind_child_model, default_task_name, run_child_agent,
    sanitize_task_name,
};
struct ActiveAgentRunGuard {
    supervisor: super::supervisor::AgentSupervisor,
    id: String,
}

impl ActiveAgentRunGuard {
    fn new(supervisor: super::supervisor::AgentSupervisor, id: String) -> Self {
        Self { supervisor, id }
    }
}

impl Drop for ActiveAgentRunGuard {
    fn drop(&mut self) {
        self.supervisor.remove_unpersisted(&self.id);
    }
}
