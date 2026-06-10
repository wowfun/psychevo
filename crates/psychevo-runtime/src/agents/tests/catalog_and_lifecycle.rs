#[allow(unused_imports)]
pub(crate) use super::*;

#[tokio::test]
pub(crate) async fn agent_name_allowlist_filters_prompt_catalog_and_spawn() {
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("coordinator.md");
    fs::write(
        &path,
        r#"---
name: coordinator
description: Coordinate selected agents
tools: Agent(worker, researcher)
---
Coordinate.
"#,
    )
    .expect("write");
    let coordinator = parse_agent_file(&path, AgentSource::Explicit).expect("coordinator");
    let worker = built_in_agent("worker", "Worker", "Work.", None);
    let researcher = built_in_agent("researcher", "Researcher", "Research.", None);
    let explore = built_in_agent("explore-extra", "Explore", "Explore.", None);
    let catalog = AgentCatalog {
        agents: vec![worker, researcher, explore],
        shadowed_agents: Vec::new(),
        diagnostics: Vec::new(),
    };

    let tools = apply_agent_tool_policy(
        vec![test_tool("Agent"), test_tool("list_agents")],
        Some(&coordinator),
        RunMode::Default,
    );
    let visible = agent_catalog_for_prompt(&catalog.agents, Some(&coordinator), &tools)
        .into_iter()
        .map(|agent| agent.name)
        .collect::<Vec<_>>();
    assert_eq!(visible, vec!["worker", "researcher"]);

    let db_path = tmp.path().join("state.sqlite");
    let store = SqliteStore::open(&db_path).expect("store");
    let parent = store
        .create_session_with_metadata(tmp.path(), "test", "model", "provider", None)
        .expect("parent");
    let (_tx, rx) = watch::channel(false);
    let err = spawn_subagent(
        AgentToolContext {
            provider: Arc::new(FakeProvider::new(Vec::new())),
            model_provider: "provider".to_string(),
            model: "model".to_string(),
            provider_label: "provider".to_string(),
            base_url: "http://127.0.0.1:9/v1".to_string(),
            api_key_env: None,
            reasoning_effort: None,
            context_limit: None,
            generation_metadata: json!({}),
            workdir: tmp.path().to_path_buf(),
            mode: RunMode::Default,
            project_context_mode: Default::default(),
            permission_config: PermissionConfig::default(),
            lsp: Default::default(),
            permission_mode: PermissionMode::Default,
            approval_mode: ApprovalMode::Manual,
            approval_handler: None,
            state: StateRuntime::from_store(db_path, store),
            config_path: None,
            parent_session_id: parent,
            parent_context_snapshot: Vec::new(),
            catalog,
            control_handle: None,
            stream_events: None,
            model_metadata: ModelMetadata::default(),
            env: BTreeMap::new(),
            path_prefixes: Vec::new(),
            sandbox_policy: crate::sandbox::SandboxPolicy::disabled(),
            tool_selection: Default::default(),
            custom_toolsets: BTreeMap::new(),
            allowed_agent_names: coordinator.tool_policy.allowed_agents.clone(),
            denied_agent_names: coordinator.tool_policy.denied_agents.clone(),
            required_agent_names: Vec::new(),
            spawn_depth_remaining: None,
        },
        AgentToolArgs {
            agent_type: Some("explore-extra".to_string()),
            name: None,
            prompt: "explore".to_string(),
            task_name: None,
            background: None,
            model: None,
            fork_context: false,
            fork_turns: None,
            max_turns: None,
            max_spawn_depth: None,
        },
        "call".to_string(),
        AbortSignal::new(rx),
    )
    .await
    .expect_err("unlisted agent should be rejected");
    assert!(err.to_string().contains("not allowed"));
}

