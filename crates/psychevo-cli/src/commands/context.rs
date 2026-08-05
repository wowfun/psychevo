use std::env;
use std::process::ExitCode;

use anyhow::{Result, anyhow};
use psychevo::{Application, ThreadListQuery, context_usage::format_context_snapshot_text};

use crate::args::ContextArgs;
use crate::env::{
    ensure_home_initialized, env_path, inherited_env, resolve_explicit_path, resolve_psychevo_home,
    resolve_state_db,
};

pub(crate) async fn run_context_command(args: ContextArgs) -> Result<ExitCode> {
    match run_context_command_inner(&args).await {
        Ok(code) => Ok(code),
        Err(err) if args.json => {
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

pub(crate) async fn run_context_command_inner(args: &ContextArgs) -> Result<ExitCode> {
    let session = args
        .session
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("pevo context requires --session <id|latest>"))?;
    if session != "latest" && args.dir.is_some() {
        return Err(anyhow!("--dir is only supported with --session latest"));
    }
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    ensure_home_initialized(&home)?;
    let config_path = env_path("PSYCHEVO_CONFIG", &env_map, &cwd)?;
    let db_path = resolve_state_db(&env_map, &home, &cwd)?;
    let cwd = match &args.dir {
        Some(dir) => resolve_explicit_path(dir, &env_map, &cwd)?,
        None => cwd,
    };
    let mut builder = Application::builder().home(&home).database_path(db_path);
    if let Some(config_path) = config_path {
        builder = builder.config_path(config_path);
    }
    let application = builder.build().await?;
    let client = application.client();
    let snapshot: Result<_> = async {
        let session_id = if session == "latest" {
            client
                .list_threads(ThreadListQuery {
                    cwd: Some(cwd.clone()),
                    sources: vec!["run".to_string(), "tui".to_string()],
                    limit: 1,
                    ..ThreadListQuery::default()
                })
                .await?
                .threads
                .into_iter()
                .next()
                .map(|thread| thread.id)
                .ok_or_else(|| anyhow!("no active session found for {}", cwd.display()))?
        } else {
            session
        };
        client
            .resume_thread(session_id)
            .await?
            .context_snapshot(Some(env_map))
            .await
            .map_err(Into::into)
    }
    .await;
    let shutdown = application
        .shutdown()
        .await
        .and_then(|report| report.require_clean());
    let snapshot = snapshot?;
    shutdown?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
    } else {
        println!("{}", format_context_snapshot_text(&snapshot, false));
    }
    Ok(ExitCode::SUCCESS)
}
