use std::collections::BTreeMap;
use std::env;
use std::io::{self, IsTerminal, Read, Write};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use futures::future::BoxFuture;
use psychevo::{
    Application, ApprovalHandler, PermissionMode, ProjectContextInstructionMode, RunMode,
    StartThreadRequest, ThreadListQuery, TurnEvent, TurnOutcome, TurnRequest,
    application::PermissionApprovalDecision, application::PermissionApprovalRequest,
};
use psychevo_gateway::gateway_event_from_turn_event;
use psychevo_gateway_protocol::events_transcript::{
    GatewayEvent, TranscriptBlock, TranscriptBlockKind, TranscriptBlockStatus, TranscriptEntry,
    TranscriptEntryRole,
};

use crate::args::{PermissionModeArg, RunArgs, RunFormatArg};
use crate::env::{
    ensure_home_initialized, env_path, env_value, inherited_env, resolve_explicit_path,
    resolve_psychevo_home, resolve_state_db,
};

pub(crate) async fn run_run_command(args: RunArgs) -> Result<ExitCode> {
    match run_run_command_inner(&args).await {
        Ok(code) => Ok(code),
        Err(err) if args.format == RunFormatArg::Json => {
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "type": "error",
                    "message": format!("{err:#}"),
                }))?
            );
            Ok(ExitCode::from(1))
        }
        Err(err) => Err(err),
    }
}