#[test]
pub(crate) fn skill_alias_controls_read_only_skill_surface() {
    let tmp = TempDir::new().expect("tmp");
    let allow_path = tmp.path().join("skill-reader.md");
    fs::write(
        &allow_path,
        "---\nname: skill-reader\ndescription: Skill reader\ntools: Skill\n---\nRead skills.\n",
    )
    .expect("write allow");
    let deny_path = tmp.path().join("no-skill-reader.md");
    fs::write(
            &deny_path,
            "---\nname: no-skill-reader\ndescription: No skill read\ndisallowedTools: Skill\n---\nNo skills.\n",
        )
        .expect("write deny");

    let allow = parse_agent_file(&allow_path, AgentSource::Explicit).expect("allow");
    let deny = parse_agent_file(&deny_path, AgentSource::Explicit).expect("deny");
    let skill_tools = vec![
        test_tool("list_skills"),
        test_tool("view_skill"),
        test_tool("create_skill"),
    ];
    let allowed = apply_agent_tool_policy(skill_tools.clone(), Some(&allow), RunMode::Default)
        .into_iter()
        .map(|tool| tool.name().to_string())
        .collect::<Vec<_>>();
    assert_eq!(allowed, vec!["list_skills", "view_skill"]);
    assert!(agent_policy_allows_skill_catalog(&allow));

    let denied = apply_agent_tool_policy(skill_tools, Some(&deny), RunMode::Default)
        .into_iter()
        .map(|tool| tool.name().to_string())
        .collect::<Vec<_>>();
    assert_eq!(denied, vec!["create_skill"]);
    assert!(!skill_catalog_visible_for_tools(&[test_tool(
        "create_skill"
    )]));
    assert!(!agent_policy_allows_skill_catalog(&deny));
}

#[test]
pub(crate) fn unknown_tool_names_are_preserved_with_diagnostics() {
    let tmp = TempDir::new().expect("tmp");
    let path = tmp.path().join("external.md");
    fs::write(
            &path,
            "---\nname: external\ndescription: External tool\ntools: custom_tool\n---\nUse external tool.\n",
        )
        .expect("write");

    let agent = parse_agent_file(&path, AgentSource::Explicit).expect("agent");

    assert!(
        agent
            .tool_policy
            .allowed
            .as_ref()
            .expect("allowed")
            .contains("custom_tool")
    );
    assert!(
        agent
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("custom_tool"))
    );
}

#[test]
pub(crate) fn recursively_discovers_agent_markdown_files() {
    let tmp = TempDir::new().expect("tmp");
    let home = tmp.path().join("home");
    let workdir = tmp.path().join("repo");
    fs::create_dir_all(workdir.join(".psychevo/agents/nested")).expect("dirs");
    fs::write(
        workdir.join(".psychevo/agents/nested/reviewer.md"),
        "---\ndescription: Nested reviewer\n---\nReview.",
    )
    .expect("write");

    let catalog = discover_agents(&AgentDiscoveryOptions {
        home,
        workdir: workdir.clone(),
        env: env(tmp.path()),
        explicit_inputs: Vec::new(),
        no_agents: false,
    })
    .expect("catalog");
    assert!(catalog.agents.iter().any(|agent| agent.name == "reviewer"));
}

#[test]
pub(crate) fn project_agent_wins_over_built_in() {
    let tmp = TempDir::new().expect("tmp");
    let home = tmp.path().join("home");
    let workdir = tmp.path().join("repo");
    fs::create_dir_all(workdir.join(".psychevo/agents")).expect("dirs");
    fs::write(
        workdir.join(".psychevo/agents/general.md"),
        "---\ndescription: Project general\n---\nProject instructions.",
    )
    .expect("write");
    let catalog = discover_agents(&AgentDiscoveryOptions {
        home,
        workdir: workdir.clone(),
        env: env(tmp.path()),
        explicit_inputs: Vec::new(),
        no_agents: false,
    })
    .expect("catalog");
    let general = catalog
        .agents
        .iter()
        .find(|agent| agent.name == "general")
        .expect("general");
    assert_eq!(general.source, AgentSource::Project);
}

