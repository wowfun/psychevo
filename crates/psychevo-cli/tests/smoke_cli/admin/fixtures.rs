#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn insert_export_fixture_messages(conn: &Connection, session_id: &str) {
    let prompt_prefix_hash = "fixture-prefix-hash";
    let user = serde_json::json!({
        "role": "user",
        "content": [{"text": "hello export"}],
        "timestamp_ms": 1_100
    });
    let assistant = serde_json::json!({
        "role": "assistant",
        "content": [
            {
                "type": "reasoning",
                "text": "private plan",
                "provider_evidence": {"secret": true}
            },
            {"type": "text", "text": "visible answer"},
            {
                "type": "tool_call",
                "id": "call_read",
                "name": "read",
                "arguments": {"path": "fixture.txt"},
                "arguments_json": "{\"path\":\"fixture.txt\"}",
                "arguments_error": null,
                "content_index": 1,
                "call_index": 0
            }
        ],
        "timestamp_ms": 1_200,
        "finish_reason": "tool_calls",
        "outcome": "normal",
        "model": "model",
        "provider": "provider"
    });
    let tool = serde_json::json!({
        "role": "tool_result",
        "tool_call_id": "call_read",
        "tool_name": "read",
        "content": "file body",
        "is_error": false,
        "timestamp_ms": 1_300
    });
    let assistant_final = serde_json::json!({
        "role": "assistant",
        "content": [
            {"type": "text", "text": "final answer after tool"}
        ],
        "timestamp_ms": 1_400,
        "finish_reason": "stop",
        "outcome": "normal",
        "model": "model",
        "provider": "provider"
    });
    for (seq, role, message, content_text, tool_call_id, tool_name, tool_calls_json) in [
        (1, "user", user, Some("hello export"), None, None, None),
        (
            2,
            "assistant",
            assistant,
            Some("visible answer"),
            None,
            None,
            Some(r#"[{"id":"call_read","name":"read"}]"#),
        ),
        (
            3,
            "tool_result",
            tool,
            Some("file body"),
            Some("call_read"),
            Some("read"),
            None,
        ),
        (
            4,
            "assistant",
            assistant_final,
            Some("final answer after tool"),
            None,
            None,
            None,
        ),
    ] {
        conn.execute(
            r#"
            INSERT INTO messages (
                session_id, session_seq, role, timestamp_ms, message_json,
                content_text, tool_call_id, tool_name, tool_calls_json
            ) VALUES (?1, ?2, ?3, ?2, ?4, ?5, ?6, ?7, ?8)
            "#,
            rusqlite::params![
                session_id,
                seq,
                role,
                message.to_string(),
                content_text,
                tool_call_id,
                tool_name,
                tool_calls_json
            ],
        )
        .expect("insert message");
    }
    let prompt_prefix_metadata = serde_json::json!({
        "prompt_prefix": {
            "hash": prompt_prefix_hash,
            "version": 1,
            "created_at_ms": 1_050,
            "provider": "provider",
            "model": "model",
            "tool_declarations_hash": "fixture-tools-hash",
            "invalidation_reason": "new_session",
            "effective_tools": [
                "read",
                "spawn_agent",
                "list_agents",
                "wait_agent",
                "send_message",
                "close_agent",
                "resume_agent",
                "list_skills",
                "view_skill"
            ],
            "agent_catalog_visible": true,
            "visible_agents": ["translate"],
            "skill_catalog_visible": true,
            "project_instructions_visible": true,
            "project_instructions_role": "system"
        }
    });
    conn.execute(
        "UPDATE messages SET metadata_json = ?1 WHERE session_id = ?2 AND role = 'user'",
        rusqlite::params![prompt_prefix_metadata.to_string(), session_id],
    )
    .expect("set user prompt prefix metadata");
    insert_export_fixture_prompt_prefix(conn, session_id, prompt_prefix_hash);
    for (
        context_seq,
        source_kind,
        source_name,
        source_path,
        provider_group,
        block_index,
        context_kind,
        content_text,
        metadata_json,
    ) in [
        (
            1,
            "project_instruction",
            "AGENTS.md",
            "/repo/AGENTS.md",
            "project_instructions",
            0,
            "project_instruction",
            "# AGENTS.md instructions for /repo\n\n<INSTRUCTIONS>\nhidden AGENTS root\n</INSTRUCTIONS>",
            r#"{"included_bytes":18,"directory":"/repo"}"#,
        ),
        (
            2,
            "project_instruction",
            "AGENTS.local.md",
            "/repo/AGENTS.local.md",
            "project_instructions",
            1,
            "project_instruction",
            "# AGENTS.md instructions for /repo\n\n<INSTRUCTIONS>\nhidden AGENTS local\n</INSTRUCTIONS>",
            r#"{"included_bytes":19,"directory":"/repo"}"#,
        ),
        (
            3,
            "selected_skill",
            "reviewer",
            "/skills/reviewer/SKILL.md",
            "selected_skill:0:reviewer",
            0,
            "selected_skill",
            "<skill>\n<name>reviewer</name>\nskill body\n</skill>",
            r#"{"base_dir":"/skills/reviewer"}"#,
        ),
    ] {
        conn.execute(
            r#"
            INSERT INTO context_evidence (
                session_id, prompt_session_seq, context_seq, role, source_kind,
                source_name, source_path, provider_group, provider_block_index,
                context_kind, timestamp_ms, content_text, metadata_json
            ) VALUES (?1, 1, ?2, 'user', ?3, ?4, ?5, ?6, ?7, ?8, 1100, ?9, ?10)
            "#,
            rusqlite::params![
                session_id,
                context_seq,
                source_kind,
                source_name,
                source_path,
                provider_group,
                block_index,
                context_kind,
                content_text,
                metadata_json
            ],
        )
        .expect("insert context evidence");
    }
}