pub(crate) async fn run_run_command_inner(args: &RunArgs) -> Result<ExitCode> {
    if args.include_reasoning && args.format != RunFormatArg::Json {
        return Err(anyhow!("--include-reasoning requires --format json"));
    }
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let config_path = env_path("PSYCHEVO_CONFIG", &env_map, &cwd)?;
    let db_path = resolve_state_db(&env_map, &home, &cwd)?;
    let bypass_home = config_path.is_some() && env_value("PSYCHEVO_DB", &env_map).is_some();
    if !bypass_home {
        ensure_home_initialized(&home)?;
    }

    let cwd = match &args.dir {
        Some(dir) => resolve_explicit_path(dir, &env_map, &cwd)?,
        None => cwd,
    };
    let prompt = read_prompt(&args.message)?;
    if prompt.trim().is_empty() {
        return Err(anyhow!("You must provide a message"));
    }
    if args.permission_mode == Some(PermissionModeArg::BypassPermissions) {
        return Err(anyhow!(
            "use --dangerously-skip-permissions to select bypassPermissions"
        ));
    }
    let mode_arg = if args.dangerously_skip_permissions {
        Some(PermissionModeArg::BypassPermissions)
    } else {
        args.permission_mode
    };
    let run_mode = mode_arg
        .map(PermissionModeArg::run_mode)
        .unwrap_or(RunMode::Default);
    let permission_mode = mode_arg
        .map(PermissionModeArg::permission_mode)
        .filter(|mode| *mode != PermissionMode::Default);
    let project_context_override = if args.isolated {
        Some(ProjectContextInstructionMode::Cwd)
    } else {
        args.project_context.map(|mode| mode.mode())
    };
    let approval_handler = interactive_approval_handler();
    let runtime_ref = args
        .runtime
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let runtime_options = args
        .runtime_option
        .iter()
        .cloned()
        .collect::<BTreeMap<_, _>>();
    let mut builder = Application::builder().home(&home).database_path(&db_path);
    if let Some(path) = config_path.as_ref() {
        builder = builder.config_path(path);
    }
    let application = builder.build().await?;
    let client = application.client();
    let execution = async {
        let thread = if let Some(thread_id) = args.session.as_ref() {
            client.resume_thread(thread_id.clone()).await?
        } else if args.continue_latest {
            match client
                .list_threads(ThreadListQuery {
                    cwd: Some(cwd.clone()),
                    archived: false,
                    sources: vec!["run".to_string()],
                    limit: 1,
                    ..ThreadListQuery::default()
                })
                .await?
                .threads
                .into_iter()
                .next()
            {
                Some(snapshot) => client.resume_thread(snapshot.id).await?,
                None => {
                    let mut request = StartThreadRequest::new(&cwd);
                    request.source = "run".to_string();
                    request.metadata = Some(serde_json::json!({
                        "caller": "pevo run",
                        "pid": std::process::id(),
                    }));
                    client.start_thread(request).await?
                }
            }
        } else {
            let mut request = StartThreadRequest::new(&cwd);
            request.source = "run".to_string();
            request.metadata = Some(serde_json::json!({
                "caller": "pevo run",
                "pid": std::process::id(),
            }));
            client.start_thread(request).await?
        };
        let request = TurnRequest::new(prompt)
            .with_identity("run", None)
            .with_model(
                args.model.clone(),
                args.variant.map(|variant| variant.as_str().to_string()),
            )
            .with_runtime(runtime_ref, runtime_options)
            .with_reasoning_output(args.include_reasoning)
            .with_execution_policy(run_mode, permission_mode, config_path)
            .with_approval(approval_handler, false)
            .with_environment(Some(env_map), project_context_override, None)
            .with_agent(args.agent.clone(), args.no_agents, args.no_skills)
            .with_skills(args.skill.clone());
        let handle = thread.start_turn(request).await?;
        let receipt = handle.receipt().clone();
        let mut stream = handle.events();
        let collect = async move {
            let mut events = Vec::new();
            while let Some(event) = stream.next().await {
                events.push(event);
            }
            events
        };
        let (result, events) = tokio::join!(handle.wait(), collect);
        Ok::<_, psychevo::Error>((receipt, events, result?))
    }
    .await;
    let shutdown = application.shutdown().await;
    let (receipt, events, result) = execution?;
    shutdown?.require_clean()?;

    let success = result.outcome == TurnOutcome::Completed && result.tool_failures == 0;
    if args.format == RunFormatArg::Json {
        let thread_id = receipt.thread_id.clone();
        let turn_id = receipt.turn_id.clone();
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "type": "thread.started",
                "threadId": &thread_id,
                "selectedSkills": &result.selected_skills,
            }))?
        );
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "type": "turn.started",
                "threadId": thread_id,
                "turnId": &turn_id,
            }))?
        );
        for warning in &result.warnings {
            let event = json_turn_event(
                &receipt,
                TurnEvent::Warning {
                    data: serde_json::to_value(warning)?,
                },
            )
            .expect("warning events always have a JSON projection");
            println!("{}", serde_json::to_string(&event)?);
        }
        for event in events {
            if !args.include_reasoning
                && matches!(
                    event,
                    TurnEvent::ReasoningDelta { .. } | TurnEvent::ReasoningCompleted { .. }
                )
            {
                continue;
            }
            if let Some(event) = json_turn_event(&receipt, event) {
                println!("{}", serde_json::to_string(&event)?);
            }
        }
        let mut terminal = serde_json::json!({
            "type": if success { "turn.completed" } else { "turn.failed" },
            "threadId": receipt.thread_id,
            "turnId": receipt.turn_id,
            "outcome": turn_outcome_name(result.outcome),
            "toolFailures": result.tool_failures,
            "finalAnswer": result.final_answer,
        });
        if let Some(reason) = result.terminal_reason
            && let Some(object) = terminal.as_object_mut()
        {
            object.insert("terminalReason".to_string(), serde_json::to_value(reason)?);
            object.insert(
                "terminalMessage".to_string(),
                serde_json::json!(reason.message()),
            );
        }
        println!("{}", serde_json::to_string(&terminal)?);
    } else {
        for warning in &result.warnings {
            eprintln!("warning: {}", warning.message);
            if let Some(suggestion) = &warning.suggestion {
                eprintln!("suggestion: {suggestion}");
            }
        }
        println!("{}", result.final_answer);
        if result.outcome != TurnOutcome::Completed
            && let Some(reason) = result.terminal_reason
        {
            eprintln!(
                "turn ended: {} - {}",
                turn_outcome_name(result.outcome),
                reason.message()
            );
        }
    }

    Ok(if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn json_turn_event(receipt: &psychevo::TurnReceipt, event: TurnEvent) -> Option<serde_json::Value> {
    match event {
        TurnEvent::Scoped {
            thread_id,
            turn_id,
            event,
        } => json_turn_event(
            &psychevo::TurnReceipt {
                accepted: true,
                thread_id,
                turn_id,
                client_turn_id: None,
            },
            *event,
        ),
        TurnEvent::Runtime { data } => {
            let projected =
                gateway_event_from_turn_event(&receipt.turn_id, &TurnEvent::Runtime { data })?;
            match projected {
                GatewayEvent::EntryStarted { entry, .. } => {
                    typed_item_event(receipt, psychevo::ItemStage::Started, entry)
                }
                GatewayEvent::EntryUpdated { entry, .. } => {
                    typed_item_event(receipt, psychevo::ItemStage::Updated, entry)
                }
                GatewayEvent::EntryCompleted { entry, .. } => {
                    typed_item_event(receipt, psychevo::ItemStage::Completed, entry)
                }
                _ => None,
            }
        }
        TurnEvent::Message {
            stage,
            message,
            usage,
            metadata,
            accounting,
        } => {
            let mut entry = projected_entry(
                &receipt.turn_id,
                &TurnEvent::Message {
                    stage,
                    message,
                    usage: usage.clone(),
                    metadata: metadata.clone(),
                    accounting: accounting.clone(),
                },
            )?;
            entry.metadata = metadata;
            entry.usage = usage;
            entry.accounting = accounting;
            typed_item_event(receipt, stage, entry)
        }
        TurnEvent::MessageDelta { text } => {
            let entry = projected_entry(&receipt.turn_id, &TurnEvent::MessageDelta { text })?;
            typed_item_event(receipt, psychevo::ItemStage::Updated, entry)
        }
        TurnEvent::Tool { stage, data } => {
            let entry = projected_entry(&receipt.turn_id, &TurnEvent::Tool { stage, data })?;
            typed_item_event(receipt, stage, entry)
        }
        TurnEvent::ReasoningDelta { text } => {
            let entry = projected_entry(&receipt.turn_id, &TurnEvent::ReasoningDelta { text })?;
            typed_item_event(receipt, psychevo::ItemStage::Updated, entry)
        }
        TurnEvent::ReasoningCompleted { text } => {
            let mut entry = projected_entry(
                &receipt.turn_id,
                &TurnEvent::ReasoningDelta {
                    text: text.unwrap_or_default(),
                },
            )?;
            entry.status = TranscriptBlockStatus::Completed;
            for block in &mut entry.blocks {
                block.status = TranscriptBlockStatus::Completed;
            }
            typed_item_event(receipt, psychevo::ItemStage::Completed, entry)
        }
        TurnEvent::InteractionRequested {
            interaction_id,
            kind,
            payload,
        } => typed_item_event(
            receipt,
            psychevo::ItemStage::Started,
            diagnostic_entry(
                receipt,
                format!("interaction:{interaction_id}"),
                interaction_block_kind(&kind),
                TranscriptBlockStatus::NeedsInput,
                Some(kind),
                None,
                Some(serde_json::json!({
                    "interactionId": interaction_id,
                    "payload": payload,
                })),
                "framework.interaction",
            ),
        ),
        TurnEvent::InteractionResolved {
            interaction_id,
            kind,
            reason,
        } => typed_item_event(
            receipt,
            psychevo::ItemStage::Completed,
            diagnostic_entry(
                receipt,
                format!("interaction:{interaction_id}"),
                interaction_block_kind(&kind),
                TranscriptBlockStatus::Completed,
                Some(kind),
                Some(reason.clone()),
                Some(serde_json::json!({
                    "interactionId": interaction_id,
                    "reason": reason,
                })),
                "framework.interaction",
            ),
        ),
        TurnEvent::Warning { data } => {
            let warning_kind = data
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("warning");
            let warning_message = data
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("runtime warning");
            typed_item_event(
                receipt,
                psychevo::ItemStage::Completed,
                diagnostic_entry(
                    receipt,
                    format!("warning:{:016x}", stable_hash_json(&data)),
                    TranscriptBlockKind::Status,
                    TranscriptBlockStatus::Info,
                    Some(warning_kind.to_string()),
                    Some(warning_message.to_string()),
                    Some(data),
                    "framework.warning",
                ),
            )
        }
        TurnEvent::ResyncRequired { missed } => Some(serde_json::json!({
            "type": "stream.resync_required",
            "threadId": receipt.thread_id,
            "turnId": receipt.turn_id,
            "missed": missed,
        })),
        TurnEvent::ActivityChanged { .. }
        | TurnEvent::Accepted { .. }
        | TurnEvent::Started { .. }
        | TurnEvent::Completed { .. }
        | TurnEvent::Failed { .. } => None,
    }
}

fn projected_entry(turn_id: &str, event: &TurnEvent) -> Option<TranscriptEntry> {
    match gateway_event_from_turn_event(turn_id, event)? {
        GatewayEvent::EntryStarted { entry, .. }
        | GatewayEvent::EntryUpdated { entry, .. }
        | GatewayEvent::EntryCompleted { entry, .. } => Some(entry),
        _ => None,
    }
}

fn typed_item_event(
    receipt: &psychevo::TurnReceipt,
    stage: psychevo::ItemStage,
    mut entry: TranscriptEntry,
) -> Option<serde_json::Value> {
    entry.thread_id.clone_from(&receipt.thread_id);
    entry.turn_id = Some(receipt.turn_id.clone());
    sanitize_transcript_entry(&mut entry);
    Some(serde_json::json!({
        "type": format!("item.{}", item_stage_name(stage)),
        "threadId": receipt.thread_id,
        "turnId": receipt.turn_id,
        "item": entry,
    }))
}

#[allow(clippy::too_many_arguments)]
fn diagnostic_entry(
    receipt: &psychevo::TurnReceipt,
    id_suffix: String,
    kind: TranscriptBlockKind,
    status: TranscriptBlockStatus,
    title: Option<String>,
    body: Option<String>,
    metadata: Option<serde_json::Value>,
    source: &str,
) -> TranscriptEntry {
    let now = now_ms();
    let id = format!("live:{}:{id_suffix}", receipt.turn_id);
    TranscriptEntry {
        id: id.clone(),
        thread_id: receipt.thread_id.clone(),
        turn_id: Some(receipt.turn_id.clone()),
        message_seq: None,
        role: TranscriptEntryRole::Diagnostic,
        status,
        source: source.to_string(),
        blocks: vec![TranscriptBlock {
            id: format!("{id}:block"),
            kind,
            status,
            order: 0,
            phase_ordinal: None,
            source: source.to_string(),
            title,
            body: body.clone(),
            preview: body,
            detail: None,
            artifact_ids: Vec::new(),
            metadata,
            result: None,
            created_at_ms: now,
            updated_at_ms: now,
        }],
        metadata: None,
        usage: None,
        accounting: None,
        created_at_ms: now,
        updated_at_ms: now,
    }
}

fn interaction_block_kind(kind: &str) -> TranscriptBlockKind {
    match kind {
        "permission" => TranscriptBlockKind::Permission,
        "clarify" => TranscriptBlockKind::Clarify,
        _ => TranscriptBlockKind::Tool,
    }
}

const TRANSCRIPT_SHORT_TEXT_LIMIT: usize = 512;
const TRANSCRIPT_LONG_TEXT_LIMIT: usize = 8_192;
const TRANSCRIPT_JSON_LIMIT: usize = 4_096;
const TRANSCRIPT_ARTIFACT_LIMIT: usize = 64;

fn sanitize_transcript_entry(entry: &mut TranscriptEntry) {
    entry.id = bounded_identifier(&entry.id);
    entry.metadata = entry.metadata.take().map(bounded_json);
    entry.usage = entry.usage.take().map(bounded_json);
    entry.accounting = entry.accounting.take().map(bounded_json);
    for block in &mut entry.blocks {
        block.id = bounded_identifier(&block.id);
        block.title = block
            .title
            .take()
            .map(|value| capped_chars(&value, TRANSCRIPT_SHORT_TEXT_LIMIT));
        block.preview = block
            .preview
            .take()
            .map(|value| capped_chars(&value, TRANSCRIPT_SHORT_TEXT_LIMIT));
        block.body = block
            .body
            .take()
            .map(|value| capped_chars(&value, TRANSCRIPT_LONG_TEXT_LIMIT));
        block.detail = block
            .detail
            .take()
            .map(|value| capped_chars(&value, TRANSCRIPT_LONG_TEXT_LIMIT));
        block.artifact_ids.truncate(TRANSCRIPT_ARTIFACT_LIMIT);
        for artifact_id in &mut block.artifact_ids {
            *artifact_id = bounded_identifier(artifact_id);
        }
        block.metadata = block.metadata.take().map(bounded_json);
        if let Some(result) = &mut block.result {
            result.content = capped_chars(&result.content, TRANSCRIPT_LONG_TEXT_LIMIT);
            result.metadata = result.metadata.take().map(bounded_json);
        }
    }
}

fn bounded_identifier(value: &str) -> String {
    if value.chars().count() <= TRANSCRIPT_SHORT_TEXT_LIMIT {
        return value.to_string();
    }
    format!(
        "{}#{:016x}",
        capped_chars(value, TRANSCRIPT_SHORT_TEXT_LIMIT - 17),
        stable_hash_bytes(value.as_bytes())
    )
}

fn bounded_json(value: serde_json::Value) -> serde_json::Value {
    let Ok(serialized) = serde_json::to_vec(&value) else {
        return serde_json::json!({
            "truncated": true,
            "originalBytes": 0,
            "preview": "<unserializable>",
        });
    };
    if serialized.len() <= TRANSCRIPT_JSON_LIMIT {
        return value;
    }
    let preview = String::from_utf8_lossy(&serialized);
    serde_json::json!({
        "truncated": true,
        "originalBytes": serialized.len(),
        "preview": capped_chars(&preview, 1_024),
    })
}

fn capped_chars(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        value.to_string()
    } else {
        value.chars().take(limit).collect()
    }
}

