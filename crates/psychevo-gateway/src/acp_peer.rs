use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    AgentCapabilities, AuthMethod, BlobResourceContents, CancelNotification, ClientCapabilities,
    CloseSessionRequest, ContentBlock, ContentChunk, CreateElicitationRequest,
    CreateElicitationResponse, CreateTerminalRequest, CreateTerminalResponse, DeleteSessionRequest,
    ElicitationAcceptAction, ElicitationAction, ElicitationCapabilities, ElicitationContentValue,
    ElicitationFormCapabilities, ElicitationMode, ElicitationPropertySchema, ElicitationScope,
    EmbeddedResource, EmbeddedResourceResource, EnvVariable, FileSystemCapabilities,
    ForkSessionRequest, HttpHeader, ImageContent, Implementation, InitializeRequest,
    KillTerminalRequest, KillTerminalResponse, ListSessionsRequest, LoadSessionRequest, McpServer,
    McpServerHttp, McpServerStdio, MultiSelectItems, NewSessionRequest, PermissionOption,
    PermissionOptionKind, PromptRequest, ReadTextFileRequest, ReadTextFileResponse,
    ReleaseTerminalRequest, ReleaseTerminalResponse, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, ResourceLink,
    Response as AcpJsonRpcResponse, ResumeSessionRequest, SelectedPermissionOutcome,
    SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory, SessionConfigOptionValue,
    SessionConfigSelectOptions, SessionModeState, SessionNotification, SessionUpdate,
    SetSessionConfigOptionRequest, SetSessionModeRequest, StringFormat, TerminalExitStatus,
    TerminalOutputRequest, TerminalOutputResponse, TextContent, TextResourceContents,
    WaitForTerminalExitRequest, WaitForTerminalExitResponse, WriteTextFileRequest,
    WriteTextFileResponse,
};
use agent_client_protocol::{
    Agent, BoxFuture, ByteStreams, Channel, Client, ConnectTo, ConnectionTo, Dispatch, Handled,
    RawJsonRpcMessage, Role, UntypedMessage,
};
use agent_client_protocol_schema::v1::InitializeResponse;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use futures::{StreamExt, channel::mpsc};
use psychevo_runtime::{
    AbortSignal, AgentDefinition, AssistantBlock, ClarifyAnswer, ClarifyInteractionOutcome,
    ClarifyQuestion, ClarifyQuestionOption, ClarifyRequestEvent, Error, ExecutableResolveOptions,
    HostPlatform, ImageInput, McpTransportInput, Message, Outcome, PermissionApprovalDecision,
    PermissionApprovalOutcome, PermissionApprovalRequest, RunControlHandle, RunResult,
    RunStreamEvent, RunStreamSink, SelectedAgent, ToolCallBlock, UserContentBlock,
    fallback_visible_session_title, resolve_executable_path, resolve_explicit_image_source,
    resolve_mcp_server_handoffs, resolve_skills_home,
};
use serde_json::{Map, Value, json};
use sha2::Digest as _;
use tokio::io::AsyncReadExt as _;
use tokio::process::Command;
use tokio::sync::{mpsc as tokio_mpsc, oneshot as tokio_oneshot, watch};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::{
    ACP_PEER_METADATA_KEY, BackendTurnRequest, ResolvedPeerTurn, gateway_now_ms, protocol as wire,
};

struct AcpBackendLaunch {
    program: PathBuf,
    cwd: PathBuf,
    env: BTreeMap<String, String>,
    platform: HostPlatform,
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
    let mut env = peer.env.clone();
    env.extend(peer.backend.env.clone());
    apply_managed_codex_default_auth(&peer.backend.id, &mut env);
    env
}

fn apply_managed_codex_default_auth(backend_id: &str, env: &mut BTreeMap<String, String>) {
    if backend_id == crate::managed_acp::CODEX_ACP_BACKEND_ID
        && !env.contains_key("DEFAULT_AUTH_REQUEST")
        && ["CODEX_API_KEY", "OPENAI_API_KEY"]
            .iter()
            .any(|name| env.get(*name).is_some_and(|value| !value.trim().is_empty()))
    {
        // The reviewed Codex ACP adapter reads the actual key from its process
        // environment. This non-secret selector lets it authenticate only when
        // its own account/read reports that login is required.
        env.insert(
            "DEFAULT_AUTH_REQUEST".to_string(),
            r#"{"methodId":"api-key"}"#.to_string(),
        );
    }
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
    let program = if peer.backend.id == crate::managed_acp::CODEX_ACP_BACKEND_ID {
        let home = resolve_skills_home(&peer.env, invocation_cwd)?;
        crate::managed_acp::verified_managed_codex_acp_command(&home, Path::new(command), platform)?
    } else {
        resolve_executable_path(
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
        })?
    };
    Ok(AcpBackendLaunch {
        program,
        cwd,
        env,
        platform,
    })
}

fn acp_backend_command_from_launch(
    peer: &ResolvedPeerTurn,
    launch: &AcpBackendLaunch,
) -> psychevo_runtime::Result<Command> {
    let args = peer
        .backend
        .args
        .iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    let mut command = psychevo_runtime::tokio_host_process_command(
        &launch.program,
        &args,
        launch.platform,
        &launch.env,
    )?;
    command
        .current_dir(&launch.cwd)
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let _ = psychevo_runtime::apply_tokio_process_env(
        &mut command,
        &launch.env,
        psychevo_runtime::ProcessEnvOptions::new(&[]),
    );
    Ok(command)
}