#[test]
pub(crate) fn backend_config_generates_peer_agent_definition() {
    let tmp = TempDir::new().expect("tmp");
    let home = tmp.path().join("home");
    let workdir = tmp.path().join("repo");
    fs::create_dir_all(&home).expect("home");
    fs::write(
        home.join("config.toml"),
        r#"[agents.backends.cursor]
kind = "acp"
description = "Cursor ACP coding agent."
command = "cursor-agent"
args = ["--acp"]
client_capabilities = ["fs.read", "terminal"]
"#,
    )
    .expect("config");

    let catalog = discover_agents(&AgentDiscoveryOptions {
        home,
        workdir,
        env: env(tmp.path()),
        explicit_inputs: Vec::new(),
        no_agents: false,
    })
    .expect("catalog");
    let agent = catalog
        .agents
        .iter()
        .find(|agent| agent.name == "cursor")
        .expect("generated cursor");
    assert_eq!(agent.source, AgentSource::Generated);
    assert_eq!(
        agent.backend.as_ref().map(|backend| backend.name.as_str()),
        Some("cursor")
    );
    assert!(agent.supports_entrypoint(AgentEntrypoint::Peer));
    assert!(agent.supports_entrypoint(AgentEntrypoint::Subagent));
    let value = list_agents_value(&catalog);
    assert_eq!(value["agents"][0]["generated"], true);
}

#[test]
pub(crate) fn markdown_agent_shadows_generated_backend_agent() {
    let tmp = TempDir::new().expect("tmp");
    let home = tmp.path().join("home");
    let workdir = tmp.path().join("repo");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(workdir.join(".psychevo/agents")).expect("agents");
    fs::write(
        home.join("config.toml"),
        r#"[agents.backends.cursor]
kind = "acp"
description = "Generated Cursor agent."
command = "cursor-agent"
"#,
    )
    .expect("config");
    fs::write(
        workdir.join(".psychevo/agents/cursor.md"),
        r#"---
description: Project Cursor wrapper.
backend:
  ref: cursor
entrypoints: [subagent]
---
Use project-specific review instructions.
"#,
    )
    .expect("agent");

    let catalog = discover_agents(&AgentDiscoveryOptions {
        home,
        workdir,
        env: env(tmp.path()),
        explicit_inputs: Vec::new(),
        no_agents: false,
    })
    .expect("catalog");
    let active = catalog
        .agents
        .iter()
        .find(|agent| agent.name == "cursor")
        .expect("active cursor");
    assert_eq!(active.source, AgentSource::Project);
    assert!(!active.supports_entrypoint(AgentEntrypoint::Peer));
    assert!(active.supports_entrypoint(AgentEntrypoint::Subagent));
    let shadowed = catalog
        .shadowed_agents
        .iter()
        .find(|agent| agent.name == "cursor")
        .expect("shadowed cursor");
    assert_eq!(shadowed.source, AgentSource::Generated);
}

#[test]
pub(crate) fn command_bearing_markdown_agent_surfaces_catalog_diagnostic() {
    let tmp = TempDir::new().expect("tmp");
    let home = tmp.path().join("home");
    let workdir = tmp.path().join("repo");
    fs::create_dir_all(workdir.join(".psychevo/agents")).expect("agents");
    fs::write(
        workdir.join(".psychevo/agents/cursor.md"),
        r#"---
description: Invalid Cursor wrapper.
command: cursor-agent
---
Invalid.
"#,
    )
    .expect("agent");

    let catalog = discover_agents(&AgentDiscoveryOptions {
        home,
        workdir,
        env: env(tmp.path()),
        explicit_inputs: Vec::new(),
        no_agents: false,
    })
    .expect("catalog");
    assert!(!catalog.agents.iter().any(|agent| agent.name == "cursor"));
    assert!(
        catalog
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("backend.ref"))
    );
}

#[test]
pub(crate) fn selected_agent_instruction_includes_description_and_body() {
    let agent = built_in_agent(
        "translate",
        "Detect the source language automatically.",
        "Preserve punctuation and return only the translation.",
        None,
    );

    let main = format_selected_agent_instruction(&agent, AgentInvocationRole::Main);
    assert!(main.contains("Main session agent: translate"));
    assert!(main.contains("Purpose:\nDetect the source language automatically."));
    assert!(main.contains("Instructions:\nPreserve punctuation"));

    let child = format_selected_agent_instruction(&agent, AgentInvocationRole::Subagent);
    assert!(child.contains("Child agent: translate"));
    assert!(child.contains("Purpose:\nDetect the source language automatically."));
}