fn stable_hash_json(value: &serde_json::Value) -> u64 {
    serde_json::to_vec(value)
        .map(|bytes| stable_hash_bytes(&bytes))
        .unwrap_or_default()
}

fn stable_hash_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}

fn item_stage_name(stage: psychevo::ItemStage) -> &'static str {
    match stage {
        psychevo::ItemStage::Started => "started",
        psychevo::ItemStage::Updated => "updated",
        psychevo::ItemStage::Completed => "completed",
    }
}

#[cfg(test)]
mod json_transcript_tests {
    use super::*;

    fn receipt() -> psychevo::TurnReceipt {
        psychevo::TurnReceipt {
            accepted: true,
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            client_turn_id: None,
        }
    }

    #[test]
    fn message_events_use_stable_typed_transcript_entries() {
        let started = json_turn_event(
            &receipt(),
            TurnEvent::Message {
                stage: psychevo::ItemStage::Started,
                message: serde_json::json!({
                    "role": "assistant",
                    "content": "hello",
                }),
                usage: None,
                metadata: None,
                accounting: None,
            },
        )
        .expect("started");
        let completed = json_turn_event(
            &receipt(),
            TurnEvent::Message {
                stage: psychevo::ItemStage::Completed,
                message: serde_json::json!({
                    "role": "assistant",
                    "content": "hello",
                }),
                usage: Some(serde_json::json!({"inputTokens": 2})),
                metadata: Some(serde_json::json!({"provider": "fake"})),
                accounting: Some(serde_json::json!({"reportedTotalTokens": 3})),
            },
        )
        .expect("completed");

        assert_eq!(started["type"], "item.started");
        assert_eq!(completed["type"], "item.completed");
        assert_eq!(started["item"]["id"], completed["item"]["id"]);
        assert_eq!(
            started["item"]["blocks"][0]["id"],
            completed["item"]["blocks"][0]["id"]
        );
        assert_eq!(completed["item"]["role"], "assistant");
        assert_eq!(completed["item"]["blocks"][0]["kind"], "text");
        assert_eq!(completed["item"]["usage"]["inputTokens"], 2);
        assert_eq!(completed["item"]["accounting"]["reportedTotalTokens"], 3);
        assert!(completed["item"].get("value").is_none());
    }