fn acp_backend_command(
    peer: &ResolvedPeerTurn,
    invocation_cwd: &Path,
) -> psychevo_runtime::Result<(Command, PathBuf)> {
    let launch = resolve_acp_backend_launch(peer, invocation_cwd)?;
    let cwd = launch.cwd.clone();
    Ok((acp_backend_command_from_launch(peer, &launch)?, cwd))
}

mod mcp_handoff;
mod prompt_input;
mod session_controls;

include!("acp_peer/turn.rs");
include!("acp_peer/runtime_options.rs");
include!("acp_peer/stream_state.rs");
include!("acp_peer/session_projection.rs");
include!("acp_peer/capability_packs.rs");
include!("acp_peer/tool_projection.rs");
include!("acp_peer/stdio_turn.rs");
include!("acp_peer/elicitation.rs");
include!("acp_peer/terminal_callbacks.rs");
include!("acp_peer/lifecycle.rs");
include!("acp_peer/process_pool.rs");
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
                optional_contributions: BTreeSet::new(),
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
            process_scope_fingerprint: Some("test-profile-v1".to_string()),
        }
    }

    fn test_python_path(cwd: &Path) -> PathBuf {
        let host_env = std::env::vars().collect::<BTreeMap<_, _>>();
        resolve_executable_path(
            "python3",
            cwd,
            &ExecutableResolveOptions {
                platform: HostPlatform::current(),
                env: &host_env,
            },
        )
        .expect("resolve ACP fixture python")
    }

    #[test]
    fn managed_codex_default_auth_selects_env_key_without_copying_secret() {
        let mut env = BTreeMap::from([(
            "OPENAI_API_KEY".to_string(),
            "test-secret-that-must-not-be-copied".to_string(),
        )]);

        apply_managed_codex_default_auth(crate::managed_acp::CODEX_ACP_BACKEND_ID, &mut env);

        assert_eq!(
            env.get("DEFAULT_AUTH_REQUEST").map(String::as_str),
            Some(r#"{"methodId":"api-key"}"#)
        );
        assert!(!env["DEFAULT_AUTH_REQUEST"].contains("test-secret"));

        env.insert(
            "DEFAULT_AUTH_REQUEST".to_string(),
            r#"{"methodId":"chat-gpt"}"#.to_string(),
        );
        apply_managed_codex_default_auth(crate::managed_acp::CODEX_ACP_BACKEND_ID, &mut env);
        assert_eq!(
            env["DEFAULT_AUTH_REQUEST"], r#"{"methodId":"chat-gpt"}"#,
            "an explicit backend choice remains authoritative"
        );
    }

    #[test]
    fn acp_backend_environment_uses_only_the_captured_effective_baseline() {
        let mut peer = test_peer(
            "opencode",
            BTreeMap::from([("PATH".to_string(), "/isolated/bin".to_string())]),
        );
        peer.backend
            .env
            .insert("BACKEND_ONLY".to_string(), "backend-value".to_string());

        assert_eq!(
            acp_backend_effective_env(&peer),
            BTreeMap::from([
                ("BACKEND_ONLY".to_string(), "backend-value".to_string()),
                ("PATH".to_string(), "/isolated/bin".to_string()),
            ])
        );
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
                (
                    "COMSPEC".to_string(),
                    r"C:\Windows\System32\cmd.exe".to_string(),
                ),
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
        let command = acp_backend_command_from_launch(&peer, &launch).expect("host command");
        assert_eq!(
            command.as_std().get_program(),
            std::ffi::OsStr::new(r"C:\Windows\System32\cmd.exe")
        );
        let args = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(&args[..4], ["/D", "/S", "/V:OFF", "/C"]);
        #[cfg(not(windows))]
        {
            assert!(args[4].contains("opencode.cmd"), "{}", args[4]);
            assert!(args[4].contains("\"acp\""), "{}", args[4]);
        }
    }

    #[test]
    fn managed_codex_acp_launch_does_not_fall_back_to_configured_or_path_executable() {
        let temp = tempfile::tempdir().expect("temp");
        let executable = std::env::current_exe().expect("current executable");
        let mut peer = test_peer(&executable.display().to_string(), BTreeMap::new());
        peer.backend.id = crate::managed_acp::CODEX_ACP_BACKEND_ID.to_string();
        peer.env.insert(
            "PSYCHEVO_HOME".to_string(),
            temp.path().join("home").display().to_string(),
        );

        let error =
            resolve_acp_backend_launch_for_platform(&peer, temp.path(), HostPlatform::Posix)
                .err()
                .expect("missing managed install");

        assert!(error.to_string().contains("backend/install"), "{error}");
    }

    #[test]
    fn acp_process_key_isolates_agent_policy_and_profile_scope() {
        let temp = tempfile::tempdir().expect("temp");
        let command = std::env::current_exe().expect("current executable");
        let first = test_peer(&command.display().to_string(), BTreeMap::new());
        let mut different_agent = test_peer(&command.display().to_string(), BTreeMap::new());
        different_agent.agent.name = "reviewer".to_string();
        let mut different_profile = test_peer(&command.display().to_string(), BTreeMap::new());
        different_profile.process_scope_fingerprint = Some("profile-v2".to_string());

        let first_key = acp_process_key(&first, temp.path()).expect("first key");
        let agent_key = acp_process_key(&different_agent, temp.path()).expect("agent key");
        let profile_key = acp_process_key(&different_profile, temp.path()).expect("profile key");

        assert_ne!(
            first_key, agent_key,
            "client policy follows the captured Agent"
        );
        assert_ne!(
            first_key, profile_key,
            "process scope follows the captured Profile"
        );
    }

    #[tokio::test]
    async fn acp_cached_inspection_does_not_spawn_a_process_on_miss() {
        let pool = AcpProcessPool::new(Duration::from_secs(30));

        let snapshot = pool
            .inspect_cached("missing-thread".to_string(), "missing-native".to_string())
            .await
            .expect("cache miss");

        assert!(snapshot.is_none());
        assert!(pool.inner.actors.lock().expect("actors").is_empty());
        assert!(
            pool.inner
                .resident_actors
                .lock()
                .expect("resident actors")
                .is_empty()
        );
    }

    #[test]
    fn acp_auth_observation_scope_ignores_presentation_but_tracks_launch_changes() {
        let temp = tempfile::tempdir().expect("temp");
        let command = std::env::current_exe().expect("current executable");
        let first = test_peer(&command.display().to_string(), BTreeMap::new());
        let mut presentation_only = first.clone();
        presentation_only.agent.name = "reviewer".to_string();
        presentation_only.backend.label = "Renamed backend".to_string();
        presentation_only.process_scope_fingerprint = Some("profile-v2".to_string());
        let mut changed_launch = first.clone();
        changed_launch.backend.args.push("--different".to_string());

        let first_key = acp_auth_observation_key(&first, temp.path()).expect("first auth key");
        let presentation_key = acp_auth_observation_key(&presentation_only, temp.path())
            .expect("presentation auth key");
        let launch_key =
            acp_auth_observation_key(&changed_launch, temp.path()).expect("launch auth key");

        assert_eq!(first_key, presentation_key);
        assert_ne!(first_key, launch_key);
    }

    #[tokio::test]
    async fn acp_elicitation_round_trips_through_shared_interaction_control() {
        let schema = agent_client_protocol::schema::v1::ElicitationSchema::new().property(
            "workspace",
            agent_client_protocol::schema::v1::StringPropertySchema::new()
                .title("Workspace")
                .description("Choose the workspace scope.")
                .enum_values(vec!["Repository".to_string(), "Package".to_string()]),
            true,
        );
        let form = agent_client_protocol::schema::v1::ElicitationFormMode::new(
            agent_client_protocol::schema::v1::ElicitationSessionScope::new("native-fixture"),
            schema,
        );
        let (request, fields) =
            project_acp_elicitation_form("acp-elicit-1".to_string(), "Select a scope.", form)
                .expect("project elicitation");
        assert_eq!(request.questions.len(), 1);
        assert_eq!(request.questions[0].header, "workspace");
        assert!(!request.questions[0].custom);

        let (handle, control) = psychevo_runtime::run_control();
        let abort = control.abort_signal();
        let events = Arc::new(Mutex::new(Vec::<RunStreamEvent>::new()));
        let stream: RunStreamSink = {
            let events = Arc::clone(&events);
            Arc::new(move |event| events.lock().expect("events").push(event))
        };
        let waiter = tokio::spawn({
            let handle = handle.clone();
            async move {
                handle
                    .request_clarification(request, stream, Some(abort))
                    .await
            }
        });
        for _ in 0..100 {
            if !events.lock().expect("events").is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(handle.submit_clarify_result(
            "acp-elicit-1",
            psychevo_runtime::ClarifyResult::Answered(psychevo_runtime::ClarifyResponse {
                answers: vec![ClarifyAnswer {
                    answers: vec!["Repository".to_string()],
                }],
            }),
        ));
        let ClarifyInteractionOutcome::Answered(answer) = waiter.await.expect("interaction task")
        else {
            panic!("interaction should be answered");
        };
        let response =
            encode_acp_elicitation_response(&fields, answer).expect("encode elicitation response");
        let ElicitationAction::Accept(accepted) = response.action else {
            panic!("elicitation should be accepted");
        };
        assert_eq!(
            accepted.content.expect("accepted content").get("workspace"),
            Some(&ElicitationContentValue::String("Repository".to_string()))
        );
        let events = events.lock().expect("events");
        assert_eq!(
            events
                .iter()
                .filter(|event| {
                    event
                        .legacy_value()
                        .and_then(|value| value.get("type"))
                        .and_then(Value::as_str)
                        == Some("action_requested")
                })
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| {
                    event
                        .legacy_value()
                        .and_then(|value| value.get("type"))
                        .and_then(Value::as_str)
                        == Some("action_resolved")
                })
                .count(),
            1
        );
        drop(control);
    }

    fn initialized_projection_fixture() -> InitializeResponse {
        serde_json::from_value(json!({
            "protocolVersion": 1,
            "agentInfo": {
                "name": "fixture-acp",
                "title": "Fixture ACP",
                "version": "1.2.3"
            },
            "agentCapabilities": {
                "loadSession": true,
                "promptCapabilities": {
                    "image": true,
                    "audio": true,
                    "embeddedContext": true
                },
                "sessionCapabilities": {
                    "list": {},
                    "delete": {},
                    "fork": {},
                    "resume": {},
                    "close": {},
                    "additionalDirectories": {}
                },
                "auth": { "logout": {} },
                "providers": {},
                "mcpCapabilities": { "http": true, "sse": true, "acp": true }
            }
        }))
        .expect("initialize fixture")
    }

    fn mode_notification(session_id: &str) -> SessionNotification {
        serde_json::from_value(json!({
            "sessionId": session_id,
            "update": {
                "sessionUpdate": "current_mode_update",
                "currentModeId": "plan"
            }
        }))
        .expect("mode notification")
    }

    fn agent_message_notification(session_id: &str, text: &str) -> SessionNotification {
        serde_json::from_value(json!({
            "sessionId": session_id,
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {
                    "type": "text",
                    "text": text,
                    "_meta": { "secret": "nested" }
                },
                "_meta": { "secret": "top-level" }
            }
        }))
        .expect("agent message notification")
    }

    #[test]
    fn acp_projection_dedupes_within_epoch_and_replays_same_sequence_in_new_epoch() {
        let initialized = initialized_projection_fixture();
        let notification = AcpPeerInboundNotification {
            sequence: 7,
            payload: AcpPeerInboundPayload::Session(Box::new(mode_notification("native-fixture"))),
        };
        let mut first = new_acp_resident_session(
            &initialized,
            AcpResidentSessionInput {
                native_session_id: "native-fixture".to_string(),
                modes: None,
                config_options: Vec::new(),
                session_epoch: 11,
                loaded_from_agent: true,
                mcp_servers: Vec::new(),
                mcp_declaration_fingerprint: String::new(),
            },
        );
        let revision_before_mode = acp_session_snapshot(&first, 4).control_revision;

        assert!(first.reduce_notification(&notification, AcpFactOrigin::History));
        assert!(!first.reduce_notification(&notification, AcpFactOrigin::Live));
        assert_eq!(first.current_mode_id.as_deref(), Some("plan"));
        assert_eq!(first.history.replay_update_count, 1);
        assert_eq!(first.history.live_update_count, 0);
        assert_ne!(
            acp_session_snapshot(&first, 4).control_revision,
            revision_before_mode
        );

        let mut next_epoch = new_acp_resident_session(
            &initialized,
            AcpResidentSessionInput {
                native_session_id: "native-fixture".to_string(),
                modes: None,
                config_options: Vec::new(),
                session_epoch: 12,
                loaded_from_agent: true,
                mcp_servers: Vec::new(),
                mcp_declaration_fingerprint: String::new(),
            },
        );
        assert!(next_epoch.reduce_notification(&notification, AcpFactOrigin::Live));
        assert_eq!(next_epoch.history.replay_update_count, 0);
        assert_eq!(next_epoch.history.live_update_count, 1);
    }

    #[test]
    fn acp_turn_projection_observes_pre_prompt_facts_without_claiming_them_as_output() {
        let mut state = AcpPeerStreamState::new(None, "local-fixture".to_string());
        state.reduce_notification(
            AcpPeerInboundNotification {
                sequence: 1,
                payload: AcpPeerInboundPayload::Session(Box::new(agent_message_notification(
                    "native-fixture",
                    "before prompt",
                ))),
            },
            AcpFactOrigin::Live,
            3,
            8,
        );
        assert_eq!(state.final_answer, "");
        assert_eq!(state.events.len(), 1);
        assert_eq!(state.events[0]["origin"], "live");
        assert_eq!(state.events[0]["process_generation"], 3);
        assert_eq!(state.events[0]["session_epoch"], 8);
        assert_eq!(state.events[0]["notification_sequence"], 1);
        assert!(state.events[0]["update"].get("_meta").is_none());
        assert!(state.events[0]["update"]["content"].get("_meta").is_none());

        state.begin_prompt();
        state.reduce_notification(
            AcpPeerInboundNotification {
                sequence: 2,
                payload: AcpPeerInboundPayload::Session(Box::new(agent_message_notification(
                    "native-fixture",
                    "after prompt",
                ))),
            },
            AcpFactOrigin::Live,
            3,
            8,
        );
        assert_eq!(state.final_answer, "after prompt");
        assert_eq!(state.events.len(), 2);
    }

    #[tokio::test]
    async fn acp_inspect_snapshot_reduces_load_replay_through_response_barrier() {
        let temp = tempfile::tempdir().expect("temp");
        let script = temp.path().join("projection_barrier_fixture.py");
        std::fs::write(
            &script,
            r#"import json
import sys

def send(value):
    print(json.dumps(value), flush=True)

def update(session_id, value):
    send({"jsonrpc": "2.0", "method": "session/update", "params": {
        "sessionId": session_id,
        "update": value
    }})

def option(current):
    return {"id": "model", "name": "Model", "category": "model", "type": "select",
            "currentValue": current, "options": [
                {"value": "from-response", "name": "Response"},
                {"value": "from-update", "name": "Update"}
            ]}

for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    method = message.get("method")
    mid = message.get("id")
    params = message.get("params") or {}
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "protocolVersion": 1,
            "agentInfo": {"name": "fixture-acp", "title": "Fixture ACP", "version": "1.2.3"},
            "agentCapabilities": {
                "loadSession": True,
                "promptCapabilities": {"image": True, "audio": True, "embeddedContext": True},
                "sessionCapabilities": {
                    "list": {}, "delete": {}, "fork": {}, "resume": {}, "close": {},
                    "additionalDirectories": {}
                },
                "auth": {"logout": {}},
                "providers": {},
                "mcpCapabilities": {"http": True, "sse": True, "acp": True}
            }
        }})
    elif method == "session/load":
        session_id = params.get("sessionId")
        update(session_id, {"sessionUpdate": "agent_message_chunk",
                            "content": {"type": "text", "text": "loaded history"}})
        update(session_id, {"sessionUpdate": "available_commands_update", "availableCommands": [
            {"name": "review", "description": "Review this workspace",
             "input": {"hint": "workspace path", "_meta": {"secret": "drop"}}}
        ]})
        update(session_id, {"sessionUpdate": "current_mode_update", "currentModeId": "plan"})
        update(session_id, {"sessionUpdate": "config_option_update", "configOptions": [option("from-update")]})
        update(session_id, {"sessionUpdate": "session_info_update", "title": "Loaded fixture"})
        update(session_id, {"sessionUpdate": "usage_update", "used": 42, "size": 100,
                            "cost": {"amount": 0.25, "currency": "USD", "_meta": {"secret": "drop"}},
                            "_meta": {"secret": "drop"}})
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "modes": {"currentModeId": "ask", "availableModes": [
                {"id": "ask", "name": "Ask", "description": "Answer questions"},
                {"id": "plan", "name": "Plan", "description": "Plan changes"}
            ]},
            "configOptions": [option("from-response")]
        }})
        update(session_id, {"sessionUpdate": "current_mode_update", "currentModeId": "ask"})
    elif method == "session/set_mode":
        session_id = params.get("sessionId")
        update(session_id, {"sessionUpdate": "current_mode_update",
                            "currentModeId": params.get("modeId")})
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
    elif method == "session/close":
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
    else:
        send({"jsonrpc": "2.0", "id": mid,
              "error": {"code": -32601, "message": "method not found"}})
