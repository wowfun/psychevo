use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use serde_json::{Value, json};

use super::command::{HookCommandExecution, run_hook_command_blocking};
use super::declarations::{matcher_matches, normalize_hook_declarations};
use super::identity::{hook_definition_hash, hook_key};
use super::output::{
    block_reason, bounded_output, parse_hook_output, parse_output_entries,
    parse_permission_decision, parse_updated_input,
};
use super::types::*;
use super::worker::call_worker_hook;

#[derive(Debug, Clone)]
pub struct HookRuntime {
    cwd: PathBuf,
    handlers: Vec<ConfiguredHook>,
}

#[derive(Debug, Clone)]
struct ConfiguredHook {
    metadata: HookMetadata,
    event: HookEventName,
    matcher: Option<String>,
    handler: HookHandler,
    worker: Option<HookWorkerAdapter>,
}

impl HookRuntime {
    pub fn new(cwd: PathBuf, config: HookRuntimeConfig) -> Self {
        let mut handlers = Vec::new();
        let mut display_order = 0usize;
        for source in config.sources {
            let source_kind = HookSourceKind::parse(&source.source_kind);
            let mut declaration_indices: BTreeMap<(HookEventName, String, String), usize> =
                BTreeMap::new();
            for (event, groups) in normalize_hook_declarations(&source.hooks) {
                for group in groups {
                    for handler in group.hooks {
                        let matcher_key = group.matcher.clone().unwrap_or_else(|| "*".to_string());
                        let index_key = (
                            event,
                            matcher_key,
                            handler.handler_type.as_str().to_string(),
                        );
                        let declaration_index = declaration_indices.entry(index_key).or_insert(0);
                        let declaration_index = {
                            let current = *declaration_index;
                            *declaration_index += 1;
                            current
                        };
                        let current_hash =
                            hook_definition_hash(event, group.matcher.as_deref(), &handler);
                        let key = hook_key(
                            &source.source_id,
                            event,
                            group.matcher.as_deref(),
                            handler.handler_type,
                            declaration_index,
                        );
                        let state = config.state.record_for(&key);
                        let managed = source_kind == HookSourceKind::Managed;
                        let trust_status = hook_trust_status(
                            source_kind,
                            state.trusted_hash.as_deref(),
                            &current_hash,
                            config.bypass_trust,
                        );
                        let enabled = state.enabled;
                        let skipped_reason = skipped_reason(
                            source_kind,
                            enabled,
                            trust_status,
                            config.bypass_trust,
                            &handler,
                            source.worker.as_ref(),
                        );
                        handlers.push(ConfiguredHook {
                            metadata: HookMetadata {
                                key,
                                event: event.as_str().to_string(),
                                matcher: group.matcher.clone(),
                                handler_type: handler.handler_type,
                                source_kind: source_kind.as_str().to_string(),
                                source_id: source.source_id.clone(),
                                source_display_name: source.display_name.clone(),
                                plugin_id: (source_kind == HookSourceKind::Plugin)
                                    .then(|| source.source_id.clone()),
                                source_path: source.path.clone(),
                                display_order,
                                enabled,
                                managed,
                                current_hash,
                                trusted_hash: state.trusted_hash.clone(),
                                trust_status,
                                timeout_secs: handler.timeout_secs,
                                status_message: handler.status_message.clone(),
                                skipped_reason,
                            },
                            event,
                            matcher: group.matcher.clone(),
                            handler,
                            worker: source.worker.clone(),
                        });
                        display_order += 1;
                    }
                }
            }
        }
        Self { cwd, handlers }
    }

    pub fn metadata(&self) -> Vec<HookMetadata> {
        self.handlers
            .iter()
            .map(|handler| handler.metadata.clone())
            .collect()
    }

    pub fn run_event(&self, event: &str, payload: &Value) -> HookResponse {
        let Some(event) = HookEventName::parse(event) else {
            return HookResponse {
                diagnostics: vec![format!("unsupported hook event `{event}`")],
                ..HookResponse::default()
            };
        };
        self.run_event_name(event, payload)
    }

    pub fn run_session_start(&self, payload: &Value) -> HookLifecycleOutcome {
        HookLifecycleOutcome::from_response(
            self.run_event_name(HookEventName::SessionStart, payload),
        )
    }

    pub fn run_session_end(&self, payload: &Value) -> HookReadOnlyOutcome {
        HookReadOnlyOutcome::from_response(self.run_event_name(HookEventName::SessionEnd, payload))
    }