    #[test]
    fn assistant_delta_is_an_append_only_typed_update() {
        let projected = json_turn_event(
            &receipt(),
            TurnEvent::MessageDelta {
                text: "hello".to_string(),
            },
        )
        .expect("assistant delta");

        assert_eq!(projected["type"], "item.updated");
        assert_eq!(projected["item"]["id"], "live:turn-1:assistant");
        assert_eq!(projected["item"]["blocks"][0]["body"], "hello");
        assert_eq!(
            projected["item"]["blocks"][0]["metadata"]["projection"],
            "assistant_text_delta"
        );
    }

    #[test]
    fn tool_transcript_entries_bound_content_and_arbitrary_json() {
        let projected = json_turn_event(
            &receipt(),
            TurnEvent::Tool {
                stage: psychevo::ItemStage::Completed,
                data: serde_json::json!({
                    "type": "tool_execution_end",
                    "tool_call_id": "call-1",
                    "tool_name": "exec_command",
                    "result": {
                        "model_content": "x".repeat(50_000),
                        "raw_output": {"output": "y".repeat(50_000)},
                    },
                    "outcome": "normal",
                }),
            },
        )
        .expect("tool");

        let block = &projected["item"]["blocks"][0];
        assert_eq!(block["kind"], "shell");
        assert!(
            block["body"]
                .as_str()
                .expect("bounded body")
                .chars()
                .count()
                <= TRANSCRIPT_LONG_TEXT_LIMIT
        );
        assert_eq!(block["metadata"]["truncated"], true);
        assert!(serde_json::to_vec(&projected).expect("serialize").len() < 24_000);
    }
}

