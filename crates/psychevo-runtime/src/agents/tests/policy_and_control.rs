#[allow(unused_imports)]
pub(crate) use super::*;

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
tools: "Read, Agent(review, explore)"
disallowedTools: "Agent(explore)"
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
    assert!(agent_allows_tool("Agent", Some(&agent), RunMode::Default));
    assert!(!agent_allows_tool(
        "exec_command",
        Some(&agent),
        RunMode::Default
    ));
}

#[tokio::test]
pub(crate) async fn background_completion_records_mailbox_event_without_parent_user_message() {
    let tmp = TempDir::new().expect("tmp");
    let store = SqliteStore::open(&tmp.path().join("state.sqlite")).expect("store");
    let parent = store
        .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
        .expect("parent");
    let child = store
        .create_child_session_with_metadata(&parent, tmp.path(), "agent", "model", "provider", None)
        .expect("child");
    let record = test_agent_run_record(parent.clone(), Some(child));

    append_parent_agent_mailbox_event(&store, &parent, &record, "normal", "mailbox final")
        .expect("mailbox event");

    assert!(store.load_messages(&parent).expect("messages").is_empty());
    let events = store.load_agent_mailbox_events(&parent).expect("events");
    assert_eq!(events.len(), 1);
    assert!(events[0].content_text.contains("mailbox final"));
    assert!(!events[0].content_text.contains("agent_id"));
    assert!(!events[0].content_text.contains("child_session_id"));
    assert!(
        events[0].payload["content"]
            .as_str()
            .expect("content")
            .contains("<subagent_notification>")
    );
    let metadata = events[0].metadata.as_ref().expect("metadata");
    assert_eq!(metadata["agent_id"], "agent-1");
    assert_eq!(
        metadata["child_session_id"].as_str(),
        events[0].child_session_id.as_deref()
    );
    assert_eq!(
        metadata["model_visible_summary"]["summary"],
        "mailbox final"
    );
}

#[tokio::test]
pub(crate) async fn wait_agent_mailbox_returns_status_without_final_answer() {
    let tmp = TempDir::new().expect("tmp");
    let store = SqliteStore::open(&tmp.path().join("state.sqlite")).expect("store");
    let parent = store
        .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
        .expect("parent");
    let record = test_agent_run_record(parent.clone(), None);
    append_parent_agent_mailbox_event(&store, &parent, &record, "normal", "mailbox final")
        .expect("mailbox event");

    let value = wait_agent_mailbox(&parent, Duration::from_millis(0), &store)
        .await
        .expect("wait");
    assert_eq!(value["timed_out"], false);
    assert_eq!(value["message"], "Wait completed.");
    assert!(value.get("final_answer").is_none());
    assert!(value.get("statuses").is_none());

    let empty_parent = store
        .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
        .expect("empty parent");
    let value = wait_agent_mailbox(&empty_parent, Duration::from_millis(0), &store)
        .await
        .expect("timeout");
    assert_eq!(value["timed_out"], true);
}

#[test]
pub(crate) fn subagent_summary_uses_prompt_task_and_direct_child_tokens() {
    let tmp = TempDir::new().expect("tmp");
    let store = SqliteStore::open(&tmp.path().join("state.sqlite")).expect("store");
    let parent = store
        .create_session_with_metadata(tmp.path(), "run", "parent-model", "provider", None)
        .expect("parent");
    let child = store
        .create_child_session_with_metadata(
            &parent,
            tmp.path(),
            "agent",
            "child-model",
            "provider",
            None,
        )
        .expect("child");
    store
        .append_message_with_metrics(
            &child,
            &Message::Assistant {
                content: vec![AssistantBlock::ToolCall(ToolCallBlock {
                    id: "tool-1".to_string(),
                    name: "read".to_string(),
                    arguments: json!({}),
                    arguments_json: "{}".to_string(),
                    arguments_error: None,
                    content_index: 0,
                    call_index: 0,
                })],
                timestamp_ms: now_ms(),
                finish_reason: Some("tool_calls".to_string()),
                outcome: Outcome::Normal,
                model: Some("child-model".to_string()),
                provider: Some("provider".to_string()),
            },
            Some(json!({
                "input_tokens": 10,
                "output_tokens": 5,
                "reasoning_tokens": 2
            })),
            None,
        )
        .expect("assistant tool call");
    store
        .append_message_with_metrics(
            &child,
            &Message::Assistant {
                content: vec![AssistantBlock::Text {
                    text: "done".to_string(),
                }],
                timestamp_ms: now_ms(),
                finish_reason: Some("stop".to_string()),
                outcome: Outcome::Normal,
                model: Some("child-model".to_string()),
                provider: Some("provider".to_string()),
            },
            Some(json!({
                "prompt_tokens": 3,
                "completion_tokens": 7,
                "total_tokens": 15
            })),
            None,
        )
        .expect("assistant final");

    let id = "agent-summary-1".to_string();
    let mut record = test_agent_run_record(parent, Some(child));
    record.id = id.clone();
    record.task_name = Some(default_task_name("worker", &id));
    record.task = "  First line   with spacing  \nsecond line".to_string();
    let value = subagent_summary_value(Some(&store), &record, false);

    assert!(value.get("agent_id").is_none());
    assert_eq!(value["agent_name"], "worker");
    assert_eq!(value["task"], "First line with spacing");
    assert_eq!(value["status"], "completed");
    assert_eq!(value["exit_reason"], "normal");
    assert_eq!(value["summary"], "mailbox final");
    assert_eq!(value["duration_ms"], 1);
    assert_eq!(value["tool_call_count"], 1);
    assert_eq!(value["model"], "child-model");
    assert_eq!(value["tokens"]["input"], 13);
    assert_eq!(value["tokens"]["output"], 12);
    assert_eq!(value["tokens"]["reasoning"], 2);
    assert_eq!(value["tokens"]["total"], 32);
    assert!(!value.to_string().contains("null"));
}