    pub fn run_subagent_start(&self, payload: &Value) -> HookLifecycleOutcome {
        HookLifecycleOutcome::from_response(
            self.run_event_name(HookEventName::SubagentStart, payload),
        )
    }

    pub fn run_subagent_stop(&self, payload: &Value) -> HookStopOutcome {
        HookStopOutcome::from_response(self.run_event_name(HookEventName::SubagentStop, payload))
    }

    pub fn run_user_prompt_submit(&self, payload: &Value) -> HookUserPromptSubmitOutcome {
        HookUserPromptSubmitOutcome::from_response(
            self.run_event_name(HookEventName::UserPromptSubmit, payload),
        )
    }

    pub fn run_pre_tool_use(&self, payload: &Value) -> HookPreToolUseOutcome {
        HookPreToolUseOutcome::from_response(
            self.run_event_name(HookEventName::PreToolUse, payload),
        )
    }

    pub fn run_permission_request(&self, payload: &Value) -> HookPermissionRequestOutcome {
        HookPermissionRequestOutcome::from_response(
            self.run_event_name(HookEventName::PermissionRequest, payload),
        )
    }

    pub fn run_post_tool_use(&self, payload: &Value) -> HookPostToolUseOutcome {
        HookPostToolUseOutcome::from_response(
            self.run_event_name(HookEventName::PostToolUse, payload),
        )
    }

    pub fn run_post_llm_call(&self, payload: &Value) -> HookReadOnlyOutcome {
        HookReadOnlyOutcome::from_response(self.run_event_name(HookEventName::PostLLMCall, payload))
    }

    pub fn run_pre_compact(&self, payload: &Value) -> HookLifecycleOutcome {
        HookLifecycleOutcome::from_response(self.run_event_name(HookEventName::PreCompact, payload))
    }

    pub fn run_post_compact(&self, payload: &Value) -> HookLifecycleOutcome {
        HookLifecycleOutcome::from_response(
            self.run_event_name(HookEventName::PostCompact, payload),
        )
    }

    pub fn run_notification(&self, payload: &Value) -> HookReadOnlyOutcome {
        HookReadOnlyOutcome::from_response(
            self.run_event_name(HookEventName::Notification, payload),
        )
    }

    pub fn run_stop(&self, payload: &Value) -> HookStopOutcome {
        HookStopOutcome::from_response(self.run_event_name(HookEventName::Stop, payload))
    }

    fn run_event_name(&self, event: HookEventName, payload: &Value) -> HookResponse {
        let mut response = HookResponse::default();
        let mut command_jobs = Vec::new();
        let (tx, rx) = mpsc::channel();

        for hook in self.handlers.iter().filter(|hook| {
            hook.event == event && matcher_matches(event, hook.matcher.as_deref(), payload)
        }) {
            if let Some(reason) = &hook.metadata.skipped_reason {
                response.summaries.push(skipped_summary(hook, reason));
                continue;
            }
            match hook.handler.handler_type {
                HookHandlerType::Command => {
                    let command = hook.handler.command.clone().unwrap_or_default();
                    let payload = runtime_payload(event, &hook.metadata, &self.cwd, payload);
                    let cwd = self.cwd.clone();
                    let metadata = hook.metadata.clone();
                    let timeout = hook.handler.timeout_secs;
                    let tx = tx.clone();
                    command_jobs.push(thread::spawn(move || {
                        let execution =
                            run_hook_command_blocking(&command, &cwd, &payload, timeout);
                        let _ = tx.send((metadata, execution));
                    }));
                }
                HookHandlerType::Worker => {
                    let started = Instant::now();
                    let summary = match hook.worker.as_ref() {
                        Some(worker) => {
                            let payload =
                                runtime_payload(event, &hook.metadata, &self.cwd, payload);
                            let result = call_worker_hook(worker, &hook.metadata, payload);
                            summary_from_worker_result(hook, started, result)
                        }
                        None => skipped_summary(hook, "worker adapter unavailable"),
                    };
                    fold_summary(event, &mut response, summary);
                }
                HookHandlerType::Prompt => {
                    let mut summary = completed_summary(hook, 0);
                    if let Some(prompt) = &hook.handler.prompt {
                        summary.entries.push(HookRunEntry {
                            kind: "context".to_string(),
                            message: prompt.clone(),
                        });
                        response.context.push(json!({
                            "source": hook.metadata.key,
                            "text": prompt,
                        }));
                    }
                    response.summaries.push(summary);
                }
                HookHandlerType::Agent | HookHandlerType::Unsupported => {
                    response.summaries.push(skipped_summary(
                        hook,
                        if hook.handler.handler_type == HookHandlerType::Agent {
                            "agent hook adapter unavailable"
                        } else {
                            "unsupported hook handler type"
                        },
                    ));
                }
            }
        }

        drop(tx);
        for (metadata, execution) in rx {
            let summary = summary_from_command_execution(event, metadata, execution);
            fold_summary(event, &mut response, summary);
        }
        for job in command_jobs {
            let _ = job.join();
        }
        response
            .summaries
            .sort_by_key(|summary| summary.display_order);
        response
    }
}

