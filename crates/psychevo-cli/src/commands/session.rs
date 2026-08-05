use std::env;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Result, anyhow};
use psychevo::{
    Application, Client as FrameworkClient, RefreshThreadContextRequest, Thread, ThreadListQuery,
    ThreadSummary, paths::canonicalize_cwd, session_export::SessionArtifactKind,
    session_export::SessionExportFormat, session_export::SessionExportIncludeSet,
    session_export::SessionExportOptions, session_export::SessionExportWriteResult,
    session_export::default_session_export_filename,
};
use serde_json::{Value, json};

use crate::args::{
    SessionArgs, SessionCommand, SessionExportArgs, SessionExportFormatArg, SessionIdArgs,
    SessionListArgs, SessionRenameArgs, SessionShareArgs,
};
use crate::commands::common::print_json_error;
use crate::env::{
    ensure_home_initialized, env_path, inherited_env, resolve_psychevo_home, resolve_state_db,
};

pub(crate) const SESSION_SOURCES: &[&str] = &["run", "tui"];

pub(crate) async fn run_session_command(args: SessionArgs) -> Result<ExitCode> {
    match run_session_command_inner(&args).await {
        Ok(code) => Ok(code),
        Err(err) if session_json(&args) => {
            print_json_error(&err)?;
            Ok(ExitCode::from(1))
        }
        Err(err) => Err(err),
    }
}

pub(crate) async fn run_session_command_inner(args: &SessionArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    ensure_home_initialized(&home)?;
    let db_path = resolve_state_db(&env_map, &home, &cwd)?;
    let config_path = env_path("PSYCHEVO_CONFIG", &env_map, &cwd)?;
    let cwd = canonicalize_cwd(&cwd)?;
    let mut builder = Application::builder().home(&home).database_path(db_path);
    if let Some(path) = config_path {
        builder = builder.config_path(path);
    }
    let application = builder.build().await?;
    let client = application.client();

    let execution: Result<()> = async {
        match &args.command {
            SessionCommand::List(args) => list_sessions(args, &client, &cwd).await?,
            SessionCommand::Show(args) => {
                let thread = resolve_session(&client, &cwd, &args.session).await?;
                let summary = thread.summary().await?;
                print_session_result("session", &summary, args.json)?;
            }
            SessionCommand::Rename(args) => rename_session(args, &client, &cwd).await?,
            SessionCommand::ReloadContext(args) => {
                reload_context(args, &client, &cwd, env_map).await?
            }
            SessionCommand::Export(args) => export_session(args, &client, &cwd).await?,
            SessionCommand::Share(args) => share_session(args, &client, &cwd).await?,
            SessionCommand::Archive(args) => {
                let summary = mutate_session(args, &client, &cwd, true).await?;
                print_session_result("archived", &summary, args.json)?;
            }
            SessionCommand::Restore(args) => {
                let summary = mutate_session(args, &client, &cwd, false).await?;
                print_session_result("restored", &summary, args.json)?;
            }
        }
        Ok(())
    }
    .await;
    let shutdown = application
        .shutdown()
        .await
        .and_then(|report| report.require_clean());
    execution?;
    shutdown?;
    Ok(ExitCode::SUCCESS)
}

pub(crate) async fn list_sessions(
    args: &SessionListArgs,
    client: &FrameworkClient,
    cwd: &std::path::Path,
) -> Result<()> {
    if args.limit == 0 {
        return Err(anyhow!("--limit must be greater than 0"));
    }
    let mut sessions = Vec::with_capacity(args.limit.min(200));
    let mut cursor = None;
    while sessions.len() < args.limit {
        let page = client
            .list_threads(ThreadListQuery {
                cwd: Some(cwd.to_path_buf()),
                archived: args.archived,
                sources: SESSION_SOURCES
                    .iter()
                    .map(|source| (*source).to_string())
                    .collect(),
                cursor,
                limit: args.limit.saturating_sub(sessions.len()).min(200),
            })
            .await?;
        sessions.extend(page.threads);
        let Some(next) = page.next_cursor else {
            break;
        };
        cursor = Some(next);
    }
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "archived": args.archived,
                "sessions": sessions.iter().map(session_value).collect::<Vec<_>>(),
            }))?
        );
    } else if sessions.is_empty() {
        println!("No sessions found.");
    } else {
        println!("ID\tSource\tUpdated\tMessages\tTitle");
        for session in sessions {
            println!(
                "{}\t{}\t{}\t{}\t{}",
                session.id,
                session.source,
                session.updated_at_ms,
                session.message_count,
                session.title.unwrap_or_default()
            );
        }
    }
    Ok(())
}

pub(crate) async fn rename_session(
    args: &SessionRenameArgs,
    client: &FrameworkClient,
    cwd: &std::path::Path,
) -> Result<()> {
    let thread = resolve_session(client, cwd, &args.session).await?;
    let title = args.title.join(" ");
    thread.set_title(&title).await?;
    let summary = thread.summary().await?;
    print_session_result("renamed", &summary, args.json)
}

pub(crate) async fn reload_context(
    args: &SessionIdArgs,
    client: &FrameworkClient,
    cwd: &Path,
    env_map: std::collections::BTreeMap<String, String>,
) -> Result<()> {
    let result = resolve_session(client, cwd, &args.session)
        .await?
        .refresh_context(RefreshThreadContextRequest {
            mode: None,
            inherited_env: Some(env_map),
            agent: None,
            no_agents: false,
            no_skills: false,
            invalidation_reason: "manual_reload".to_string(),
            notice: None,
        })
        .await?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "action": "reload-context",
                "session": result.thread_id,
                "prefix_hash": result.prefix_hash,
                "version": result.version,
                "provider": result.provider,
                "model": result.model,
                "invalidation_reason": result.invalidation_reason,
            }))?
        );
    } else {
        println!("reloaded context: {}", result.thread_id);
        println!("prefix: {} v{}", result.prefix_hash, result.version);
    }
    Ok(())
}

