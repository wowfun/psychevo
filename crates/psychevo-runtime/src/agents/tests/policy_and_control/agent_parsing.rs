#[allow(unused_imports)]
pub(crate) use super::*;

#[derive(Debug, Default)]
struct FakeExternalAgentDelegate {
    calls: Arc<Mutex<Vec<ExternalAgentDelegateRequest>>>,
}

impl crate::types::ExternalAgentDelegate for FakeExternalAgentDelegate {
    fn run(
        &self,
        request: ExternalAgentDelegateRequest,
    ) -> BoxFuture<'static, Result<crate::types::ExternalAgentDelegateResult>> {
        self.calls
            .lock()
            .expect("delegate calls lock poisoned")
            .push(request.clone());
        Box::pin(async move {
            Ok(crate::types::ExternalAgentDelegateResult {
                child_session_id: request.child_session_id,
                final_answer: "delegated final".to_string(),
                outcome: Outcome::Normal,
            })
        })
    }
}

#[derive(Debug, Default)]
struct AbortAwareExternalAgentDelegate {
    started: Arc<tokio::sync::Notify>,
}

impl crate::types::ExternalAgentDelegate for AbortAwareExternalAgentDelegate {
    fn run(
        &self,
        request: ExternalAgentDelegateRequest,
    ) -> BoxFuture<'static, Result<crate::types::ExternalAgentDelegateResult>> {
        let started = Arc::clone(&self.started);
        Box::pin(async move {
            started.notify_waiters();
            let mut abort = request.abort.clone();
            abort.wait_for_abort().await;
            Ok(crate::types::ExternalAgentDelegateResult {
                child_session_id: request.child_session_id,
                final_answer: String::new(),
                outcome: Outcome::Aborted,
            })
        })
    }
}

#[derive(Debug, Default)]
struct AbortAwareProvider {
    started: Arc<tokio::sync::Notify>,
}

impl GenerationProvider for AbortAwareProvider {
    fn stream(
        &self,
        _request: psychevo_ai::GenerationRequest,
        mut abort: AbortSignal,
    ) -> BoxFuture<'static, psychevo_ai::Result<psychevo_ai::GenerationStream>> {
        let started = Arc::clone(&self.started);
        Box::pin(async move {
            started.notify_waiters();
            abort.wait_for_abort().await;
            let stream: psychevo_ai::GenerationStream = Box::pin(futures::stream::iter([Ok(
                psychevo_ai::StreamEvent::Done {
                    outcome: Outcome::Aborted,
                    finish_reason: Some("aborted".to_string()),
                },
            )]));
            Ok(stream)
        })
    }
}

fn backend_backed_agent(name: &str, backend: &str) -> AgentDefinition {
    let mut agent = built_in_agent(name, "Backend agent", "Delegates.", None);
    agent.backend = Some(AgentBackendRef {
        name: backend.to_string(),
    });
    agent.entrypoints = default_subagent_entrypoints();
    agent
}

#[test]
pub(crate) fn agent_control_tool_schemas_describe_parameters() {
    let tmp = TempDir::new().expect("tmp");
    let db_path = tmp.path().join("state.sqlite");
    let store = SqliteStore::open(&db_path).expect("store");
    let context = test_agent_tool_context(
        &tmp,
        Arc::new(FakeProvider::new(Vec::new())),
        store,
        db_path,
        "parent".to_string(),
        AgentCatalog::default(),
    );

    for tool in agent_tools(context) {
        assert_tool_schema_descriptions(tool.as_ref());
    }
}

#[test]
pub(crate) fn parses_claude_style_agent_frontmatter() {
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("reviewer.md");
    fs::write(
        &path,
        r#"---
name: reviewer
description: Review code carefully
tools: Read, ExecCommand, Agent
disallowedTools:
  - ExecCommand
memory: true
maxSpawnDepth: 1
---
Review the code.
"#,
    )
    .expect("write");

    let agent = parse_agent_file(&path, AgentSource::Explicit).expect("agent");
    assert_eq!(agent.name, "reviewer");
    assert_eq!(
        agent
            .tool_policy
            .allowed
            .as_ref()
            .unwrap()
            .get("read")
            .map(String::as_str),
        Some("read")
    );
    assert!(
        agent
            .tool_policy
            .allowed
            .as_ref()
            .unwrap()
            .contains("exec_command")
    );
    assert!(
        agent
            .tool_policy
            .allowed
            .as_ref()
            .unwrap()
            .contains("Agent")
    );
    assert!(agent.tool_policy.denied.contains("exec_command"));
    assert_eq!(agent.max_spawn_depth, 1);
    assert!(
        agent
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("memory"))
    );
}

