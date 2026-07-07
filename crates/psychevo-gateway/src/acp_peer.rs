use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use agent_client_protocol::schema::{
    ClientCapabilities, ContentBlock, ContentChunk, FileSystemCapabilities, Implementation,
    InitializeRequest, LoadSessionRequest, NewSessionRequest, NewSessionResponse, PermissionOption,
    PermissionOptionKind, ProtocolVersion, ReadTextFileRequest, ReadTextFileResponse,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, WriteTextFileRequest,
    WriteTextFileResponse,
};
use agent_client_protocol::util::MatchDispatch;
use agent_client_protocol::{
    Agent, ByteStreams, Client, ConnectionTo, SessionMessage, schema::v2 as acp_v2,
};
use futures::{FutureExt, StreamExt, channel::mpsc, channel::oneshot};
use psychevo_runtime::{
    AbortSignal, AgentDefinition, AssistantBlock, Error, ExecutableResolveOptions, HostPlatform,
    ImageInput, Message, Outcome, PermissionApprovalDecision, PermissionApprovalOutcome,
    PermissionApprovalRequest, RunResult, RunStreamEvent, RunStreamSink, SelectedAgent,
    ToolCallBlock, UserContentBlock, fallback_visible_session_title, resolve_executable_path,
};
use serde_json::{Map, Value, json};
use tokio::process::Command;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::{
    ACP_PEER_METADATA_KEY, BackendTurnRequest, ResolvedPeerTurn, gateway_now_ms, protocol as wire,
};

struct AcpBackendLaunch {
    program: PathBuf,
    cwd: PathBuf,
    env: BTreeMap<String, String>,
}

fn acp_backend_command_text(peer: &ResolvedPeerTurn) -> psychevo_runtime::Result<&str> {
    peer.backend
        .command
        .as_deref()
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .ok_or_else(|| {
            Error::Message(format!(
                "agent backend `{}` is missing command",
                peer.backend.id
            ))
        })
}

fn acp_backend_effective_env(peer: &ResolvedPeerTurn) -> BTreeMap<String, String> {
    let mut env = std::env::vars().collect::<BTreeMap<_, _>>();
    env.extend(peer.env.clone());
    env.extend(peer.backend.env.clone());
    env
}

fn resolve_acp_backend_launch(
    peer: &ResolvedPeerTurn,
    invocation_cwd: &Path,
) -> psychevo_runtime::Result<AcpBackendLaunch> {
    resolve_acp_backend_launch_for_platform(peer, invocation_cwd, HostPlatform::current())
}

fn resolve_acp_backend_launch_for_platform(
    peer: &ResolvedPeerTurn,
    invocation_cwd: &Path,
    platform: HostPlatform,
) -> psychevo_runtime::Result<AcpBackendLaunch> {
    let command = acp_backend_command_text(peer)?;
    let cwd = backend_cwd(&peer.backend.cwd, invocation_cwd);
    let env = acp_backend_effective_env(peer);
    let program = resolve_executable_path(
        command,
        &cwd,
        &ExecutableResolveOptions {
            platform,
            env: &env,
        },
    )
    .ok_or_else(|| {
        Error::Message(format!(
            "ACP backend `{}` command `{command}` program not found on PATH/PATHEXT; install it or configure an absolute command path",
            peer.backend.id
        ))
    })?;
    Ok(AcpBackendLaunch { program, cwd, env })
}