pub(crate) async fn export_session(
    args: &SessionExportArgs,
    client: &FrameworkClient,
    cwd: &Path,
) -> Result<()> {
    let thread = resolve_session(client, cwd, &args.session).await?;
    let artifact_kind = SessionArtifactKind::Export;
    let options = SessionExportOptions {
        format: args.format.into(),
        include: parse_include(args.include.as_deref(), artifact_kind)?,
        artifact_kind,
    };
    if let Some(output) = &args.output {
        let result = thread.write_export(output, options).await?;
        println!("exported: {}", result.path.display());
    } else {
        let artifact = thread.render_export(options).await?;
        print!("{}", artifact.content);
    }
    Ok(())
}

pub(crate) async fn share_session(
    args: &SessionShareArgs,
    client: &FrameworkClient,
    cwd: &Path,
) -> Result<()> {
    let thread = resolve_session(client, cwd, &args.session).await?;
    let artifact_kind = SessionArtifactKind::Share;
    let output = args.output.clone().unwrap_or_else(|| {
        cwd.join(default_session_export_filename(
            thread.id(),
            SessionExportFormat::Markdown,
            artifact_kind,
        ))
    });
    let options = SessionExportOptions {
        format: SessionExportFormat::Markdown,
        include: parse_include(args.include.as_deref(), artifact_kind)?,
        artifact_kind,
    };
    let result = thread.write_export(&output, options).await?;
    print_share_result(&result, args.json)
}

pub(crate) fn parse_include(
    include: Option<&str>,
    artifact_kind: SessionArtifactKind,
) -> psychevo::Result<SessionExportIncludeSet> {
    match include {
        Some(value) => SessionExportIncludeSet::parse(value, artifact_kind),
        None => Ok(SessionExportIncludeSet::default_for(artifact_kind)),
    }
}

pub(crate) async fn mutate_session(
    args: &SessionIdArgs,
    client: &FrameworkClient,
    cwd: &std::path::Path,
    archive: bool,
) -> Result<ThreadSummary> {
    let thread = resolve_session(client, cwd, &args.session).await?;
    if archive {
        thread.archive().await?;
    } else {
        thread.restore().await?;
    }
    Ok(thread.summary().await?)
}

pub(crate) async fn resolve_session_id(
    client: &FrameworkClient,
    cwd: &std::path::Path,
    raw: &str,
) -> Result<String> {
    let raw = raw.trim();
    if raw == "latest" {
        return client
            .list_threads(ThreadListQuery {
                cwd: Some(cwd.to_path_buf()),
                archived: false,
                sources: SESSION_SOURCES
                    .iter()
                    .map(|source| (*source).to_string())
                    .collect(),
                limit: 1,
                ..ThreadListQuery::default()
            })
            .await?
            .threads
            .into_iter()
            .next()
            .map(|summary| summary.id)
            .ok_or_else(|| anyhow!("no active session found for {}", cwd.display()));
    }
    if raw.is_empty() {
        return Err(anyhow!("session id is required"));
    }
    Ok(raw.to_string())
}

pub(crate) async fn resolve_session(
    client: &FrameworkClient,
    cwd: &Path,
    raw: &str,
) -> Result<Thread> {
    let session_id = resolve_session_id(client, cwd, raw).await?;
    Ok(client.resume_thread(session_id).await?)
}

pub(crate) fn print_session_result(
    action: &str,
    summary: &ThreadSummary,
    as_json: bool,
) -> Result<()> {
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "action": action,
                "session": session_value(summary),
            }))?
        );
    } else {
        println!("{action}: {}", summary.id);
        if let Some(title) = &summary.title {
            println!("title: {title}");
        }
        println!("source: {}", summary.source);
        println!("cwd: {}", summary.cwd);
        println!("messages: {}", summary.message_count);
    }
    Ok(())
}

pub(crate) fn session_value(session: &ThreadSummary) -> Value {
    json!({
        "id": session.id,
        "source": session.source,
        "cwd": session.cwd,
        "model": session.model,
        "provider": session.provider,
        "started_at_ms": session.started_at_ms,
        "updated_at_ms": session.updated_at_ms,
        "ended_at_ms": session.ended_at_ms,
        "end_reason": session.end_reason,
        "archived_at_ms": session.archived_at_ms,
        "message_count": session.message_count,
        "tool_call_count": session.tool_call_count,
        "title": session.title,
    })
}

pub(crate) fn print_share_result(result: &SessionExportWriteResult, as_json: bool) -> Result<()> {
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "action": "share",
                "session": result.session_id,
                "path": result.path,
                "bytes": result.bytes,
                "format": result.format.as_str(),
            }))?
        );
    } else {
        println!("share: {}", result.path.display());
    }
    Ok(())
}

pub(crate) fn session_json(args: &SessionArgs) -> bool {
    match &args.command {
        SessionCommand::List(args) => args.json,
        SessionCommand::Show(args)
        | SessionCommand::Archive(args)
        | SessionCommand::Restore(args)
        | SessionCommand::ReloadContext(args) => args.json,
        SessionCommand::Rename(args) => args.json,
        SessionCommand::Export(_) => false,
        SessionCommand::Share(args) => args.json,
    }
}

impl From<SessionExportFormatArg> for SessionExportFormat {
    fn from(value: SessionExportFormatArg) -> Self {
        match value {
            SessionExportFormatArg::Markdown => Self::Markdown,
            SessionExportFormatArg::Json => Self::Json,
        }
    }
}