fn hook_trust_status(
    source_kind: HookSourceKind,
    trusted_hash: Option<&str>,
    current_hash: &str,
    bypass_trust: bool,
) -> HookTrustStatus {
    if source_kind == HookSourceKind::Managed {
        return HookTrustStatus::Managed;
    }
    if source_kind.trusted_by_source() || (source_kind.requires_hash_review() && bypass_trust) {
        return HookTrustStatus::Trusted;
    }
    match trusted_hash {
        Some(hash) if hash == current_hash => HookTrustStatus::Trusted,
        Some(_) => HookTrustStatus::Modified,
        None => HookTrustStatus::Untrusted,
    }
}

fn skipped_reason(
    source_kind: HookSourceKind,
    enabled: bool,
    trust_status: HookTrustStatus,
    bypass_trust: bool,
    handler: &HookHandler,
    worker: Option<&HookWorkerAdapter>,
) -> Option<String> {
    if !enabled {
        return Some("disabled".to_string());
    }
    if matches!(
        trust_status,
        HookTrustStatus::Untrusted | HookTrustStatus::Modified
    ) && !(source_kind.requires_hash_review() && bypass_trust)
    {
        return Some(trust_status.as_str().to_string());
    }
    match handler.handler_type {
        HookHandlerType::Command if handler.command.as_deref().unwrap_or("").trim().is_empty() => {
            Some("command handler missing command".to_string())
        }
        HookHandlerType::Worker if worker.is_none() => {
            Some("worker adapter unavailable".to_string())
        }
        HookHandlerType::Unsupported => Some("unsupported hook handler type".to_string()),
        _ => None,
    }
}

fn runtime_payload(
    event: HookEventName,
    metadata: &HookMetadata,
    cwd: &Path,
    payload: &Value,
) -> Value {
    json!({
        "hook_event_name": event.as_str(),
        "event": event.as_str(),
        "cwd": cwd,
        "source": {
            "id": metadata.source_id,
            "kind": metadata.source_kind,
            "path": metadata.source_path,
        },
        "hook": {
            "key": metadata.key,
            "handler_type": metadata.handler_type.as_str(),
            "matcher": metadata.matcher,
            "display_order": metadata.display_order,
        },
        "payload": payload,
    })
}

fn summary_from_command_execution(
    event: HookEventName,
    metadata: HookMetadata,
    execution: HookCommandExecution,
) -> HookRunSummary {
    let mut diagnostics = Vec::new();
    let exit_code = execution.status.and_then(|status| status.code());
    let mut status = if execution.timed_out {
        diagnostics.push("hook command timed out".to_string());
        HookRunStatus::TimedOut
    } else if let Some(error) = execution.error {
        diagnostics.push(error);
        HookRunStatus::Failed
    } else if execution.status.is_some_and(|status| status.success()) {
        HookRunStatus::Completed
    } else {
        HookRunStatus::Failed
    };
    let entries = parse_output_entries(event, &execution.stdout, &mut diagnostics);
    let parsed = parse_hook_output(&execution.stdout);
    if event.supports_block()
        && (parsed
            .as_ref()
            .and_then(|value| value.get("continue"))
            .and_then(Value::as_bool)
            == Some(false)
            || (event == HookEventName::PreToolUse && exit_code == Some(2)))
    {
        status = HookRunStatus::Blocked;
    }
    HookRunSummary {
        run_id: format!("hook-run-{}", metadata.display_order),
        event: event.as_str().to_string(),
        handler_type: metadata.handler_type,
        source_kind: metadata.source_kind,
        source_id: metadata.source_id,
        display_order: metadata.display_order,
        status,
        trust_status: metadata.trust_status,
        exit_code,
        stdout: execution.stdout,
        stderr: execution.stderr,
        elapsed_ms: execution.elapsed_ms,
        diagnostics,
        entries,
    }
}