fn turn_outcome_name(outcome: TurnOutcome) -> &'static str {
    match outcome {
        TurnOutcome::Completed => "completed",
        TurnOutcome::Stopped => "stopped",
        TurnOutcome::Failed => "failed",
        TurnOutcome::Interrupted => "interrupted",
    }
}

pub(crate) fn read_prompt(message: &[String]) -> Result<String> {
    let mut prompt = message.join(" ");
    if !io::stdin().is_terminal() {
        let mut stdin = String::new();
        io::stdin().read_to_string(&mut stdin)?;
        if !stdin.is_empty() {
            if prompt.is_empty() {
                prompt = stdin;
            } else {
                prompt.push('\n');
                prompt.push_str(&stdin);
            }
        }
    }
    Ok(prompt)
}

pub(crate) fn interactive_approval_handler() -> Option<Arc<dyn ApprovalHandler>> {
    (io::stdin().is_terminal() && io::stderr().is_terminal())
        .then(|| Arc::new(CliApprovalHandler) as Arc<dyn ApprovalHandler>)
}

#[derive(Debug)]
pub(crate) struct CliApprovalHandler;

impl ApprovalHandler for CliApprovalHandler {
    fn timeout_secs(&self) -> u64 {
        60
    }

    fn request_permission(
        &self,
        request: PermissionApprovalRequest,
    ) -> BoxFuture<'static, PermissionApprovalDecision> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || prompt_for_permission(request))
                .await
                .unwrap_or_else(|_| PermissionApprovalDecision::deny())
        })
    }
}