"#,
        )
        .expect("fixture script");
        let python = test_python_path(temp.path());
        let mut peer = test_peer(&python.display().to_string(), BTreeMap::new());
        peer.backend.args = vec![script.display().to_string()];
        let pool = AcpProcessPool::new(Duration::from_secs(30));

        let snapshot = tokio::time::timeout(
            Duration::from_secs(10),
            pool.inspect(
                peer.clone(),
                temp.path().to_path_buf(),
                "local-fixture".to_string(),
                "native-fixture".to_string(),
                Vec::new(),
            ),
        )
        .await
        .expect("inspect timeout")
        .expect("inspect snapshot");

        assert_eq!(snapshot.native_session_id, "native-fixture");
        assert_eq!(
            snapshot.agent_pack_identity(),
            Some(("fixture-acp", "1.2.3"))
        );
        assert_eq!(
            snapshot
                .agent
                .as_ref()
                .and_then(|agent| agent.title.as_deref()),
            Some("Fixture ACP")
        );
        assert_eq!(snapshot.supports_input_kind("text"), Some(true));
        assert_eq!(snapshot.supports_input_kind("image"), Some(true));
        assert_eq!(snapshot.supports_input_kind("audio"), Some(true));
        assert_eq!(snapshot.supports_input_kind("resource"), Some(true));
        assert_eq!(snapshot.supports_input_kind("resourceLink"), Some(true));
        assert_eq!(snapshot.supports_input_kind("embeddedContext"), Some(true));
        assert_eq!(snapshot.supports_input_kind("future"), None);
        assert!(snapshot.capabilities.session.load);
        assert!(snapshot.capabilities.session.list);
        assert!(snapshot.capabilities.session.delete);
        assert!(snapshot.capabilities.session.fork);
        assert!(snapshot.capabilities.session.resume);
        assert!(snapshot.capabilities.session.close);
        assert!(snapshot.capabilities.session.additional_directories);
        assert!(snapshot.capabilities.auth_logout);
        assert!(snapshot.capabilities.mcp_http);
        assert!(snapshot.capabilities.mcp_sse);
        assert!(snapshot.capabilities.mcp_acp);
        assert_eq!(snapshot.history.owner, AcpHistoryOwnerSnapshot::Agent);
        assert!(snapshot.history.resumable);
        assert!(snapshot.history.load_supported);
        assert!(snapshot.history.resume_supported);
        assert!(snapshot.history.loaded_from_agent);
        assert!(snapshot.history.replay_complete);
        assert_eq!(snapshot.history.replay_update_count, 6);
        assert_eq!(snapshot.history.live_update_count, 0);
        assert_eq!(snapshot.current_mode_id.as_deref(), Some("plan"));
        assert_eq!(snapshot.available_modes.len(), 2);
        assert_eq!(snapshot.available_modes[0].id, "ask");
        assert_eq!(snapshot.available_modes[1].name, "Plan");
        assert_eq!(snapshot.available_commands.len(), 1);
        assert_eq!(snapshot.available_commands[0].name, "review");
        assert_eq!(
            snapshot.available_commands[0].description,
            "Review this workspace"
        );
        assert_eq!(
            snapshot.available_commands[0].input,
            Some(json!({ "hint": "workspace path" }))
        );
        assert_eq!(
            snapshot.session_info.title.as_deref(),
            Some("Loaded fixture")
        );
        assert_eq!(
            snapshot
                .session_info
                .usage
                .as_ref()
                .and_then(|usage| usage["used"].as_u64()),
            Some(42)
        );
        let usage = snapshot
            .session_info
            .usage
            .as_ref()
            .expect("usage snapshot");
        assert_eq!(usage["cost"]["amount"], 0.25);
        assert!(usage.get("_meta").is_none());
        assert!(usage["cost"].get("_meta").is_none());
        assert_eq!(snapshot.options.len(), 1);
        assert_eq!(snapshot.options[0].id, "model");
        assert_eq!(
            snapshot.options[0].current_value.as_deref(),
            Some("from-update")
        );
        assert_eq!(snapshot.generation, 1);
        assert_eq!(snapshot.session_epoch, 1);
        assert_eq!(snapshot.control_revision.len(), 64);
        assert_eq!(snapshot.projection_revision.len(), 64);

        let after_response = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let snapshot = pool
                    .inspect(
                        peer.clone(),
                        temp.path().to_path_buf(),
                        "local-fixture".to_string(),
                        "native-fixture".to_string(),
                        Vec::new(),
                    )
                    .await
                    .expect("second inspect snapshot");
                if snapshot.history.live_update_count >= 1 {
                    break snapshot;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("post-response notification observation");
        assert_eq!(after_response.current_mode_id.as_deref(), Some("ask"));
        assert_eq!(after_response.history.replay_update_count, 6);
        assert_eq!(after_response.history.live_update_count, 1);

        let after_mode = pool
            .set_control(AcpSetControlInput {
                peer,
                cwd: temp.path().to_path_buf(),
                local_session_id: "local-fixture".to_string(),
                native_session_id: "native-fixture".to_string(),
                mcp_servers: Vec::new(),
                control_id: "mode".to_string(),
                value: json!("plan"),
            })
            .await
            .expect("set negotiated ACP mode");
        assert_eq!(after_mode.current_mode_id.as_deref(), Some("plan"));
        assert_eq!(after_mode.history.live_update_count, 2);
        assert_ne!(after_mode.control_revision, after_response.control_revision);

        pool.shutdown(false).await.expect("shutdown fixture");
    }

    fn lifecycle_fixture_peer(temp: &tempfile::TempDir, mode: &str) -> (ResolvedPeerTurn, PathBuf) {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_acp_lifecycle.py");
        let log = temp.path().join(format!("lifecycle-{mode}.jsonl"));
        let python = test_python_path(temp.path());
        let mut peer = test_peer(
            &python.display().to_string(),
            BTreeMap::from([
                ("ACP_LIFECYCLE_LOG".to_string(), log.display().to_string()),
                ("ACP_LIFECYCLE_MODE".to_string(), mode.to_string()),
            ]),
        );
        peer.backend.args = vec![fixture.display().to_string()];
        (peer, log)
    }

    fn lifecycle_mcp_fixture() -> psychevo_runtime::ResolvedMcpServerInput {
        psychevo_runtime::ResolvedMcpServerInput {
            server: psychevo_runtime::McpServerInput::new(
                "repo",
                psychevo_runtime::McpTransportInput::Stdio {
                    command: PathBuf::from("/fixture/bin/repo-mcp"),
                    args: vec!["--serve".to_string()],
                    env: BTreeMap::from([("FIXTURE_SCOPE".to_string(), "workspace".to_string())]),
                    cwd: None,
                },
            ),
            bearer_token: None,
        }
    }

    fn read_lifecycle_log(path: &Path) -> Vec<Value> {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    async fn wait_for_cleanup_probe_responses(path: &Path, expected: usize) -> Vec<Value> {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let entries = read_lifecycle_log(path);
                if entries
                    .iter()
                    .filter(|entry| entry["event"] == "callback_response")
                    .count()
                    >= expected
                {
                    return entries;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("cleanup probe response timeout")
    }

    fn lifecycle_requests<'a>(entries: &'a [Value], method: &str) -> Vec<&'a Value> {
        entries
            .iter()
            .filter(|entry| entry["event"] == "request" && entry["method"] == method)
            .collect()
    }

    #[tokio::test]
    async fn acp_lifecycle_preserves_mcp_shapes_epochs_barriers_and_cleanup() {
        let temp = tempfile::tempdir().expect("temp");
        let (peer, log) = lifecycle_fixture_peer(&temp, "all");
        let pool = AcpProcessPool::new(Duration::from_secs(30));
        let source = AcpResidentSessionRef {
            local_session_id: "local-source".to_string(),
            native_session_id: "native-source".to_string(),
        };
        let mcp = lifecycle_mcp_fixture();

        let resumed = pool
            .resume_session(
                peer.clone(),
                temp.path().to_path_buf(),
                source.clone(),
                vec![mcp],
            )
            .await
            .expect("resume session");
        assert_eq!(resumed.native_session_id, "native-source");
        assert_eq!(resumed.session_epoch, 1);
        assert!(resumed.history.resumable);
        assert!(resumed.history.resume_supported);
        assert!(resumed.history.loaded_from_agent);
        assert!(resumed.history.replay_complete);
        assert_eq!(resumed.history.replay_update_count, 0);
        assert_eq!(resumed.history.live_update_count, 1);
        assert_eq!(resumed.current_mode_id.as_deref(), Some("resume-mode"));

        let listed = pool
            .list_sessions(
                peer.clone(),
                temp.path().to_path_buf(),
                Some(temp.path().to_path_buf()),
                Some("opaque-cursor".to_string()),
            )
            .await
            .expect("list sessions");
        assert_eq!(listed.sessions.len(), 1);
        assert_eq!(listed.sessions[0].native_session_id, "listed-native");
        assert_eq!(listed.sessions[0].cwd, temp.path());
        assert_eq!(listed.next_cursor.as_deref(), Some("next-cursor"));

        let forked = pool
            .fork_session(
                peer.clone(),
                temp.path().to_path_buf(),
                source.clone(),
                "local-fork".to_string(),
            )
            .await
            .expect("fork session");
        assert_eq!(forked.native_session_id, "fork-native");
        assert_eq!(forked.session_epoch, 2);
        assert_eq!(forked.history.replay_update_count, 1);
        assert_eq!(forked.history.live_update_count, 0);
        assert!(forked.history.replay_complete);

        let fork_ref = AcpResidentSessionRef {
            local_session_id: "local-fork".to_string(),
            native_session_id: "fork-native".to_string(),
        };
        pool.close_session(peer.clone(), temp.path().to_path_buf(), fork_ref)
            .await
            .expect("close fork");
        pool.list_sessions(peer.clone(), temp.path().to_path_buf(), None, None)
            .await
            .expect("probe close cleanup");
        let after_close = wait_for_cleanup_probe_responses(&log, 1).await;
        let close_probe = after_close
            .iter()
            .find(|entry| {
                entry["event"] == "callback_response"
                    && entry["response"]["error"]["data"]
                        .as_str()
                        .is_some_and(|data| data.contains("unknown ACP session context"))
            })
            .expect("closed session context rejected");
        assert_eq!(close_probe["response"]["error"]["code"], -32600);

        pool.delete_session(
            peer.clone(),
            temp.path().to_path_buf(),
            "native-source".to_string(),
            Some(source.clone()),
        )
        .await
        .expect("delete source");
        pool.list_sessions(peer.clone(), temp.path().to_path_buf(), None, None)
            .await
            .expect("probe delete cleanup");
        let entries = wait_for_cleanup_probe_responses(&log, 2).await;

        let expected_mcp = json!([{
            "name": "repo",
            "command": "/fixture/bin/repo-mcp",
            "args": ["--serve"],
            "env": [{"name": "FIXTURE_SCOPE", "value": "workspace"}]
        }]);
        let resume_requests = lifecycle_requests(&entries, "session/resume");
        assert_eq!(resume_requests.len(), 1);
        assert_eq!(resume_requests[0]["params"]["sessionId"], "native-source");
        assert_eq!(resume_requests[0]["params"]["cwd"], json!(temp.path()));
        assert_eq!(resume_requests[0]["params"]["mcpServers"], expected_mcp);
        let fork_requests = lifecycle_requests(&entries, "session/fork");
        assert_eq!(fork_requests.len(), 1);
        assert_eq!(fork_requests[0]["params"]["sessionId"], "native-source");
        assert_eq!(
            fork_requests[0]["params"]["mcpServers"], resume_requests[0]["params"]["mcpServers"],
            "fork inherits the exact resident MCP declaration set"
        );
        let filtered_list = lifecycle_requests(&entries, "session/list")
            .into_iter()
            .find(|request| request["params"].get("cursor").is_some())
            .expect("filtered list request");
        assert_eq!(filtered_list["params"]["cwd"], json!(temp.path()));
        assert_eq!(filtered_list["params"]["cursor"], "opaque-cursor");
        let close_requests = lifecycle_requests(&entries, "session/close");
        assert_eq!(close_requests.len(), 1);
        assert_eq!(
            close_requests[0]["params"],
            json!({"sessionId": "fork-native"})
        );
        let delete_requests = lifecycle_requests(&entries, "session/delete");
        assert_eq!(delete_requests.len(), 1);
        assert_eq!(
            delete_requests[0]["params"],
            json!({"sessionId": "native-source"})
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| {
                    entry["event"] == "callback_response"
                        && entry["response"]["error"]["data"]
                            .as_str()
                            .is_some_and(|data| data.contains("unknown ACP session context"))
                })
                .count(),
            2,
            "close and delete both erase callback contexts"
        );

        let missing = pool
            .fork_session(
                peer,
                temp.path().to_path_buf(),
                source,
                "local-after-delete".to_string(),
            )
            .await
            .expect_err("deleted session is not resident");
        assert_eq!(
            missing
                .structured_data()
                .and_then(|data| data["delivery"].as_str()),
            Some("not_delivered")
        );
        pool.shutdown(false)
            .await
            .expect("shutdown lifecycle fixture");
    }

    #[tokio::test]
    async fn acp_lifecycle_capabilities_gate_every_wire_request() {
        let temp = tempfile::tempdir().expect("temp");
        let (peer, log) = lifecycle_fixture_peer(&temp, "none");
        let pool = AcpProcessPool::new(Duration::from_secs(30));
        let session = AcpResidentSessionRef {
            local_session_id: "local-gated".to_string(),
            native_session_id: "native-gated".to_string(),
        };

        let errors = vec![
            pool.list_sessions(peer.clone(), temp.path().to_path_buf(), None, None)
                .await
                .expect_err("list gated"),
            pool.resume_session(
                peer.clone(),
                temp.path().to_path_buf(),
                session.clone(),
                Vec::new(),
            )
            .await
            .expect_err("resume gated"),
            pool.fork_session(
                peer.clone(),
                temp.path().to_path_buf(),
                session.clone(),
                "local-fork-gated".to_string(),
            )
            .await
            .expect_err("fork gated"),
            pool.close_session(peer.clone(), temp.path().to_path_buf(), session.clone())
                .await
                .expect_err("close gated"),
            pool.delete_session(
                peer,
                temp.path().to_path_buf(),
                session.native_session_id.clone(),
                Some(session),
            )
            .await
            .expect_err("delete gated"),
        ];
        for error in errors {
            let data = error.structured_data().expect("structured lifecycle error");
            assert_eq!(data["code"], "acp_lifecycle_unsupported");
            assert_eq!(data["delivery"], "not_delivered");
        }
        pool.shutdown(false).await.expect("shutdown gated fixture");
        let entries = read_lifecycle_log(&log);
        assert_eq!(lifecycle_requests(&entries, "initialize").len(), 1);
        assert_eq!(
            entries
                .iter()
                .filter(|entry| { entry["event"] == "request" && entry["method"] != "initialize" })
                .count(),
            0,
            "unsupported lifecycle capabilities send no request or cancel notification"
        );
    }

    #[tokio::test]
    async fn acp_auth_required_is_recoverable_and_agent_error_data_is_redacted() {
        let temp = tempfile::tempdir().expect("temp");
        let (peer, log) = lifecycle_fixture_peer(&temp, "auth-list");
        let pool = AcpProcessPool::new(Duration::from_secs(30));
        assert_eq!(
            pool.probe_authentication(peer.clone(), temp.path().to_path_buf())
                .await
                .expect("generic preflight auth status"),
            AcpAuthDoctorStatus::Unchecked,
            "a generic Agent cannot be called authenticated from initialize alone"
        );
        let error = pool
            .list_sessions(
                peer.clone(),
                temp.path().to_path_buf(),
                Some(temp.path().to_path_buf()),
                None,
            )
            .await
            .expect_err("auth required");
        let data = error.structured_data().expect("structured auth error");
        assert_eq!(data["code"], "acp_auth_required");
        assert_eq!(data["stage"], "configuration");
        assert_eq!(data["delivery"], "not_delivered");
        assert_eq!(data["recoveryAction"], "backend/doctor");
        assert!(!error.to_string().contains("must-not-leak-from-agent-data"));
        assert!(
            !serde_json::to_string(data)
                .expect("auth error json")
                .contains("must-not-leak-from-agent-data")
        );
        assert!(!error.to_string().contains('\n'));
        assert_eq!(
            pool.probe_authentication(peer, temp.path().to_path_buf())
                .await
                .expect("cached generic auth status"),
            AcpAuthDoctorStatus::Required,
            "doctor reflects a real stable-v1 AuthRequired observation"
        );
        pool.shutdown(false).await.expect("shutdown auth fixture");
        let entries = read_lifecycle_log(&log);
        assert!(lifecycle_requests(&entries, "session/new").is_empty());
        assert!(lifecycle_requests(&entries, "session/prompt").is_empty());
        assert!(lifecycle_requests(&entries, "authentication/status").is_empty());
    }

    #[tokio::test]
    async fn codex_exact_pack_doctor_uses_typed_auth_status_without_creating_a_session() {
        for (mode, expected) in [
            ("codex-auth-unauthenticated", AcpAuthDoctorStatus::Required),
            (
                "codex-auth-api-key",
                AcpAuthDoctorStatus::Authenticated(AcpAuthenticatedKind::ApiKey),
            ),
            (
                "codex-auth-chat-gpt",
                AcpAuthDoctorStatus::Authenticated(AcpAuthenticatedKind::ChatGpt),
            ),
            (
                "codex-auth-gateway",
                AcpAuthDoctorStatus::Authenticated(AcpAuthenticatedKind::Gateway),
            ),
        ] {
            let temp = tempfile::tempdir().expect("temp");
            let (peer, log) = lifecycle_fixture_peer(&temp, mode);
            let pool = AcpProcessPool::new(Duration::from_secs(30));
            assert_eq!(
                pool.probe_authentication(peer, temp.path().to_path_buf())
                    .await
                    .expect("Codex authentication/status"),
                expected
            );
            pool.shutdown(false).await.expect("shutdown Codex fixture");
            let entries = read_lifecycle_log(&log);
            assert_eq!(lifecycle_requests(&entries, "initialize").len(), 1);
            assert_eq!(
                lifecycle_requests(&entries, "authentication/status").len(),
                1
            );
            assert!(lifecycle_requests(&entries, "session/new").is_empty());
            assert!(lifecycle_requests(&entries, "session/load").is_empty());
            assert!(lifecycle_requests(&entries, "session/prompt").is_empty());
        }

        let temp = tempfile::tempdir().expect("temp");
        let (peer, log) = lifecycle_fixture_peer(&temp, "codex-auth-future");
        let pool = AcpProcessPool::new(Duration::from_secs(30));
        assert_eq!(
            pool.probe_authentication(peer, temp.path().to_path_buf())
                .await
                .expect("unreviewed Codex identity is generic"),
            AcpAuthDoctorStatus::Unchecked
        );
        pool.shutdown(false)
            .await
            .expect("shutdown future Codex fixture");
        let entries = read_lifecycle_log(&log);
        assert!(
            lifecycle_requests(&entries, "authentication/status").is_empty(),
            "a same-name future Adapter must not receive an uncertified extension"
        );
    }

    #[test]
    fn successful_turn_clears_an_observed_generic_auth_requirement() {
        let observation = Arc::new(Mutex::new(AcpObservedAuthState::Required));
        let successful_turn: psychevo_runtime::Result<()> = Ok(());
        observe_acp_auth_result(&observation, &successful_turn, true);
        assert_eq!(
            *observation.lock().expect("auth observation"),
            AcpObservedAuthState::Unchecked
        );
    }

    #[test]
    fn safe_acp_error_drops_untrusted_data_and_terminal_cleanup_is_session_scoped() {
        let wire_error = agent_client_protocol::Error::auth_required()
            .data(json!({"secret": "agent-private-data"}));
        let safe = safe_acp_error(&wire_error);
        assert_eq!(safe, "ACP error -32000: Authentication required");
        assert!(!safe.contains("agent-private-data"));

        let registry = AcpTerminalRegistry::default();
        let state = Arc::new(Mutex::new(AcpTerminalState::new(128)));
        let completed = Arc::new(tokio::sync::Notify::new());
        let (first_kill, first_kill_rx) = watch::channel(false);
        let (second_kill, second_kill_rx) = watch::channel(false);
        registry.records.lock().expect("terminal records").extend([
            (
                "first".to_string(),
                AcpTerminalRecord {
                    session_id: "native-first".to_string(),
                    state: Arc::clone(&state),
                    kill: first_kill,
                    completed: Arc::clone(&completed),
                },
            ),
            (
                "second".to_string(),
                AcpTerminalRecord {
                    session_id: "native-second".to_string(),
                    state,
                    kill: second_kill,
                    completed,
                },
            ),
        ]);
        registry
            .terminate_session("native-first")
            .expect("terminate first session");
        assert!(*first_kill_rx.borrow());
        assert!(!*second_kill_rx.borrow());
        assert_eq!(registry.records.lock().expect("terminal records").len(), 1);
    }
}
