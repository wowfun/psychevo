use std::env;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Result, anyhow};
use psychevo_runtime::{
    ReloadContextOptions, SessionArtifactKind, SessionExportFormat, SessionExportIncludeSet,
    SessionExportOptions, SessionExportWriteResult, SessionSummary, SqliteStore,
    canonicalize_workdir, default_session_export_filename, reload_session_context,
    render_session_export, write_session_export,
};
use serde_json::{Value, json};

use crate::args::{
    SessionArgs, SessionCommand, SessionExportArgs, SessionExportFormatArg, SessionIdArgs,
    SessionListArgs, SessionRenameArgs, SessionShareArgs,
};
use crate::commands::common::print_json_error;
use crate::env::{ensure_home_initialized, inherited_env, resolve_psychevo_home, resolve_state_db};

const SESSION_SOURCES: &[&str] = &["run", "tui"];

pub(crate) fn run_session_command(args: SessionArgs) -> Result<ExitCode> {
    match run_session_command_inner(&args) {
        Ok(code) => Ok(code),
        Err(err) if session_json(&args) => {
            print_json_error(&err)?;
            Ok(ExitCode::from(1))
        }
        Err(err) => Err(err),
    }
}

fn run_session_command_inner(args: &SessionArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    ensure_home_initialized(&home)?;
    let db_path = resolve_state_db(&env_map, &home, &cwd)?;
    let store = SqliteStore::open(&db_path)?;
    let workdir = canonicalize_workdir(&cwd)?;

    match &args.command {
        SessionCommand::List(args) => list_sessions(args, &store, &workdir)?,
        SessionCommand::Show(args) => {
            let session_id = resolve_session_id(&store, &workdir, &args.session)?;
            let summary = store
                .session_summary(&session_id)?
                .ok_or_else(|| anyhow!("session not found: {session_id}"))?;
            print_session_result("session", &summary, args.json)?;
        }
        SessionCommand::Rename(args) => rename_session(args, &store, &workdir)?,
        SessionCommand::ReloadContext(args) => {
            reload_context(args, &store, &workdir, &db_path, env_map)?
        }
        SessionCommand::Export(args) => export_session(args, &store, &workdir)?,
        SessionCommand::Share(args) => share_session(args, &store, &workdir)?,
        SessionCommand::Archive(args) => {
            let summary = mutate_session(args, &store, &workdir, |store, session_id| {
                store.archive_session(session_id)
            })?;
            print_session_result("archived", &summary, args.json)?;
        }
        SessionCommand::Restore(args) => {
            let summary = mutate_session(args, &store, &workdir, |store, session_id| {
                store.restore_session(session_id)
            })?;
            print_session_result("restored", &summary, args.json)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn list_sessions(
    args: &SessionListArgs,
    store: &SqliteStore,
    workdir: &std::path::Path,
) -> Result<()> {
    if args.limit == 0 {
        return Err(anyhow!("--limit must be greater than 0"));
    }
    let mut sessions = if args.archived {
        store.list_archived_sessions_for_workdir_with_sources(workdir, SESSION_SOURCES)?
    } else {
        store.list_sessions_for_workdir_with_sources(workdir, SESSION_SOURCES)?
    };
    sessions.truncate(args.limit);
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

fn rename_session(
    args: &SessionRenameArgs,
    store: &SqliteStore,
    workdir: &std::path::Path,
) -> Result<()> {
    let session_id = resolve_session_id(store, workdir, &args.session)?;
    let title = args.title.join(" ");
    store.set_session_title(&session_id, &title)?;
    let summary = store
        .session_summary(&session_id)?
        .ok_or_else(|| anyhow!("session not found: {session_id}"))?;
    print_session_result("renamed", &summary, args.json)
}

fn reload_context(
    args: &SessionIdArgs,
    store: &SqliteStore,
    workdir: &Path,
    db_path: &Path,
    env_map: std::collections::BTreeMap<String, String>,
) -> Result<()> {
    let session_id = resolve_session_id(store, workdir, &args.session)?;
    let result = reload_session_context(ReloadContextOptions {
        db_path: db_path.to_path_buf(),
        session: session_id,
        config_path: None,
        mode: None,
        inherited_env: Some(env_map),
        agent: None,
        no_agents: false,
        no_skills: false,
        invalidation_reason: "manual_reload".to_string(),
        notice: None,
    })?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "action": "reload-context",
                "session": result.session_id,
                "prefix_hash": result.prefix_hash,
                "version": result.version,
                "provider": result.provider,
                "model": result.model,
                "invalidation_reason": result.invalidation_reason,
            }))?
        );
    } else {
        println!("reloaded context: {}", result.session_id);
        println!("prefix: {} v{}", result.prefix_hash, result.version);
    }
    Ok(())
}