pub(crate) fn insert_export_fixture_prompt_prefix(
    conn: &Connection,
    session_id: &str,
    prefix_hash: &str,
) {
    let slots = serde_json::json!([
        {
            "slot": "base/mode",
            "tier": "base",
            "semantic_role": "base_policy",
            "provider_role": "system",
            "order": 0,
            "content": "Runtime mode: default from prefix snapshot.",
            "content_hash": "base-hash",
            "source_kind": "runtime",
            "source_name": "mode",
            "source_path": null
        },
        {
            "slot": "agent_catalog",
            "tier": "prefix",
            "semantic_role": "developer_prompt",
            "provider_role": "system",
            "order": 1,
            "content": "Available agents:\n- translate: Translate between Chinese and English.",
            "content_hash": "agent-catalog-hash",
            "source_kind": "agent_catalog",
            "source_name": "active_agents",
            "source_path": null
        },
        {
            "slot": "skill_index",
            "tier": "prefix",
            "semantic_role": "developer_prompt",
            "provider_role": "system",
            "order": 2,
            "content": "Available skills:\n- reviewer: Review text carefully.",
            "content_hash": "skill-index-hash",
            "source_kind": "skill_catalog",
            "source_name": "active_skills",
            "source_path": null
        },
        {
            "slot": "project_context:0",
            "tier": "prefix",
            "semantic_role": "developer_prompt",
            "provider_role": "system",
            "order": 3,
            "content": "Project instructions below are policy context, not user task content.\n\n# AGENTS.md instructions for /repo\n\n<INSTRUCTIONS>\nhidden AGENTS root\n</INSTRUCTIONS>",
            "content_hash": "project-root-hash",
            "source_kind": "project_instruction",
            "source_name": "AGENTS.md",
            "source_path": "/repo/AGENTS.md"
        },
        {
            "slot": "project_context:1",
            "tier": "prefix",
            "semantic_role": "developer_prompt",
            "provider_role": "system",
            "order": 4,
            "content": "Project instructions below are policy context, not user task content.\n\n# AGENTS.md instructions for /repo\n\n<INSTRUCTIONS>\nhidden AGENTS local\n</INSTRUCTIONS>",
            "content_hash": "project-local-hash",
            "source_kind": "project_instruction",
            "source_name": "AGENTS.local.md",
            "source_path": "/repo/AGENTS.local.md"
        }
    ]);
    let metadata = serde_json::json!({
        "mode": "default",
        "selected_agent": null,
        "agents_enabled": true,
        "effective_tools": [
            "read",
            "spawn_agent",
            "list_agents",
            "wait_agent",
            "send_message",
            "close_agent",
            "resume_agent",
            "list_skills",
            "view_skill"
        ],
        "agent_catalog_visible": true,
        "visible_agents": ["translate"],
        "skill_catalog_visible": true,
        "project_instructions_visible": true,
        "project_instructions_role": "system"
    });
    conn.execute(
        r#"
        INSERT INTO session_prompt_prefixes (
            session_id, version, created_at_ms, provider, model,
            prefix_hash, tool_declarations_hash, invalidation_reason,
            slots_json, metadata_json
        ) VALUES (?1, 1, 1050, 'provider', 'model', ?2, 'fixture-tools-hash',
            'new_session', ?3, ?4)
        "#,
        rusqlite::params![
            session_id,
            prefix_hash,
            slots.to_string(),
            metadata.to_string()
        ],
    )
    .expect("insert prompt prefix");
}

pub(crate) fn set_export_fixture_session_metadata(conn: &Connection, session_id: &str) {
    let metadata = serde_json::json!({
        "base_url": "https://example.test/v1",
        "mode": "default",
        "reasoning_effort": "medium",
        "model_metadata": {
            "capabilities": {
                "reasoning": true,
                "tool_call": true
            }
        }
    });
    conn.execute(
        "UPDATE sessions SET metadata_json = ?1 WHERE id = ?2",
        rusqlite::params![metadata.to_string(), session_id],
    )
    .expect("set session metadata");
}

pub(crate) fn insert_session(
    conn: &Connection,
    id: &str,
    cwd: &Path,
    source: &str,
    started_at_ms: i64,
    updated_at_ms: i64,
) {
    conn.execute(
        r#"
        INSERT INTO sessions (
            id, source, parent_session_id, cwd, model, provider,
            started_at_ms, updated_at_ms, ended_at_ms, end_reason, archived_at_ms,
            message_count, tool_call_count, title, metadata_json
        ) VALUES (?1, ?2, NULL, ?3, 'model', 'provider',
            ?4, ?5, NULL, NULL, NULL, 0, 0, NULL, NULL)
        "#,
        rusqlite::params![
            id,
            source,
            cwd.to_string_lossy(),
            started_at_ms,
            updated_at_ms
        ],
    )
    .expect("insert session");
}

pub(crate) struct CatalogJsonServer {
    pub(crate) base_url: String,
    pub(crate) requests: Arc<Mutex<Vec<String>>>,
}

impl CatalogJsonServer {
    pub(crate) fn start(body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base_url = format!("http://{}", listener.local_addr().expect("addr"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_thread = Arc::clone(&requests);
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let request = read_http_request(&mut stream);
            requests_for_thread.lock().expect("requests").push(request);
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).expect("response");
        });
        Self { base_url, requests }
    }
}