fn acp_backend_command_from_launch(peer: &ResolvedPeerTurn, launch: &AcpBackendLaunch) -> Command {
    let mut command = Command::new(&launch.program);
    command
        .args(&peer.backend.args)
        .current_dir(&launch.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let _ = psychevo_runtime::apply_tokio_process_env(
        &mut command,
        &launch.env,
        psychevo_runtime::ProcessEnvOptions::new(&[]),
    );
    command
}

fn acp_backend_command(
    peer: &ResolvedPeerTurn,
    invocation_cwd: &Path,
) -> psychevo_runtime::Result<(Command, PathBuf)> {
    let launch = resolve_acp_backend_launch(peer, invocation_cwd)?;
    let cwd = launch.cwd.clone();
    Ok((acp_backend_command_from_launch(peer, &launch), cwd))
}

fn acp_backend_attempt_command(
    peer: &ResolvedPeerTurn,
    invocation_cwd: &Path,
) -> Result<(Command, PathBuf), AcpProtocolAttemptError> {
    acp_backend_command(peer, invocation_cwd).map_err(|error| AcpProtocolAttemptError {
        fallback_safe: false,
        error,
    })
}

include!("acp_peer/turn.rs");
include!("acp_peer/runtime_options.rs");
include!("acp_peer/stream_state.rs");
include!("acp_peer/tool_projection.rs");
include!("acp_peer/stdio_turn.rs");
include!("acp_peer/metadata_permissions.rs");

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn test_peer(command: &str, env: BTreeMap<String, String>) -> ResolvedPeerTurn {
        ResolvedPeerTurn {
            agent: AgentDefinition {
                name: "opencode".to_string(),
                description: "OpenCode".to_string(),
                instructions: String::new(),
                enabled: true,
                file_path: None,
                source: psychevo_runtime::AgentSource::Generated,
                backend: Some(psychevo_runtime::AgentBackendRef {
                    name: "opencode".to_string(),
                }),
                entrypoints: BTreeSet::from([psychevo_runtime::AgentEntrypoint::Peer]),
                model: None,
                tool_policy: psychevo_runtime::AgentToolPolicy::default(),
                skills: Vec::new(),
                hooks: None,
                background: None,
                initial_prompt: None,
                max_turns: None,
                max_spawn_depth: 0,
                project_instructions: None,
                effort: None,
                diagnostics: Vec::new(),
            },
            backend: psychevo_runtime::AgentBackendConfig {
                id: "opencode".to_string(),
                kind: psychevo_runtime::AgentBackendKind::Acp,
                enabled: true,
                label: "OpenCode".to_string(),
                description: None,
                command: Some(command.to_string()),
                args: vec!["acp".to_string()],
                env: BTreeMap::new(),
                cwd: "invocation".to_string(),
                entrypoints: BTreeSet::from([psychevo_runtime::AgentEntrypoint::Peer]),
                client_capabilities: BTreeSet::new(),
                mcp_servers: BTreeSet::new(),
            },
            env,
        }
    }

    #[test]
    fn acp_peer_launch_resolves_windows_command_shim() {
        let temp = tempfile::tempdir().expect("temp");
        let bin = temp.path().join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        let shim = bin.join("opencode.cmd");
        std::fs::write(&shim, "@echo off\n").expect("shim");
        let mut peer = test_peer(
            "opencode",
            BTreeMap::from([
                (
                    "PATH".to_string(),
                    temp.path().join("missing").display().to_string(),
                ),
                ("PATHEXT".to_string(), ".CMD".to_string()),
            ]),
        );
        peer.backend
            .env
            .insert("PATH".to_string(), bin.display().to_string());

        let launch =
            resolve_acp_backend_launch_for_platform(&peer, temp.path(), HostPlatform::Windows)
                .expect("launch");

        assert_eq!(launch.program, shim);
        assert_eq!(launch.cwd, temp.path());
        assert_eq!(launch.env.get("PATH"), Some(&bin.display().to_string()));
    }

    #[tokio::test]
    async fn acp_peer_runtime_options_missing_command_does_not_repeat_v1_fallback() {
        let temp = tempfile::tempdir().expect("temp");
        let peer = test_peer(
            "opencode",
            BTreeMap::from([(
                "PATH".to_string(),
                temp.path().join("missing").display().to_string(),
            )]),
        );

        let error = read_acp_peer_runtime_options(peer, temp.path().to_path_buf(), None)
            .await
            .expect_err("missing command");
        let message = error.to_string();

        assert!(message.contains("program not found on PATH/PATHEXT"));
        assert!(!message.contains("v1 fallback failed"), "{message}");
    }
}
