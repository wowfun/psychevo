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
    AbortSignal, AgentDefinition, AssistantBlock, Error, ImageInput, Message, Outcome,
    PermissionApprovalDecision, PermissionApprovalOutcome, PermissionApprovalRequest, RunResult,
    RunStreamEvent, RunStreamSink, SelectedAgent, ToolCallBlock, UserContentBlock,
    fallback_visible_session_title,
};
use serde_json::{Map, Value, json};
use tokio::process::Command;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::{
    ACP_PEER_METADATA_KEY, BackendTurnRequest, ResolvedPeerTurn, gateway_now_ms, protocol as wire,
};

include!("acp_peer/turn.rs");
include!("acp_peer/runtime_options.rs");
include!("acp_peer/stream_state.rs");
include!("acp_peer/tool_projection.rs");
include!("acp_peer/stdio_turn.rs");
include!("acp_peer/metadata_permissions.rs");
