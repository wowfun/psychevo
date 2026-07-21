use std::collections::BTreeMap;
use std::env;
use std::io::{self, IsTerminal, Read, Write};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use futures::future::BoxFuture;
use psychevo_ai::Outcome;
use psychevo_gateway::{
    Gateway, GatewayInputPart, GatewaySource, ThreadTurnRequest, TranscriptBlockKind,
};
use psychevo_runtime::{
    ApprovalHandler, PermissionApprovalDecision, PermissionApprovalRequest, PermissionMode,
    ProjectContextInstructionMode, RunMode, RunWarning, StateRuntime,
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
    let state = StateRuntime::open(&db_path)?;
    let gateway = Gateway::new(state.clone());
    let source = GatewaySource::new("cli", format!("run:{}", std::process::id()))
        .invocation()
        .with_raw_identity(serde_json::json!({
            "kind": "cli",
            "entrypoint": "run",
            "cwd": cwd.display().to_string(),
        }));

    let mut request = ThreadTurnRequest::new(cwd, vec![GatewayInputPart::Text { text: prompt }]);
    request.thread_id = args.session.clone();
    request.source = Some(source);
    request.policy.snapshot_root = Some(home.join("snapshots"));
    request.policy.continue_latest = args.continue_latest;
    request.policy.extract_prompt_image_sources = true;
    request.policy.config_path = config_path;
    request.policy.project_context_override = project_context_override;
    request.policy.model = args.model.clone();
    request.policy.reasoning_effort = args.variant.map(|variant| variant.as_str().to_string());
    request.policy.runtime_profile_ref = runtime_ref;
    request.policy.control_values = runtime_options;
    request.policy.include_reasoning = args.include_reasoning;
    request.policy.mode = run_mode;
    request.policy.permission_mode = permission_mode;
    request.policy.approval_handler = approval_handler;
    request.policy.inherited_env = Some(env_map);
    request.policy.agent_ref = args.agent.clone();
    request.policy.no_agents = args.no_agents;
    request.policy.no_skills = args.no_skills;
    request.policy.skill_inputs = args.skill.clone();
    request.runtime_source = Some("run".to_string());
    request.continue_sources = vec!["run".to_string()];
    let turn_result = gateway.run_turn(request).await?;
    let committed_entries = turn_result.committed_entries;
    let result = turn_result.result;

    let success = result.outcome == Outcome::Normal && result.tool_failures == 0;
    if args.format == RunFormatArg::Json {
        let thread_id = result.session_id.clone();
        let turn_id = format!("run:{thread_id}");
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "type": "thread.started",
                "threadId": thread_id,
                "selectedSkills": &result.selected_skills,
            }))?
        );
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "type": "turn.started",
                "threadId": result.session_id,
                "turnId": turn_id,
            }))?
        );
        for (index, warning) in result.warnings.iter().enumerate() {
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "type": "entry.completed",
                    "threadId": result.session_id,
                    "turnId": turn_id,
                    "entry": warning_entry_value(index, warning),
                }))?
            );
        }
        for mut entry in committed_entries {
            if !args.include_reasoning {
                entry
                    .blocks
                    .retain(|block| block.kind != TranscriptBlockKind::Reasoning);
            }
            if entry.blocks.is_empty() {
                continue;
            }
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "type": "entry.completed",
                    "threadId": result.session_id,
                    "turnId": turn_id,
                    "entry": entry,
                }))?
            );
        }
        let mut terminal = serde_json::json!({
                "type": if success { "turn.completed" } else { "turn.failed" },
                "threadId": result.session_id,
                "turnId": turn_id,
                "outcome": result.outcome.as_str(),
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
        if result.outcome != Outcome::Normal
            && let Some(reason) = result.terminal_reason
        {
            eprintln!(
                "turn ended: {} - {}",
                result.outcome.as_str(),
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

fn warning_entry_value(index: usize, warning: &RunWarning) -> serde_json::Value {
    serde_json::json!({
        "id": format!("warning:{index}"),
        "threadId": null,
        "turnId": null,
        "messageSeq": null,
        "role": "diagnostic",
        "status": "info",
        "source": "runtime.warning",
        "blocks": [{
            "id": format!("warning:{index}:block:0"),
            "kind": "status",
            "status": "info",
            "order": 0,
            "source": "runtime.warning",
            "title": "warning",
            "body": &warning.message,
            "preview": &warning.message,
            "detail": &warning.message,
            "artifactIds": [],
            "metadata": {
                "kind": &warning.kind,
                "sourcePath": &warning.source_path,
                "suggestion": &warning.suggestion,
            },
            "result": null,
            "createdAtMs": null,
            "updatedAtMs": null
        }],
        "metadata": {
            "kind": &warning.kind,
            "sourcePath": &warning.source_path,
            "suggestion": &warning.suggestion,
        },
        "usage": null,
        "accounting": null,
        "createdAtMs": null,
        "updatedAtMs": null,
    })
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
