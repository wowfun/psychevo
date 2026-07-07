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
                    task_name: Some("worker_task".to_string()),
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
    assert_eq!(agent["task_name"], "worker_task");
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
    let task = format!(
        "duplicate_task_{}",
        Uuid::now_v7().to_string().replace('-', "_")
    );
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
                        task_name: Some(task.clone()),
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
        enabled: true,
        file_path: None,
        source: AgentSource::Explicit,
        backend: None,
        entrypoints: default_subagent_entrypoints(),
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
        "spawn_agent",
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
        enabled: true,
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
        required_tools: Vec::new(),
        fallback_for_tools: Vec::new(),
        required_toolsets: Vec::new(),
        fallback_for_toolsets: Vec::new(),
        supported_on_current_platform: true,
        collision_group: Vec::new(),
    };
    let tools = apply_agent_tool_policy(
        vec![
            test_tool("spawn_agent"),
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
        crate::prompt_assembly::MainPromptPrefixInput {
            mode: RunMode::Default,
            cwd: &PathBuf::from("/tmp/repo"),
            selected_agent: Some(&agent),
            agents: &prompt_agents,
            skills: &prompt_skills,
            project_instruction_fragments: &[],
            capabilities: &Default::default(),
            tools_available: !tools.is_empty(),
        },
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
pub(crate) fn prompt_prefix_includes_runtime_cwd_environment() {
    let cwd = PathBuf::from("/tmp/repo/task");
    let assembly = crate::prompt_assembly::assemble_main_prompt_prefix(
        crate::prompt_assembly::MainPromptPrefixInput {
            mode: RunMode::Default,
            cwd: &cwd,
            selected_agent: None,
            agents: &[],
            skills: &[],
            project_instruction_fragments: &[],
            capabilities: &Default::default(),
            tools_available: true,
        },
    );

    let environment_slot = assembly
        .prefix_slots
        .iter()
        .find(|slot| slot.slot == "runtime_environment")
        .expect("runtime environment slot");
    assert_eq!(environment_slot.semantic_role, "base_policy");
    assert!(environment_slot.content.contains("/tmp/repo/task"));
    assert!(environment_slot.content.contains("Relative file paths"));
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
        crate::prompt_assembly::MainPromptPrefixInput {
            mode: RunMode::Default,
            cwd: &PathBuf::from("/tmp/repo"),
            selected_agent: None,
            agents: &[],
            skills: &[],
            project_instruction_fragments: std::slice::from_ref(&fragment),
            capabilities: &developer_caps,
            tools_available: true,
        },
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
        crate::prompt_assembly::MainPromptPrefixInput {
            mode: RunMode::Default,
            cwd: &PathBuf::from("/tmp/repo"),
            selected_agent: None,
            agents: &[],
            skills: &[],
            project_instruction_fragments: &[fragment],
            capabilities: &Default::default(),
            tools_available: true,
        },
    );
    let fallback_slot = fallback_assembly
        .prefix_slots
        .iter()
        .find(|slot| slot.source_kind.as_deref() == Some("project_instruction"))
        .expect("fallback project slot");
    assert_eq!(fallback_slot.provider_role, "system");
}