#[test]
pub(crate) fn parses_explicit_optional_contributions_and_diagnoses_unknown_names() {
    let agent = parse_agent_definition_text(
        r#"---
name: flexible-reviewer
description: Review with optional runtime contributions
skills: [review-checklist]
optionalContributions: [instructions, tools, mcp, skills, unknown]
---
Review carefully.
"#,
        "flexible-reviewer",
        None,
        AgentSource::Explicit,
    )
    .expect("agent");

    assert_eq!(
        agent.optional_contributions,
        [
            AgentContribution::Instructions,
            AgentContribution::Tools,
            AgentContribution::Mcp,
            AgentContribution::Skills,
        ]
        .into_iter()
        .collect()
    );
    assert!(
        agent
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("`unknown` is unsupported"))
    );
}

#[test]
pub(crate) fn parses_backend_ref_and_peer_entrypoint_defaults() {
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("cursor-reviewer.md");
    fs::write(
        &path,
        r#"---
name: cursor-reviewer
description: Review code through an ACP peer
backend:
  ref: cursor
---
Review the code.
"#,
    )
    .expect("write");

    let agent = parse_agent_file(&path, AgentSource::Explicit).expect("agent");
    assert_eq!(
        agent.backend.as_ref().map(|backend| backend.name.as_str()),
        Some("cursor")
    );
    assert!(agent.supports_entrypoint(AgentEntrypoint::Peer));
    assert!(agent.supports_entrypoint(AgentEntrypoint::Subagent));
}

#[test]
pub(crate) fn removed_list_search_tool_names_are_not_aliases() {
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("legacy-tools.md");
    fs::write(
        &path,
        r#"---
description: Legacy tools
tools: Search, List, Grep, Glob
---
Use removed tools.
"#,
    )
    .expect("write");

    let agent = parse_agent_file(&path, AgentSource::Explicit).expect("agent");
    let allowed = agent.tool_policy.allowed.as_ref().expect("allowed");
    assert!(allowed.contains("Search"));
    assert!(allowed.contains("List"));
    assert!(allowed.contains("Grep"));
    assert!(allowed.contains("Glob"));
    assert!(!allowed.contains("exec_command"));
    assert_eq!(
        agent
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.message.contains("not a known built-in tool"))
            .count(),
        4
    );
}

#[test]
pub(crate) fn parses_named_agent_tool_restrictions_and_permission_mode() {
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("planner.md");
    fs::write(
        &path,
        r#"---
name: planner
description: Plan with a specific delegate
model: inherit
tools: "Read, spawn_agent(review, explore)"
disallowedTools: "spawn_agent(explore)"
permissionMode: plan
initialPrompt: "draft a plan"
---
Plan the work.
"#,
    )
    .expect("write");

    let agent = parse_agent_file(&path, AgentSource::Explicit).expect("agent");
    assert_eq!(agent.model, None);
    assert_eq!(agent.initial_prompt.as_deref(), Some("draft a plan"));
    assert_eq!(
        agent.tool_policy.permission_mode,
        Some(AgentPermissionMode::Plan)
    );
    assert!(
        agent
            .tool_policy
            .allowed_agents
            .as_ref()
            .expect("allowed agents")
            .contains("review")
    );
    assert!(agent.tool_policy.denied_agents.contains("explore"));
    assert!(agent_allows_tool("read", Some(&agent), RunMode::Default));
    assert!(agent_allows_tool("spawn_agent", Some(&agent), RunMode::Default));
    assert!(!agent_allows_tool(
        "exec_command",
        Some(&agent),
        RunMode::Default
    ));
}