#[tokio::test]
pub(crate) async fn foreground_agent_tool_result_uses_compact_model_summary() {
    let tmp = TempDir::new().expect("tmp");
    let db_path = tmp.path().join("state.sqlite");
    let store = SqliteStore::open(&db_path).expect("store");
    let parent = store
        .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
        .expect("parent");
    let catalog = AgentCatalog {
        agents: vec![built_in_agent("worker", "Worker", "Work.", None)],
        shadowed_agents: Vec::new(),
        diagnostics: Vec::new(),
    };
    let (_tx, rx) = watch::channel(false);
    let output = spawn_subagent(
        test_agent_tool_context(
            &tmp,
            Arc::new(FakeProvider::new(vec![vec![
                RawStreamEvent::Text("child final".to_string()),
                RawStreamEvent::Done(Outcome::Normal),
            ]])),
            store,
            db_path,
            parent,
            catalog,
        ),
        AgentToolArgs {
            agent_type: Some("worker".to_string()),
            name: None,
            prompt: "Summarize this task.\nDo not echo metadata.".to_string(),
            task_name: None,
            background: Some(false),
            model: None,
            fork_context: false,
            fork_turns: None,
            max_turns: Some(1),
            max_spawn_depth: None,
        },
        "call".to_string(),
        AbortSignal::new(rx),
    )
    .await
    .expect("spawn");

    assert!(output.json.get("child_session_id").is_some());
    assert!(output.json.get("effective_max_spawn_depth").is_some());
    let model_value: Value =
        serde_json::from_str(output.model_content.as_deref().expect("model content"))
            .expect("model json");
    assert_eq!(model_value["agent_name"], "worker");
    assert_eq!(model_value["task"], "Summarize this task.");
    assert_eq!(model_value["status"], "completed");
    assert_eq!(model_value["summary"], "child final");
    assert!(model_value.get("agent_id").is_none());
    assert!(model_value.get("child_session_id").is_none());
    assert!(model_value.get("effective_max_spawn_depth").is_none());
}