pub(crate) fn prompt_for_permission(
    request: PermissionApprovalRequest,
) -> PermissionApprovalDecision {
    let mut stderr = io::stderr();
    let _ = writeln!(stderr, "permission required: {}", request.reason);
    let _ = writeln!(stderr, "tool: {}", request.tool_name);
    let _ = writeln!(stderr, "action: {}", request.summary);
    if let Some(rule) = &request.matched_rule {
        let _ = writeln!(stderr, "matched rule: {rule}");
    }
    if let Some(filesystem) = &request.filesystem {
        for target in &filesystem.targets {
            let _ = writeln!(stderr, "requested path: {}", target.requested_path);
            if target.requested_path != target.resolved_path {
                let _ = writeln!(stderr, "resolved path:  {}", target.resolved_path);
            }
        }
    }
    if request.allow_always
        && let Some(rule) = &request.suggested_rule
    {
        let _ = writeln!(stderr, "suggested always rule: {rule}");
    }
    let prompt = if request.filesystem.is_some() {
        "Allow? [o]nce, [t]urn directory, [s]ession directory, [d]eny: "
    } else if request.allow_always {
        "Allow? [o]nce, [s]ession, [a]lways, [d]eny: "
    } else {
        "Allow? [o]nce, [s]ession, [d]eny: "
    };
    let _ = write!(stderr, "{prompt}");
    let _ = stderr.flush();
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return PermissionApprovalDecision::deny();
    }
    let choice = line.trim().to_ascii_lowercase();
    if matches!(choice.as_str(), "t" | "turn" | "s" | "session")
        && let Some(filesystem) = &request.filesystem
    {
        if filesystem.scope_candidates.is_empty() {
            return PermissionApprovalDecision::deny();
        }
        for (index, directory) in filesystem.scope_candidates.iter().enumerate() {
            let _ = writeln!(stderr, "  {}. {}", index + 1, directory);
        }
        let _ = write!(
            stderr,
            "Directory [1-{}]: ",
            filesystem.scope_candidates.len()
        );
        let _ = stderr.flush();
        let mut directory = String::new();
        if io::stdin().read_line(&mut directory).is_err() {
            return PermissionApprovalDecision::deny();
        }
        let Some(directory) = directory
            .trim()
            .parse::<usize>()
            .ok()
            .and_then(|index| index.checked_sub(1))
            .and_then(|index| filesystem.scope_candidates.get(index))
            .cloned()
        else {
            return PermissionApprovalDecision::deny();
        };
        return if matches!(choice.as_str(), "t" | "turn") {
            PermissionApprovalDecision::allow_filesystem_turn(directory)
        } else {
            PermissionApprovalDecision::allow_filesystem_session(directory)
        };
    }
    match choice.as_str() {
        "o" | "once" | "y" | "yes" => PermissionApprovalDecision::allow_once(),
        "s" | "session" => PermissionApprovalDecision::allow_session(),
        "a" | "always" if request.allow_always => PermissionApprovalDecision::allow_always(),
        _ => PermissionApprovalDecision::deny(),
    }
}