#[test]
pub(crate) fn duplicate_agents_are_available_as_shadowed_definitions() {
    let tmp = TempDir::new().expect("tmp");
    let home = tmp.path().join("home");
    let workdir = tmp.path().join("repo");
    fs::create_dir_all(workdir.join(".psychevo/agents")).expect("project dirs");
    fs::create_dir_all(home.join("agents")).expect("global dirs");
    fs::write(
        workdir.join(".psychevo/agents/review.md"),
        "---\ndescription: Project review\n---\nProject instructions.",
    )
    .expect("write project");
    fs::write(
        home.join("agents/review.md"),
        "---\ndescription: Global review\n---\nGlobal instructions.",
    )
    .expect("write global");

    let catalog = discover_agents(&AgentDiscoveryOptions {
        home,
        workdir: workdir.clone(),
        env: env(tmp.path()),
        explicit_inputs: Vec::new(),
        no_agents: false,
    })
    .expect("catalog");

    let active = catalog
        .agents
        .iter()
        .find(|agent| agent.name == "review")
        .expect("active review");
    assert_eq!(active.source, AgentSource::Project);
    let shadowed = catalog
        .shadowed_agents
        .iter()
        .find(|agent| agent.name == "review")
        .expect("shadowed review");
    assert_eq!(shadowed.source, AgentSource::Global);
    assert!(
        catalog
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == "collision")
    );
}

#[test]
pub(crate) fn agent_tool_name_alias_and_conflict_resolution() {
    let args = AgentToolArgs {
        agent_type: None,
        name: Some("translate".to_string()),
        prompt: "translate this".to_string(),
        task_name: None,
        background: None,
        model: None,
        fork_context: false,
        fork_turns: None,
        max_turns: None,
        max_spawn_depth: None,
    };
    assert_eq!(
        resolve_agent_tool_name(&args, &[]).expect("alias"),
        "translate"
    );

    let args = AgentToolArgs {
        agent_type: Some("general".to_string()),
        name: Some("translate".to_string()),
        prompt: "translate this".to_string(),
        task_name: None,
        background: None,
        model: None,
        fork_context: false,
        fork_turns: None,
        max_turns: None,
        max_spawn_depth: None,
    };
    let err = resolve_agent_tool_name(&args, &[]).expect_err("conflict");
    assert!(err.to_string().contains("conflict"));
}

#[test]
pub(crate) fn required_agent_mention_supplies_omitted_agent_type() {
    let args = AgentToolArgs {
        agent_type: None,
        name: None,
        prompt: "translate this".to_string(),
        task_name: None,
        background: None,
        model: None,
        fork_context: false,
        fork_turns: None,
        max_turns: None,
        max_spawn_depth: None,
    };

    assert_eq!(
        resolve_agent_tool_name(&args, &["translate".to_string()]).expect("required"),
        "translate"
    );
    assert_eq!(
        resolve_agent_tool_name(&args, &[]).expect("default"),
        "general"
    );
    let err = resolve_agent_tool_name(&args, &["translate".to_string(), "review".to_string()])
        .expect_err("ambiguous");
    assert!(err.to_string().contains("multiple agents"));
}

#[test]
pub(crate) fn max_spawn_depth_defaults_to_leaf_and_decrements() {
    assert_eq!(resolved_child_spawn_depth_remaining(None, 0, None), 0);
    assert_eq!(resolved_child_spawn_depth_remaining(None, 1, None), 1);
    assert_eq!(resolved_child_spawn_depth_remaining(None, 0, Some(1)), 1);
    assert_eq!(resolved_child_spawn_depth_remaining(Some(1), 0, None), 0);
    assert_eq!(resolved_child_spawn_depth_remaining(Some(1), 1, None), 0);
    assert_eq!(
        resolved_child_spawn_depth_remaining(
            None,
            MAX_AGENT_SPAWN_DEPTH_CAP.saturating_add(1),
            None
        ),
        MAX_AGENT_SPAWN_DEPTH_CAP
    );
}