fn summary_from_worker_result(
    hook: &ConfiguredHook,
    started: Instant,
    result: std::result::Result<Value, String>,
) -> HookRunSummary {
    match result {
        Ok(value) => {
            let stdout = bounded_output(value.to_string().as_bytes());
            let mut diagnostics = Vec::new();
            let entries = parse_output_entries(hook.event, &stdout, &mut diagnostics);
            HookRunSummary {
                run_id: format!("hook-run-{}", hook.metadata.display_order),
                event: hook.event.as_str().to_string(),
                handler_type: hook.handler.handler_type,
                source_kind: hook.metadata.source_kind.clone(),
                source_id: hook.metadata.source_id.clone(),
                display_order: hook.metadata.display_order,
                status: HookRunStatus::Completed,
                trust_status: hook.metadata.trust_status,
                exit_code: None,
                stdout: stdout.clone(),
                stderr: String::new(),
                elapsed_ms: started.elapsed().as_millis(),
                diagnostics,
                entries,
            }
        }
        Err(err) => HookRunSummary {
            run_id: format!("hook-run-{}", hook.metadata.display_order),
            event: hook.event.as_str().to_string(),
            handler_type: hook.handler.handler_type,
            source_kind: hook.metadata.source_kind.clone(),
            source_id: hook.metadata.source_id.clone(),
            display_order: hook.metadata.display_order,
            status: HookRunStatus::Failed,
            trust_status: hook.metadata.trust_status,
            exit_code: None,
            stdout: String::new(),
            stderr: err.clone(),
            elapsed_ms: started.elapsed().as_millis(),
            diagnostics: vec![err],
            entries: Vec::new(),
        },
    }
}

fn skipped_summary(hook: &ConfiguredHook, reason: &str) -> HookRunSummary {
    HookRunSummary {
        run_id: format!("hook-run-{}", hook.metadata.display_order),
        event: hook.event.as_str().to_string(),
        handler_type: hook.handler.handler_type,
        source_kind: hook.metadata.source_kind.clone(),
        source_id: hook.metadata.source_id.clone(),
        display_order: hook.metadata.display_order,
        status: HookRunStatus::Skipped,
        trust_status: hook.metadata.trust_status,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        elapsed_ms: 0,
        diagnostics: vec![reason.to_string()],
        entries: Vec::new(),
    }
}

fn completed_summary(hook: &ConfiguredHook, elapsed_ms: u128) -> HookRunSummary {
    HookRunSummary {
        run_id: format!("hook-run-{}", hook.metadata.display_order),
        event: hook.event.as_str().to_string(),
        handler_type: hook.handler.handler_type,
        source_kind: hook.metadata.source_kind.clone(),
        source_id: hook.metadata.source_id.clone(),
        display_order: hook.metadata.display_order,
        status: HookRunStatus::Completed,
        trust_status: hook.metadata.trust_status,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        elapsed_ms,
        diagnostics: Vec::new(),
        entries: Vec::new(),
    }
}

fn fold_summary(event: HookEventName, response: &mut HookResponse, summary: HookRunSummary) {
    if matches!(
        summary.status,
        HookRunStatus::Blocked | HookRunStatus::TimedOut
    ) && response.blocked_reason.is_none()
    {
        response.blocked_reason = Some(block_reason(&summary.stdout, &summary.stderr));
    }
    if event == HookEventName::PermissionRequest
        && let Some(decision) = parse_permission_decision(&summary.stdout)
        && (response.permission_decision != Some(HookPermissionDecision::Deny))
    {
        response.permission_decision = Some(decision);
    }
    if event == HookEventName::PreToolUse
        && let Some(updated) = parse_updated_input(&summary.stdout)
    {
        response.updated_input = Some(updated);
    }
    for entry in &summary.entries {
        match entry.kind.as_str() {
            "context" => response.context.push(json!({
                "source": summary.run_id,
                "text": entry.message,
            })),
            "feedback" => response.feedback.push(entry.message.clone()),
            "compaction_guidance" => response.compaction_guidance.push(entry.message.clone()),
            "model_content" if event == HookEventName::PostToolUse => {
                response.model_content = Some(entry.message.clone());
            }
            "error" | "warning" => response.diagnostics.push(entry.message.clone()),
            _ => {}
        }
    }
    response.diagnostics.extend(summary.diagnostics.clone());
    response.summaries.push(summary);
}