#[tokio::test]
pub(crate) async fn list_agents_model_content_uses_compact_control_summaries() {
    let tmp = TempDir::new().expect("tmp");
    let db_path = tmp.path().join("state.sqlite");
    let store = SqliteStore::open(&db_path).expect("store");
    let parent = store
        .create_session_with_metadata(tmp.path(), "run", "model", "provider", None)
        .expect("parent");
    let id = format!("list-agent-{}", Uuid::now_v7());
    {
        let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
        runs.insert(
            id.clone(),
            AgentRunState {
                record: AgentRunRecord {
                    id: id.clone(),
                    task_name: Some(default_task_name("worker", &id)),
                    agent_name: "worker".to_string(),
                    task: "List task\nraw prompt detail".to_string(),
                    parent_session_id: parent.clone(),
                    child_session_id: Some(format!("child-{id}")),
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
                control: None,
            },
        );
    }

    let (_tx, rx) = watch::channel(false);
    let output = ListAgentsTool::new(test_agent_tool_context(
        &tmp,
        Arc::new(FakeProvider::new(Vec::new())),
        store,
        db_path,
        parent,
        AgentCatalog::default(),
    ))
    .execute("call".to_string(), json!({}), AbortSignal::new(rx))
    .await;

    assert_eq!(output.json["agents"][0]["id"], id);
    assert!(output.json["agents"][0].get("child_session_id").is_some());
    let model_value: Value =
        serde_json::from_str(output.model_content.as_deref().expect("model content"))
            .expect("model json");
    let agent = model_value["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .find(|agent| agent["agent_id"] == id)
        .expect("compact agent");
    assert_eq!(agent["task"], "List task");
    assert_eq!(agent["status"], "running");
    assert!(agent.get("child_session_id").is_none());
    assert!(agent.get("effective_max_spawn_depth").is_none());
    assert!(
        !output
            .model_content
            .as_deref()
            .expect("model content")
            .contains("raw prompt detail")
    );

    let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
    runs.remove(&id);
}

#[test]
pub(crate) fn control_targets_resolve_by_model_visible_task_or_report_ambiguity() {
    let task = format!("duplicate task {}", Uuid::now_v7());
    let id_one = format!("target-one-{}", Uuid::now_v7());
    let id_two = format!("target-two-{}", Uuid::now_v7());
    {
        let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
        for id in [&id_one, &id_two] {
            runs.insert(
                id.clone(),
                AgentRunState {
                    record: AgentRunRecord {
                        id: id.clone(),
                        task_name: Some(default_task_name("worker", id)),
                        agent_name: "worker".to_string(),
                        task: task.clone(),
                        parent_session_id: "parent".to_string(),
                        child_session_id: Some(format!("child-{id}")),
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
                    control: None,
                },
            );
        }
    }

    let err = close_agent_id(&task, None).expect_err("ambiguous task");
    assert!(err.to_string().contains("multiple agents match task"));
    assert!(err.to_string().contains("use agent_id"));

    {
        let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
        runs.remove(&id_two);
    }
    let resolved = resume_agent_id(&task, None)
        .expect("resolve task")
        .expect("record");
    assert_eq!(resolved.id, id_one);
    let resolved = resume_agent_id(&id_one, None)
        .expect("resolve agent id")
        .expect("record");
    assert_eq!(resolved.id, id_one);

    let mut runs = AGENT_RUNS.lock().expect("agent run registry poisoned");
    runs.remove(&id_one);
}

#[test]
pub(crate) fn agent_permission_mode_can_only_narrow_parent_mode() {
    let mut agent = AgentDefinition {
        name: "worker".to_string(),
        description: "Worker".to_string(),
        instructions: String::new(),
        file_path: None,
        source: AgentSource::Explicit,
        model: None,
        tool_policy: AgentToolPolicy::default(),
        skills: Vec::new(),
        hooks: None,
        background: None,
        initial_prompt: None,
        max_turns: None,
        max_spawn_depth: 0,
        project_instructions: None,
        effort: None,
        diagnostics: Vec::new(),
    };
    agent.tool_policy.permission_mode = Some(AgentPermissionMode::AcceptEdits);
    assert_eq!(
        narrow_permission_mode_for_agent(PermissionMode::Default, Some(&agent)),
        PermissionMode::Default
    );
    assert_eq!(
        narrow_permission_mode_for_agent(PermissionMode::BypassPermissions, Some(&agent)),
        PermissionMode::AcceptEdits
    );

    agent.tool_policy.permission_mode = Some(AgentPermissionMode::Default);
    assert_eq!(
        narrow_permission_mode_for_agent(PermissionMode::AcceptEdits, Some(&agent)),
        PermissionMode::Default
    );
    assert_eq!(
        narrow_permission_mode_for_agent(PermissionMode::DontAsk, Some(&agent)),
        PermissionMode::DontAsk
    );
}

#[test]
pub(crate) fn empty_tools_array_is_explicit_empty_allowlist() {
    let tmp = TempDir::new().expect("tmp");
    let inherit_path = tmp.path().join("inherit.md");
    fs::write(
        &inherit_path,
        "---\nname: inherit\ndescription: Inherit tools\n---\nBody.\n",
    )
    .expect("write inherit");
    let empty_path = tmp.path().join("translate.md");
    fs::write(
        &empty_path,
        "---\nname: translate\ndescription: Translate only\ntools: []\n---\nTranslate.\n",
    )
    .expect("write empty");
    let empty_string_path = tmp.path().join("empty-string.md");
    fs::write(
        &empty_string_path,
        "---\nname: empty-string\ndescription: Empty string inherits\ntools: ''\n---\nBody.\n",
    )
    .expect("write empty string");

    let inherit = parse_agent_file(&inherit_path, AgentSource::Explicit).expect("inherit");
    let empty = parse_agent_file(&empty_path, AgentSource::Explicit).expect("empty");
    let empty_string =
        parse_agent_file(&empty_string_path, AgentSource::Explicit).expect("empty string");

    assert_eq!(inherit.tool_policy.allowed, None);
    assert!(agent_allows_tool("read", Some(&inherit), RunMode::Default));
    assert_eq!(empty.tool_policy.allowed, Some(BTreeSet::new()));
    for name in [
        "read",
        "write",
        "exec_command",
        "Agent",
        "list_skills",
        "view_skill",
    ] {
        assert!(
            !agent_allows_tool(name, Some(&empty), RunMode::Default),
            "{name} should be blocked"
        );
    }
    assert_eq!(empty_string.tool_policy.allowed, None);
    assert!(agent_allows_tool(
        "read",
        Some(&empty_string),
        RunMode::Default
    ));
}

#[test]
pub(crate) fn clarify_tool_policy_is_plan_safe_and_allow_deny_controllable() {
    let tmp = TempDir::new().expect("tmp");
    let allow_path = tmp.path().join("clarifier.md");
    fs::write(
        &allow_path,
        "---\nname: clarifier\ndescription: Ask user\ntools: Clarify\n---\nAsk.\n",
    )
    .expect("write allow");
    let deny_path = tmp.path().join("no-clarify.md");
    fs::write(
        &deny_path,
        "---\nname: no-clarify\ndescription: No ask\ndisallowedTools: Clarify\n---\nNo ask.\n",
    )
    .expect("write deny");

    let allow = parse_agent_file(&allow_path, AgentSource::Explicit).expect("allow");
    let deny = parse_agent_file(&deny_path, AgentSource::Explicit).expect("deny");
    assert_eq!(
        allow.tool_policy.allowed,
        Some(BTreeSet::from(["clarify".to_string()]))
    );
    assert!(deny.tool_policy.denied.contains("clarify"));
    assert!(allow.diagnostics.is_empty());
    assert!(deny.diagnostics.is_empty());

    let base_tools = vec![
        test_tool("read"),
        test_tool("exec_command"),
        test_tool("clarify"),
    ];
    let allowed = apply_agent_tool_policy(base_tools.clone(), Some(&allow), RunMode::Default)
        .into_iter()
        .map(|tool| tool.name().to_string())
        .collect::<Vec<_>>();
    assert_eq!(allowed, vec!["clarify"]);

    let denied = apply_agent_tool_policy(base_tools.clone(), Some(&deny), RunMode::Default)
        .into_iter()
        .map(|tool| tool.name().to_string())
        .collect::<Vec<_>>();
    assert_eq!(denied, vec!["read", "exec_command"]);

    let plan = apply_agent_tool_policy(base_tools, None, RunMode::Plan)
        .into_iter()
        .map(|tool| tool.name().to_string())
        .collect::<Vec<_>>();
    assert_eq!(plan, vec!["read", "exec_command", "clarify"]);
}

#[test]
pub(crate) fn project_instructions_policy_parses_boolean_and_defaults_to_injected() {
    let tmp = TempDir::new().expect("tmp");
    let omitted_path = tmp.path().join("omitted.md");
    fs::write(
        &omitted_path,
        "---\nname: omitted\ndescription: Omitted policy\n---\nBody.\n",
    )
    .expect("write omitted");
    let null_path = tmp.path().join("null.md");
    fs::write(
        &null_path,
        "---\nname: null-agent\ndescription: Null policy\nprojectInstructions: null\n---\nBody.\n",
    )
    .expect("write null");
    let false_path = tmp.path().join("false.md");
    fs::write(
            &false_path,
            "---\nname: no-project\ndescription: No project instructions\nprojectInstructions: false\n---\nBody.\n",
        )
        .expect("write false");
    let invalid_path = tmp.path().join("invalid.md");
    fs::write(
        &invalid_path,
        "---\nname: invalid\ndescription: Invalid policy\nprojectInstructions: off\n---\nBody.\n",
    )
    .expect("write invalid");

    let omitted = parse_agent_file(&omitted_path, AgentSource::Explicit).expect("omitted");
    let null = parse_agent_file(&null_path, AgentSource::Explicit).expect("null");
    let disabled = parse_agent_file(&false_path, AgentSource::Explicit).expect("false");
    let invalid = parse_agent_file(&invalid_path, AgentSource::Explicit).expect("invalid");

    assert_eq!(omitted.project_instructions, None);
    assert!(agent_project_instructions_enabled(Some(&omitted)));
    assert_eq!(null.project_instructions, None);
    assert!(agent_project_instructions_enabled(Some(&null)));
    assert_eq!(disabled.project_instructions, Some(false));
    assert!(!agent_project_instructions_enabled(Some(&disabled)));
    assert_eq!(invalid.project_instructions, None);
    assert!(agent_project_instructions_enabled(Some(&invalid)));
    assert!(
        invalid
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("projectInstructions"))
    );
    assert_eq!(
        view_agent_value(&disabled)["effective_policy"]["project_instructions"]["visible"],
        false
    );
}

#[test]
pub(crate) fn empty_tools_suppresses_agent_and_skill_prompt_catalogs() {
    let agent = built_in_agent(
        "translate",
        "Translate only",
        "Translate.",
        Some(BTreeSet::new()),
    );
    let worker = built_in_agent("worker", "Worker", "Work.", None);
    let skill = crate::skills::Skill {
        name: "reviewer".to_string(),
        description: "Review code".to_string(),
        file_path: PathBuf::from("/tmp/reviewer/SKILL.md"),
        base_dir: PathBuf::from("/tmp/reviewer"),
        source: crate::skills::SkillSource::Project,
        disable_model_invocation: false,
        category: None,
        tags: Vec::new(),
        related: Vec::new(),
        platforms: Vec::new(),
        required_environment_variables: Vec::new(),
        required_credential_files: Vec::new(),
        setup_help: None,
        compatibility: None,
        license: None,
        allowed_tools: Vec::new(),
        supported_on_current_platform: true,
    };
    let tools = apply_agent_tool_policy(
        vec![
            test_tool("Agent"),
            test_tool("list_skills"),
            test_tool("view_skill"),
        ],
        Some(&agent),
        RunMode::Default,
    );

    let prompt_agents = agent_catalog_for_prompt(&[worker], Some(&agent), &tools);
    let prompt_skills = if skill_catalog_visible_for_tools(&tools) {
        vec![skill]
    } else {
        Vec::new()
    };
    let assembly = crate::prompt_assembly::assemble_main_prompt_prefix(
        RunMode::Default,
        Some(&agent),
        &prompt_agents,
        &prompt_skills,
        &[],
        &Default::default(),
        !tools.is_empty(),
    );

    assert!(tools.is_empty());
    assert!(prompt_agents.is_empty());
    assert!(prompt_skills.is_empty());
    assert!(
        !assembly
            .prefix_slots
            .iter()
            .any(|slot| slot.slot == "agent_catalog")
    );
    assert!(
        !assembly
            .prefix_slots
            .iter()
            .any(|slot| slot.slot == "skill_index")
    );
    assert!(
        assembly.prefix_slots[0]
            .content
            .contains("No callable tools are available")
    );
    assert!(!assembly.prefix_slots[0].content.contains("read, edit"));
}

#[test]
pub(crate) fn project_instructions_are_developer_prompt_slots_with_system_fallback() {
    let fragment = crate::project_instructions::ProjectInstructionFragment {
            source_name: "AGENTS.md".to_string(),
            source_path: PathBuf::from("/tmp/repo/AGENTS.md"),
            directory: PathBuf::from("/tmp/repo"),
            content:
                "# AGENTS.md instructions for /tmp/repo\n\n<INSTRUCTIONS>\nUse Chinese.\n</INSTRUCTIONS>"
                    .to_string(),
            truncated: false,
            original_bytes: 12,
            included_bytes: 12,
        };
    let developer_caps = crate::types::ModelCapabilities {
        developer_role: Some(true),
        ..Default::default()
    };
    let developer_assembly = crate::prompt_assembly::assemble_main_prompt_prefix(
        RunMode::Default,
        None,
        &[],
        &[],
        std::slice::from_ref(&fragment),
        &developer_caps,
        true,
    );
    assert!(
        developer_assembly
            .prefix_contextual_user_messages
            .is_empty()
    );
    let project_slot = developer_assembly
        .prefix_slots
        .iter()
        .find(|slot| slot.source_kind.as_deref() == Some("project_instruction"))
        .expect("project slot");
    assert_eq!(project_slot.provider_role, "developer");
    assert_eq!(project_slot.semantic_role, "developer_prompt");
    assert!(
        project_slot
            .content
            .contains("policy context, not user task content")
    );

    let fallback_assembly = crate::prompt_assembly::assemble_main_prompt_prefix(
        RunMode::Default,
        None,
        &[],
        &[],
        &[fragment],
        &Default::default(),
        true,
    );
    let fallback_slot = fallback_assembly
        .prefix_slots
        .iter()
        .find(|slot| slot.source_kind.as_deref() == Some("project_instruction"))
        .expect("fallback project slot");
    assert_eq!(fallback_slot.provider_role, "system");
}