#[test]
pub(crate) fn pause_new_spawns_state_is_explicit() {
    set_agent_spawn_paused(false);
    assert!(!agent_spawn_paused());
    let previous = set_agent_spawn_paused(true);
    assert!(!previous);
    assert!(agent_spawn_paused());
    set_agent_spawn_paused(false);
}

#[test]
pub(crate) fn child_session_summary_uses_latest_assistant_usage_tokens() {
    let tmp = TempDir::new().expect("tmp");
    let store = SqliteStore::open(&tmp.path().join("state.sqlite")).expect("store");
    let session = store
        .create_session_with_metadata(tmp.path(), "agent", "mock-model", "mock", None)
        .expect("session");
    for (text, total) in [("first", 10), ("latest", 25)] {
        store
            .append_message_with_metrics(
                &session,
                &Message::Assistant {
                    content: vec![AssistantBlock::Text {
                        text: text.to_string(),
                    }],
                    timestamp_ms: now_ms(),
                    finish_reason: Some("stop".to_string()),
                    outcome: Outcome::Normal,
                    model: Some("mock-model".to_string()),
                    provider: Some("mock".to_string()),
                },
                Some(json!({"total_tokens": total})),
                None,
            )
            .expect("assistant message");
    }
    let summary = store
        .session_summary(&session)
        .expect("summary")
        .expect("session summary");
    let value = agent_child_session_summary_value(&store, &summary);

    assert_eq!(value["latest_total_tokens"], 25);
    assert_eq!(value["latest_usage"]["total_tokens"], 25);
}

#[test]
pub(crate) fn stop_agent_with_grace_marks_live_run_interrupted() {
    let id = format!("test-stop-{}-{:?}", now_ms(), std::thread::current().id());
    let (control, _receivers) = ControlHandle::new();
    {
        let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
        runs.insert(
            id.clone(),
            AgentRunState {
                record: AgentRunRecord {
                    id: id.clone(),
                    task_name: Some("test-stop".to_string()),
                    agent_name: "general".to_string(),
                    task: "stop gracefully".to_string(),
                    parent_session_id: "parent".to_string(),
                    child_session_id: Some("child".to_string()),
                    role: AgentInvocationRole::Subagent,
                    background: true,
                    status: AgentRunStatus::Running,
                    edge_status: Some(AgentEdgeStatus::Open),
                    started_at_ms: now_ms(),
                    ended_at_ms: None,
                    outcome: None,
                    final_answer: None,
                    error: None,
                    effective_max_spawn_depth: Some(0),
                },
                control: Some(control),
            },
        );
    }

    let previous = stop_agent_id_with_grace(&id, None, Duration::ZERO)
        .expect("stop")
        .expect("previous record");
    assert_eq!(previous.status, AgentRunStatus::Running);

    let record = {
        let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
        runs.remove(&id).expect("run state").record
    };
    assert_eq!(record.status, AgentRunStatus::Interrupted);
    assert_eq!(record.edge_status, Some(AgentEdgeStatus::Closed));
    assert_eq!(record.outcome.as_deref(), Some("interrupted"));
    assert!(record.ended_at_ms.is_some());
}

#[test]
pub(crate) fn pre_tool_hook_exit_two_blocks_with_stderr() {
    let tmp = TempDir::new().expect("tmp");
    let hooks = json!({
        "PreToolUse": ["cat >/dev/null; echo blocked >&2; exit 2"]
    });
    let blocked = run_hook_commands(
        Some(&hooks),
        "PreToolUse",
        tmp.path(),
        &json!({"tool": "read"}),
    );
    assert_eq!(blocked.as_deref(), Some("blocked"));
}

#[test]
pub(crate) fn mcp_server_scope_filters_canonical_mcp_tools() {
    let agent = built_in_agent("mcp-test", "MCP test", "Test", None);
    let mut agent = agent;
    agent.tool_policy.mcp_servers = ["repo".to_string()].into_iter().collect();
    assert!(agent_allows_tool(
        "mcp:repo:read",
        Some(&agent),
        RunMode::Default
    ));
    assert!(!agent_allows_tool(
        "mcp:other:read",
        Some(&agent),
        RunMode::Default
    ));
}