fn export_session(args: &SessionExportArgs, store: &SqliteStore, workdir: &Path) -> Result<()> {
    let session_id = resolve_session_id(store, workdir, &args.session)?;
    let artifact_kind = SessionArtifactKind::Export;
    let options = SessionExportOptions {
        format: args.format.into(),
        include: parse_include(args.include.as_deref(), artifact_kind)?,
        artifact_kind,
    };
    if let Some(output) = &args.output {
        let result = write_session_export(store, &session_id, output, options)?;
        println!("exported: {}", result.path.display());
    } else {
        let artifact = render_session_export(store, &session_id, options)?;
        print!("{}", artifact.content);
    }
    Ok(())
}

fn share_session(args: &SessionShareArgs, store: &SqliteStore, workdir: &Path) -> Result<()> {
    let session_id = resolve_session_id(store, workdir, &args.session)?;
    let artifact_kind = SessionArtifactKind::Share;
    let output = args.output.clone().unwrap_or_else(|| {
        workdir.join(default_session_export_filename(
            &session_id,
            SessionExportFormat::Markdown,
            artifact_kind,
        ))
    });
    let options = SessionExportOptions {
        format: SessionExportFormat::Markdown,
        include: parse_include(args.include.as_deref(), artifact_kind)?,
        artifact_kind,
    };
    let result = write_session_export(store, &session_id, &output, options)?;
    print_share_result(&result, args.json)
}

fn parse_include(
    include: Option<&str>,
    artifact_kind: SessionArtifactKind,
) -> psychevo_runtime::Result<SessionExportIncludeSet> {
    match include {
        Some(value) => SessionExportIncludeSet::parse(value, artifact_kind),
        None => Ok(SessionExportIncludeSet::default_for(artifact_kind)),
    }
}

fn mutate_session(
    args: &SessionIdArgs,
    store: &SqliteStore,
    workdir: &std::path::Path,
    mutate: impl Fn(&SqliteStore, &str) -> psychevo_runtime::Result<()>,
) -> Result<SessionSummary> {
    let session_id = resolve_session_id(store, workdir, &args.session)?;
    mutate(store, &session_id)?;
    store
        .session_summary(&session_id)?
        .ok_or_else(|| anyhow!("session not found: {session_id}"))
}

fn resolve_session_id(store: &SqliteStore, workdir: &std::path::Path, raw: &str) -> Result<String> {
    let raw = raw.trim();
    if raw == "latest" {
        return store
            .latest_session_for_workdir_with_sources(workdir, SESSION_SOURCES)?
            .ok_or_else(|| anyhow!("no active session found for {}", workdir.display()));
    }
    if raw.is_empty() {
        return Err(anyhow!("session id is required"));
    }
    Ok(raw.to_string())
}

fn print_session_result(action: &str, summary: &SessionSummary, as_json: bool) -> Result<()> {
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
        println!("workdir: {}", summary.workdir);
        println!("messages: {}", summary.message_count);
    }
    Ok(())
}

fn session_value(session: &SessionSummary) -> Value {
    json!({
        "id": session.id,
        "source": session.source,
        "workdir": session.workdir,
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

fn print_share_result(result: &SessionExportWriteResult, as_json: bool) -> Result<()> {
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

fn session_json(args: &SessionArgs) -> bool {
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
