use std::env;
use std::io::{self, IsTerminal, Read, Write};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use futures::future::BoxFuture;
use psychevo_ai::Outcome;
use psychevo_runtime::{
    ApprovalHandler, PermissionApprovalDecision, PermissionApprovalRequest, PermissionMode,
    RunMode, RunOptions, run_live,
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

async fn run_run_command_inner(args: &RunArgs) -> Result<ExitCode> {
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

    let workdir = match &args.dir {
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
    let approval_handler = interactive_approval_handler();

    let result = run_live(RunOptions {
        db_path,
        workdir,
        snapshot_root: Some(home.join("snapshots")),
        session: args.session.clone(),
        continue_latest: args.continue_latest,
        prompt,
        image_inputs: Vec::new(),
        extract_prompt_image_sources: true,
        prompt_display: None,
        max_context_messages: None,
        config_path,
        model: args.model.clone(),
        reasoning_effort: args.variant.map(|variant| variant.as_str().to_string()),
        include_reasoning: args.include_reasoning,
        mode: run_mode,
        permission_mode,
        approval_mode: None,
        approval_handler,
        clarify_enabled: false,
        inherited_env: Some(env_map),
        agent: args.agent.clone(),
        no_agents: args.no_agents,
        no_skills: args.no_skills,
        skill_inputs: args.skill.clone(),
        mcp_servers: Vec::new(),
    })
    .await?;

    if args.format == RunFormatArg::Json {
        for event in &result.events {
            println!("{}", serde_json::to_string(event)?);
        }
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

    let success = result.outcome == Outcome::Normal && result.tool_failures == 0;
    Ok(if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn read_prompt(message: &[String]) -> Result<String> {
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

fn interactive_approval_handler() -> Option<Arc<dyn ApprovalHandler>> {
    (io::stdin().is_terminal() && io::stderr().is_terminal())
        .then(|| Arc::new(CliApprovalHandler) as Arc<dyn ApprovalHandler>)
}

#[derive(Debug)]
struct CliApprovalHandler;

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

fn prompt_for_permission(request: PermissionApprovalRequest) -> PermissionApprovalDecision {
    let mut stderr = io::stderr();
    let _ = writeln!(stderr, "permission required: {}", request.reason);
    let _ = writeln!(stderr, "tool: {}", request.tool_name);
    let _ = writeln!(stderr, "action: {}", request.summary);
    if let Some(rule) = &request.matched_rule {
        let _ = writeln!(stderr, "matched rule: {rule}");
    }
    if request.allow_always
        && let Some(rule) = &request.suggested_rule
    {
        let _ = writeln!(stderr, "suggested always rule: {rule}");
    }
    let prompt = if request.allow_always {
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
    match line.trim().to_ascii_lowercase().as_str() {
        "o" | "once" | "y" | "yes" => PermissionApprovalDecision::allow_once(),
        "s" | "session" => PermissionApprovalDecision::allow_session(),
        "a" | "always" if request.allow_always => PermissionApprovalDecision::allow_always(),
        _ => PermissionApprovalDecision::deny(),
    }
}
