use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message as WebSocketMessage, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header::AUTHORIZATION};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures::future::BoxFuture;
use futures::{SinkExt, StreamExt};
use psychevo::__agent_core::{ToolBinding, ToolExecutionMode, ToolOutput};
use psychevo::__product::runtime::{
    ApprovalHandler, FilesystemApprovalLifetime, FilesystemApprovalScope,
    PermissionApprovalDecision, PermissionApprovalOutcome, PermissionApprovalRequest,
};
use psychevo::{
    Application, Client, CompactThreadRequest, ForkThreadRequest, InteractionResponse,
    StartThreadRequest, Thread, ThreadListQuery, TurnEvent, TurnHandle, TurnRequest,
};
use psychevo_gateway_protocol as wire;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, RwLock, mpsc, oneshot, watch};
use tokio::task::JoinSet;

const PROTOCOL_VERSION: u32 = 1;
const OUTPUT_CAPACITY: usize = 256;
const RELAY_TOMBSTONE_CAPACITY: usize = 1_024;
const CONNECTION_REQUEST_LIMIT: usize = 64;
const CONNECTION_CONTROL_RESERVE: usize = 1;
const CONNECTION_ORDINARY_REQUEST_LIMIT: usize =
    CONNECTION_REQUEST_LIMIT - CONNECTION_CONTROL_RESERVE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionPhase {
    New,
    Initialized,
    Ready,
}

#[derive(Debug, Deserialize)]
struct RpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Debug)]
struct RpcError {
    code: i32,
    message: String,
    data: Option<Value>,
}

impl RpcError {
    fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: -32600,
            message: message.into(),
            data: None,
        }
    }

    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
            data: None,
        }
    }

    fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("method not found: {method}"),
            data: None,
        }
    }

    fn overloaded() -> Self {
        Self {
            code: -32001,
            message: "App Server connection request limit exceeded".to_string(),
            data: Some(json!({
                "limit": CONNECTION_REQUEST_LIMIT,
                "ordinaryLimit": CONNECTION_ORDINARY_REQUEST_LIMIT,
                "controlReserve": CONNECTION_CONTROL_RESERVE,
            })),
        }
    }

    fn protocol(message: impl Into<String>, data: Option<Value>) -> Self {
        Self {
            code: -32001,
            message: message.into(),
            data,
        }
    }

    fn application(error: psychevo::Error) -> Self {
        Self {
            code: -32000,
            message: error.to_string(),
            data: None,
        }
    }
}

#[derive(Clone)]
struct Output {
    sender: mpsc::Sender<Value>,
}

impl Output {
    async fn send(&self, value: Value) -> Result<(), RpcError> {
        self.sender.send(value).await.map_err(|_| RpcError {
            code: -32003,
            message: "App Server output is closed".to_string(),
            data: None,
        })
    }
}

#[derive(Clone)]
struct CallbackBroker {
    output: Output,
    pending: PendingCallbacks,
}

type PendingCallbacks = Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value, RpcError>>>>>;

impl CallbackBroker {
    async fn call(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, RpcError> {
        let id = format!("server:{}", uuid::Uuid::now_v7());
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(id.clone(), sender);
        if let Err(error) = self
            .output
            .send(json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            }))
            .await
        {
            self.pending.lock().await.remove(&id);
            return Err(error);
        }
        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(RpcError::protocol(
                format!("{method} callback connection closed"),
                None,
            )),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(RpcError::protocol(
                    format!("{method} callback timed out"),
                    None,
                ))
            }
        }
    }

    async fn resolve(&self, value: Value) {
        let Some(id) = value.get("id").and_then(Value::as_str) else {
            return;
        };
        let Some(sender) = self.pending.lock().await.remove(id) else {
            return;
        };
        let result = match value.get("error") {
            Some(Value::Object(error)) => Err(RpcError {
                code: error
                    .get("code")
                    .and_then(Value::as_i64)
                    .and_then(|code| i32::try_from(code).ok())
                    .unwrap_or(-32000),
                message: error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("client callback failed")
                    .to_string(),
                data: error.get("data").cloned(),
            }),
            Some(_) => Err(RpcError::invalid_request(
                "client callback error must be an object",
            )),
            None => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
        };
        let _ = sender.send(result);
    }

    async fn disconnect(&self) {
        let pending = std::mem::take(&mut *self.pending.lock().await);
        for (_, sender) in pending {
            let _ = sender.send(Err(RpcError::protocol(
                "client callback connection closed",
                None,
            )));
        }
    }
}

#[derive(Clone, Debug, Default)]
struct ConnectionRegistrations {
    tools: Vec<RegisteredRemoteTool>,
    approval_handler: bool,
}

#[derive(Clone)]
struct RegisteredRemoteTool {
    definition: wire::AppToolDefinition,
    validator: Arc<jsonschema::Validator>,
}

impl fmt::Debug for RegisteredRemoteTool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RegisteredRemoteTool")
            .field("name", &self.definition.name)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
struct CapturedTurnContext {
    receiver: watch::Receiver<Option<(String, String)>>,
}

impl CapturedTurnContext {
    async fn get(mut self) -> Result<(String, String), RpcError> {
        loop {
            if let Some(context) = self.receiver.borrow().clone() {
                return Ok(context);
            }
            self.receiver.changed().await.map_err(|_| {
                RpcError::protocol("Turn ended before callback routing was captured", None)
            })?;
        }
    }
}

#[derive(Clone)]
struct RemoteTool {
    definition: wire::AppToolDefinition,
    validator: Arc<jsonschema::Validator>,
    context: CapturedTurnContext,
    callbacks: CallbackBroker,
}

impl fmt::Debug for RemoteTool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteTool")
            .field("name", &self.definition.name)
            .finish_non_exhaustive()
    }
}

impl ToolBinding for RemoteTool {
    fn name(&self) -> &str {
        &self.definition.name
    }

    fn description(&self) -> &str {
        &self.definition.description
    }

    fn parameters(&self) -> Value {
        self.definition.parameters.clone()
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        match self.definition.execution_mode {
            wire::AppToolExecutionMode::Parallel => ToolExecutionMode::Parallel,
            wire::AppToolExecutionMode::Sequential => ToolExecutionMode::Sequential,
        }
    }

    fn execute(
        &self,
        tool_call_id: String,
        arguments: Value,
        mut abort: psychevo::__ai::AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let this = self.clone();
        Box::pin(async move {
            if let Err(error) = this.validator.validate(&arguments) {
                return ToolOutput::error(format!(
                    "custom Tool arguments failed JSON Schema validation: {error}"
                ));
            }
            let (thread_id, turn_id) = match this.context.get().await {
                Ok(context) => context,
                Err(error) => return ToolOutput::error(error.message),
            };
            let params = wire::AppToolCallParams {
                call_id: tool_call_id,
                tool_name: this.definition.name.clone(),
                arguments,
                thread_id,
                turn_id,
            };
            let callback = this.callbacks.call(
                "tool/call",
                match serde_json::to_value(params) {
                    Ok(value) => value,
                    Err(error) => return ToolOutput::error(error.to_string()),
                },
                Duration::from_millis(this.definition.timeout_ms),
            );
            tokio::select! {
                _ = abort.wait_for_abort() => ToolOutput::error("custom Tool call interrupted"),
                result = callback => match result {
                    Ok(value) => match serde_json::from_value::<wire::AppToolCallResult>(value) {
                        Ok(result) => ToolOutput {
                            json: result.result,
                            model_content: result.model_content,
                            attachments: Vec::new(),
                            is_error: result.is_error,
                        },
                        Err(error) => ToolOutput::error(format!(
                            "custom Tool returned a malformed result: {error}"
                        )),
                    },
                    Err(error) => ToolOutput::error(error.message),
                }
            }
        })
    }
}

#[derive(Clone)]
struct RemoteApprovalHandler {
    context: CapturedTurnContext,
    callbacks: CallbackBroker,
}

impl fmt::Debug for RemoteApprovalHandler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RemoteApprovalHandler(..)")
    }
}

impl ApprovalHandler for RemoteApprovalHandler {
    fn request_permission(
        &self,
        request: PermissionApprovalRequest,
    ) -> BoxFuture<'static, PermissionApprovalDecision> {
        let this = self.clone();
        Box::pin(async move {
            let Ok((thread_id, turn_id)) = this.context.get().await else {
                return PermissionApprovalDecision::deny();
            };
            let params = wire::AppApprovalRequestParams {
                call_id: uuid::Uuid::now_v7().to_string(),
                thread_id,
                turn_id,
                tool_call_id: request.tool_call_id,
                tool_name: request.tool_name,
                summary: request.summary,
                reason: request.reason,
                matched_rule: request.matched_rule,
                suggested_rule: request.suggested_rule,
                allow_always: request.allow_always,
                filesystem: request
                    .filesystem
                    .and_then(|value| serde_json::to_value(value).ok()),
            };
            let timeout = Duration::from_secs(request.timeout_secs.max(1));
            let Ok(value) = serde_json::to_value(params) else {
                return PermissionApprovalDecision::deny();
            };
            let Ok(value) = this
                .callbacks
                .call("approval/request", value, timeout)
                .await
            else {
                return PermissionApprovalDecision::deny();
            };
            let Ok(result) = serde_json::from_value::<wire::AppApprovalResult>(value) else {
                return PermissionApprovalDecision::deny();
            };
            app_approval_decision(result.outcome, result.filesystem_directory)
        })
    }
}

fn app_approval_decision(
    outcome: wire::AppApprovalOutcome,
    filesystem_directory: Option<String>,
) -> PermissionApprovalDecision {
    let outcome = match outcome {
        wire::AppApprovalOutcome::AllowOnce => PermissionApprovalOutcome::AllowOnce,
        wire::AppApprovalOutcome::AllowTurn => PermissionApprovalOutcome::AllowTurn,
        wire::AppApprovalOutcome::AllowSession => PermissionApprovalOutcome::AllowSession,
        wire::AppApprovalOutcome::AllowAlways => PermissionApprovalOutcome::AllowAlways,
        wire::AppApprovalOutcome::Deny => PermissionApprovalOutcome::Deny,
    };
    let filesystem_scope = filesystem_directory.map(|directory| FilesystemApprovalScope {
        directory,
        lifetime: match outcome {
            PermissionApprovalOutcome::AllowSession | PermissionApprovalOutcome::AllowAlways => {
                FilesystemApprovalLifetime::Session
            }
            _ => FilesystemApprovalLifetime::Turn,
        },
    });
    PermissionApprovalDecision {
        outcome,
        filesystem_scope,
    }
}

#[derive(Clone)]
pub struct AppServerConnection {
    application: Application,
    client: Client,
    phase: Arc<Mutex<ConnectionPhase>>,
    turns: Arc<RwLock<HashMap<String, TurnHandle>>>,
    relays: Arc<Mutex<RelayRegistry>>,
    relay_tasks: Arc<Mutex<JoinSet<()>>>,
    registrations: Arc<RwLock<ConnectionRegistrations>>,
    callbacks: CallbackBroker,
    output: Output,
}

struct RelayRegistry {
    active: HashSet<String>,
    completed: HashSet<String>,
    completed_order: VecDeque<String>,
    completed_capacity: usize,
}

impl RelayRegistry {
    fn with_capacity(completed_capacity: usize) -> Self {
        Self {
            active: HashSet::new(),
            completed: HashSet::new(),
            completed_order: VecDeque::new(),
            completed_capacity,
        }
    }

    fn start(&mut self, turn_id: String) -> bool {
        if self.active.contains(&turn_id) || self.completed.contains(&turn_id) {
            return false;
        }
        self.active.insert(turn_id)
    }

    fn complete(&mut self, turn_id: &str) {
        if !self.active.remove(turn_id) || self.completed_capacity == 0 {
            return;
        }
        let turn_id = turn_id.to_string();
        self.completed.insert(turn_id.clone());
        self.completed_order.push_back(turn_id);
        while self.completed_order.len() > self.completed_capacity {
            if let Some(evicted) = self.completed_order.pop_front() {
                self.completed.remove(&evicted);
            }
        }
    }

    #[cfg(test)]
    fn contains(&self, turn_id: &str) -> bool {
        self.active.contains(turn_id) || self.completed.contains(turn_id)
    }
}

impl std::fmt::Debug for AppServerConnection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AppServerConnection")
            .finish_non_exhaustive()
    }
}

impl AppServerConnection {
    fn new(application: Application, sender: mpsc::Sender<Value>) -> Self {
        let output = Output { sender };
        let callbacks = CallbackBroker {
            output: output.clone(),
            pending: Arc::new(Mutex::new(HashMap::new())),
        };
        Self {
            client: application.client(),
            application,
            phase: Arc::new(Mutex::new(ConnectionPhase::New)),
            turns: Arc::new(RwLock::new(HashMap::new())),
            relays: Arc::new(Mutex::new(RelayRegistry::with_capacity(
                RELAY_TOMBSTONE_CAPACITY,
            ))),
            relay_tasks: Arc::new(Mutex::new(JoinSet::new())),
            registrations: Arc::new(RwLock::new(ConnectionRegistrations::default())),
            callbacks,
            output,
        }
    }

    async fn handle_line(&self, line: &str) {
        let parsed = serde_json::from_str::<Value>(line);
        let value = match parsed {
            Ok(value) => value,
            Err(error) => {
                let _ = self
                    .output
                    .send(error_response(
                        Value::Null,
                        RpcError {
                            code: -32700,
                            message: format!("parse error: {error}"),
                            data: None,
                        },
                    ))
                    .await;
                return;
            }
        };
        self.handle_value(value).await;
    }

    async fn handle_value(&self, value: Value) {
        if value.get("method").is_none() {
            self.callbacks.resolve(value).await;
            return;
        }
        let request = match serde_json::from_value::<RpcRequest>(value) {
            Ok(request) => request,
            Err(error) => {
                let _ = self
                    .output
                    .send(error_response(
                        Value::Null,
                        RpcError::invalid_request(error.to_string()),
                    ))
                    .await;
                return;
            }
        };
        let id = request.id.clone();
        let response = self.dispatch(request).await;
        if let Some(id) = id {
            let value = match response {
                Ok(result) => success_response(id, result),
                Err(error) => error_response(id, error),
            };
            let _ = self.output.send(value).await;
        } else if let Err(error) = response {
            let _ = self
                .output
                .send(json!({
                    "jsonrpc": "2.0",
                    "method": "server/error",
                    "params": {
                        "code": error.code,
                        "message": error.message,
                        "data": error.data,
                    }
                }))
                .await;
        }
    }

    async fn dispatch(&self, request: RpcRequest) -> Result<Value, RpcError> {
        if request.jsonrpc != "2.0" {
            return Err(RpcError::invalid_request("jsonrpc must be exactly \"2.0\""));
        }
        if request.method == "initialize" {
            return self.initialize(request).await;
        }
        if request.method == "initialized" {
            if request.id.is_some() {
                return Err(RpcError::invalid_request(
                    "initialized must be a notification without an id",
                ));
            }
            let mut phase = self.phase.lock().await;
            if *phase != ConnectionPhase::Initialized {
                return Err(RpcError::protocol(
                    "initialize must complete before initialized",
                    None,
                ));
            }
            *phase = ConnectionPhase::Ready;
            return Ok(Value::Null);
        }
        if *self.phase.lock().await != ConnectionPhase::Ready {
            return Err(RpcError::protocol(
                "initialize and initialized must complete before normal requests",
                None,
            ));
        }

        match request.method.as_str() {
            "thread/start" => {
                let params = required_params::<wire::AppThreadStartParams>(&request)?;
                let mut start = StartThreadRequest::new(params.cwd);
                if let Some(source) = params.source {
                    start.source = source;
                }
                start.metadata = params.metadata;
                let thread = self
                    .client
                    .start_thread(start)
                    .await
                    .map_err(RpcError::application)?;
                let snapshot = thread.snapshot().await.map_err(RpcError::application)?;
                serde_json::to_value(snapshot).map_err(json_error)
            }
            "thread/resume" => {
                let params = required_params::<wire::AppThreadIdParams>(&request)?;
                let thread = self
                    .client
                    .resume_thread(params.thread_id)
                    .await
                    .map_err(RpcError::application)?;
                let snapshot = thread.snapshot().await.map_err(RpcError::application)?;
                serde_json::to_value(snapshot).map_err(json_error)
            }
            "thread/read" => {
                let params = required_params::<wire::AppThreadIdParams>(&request)?;
                let thread = self.thread(&params.thread_id).await?;
                serde_json::to_value(thread.snapshot().await.map_err(RpcError::application)?)
                    .map_err(json_error)
            }
            "thread/list" => {
                let params = params::<wire::AppThreadListParams>(&request)?;
                let page = self
                    .client
                    .list_threads(ThreadListQuery {
                        cwd: params.cwd.map(PathBuf::from),
                        archived: params.archived,
                        sources: params.sources,
                        cursor: params.cursor,
                        limit: params.limit.unwrap_or(50).clamp(1, 200),
                    })
                    .await
                    .map_err(RpcError::application)?;
                serde_json::to_value(json!({
                    "threads": page.threads,
                    "nextCursor": page.next_cursor,
                }))
                .map_err(json_error)
            }
            "thread/archive" => {
                let params = required_params::<wire::AppThreadIdParams>(&request)?;
                let thread = self.thread(&params.thread_id).await?;
                thread.archive().await.map_err(RpcError::application)?;
                Ok(json!({ "archived": true, "threadId": params.thread_id }))
            }
            "thread/compact" => {
                let params = required_params::<wire::AppThreadCompactParams>(&request)?;
                let thread = self.thread(&params.thread_id).await?;
                let result = thread
                    .compact(CompactThreadRequest {
                        model: params.model,
                        reasoning_effort: params.reasoning_effort,
                        instructions: params.instructions,
                        force: params.force,
                        ..CompactThreadRequest::default()
                    })
                    .await
                    .map_err(RpcError::application)?;
                serde_json::to_value(result).map_err(json_error)
            }
            "thread/fork" => {
                let params = required_params::<wire::AppThreadForkParams>(&request)?;
                let thread = self.thread(&params.thread_id).await?;
                let fork = thread
                    .fork(ForkThreadRequest {
                        before_session_seq: params.before_session_seq,
                    })
                    .await
                    .map_err(RpcError::application)?;
                let snapshot = fork.snapshot().await.map_err(RpcError::application)?;
                serde_json::to_value(snapshot).map_err(json_error)
            }
            "tool/register" => {
                let params = params::<wire::AppToolRegisterParams>(&request)?;
                let tools = validate_registrations(&params)?;
                let count = params.tools.len();
                *self.registrations.write().await = ConnectionRegistrations {
                    tools,
                    approval_handler: params.approval_handler,
                };
                Ok(json!({
                    "registered": true,
                    "toolCount": count,
                    "approvalHandler": params.approval_handler,
                    "clarifyHandler": params.clarify_handler,
                }))
            }
            "turn/start" => {
                let params = required_params::<wire::AppTurnStartParams>(&request)?;
                validate_caller_turn_id(&params.turn_id)?;
                let thread = self.thread(&params.thread_id).await?;
                let registrations = self.registrations.read().await.clone();
                let (context_sender, context_receiver) = watch::channel(None);
                let context = CapturedTurnContext {
                    receiver: context_receiver,
                };
                let approval_handler = (params.use_registered_approval_handler
                    && registrations.approval_handler)
                    .then(|| {
                        Arc::new(RemoteApprovalHandler {
                            context: context.clone(),
                            callbacks: self.callbacks.clone(),
                        }) as Arc<dyn ApprovalHandler>
                    });
                let mut input = TurnRequest::new(params.prompt)
                    .with_identity(
                        params.source.unwrap_or_else(|| "sdk".to_string()),
                        params.client_turn_id,
                    )
                    .with_model(params.model, params.reasoning_effort)
                    .with_agent(None, params.no_agents, params.no_skills)
                    .with_environment(params.inherited_env, None, None)
                    .with_approval(None, approval_handler, true);
                input.__set_turn_id(params.turn_id);
                for registration in registrations.tools {
                    input = input.tool(Arc::new(RemoteTool {
                        definition: registration.definition,
                        validator: registration.validator,
                        context: context.clone(),
                        callbacks: self.callbacks.clone(),
                    }));
                }
                let handle = thread
                    .start_turn(input)
                    .await
                    .map_err(RpcError::application)?;
                let receipt = handle.receipt().clone();
                let _ =
                    context_sender.send(Some((receipt.thread_id.clone(), receipt.turn_id.clone())));
                self.turns
                    .write()
                    .await
                    .insert(receipt.turn_id.clone(), handle.clone());
                self.ensure_event_relay(handle).await;
                serde_json::to_value(receipt).map_err(json_error)
            }
            "turn/wait" => {
                let params = required_params::<wire::AppTurnIdParams>(&request)?;
                let handle = self.turn(&params.turn_id).await?;
                let result = handle.wait().await;
                self.turns.write().await.remove(&params.turn_id);
                let result = result.map_err(RpcError::application)?;
                serde_json::to_value(result).map_err(json_error)
            }
            "turn/resume" => {
                let params = required_params::<wire::AppTurnIdParams>(&request)?;
                let handle = self.turn(&params.turn_id).await?;
                let receipt = handle.receipt().clone();
                self.turns
                    .write()
                    .await
                    .insert(receipt.turn_id.clone(), handle.clone());
                self.ensure_event_relay(handle).await;
                serde_json::to_value(receipt).map_err(json_error)
            }
            "turn/interrupt" => {
                let params = required_params::<wire::AppTurnIdParams>(&request)?;
                self.turn(&params.turn_id).await?.interrupt();
                Ok(json!({ "interrupted": true, "turnId": params.turn_id }))
            }
            "turn/steer" => {
                let params = required_params::<wire::AppTurnSteerParams>(&request)?;
                let accepted = self.turn(&params.turn_id).await?.steer(params.input);
                Ok(json!({ "accepted": accepted, "turnId": params.turn_id }))
            }
            "interaction/respond" => {
                let params = required_params::<wire::AppInteractionRespondParams>(&request)?;
                let response = match params.response {
                    wire::AppInteractionResponse::Permission {
                        outcome,
                        filesystem_directory,
                    } => InteractionResponse::Permission(app_approval_decision(
                        outcome,
                        filesystem_directory,
                    )),
                    wire::AppInteractionResponse::Clarify { answers } => {
                        InteractionResponse::Clarify(answers)
                    }
                    wire::AppInteractionResponse::Cancel => InteractionResponse::Cancel,
                };
                let accepted = self
                    .turn(&params.turn_id)
                    .await?
                    .respond(&params.interaction_id, response)
                    .await
                    .map_err(RpcError::application)?
                    .accepted;
                Ok(json!({
                    "accepted": accepted,
                    "turnId": params.turn_id,
                    "interactionId": params.interaction_id,
                }))
            }
            "shutdown" => {
                let report = self
                    .application
                    .shutdown()
                    .await
                    .map_err(RpcError::application)?;
                Ok(json!({ "shutdown": true, "report": report }))
            }
            method => Err(RpcError::method_not_found(method)),
        }
    }

    async fn initialize(&self, request: RpcRequest) -> Result<Value, RpcError> {
        if request.id.is_none() {
            return Err(RpcError::invalid_request(
                "initialize must be a request with an id",
            ));
        }
        let params = required_params::<wire::AppInitializeParams>(&request)?;
        let mut phase = self.phase.lock().await;
        if *phase != ConnectionPhase::New {
            return Err(RpcError::protocol("initialize may be sent only once", None));
        }
        if params.protocol_min > params.protocol_max {
            return Err(RpcError::invalid_params(
                "protocolMin must not exceed protocolMax",
            ));
        }
        if !(params.protocol_min..=params.protocol_max).contains(&PROTOCOL_VERSION) {
            return Err(RpcError::protocol(
                "no compatible Psychevo App Server protocol version",
                Some(json!({
                    "clientMin": params.protocol_min,
                    "clientMax": params.protocol_max,
                    "serverMin": PROTOCOL_VERSION,
                    "serverMax": PROTOCOL_VERSION,
                })),
            ));
        }
        *phase = ConnectionPhase::Initialized;
        serde_json::to_value(wire::AppInitializeResult {
            server: wire::AppProductInfo {
                name: "psychevo-app-server".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            protocol_version: PROTOCOL_VERSION,
            protocol_min: PROTOCOL_VERSION,
            protocol_max: PROTOCOL_VERSION,
            capabilities: wire::AppServerCapabilities {
                threads: true,
                turns: true,
                event_replay: "bounded".to_string(),
                interactions: true,
                custom_tools: true,
            },
        })
        .map_err(json_error)
    }

    async fn thread(&self, thread_id: &str) -> Result<Thread, RpcError> {
        self.client
            .resume_thread(thread_id.to_string())
            .await
            .map_err(RpcError::application)
    }

    async fn turn(&self, turn_id: &str) -> Result<TurnHandle, RpcError> {
        if let Some(turn) = self.turns.read().await.get(turn_id).cloned() {
            return Ok(turn);
        }
        self.client
            .resume_turn(turn_id.to_string())
            .await
            .map_err(RpcError::application)
    }

    async fn ensure_event_relay(&self, handle: TurnHandle) {
        let turn_id = handle.receipt().turn_id.clone();
        if !self.relays.lock().await.start(turn_id) {
            return;
        }
        let output = self.output.clone();
        let turns = self.turns.clone();
        let relays = self.relays.clone();
        let relay_turn_id = handle.receipt().turn_id.clone();
        let mut relay_tasks = self.relay_tasks.lock().await;
        while relay_tasks.try_join_next().is_some() {}
        relay_tasks.spawn(async move {
            let mut events = handle.events();
            while let Some(event) = events.next().await {
                if output
                    .send(turn_event_notification(
                        handle.receipt().thread_id.as_str(),
                        handle.receipt().turn_id.as_str(),
                        event,
                    ))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            turns.write().await.remove(&relay_turn_id);
            relays.lock().await.complete(&relay_turn_id);
        });
    }

    async fn disconnect(&self) {
        self.callbacks.disconnect().await;
        let mut relays = self.relay_tasks.lock().await;
        relays.abort_all();
        while relays.join_next().await.is_some() {}
    }
}

fn success_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn error_response(id: Value, error: RpcError) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": error.code,
            "message": error.message,
            "data": error.data,
        }
    })
}

fn turn_event_notification(thread_id: &str, turn_id: &str, event: TurnEvent) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "turn/event",
        "params": {
            "threadId": thread_id,
            "turnId": turn_id,
            "event": event,
        }
    })
}

fn json_error(error: serde_json::Error) -> RpcError {
    RpcError {
        code: -32603,
        message: format!("response serialization failed: {error}"),
        data: None,
    }
}

fn params<T: DeserializeOwned + Default>(request: &RpcRequest) -> Result<T, RpcError> {
    request
        .params
        .clone()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| RpcError::invalid_params(error.to_string()))
        .map(|value| value.unwrap_or_default())
}

fn required_params<T: DeserializeOwned>(request: &RpcRequest) -> Result<T, RpcError> {
    let value = request
        .params
        .clone()
        .ok_or_else(|| RpcError::invalid_params("params are required"))?;
    serde_json::from_value(value).map_err(|error| RpcError::invalid_params(error.to_string()))
}

fn requires_receive_order(value: &Value) -> bool {
    matches!(
        value.get("method").and_then(Value::as_str),
        Some("initialize" | "initialized" | "tool/register")
    )
}

fn line_requires_receive_order(line: &str) -> bool {
    serde_json::from_str::<Value>(line)
        .ok()
        .as_ref()
        .is_some_and(requires_receive_order)
}

fn is_durable_mutation(value: &Value) -> bool {
    matches!(
        value.get("method").and_then(Value::as_str),
        Some(
            "thread/start"
                | "thread/fork"
                | "thread/archive"
                | "thread/compact"
                | "turn/start"
                | "interaction/respond"
                | "shutdown"
        )
    )
}

fn is_control_request(value: &Value) -> bool {
    matches!(
        value.get("method").and_then(Value::as_str),
        Some("shutdown" | "turn/interrupt" | "turn/steer" | "interaction/respond")
    )
}

fn is_callback_response(value: &Value) -> bool {
    value.get("method").is_none()
}

fn request_capacity_available(active_requests: usize, value: &Value) -> bool {
    if is_callback_response(value) {
        return true;
    }
    let limit = if is_control_request(value) {
        CONNECTION_REQUEST_LIMIT
    } else {
        CONNECTION_ORDINARY_REQUEST_LIMIT
    };
    active_requests < limit
}

async fn send_connection_overload(connection: &AppServerConnection, value: &Value) {
    let id = value.get("id").cloned().unwrap_or(Value::Null);
    let _ = connection
        .output
        .send(error_response(id, RpcError::overloaded()))
        .await;
}

fn validate_registrations(
    params: &wire::AppToolRegisterParams,
) -> Result<Vec<RegisteredRemoteTool>, RpcError> {
    let mut names = std::collections::HashSet::new();
    let mut tools = Vec::with_capacity(params.tools.len());
    for tool in &params.tools {
        let name = tool.name.trim();
        if name.is_empty() {
            return Err(RpcError::invalid_params(
                "custom Tool name must not be empty",
            ));
        }
        if name != tool.name {
            return Err(RpcError::invalid_params(
                "custom Tool name must not have surrounding whitespace",
            ));
        }
        if !names.insert(name) {
            return Err(RpcError::invalid_params(format!(
                "duplicate custom Tool name: {name}"
            )));
        }
        if tool.description.trim().is_empty() {
            return Err(RpcError::invalid_params(format!(
                "custom Tool description must not be empty: {name}"
            )));
        }
        if !tool.parameters.is_object() {
            return Err(RpcError::invalid_params(format!(
                "custom Tool parameters must be a JSON Schema object: {name}"
            )));
        }
        let validator = jsonschema::validator_for(&tool.parameters).map_err(|error| {
            RpcError::invalid_params(format!(
                "custom Tool parameters are not a valid JSON Schema for {name}: {error}"
            ))
        })?;
        if tool.timeout_ms == 0 {
            return Err(RpcError::invalid_params(format!(
                "custom Tool timeout must be greater than zero: {name}"
            )));
        }
        tools.push(RegisteredRemoteTool {
            definition: tool.clone(),
            validator: Arc::new(validator),
        });
    }
    Ok(tools)
}

fn validate_caller_turn_id(turn_id: &str) -> Result<(), RpcError> {
    if turn_id.is_empty() {
        return Err(RpcError::invalid_params("Turn id must not be empty"));
    }
    if turn_id.trim() != turn_id {
        return Err(RpcError::invalid_params(
            "Turn id must not have surrounding whitespace",
        ));
    }
    Ok(())
}

pub async fn run_stdio(application: Application) -> psychevo::Result<()> {
    run_stdio_streams(application, tokio::io::stdin(), tokio::io::stdout()).await
}

async fn run_stdio_streams<R, W>(
    application: Application,
    input: R,
    output: W,
) -> psychevo::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Send + Unpin + 'static,
{
    let (output_tx, mut output_rx) = mpsc::channel::<Value>(OUTPUT_CAPACITY);
    let connection = AppServerConnection::new(application.clone(), output_tx);
    let writer = tokio::spawn(async move {
        let mut output = output;
        while let Some(value) = output_rx.recv().await {
            let line = serde_json::to_vec(&value)?;
            output.write_all(&line).await?;
            output.write_all(b"\n").await?;
            output.flush().await?;
        }
        Ok::<(), std::io::Error>(())
    });
    let mut lines = BufReader::new(input).lines();
    let mut requests = JoinSet::new();
    loop {
        tokio::select! {
            completed = requests.join_next(), if !requests.is_empty() => {
                let _ = completed;
            }
            line = lines.next_line() => {
                let Some(line) = line? else {
                    break;
                };
                if line.trim().is_empty() {
                    continue;
                }
                if line_requires_receive_order(&line) {
                    connection.handle_line(&line).await;
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    if is_callback_response(&value) {
                        connection.handle_value(value).await;
                        continue;
                    }
                    if !request_capacity_available(requests.len(), &value) {
                        send_connection_overload(&connection, &value).await;
                        continue;
                    }
                }
                let connection = connection.clone();
                requests.spawn(async move {
                    connection.handle_line(&line).await;
                });
            }
        }
    }
    while requests.join_next().await.is_some() {}
    application.shutdown().await?.require_clean()?;
    drop(connection);
    writer
        .await
        .map_err(|error| psychevo::Error::Message(format!("App Server writer failed: {error}")))?
        .map_err(psychevo::Error::from)
}

#[derive(Clone)]
struct AppWebSocketState {
    application: Application,
    token: Arc<str>,
}

pub struct BoundAppServerWebSocket {
    local_addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<psychevo::Result<()>>,
}

impl fmt::Debug for BoundAppServerWebSocket {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BoundAppServerWebSocket")
            .field("local_addr", &self.local_addr)
            .finish_non_exhaustive()
    }
}

impl BoundAppServerWebSocket {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn uri(&self) -> String {
        format!("ws://{}/app-server", self.local_addr)
    }

    pub async fn shutdown(mut self) -> psychevo::Result<()> {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.task.await.map_err(|error| {
            psychevo::Error::Message(format!("App Server WebSocket task failed: {error}"))
        })?
    }
}

pub async fn bind_websocket(
    application: Application,
    address: SocketAddr,
    token: impl Into<String>,
) -> psychevo::Result<BoundAppServerWebSocket> {
    let token = token.into();
    if token.is_empty() {
        return Err(psychevo::Error::Message(
            "App Server WebSocket requires a non-empty bearer token".to_string(),
        ));
    }
    let listener = TcpListener::bind(address).await?;
    let local_addr = listener.local_addr()?;
    let state = AppWebSocketState {
        application: application.clone(),
        token: Arc::from(token),
    };
    let app = Router::new()
        .route("/app-server", get(app_websocket_upgrade))
        .with_state(state);
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .map_err(psychevo::Error::from)?;
        application.shutdown().await?.require_clean().map(|_| ())
    });
    Ok(BoundAppServerWebSocket {
        local_addr,
        shutdown: Some(shutdown_tx),
        task,
    })
}

async fn app_websocket_upgrade(
    State(state): State<AppWebSocketState>,
    headers: HeaderMap,
    websocket: WebSocketUpgrade,
) -> Response {
    let authorized = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| token == state.token.as_ref());
    if !authorized {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    websocket
        .on_upgrade(move |socket| run_websocket_connection(socket, state.application))
        .into_response()
}

async fn run_websocket_connection(socket: WebSocket, application: Application) {
    let (output_tx, mut output_rx) = mpsc::channel::<Value>(OUTPUT_CAPACITY);
    let connection = AppServerConnection::new(application, output_tx);
    let (mut sender, mut receiver) = socket.split();
    let writer = tokio::spawn(async move {
        while let Some(value) = output_rx.recv().await {
            if sender
                .send(WebSocketMessage::Text(value.to_string().into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });
    let mut wait_requests = JoinSet::new();
    let mut durable_requests = JoinSet::new();
    loop {
        let message = tokio::select! {
            completed = wait_requests.join_next(), if !wait_requests.is_empty() => {
                let _ = completed;
                continue;
            }
            completed = durable_requests.join_next(), if !durable_requests.is_empty() => {
                let _ = completed;
                continue;
            }
            message = receiver.next() => message,
        };
        let Some(message) = message else {
            break;
        };
        match message {
            Ok(WebSocketMessage::Text(text)) => {
                let value = match serde_json::from_str::<Value>(&text) {
                    Ok(value) => value,
                    Err(error) => {
                        let _ = connection
                            .output
                            .send(error_response(
                                Value::Null,
                                RpcError {
                                    code: -32700,
                                    message: format!("parse error: {error}"),
                                    data: None,
                                },
                            ))
                            .await;
                        continue;
                    }
                };
                if requires_receive_order(&value) {
                    connection.handle_value(value).await;
                    continue;
                }
                if is_callback_response(&value) {
                    connection.handle_value(value).await;
                    continue;
                }
                if !request_capacity_available(wait_requests.len() + durable_requests.len(), &value)
                {
                    send_connection_overload(&connection, &value).await;
                    continue;
                }
                let durable = is_durable_mutation(&value);
                let connection = connection.clone();
                let request = async move {
                    connection.handle_value(value).await;
                };
                if durable {
                    durable_requests.spawn(request);
                } else {
                    wait_requests.spawn(request);
                }
            }
            Ok(WebSocketMessage::Close(_)) | Err(_) => break,
            Ok(WebSocketMessage::Binary(_)) => {
                let _ = connection
                    .output
                    .send(error_response(
                        Value::Null,
                        RpcError::invalid_request(
                            "App Server WebSocket accepts text JSON messages only",
                        ),
                    ))
                    .await;
            }
            Ok(WebSocketMessage::Ping(_)) | Ok(WebSocketMessage::Pong(_)) => {}
        }
    }
    connection.disconnect().await;
    wait_requests.abort_all();
    while wait_requests.join_next().await.is_some() {}
    if !durable_requests.is_empty() {
        tokio::spawn(async move { while durable_requests.join_next().await.is_some() {} });
    }
    drop(connection);
    writer.abort();
    let _ = writer.await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::BoxFuture;
    use psychevo::{AgentSessionAdapter, AgentTurnRequest, TurnOutcome, TurnResult};
    use std::sync::atomic::{AtomicBool, Ordering};

    fn object_validator() -> Arc<jsonschema::Validator> {
        Arc::new(jsonschema::validator_for(&json!({"type": "object"})).expect("object JSON Schema"))
    }

    #[derive(Debug)]
    struct ImmediateAdapter;

    #[derive(Debug)]
    struct BlockingAdapter {
        started: Arc<tokio::sync::Notify>,
    }

    #[derive(Debug)]
    struct ClarifyInspectAdapter {
        clarify_enabled: Arc<AtomicBool>,
    }

    impl AgentSessionAdapter for ImmediateAdapter {
        fn run_turn(
            &self,
            request: AgentTurnRequest,
        ) -> BoxFuture<'static, psychevo::Result<TurnResult>> {
            Box::pin(async move {
                Ok(TurnResult {
                    thread_id: request.receipt.thread_id,
                    outcome: TurnOutcome::Completed,
                    final_answer: "app server answer".to_string(),
                    provider: "fake".to_string(),
                    model: "fake".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }
    }

    impl AgentSessionAdapter for BlockingAdapter {
        fn run_turn(
            &self,
            request: AgentTurnRequest,
        ) -> BoxFuture<'static, psychevo::Result<TurnResult>> {
            let started = Arc::clone(&self.started);
            Box::pin(async move {
                started.notify_one();
                while !request.control.is_interrupted() {
                    tokio::task::yield_now().await;
                }
                Ok(TurnResult {
                    thread_id: request.receipt.thread_id,
                    outcome: TurnOutcome::Interrupted,
                    final_answer: String::new(),
                    provider: "fake".to_string(),
                    model: "fake".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }
    }

    impl AgentSessionAdapter for ClarifyInspectAdapter {
        fn run_turn(
            &self,
            request: AgentTurnRequest,
        ) -> BoxFuture<'static, psychevo::Result<TurnResult>> {
            let clarify_enabled = Arc::clone(&self.clarify_enabled);
            Box::pin(async move {
                clarify_enabled.store(request.input.clarify_enabled(), Ordering::SeqCst);
                Ok(TurnResult {
                    thread_id: request.receipt.thread_id,
                    outcome: TurnOutcome::Completed,
                    final_answer: "clarify inspected".to_string(),
                    provider: "fake".to_string(),
                    model: "fake".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }
    }

    async fn test_connection() -> (
        tempfile::TempDir,
        AppServerConnection,
        mpsc::Receiver<Value>,
    ) {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(ImmediateAdapter))
            .build()
            .await
            .expect("application");
        let (tx, rx) = mpsc::channel(32);
        (temp, AppServerConnection::new(application, tx), rx)
    }

    fn request(id: i64, method: &str, params: Value) -> RpcRequest {
        RpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(id)),
            method: method.to_string(),
            params: Some(params),
        }
    }

    async fn initialize(connection: &AppServerConnection) {
        connection
            .dispatch(request(
                1,
                "initialize",
                json!({
                    "client": { "name": "test", "version": "0" },
                    "protocolMin": 1,
                    "protocolMax": 1,
                    "capabilities": {},
                }),
            ))
            .await
            .expect("initialize");
        connection
            .dispatch(RpcRequest {
                jsonrpc: "2.0".to_string(),
                id: None,
                method: "initialized".to_string(),
                params: Some(json!({})),
            })
            .await
            .expect("initialized");
    }

    #[tokio::test]
    async fn initialize_negotiates_protocol_without_telemetry() {
        let (_temp, connection, _rx) = test_connection().await;
        let result = connection
            .dispatch(request(
                1,
                "initialize",
                json!({
                    "client": { "name": "test", "version": "0" },
                    "protocolMin": 1,
                    "protocolMax": 1,
                    "capabilities": {},
                }),
            ))
            .await
            .expect("initialize");
        assert_eq!(result["protocolVersion"], 1);
        assert!(result["capabilities"].get("telemetry").is_none());
        let error = connection
            .dispatch(request(
                2,
                "initialize",
                json!({
                    "client": { "name": "test", "version": "0" },
                    "protocolMin": 1,
                    "protocolMax": 1,
                    "capabilities": {},
                }),
            ))
            .await
            .expect_err("duplicate initialize");
        assert_eq!(error.code, -32001);
    }

    #[tokio::test]
    async fn incompatible_or_missing_handshake_is_rejected() {
        let (_temp, connection, _rx) = test_connection().await;
        let before_initialize = connection
            .dispatch(request(1, "thread/list", json!({})))
            .await
            .expect_err("handshake required");
        assert_eq!(before_initialize.code, -32001);
        let incompatible = connection
            .dispatch(request(
                2,
                "initialize",
                json!({
                    "client": { "name": "test", "version": "0" },
                    "protocolMin": 2,
                    "protocolMax": 3,
                    "capabilities": {},
                }),
            ))
            .await
            .expect_err("incompatible");
        assert_eq!(incompatible.code, -32001);
        assert_eq!(incompatible.data.expect("version data")["serverMax"], 1);
    }

    #[tokio::test]
    async fn thread_and_turn_methods_use_framework_objects_and_emit_events() {
        let (temp, connection, mut rx) = test_connection().await;
        initialize(&connection).await;
        let thread = connection
            .dispatch(request(
                2,
                "thread/start",
                json!({ "cwd": temp.path(), "source": "python" }),
            ))
            .await
            .expect("thread start");
        let thread_id = thread["id"].as_str().expect("thread id");
        let receipt = connection
            .dispatch(request(
                3,
                "turn/start",
                json!({
                    "threadId": thread_id,
                    "turnId": "turn-caller-1",
                    "prompt": "hello",
                    "noAgents": true,
                    "noSkills": true,
                }),
            ))
            .await
            .expect("turn start");
        let turn_id = receipt["turnId"].as_str().expect("turn id");
        assert_eq!(turn_id, "turn-caller-1");
        let result = connection
            .dispatch(request(4, "turn/wait", json!({ "turnId": turn_id })))
            .await
            .expect("turn wait");
        assert_eq!(result["finalAnswer"], "app server answer");
        let mut saw_completed = false;
        while let Ok(notification) =
            tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await
        {
            let Some(notification) = notification else {
                break;
            };
            if notification["method"] == "turn/event"
                && notification["params"]["event"]["type"] == "completed"
            {
                saw_completed = true;
                break;
            }
        }
        assert!(saw_completed);
    }

    #[tokio::test]
    async fn turn_start_rejects_an_empty_caller_turn_id_before_dispatch() {
        let (temp, connection, _rx) = test_connection().await;
        initialize(&connection).await;
        let thread = connection
            .dispatch(request(
                2,
                "thread/start",
                json!({ "cwd": temp.path(), "source": "python" }),
            ))
            .await
            .expect("thread start");

        let error = connection
            .dispatch(request(
                3,
                "turn/start",
                json!({
                    "threadId": thread["id"],
                    "turnId": "",
                    "prompt": "must not dispatch",
                }),
            ))
            .await
            .expect_err("empty Turn id");

        assert_eq!(error.code, -32602);
        assert_eq!(error.message, "Turn id must not be empty");
    }

    #[tokio::test]
    async fn app_server_turn_keeps_durable_clarify_without_a_registered_handler() {
        let temp = tempfile::tempdir().expect("tempdir");
        let clarify_enabled = Arc::new(AtomicBool::new(false));
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(ClarifyInspectAdapter {
                clarify_enabled: Arc::clone(&clarify_enabled),
            }))
            .build()
            .await
            .expect("application");
        let (output, _events) = mpsc::channel(32);
        let connection = AppServerConnection::new(application, output);
        initialize(&connection).await;
        let thread = connection
            .dispatch(request(
                2,
                "thread/start",
                json!({"cwd": temp.path(), "source": "python"}),
            ))
            .await
            .expect("thread");
        let turn = connection
            .dispatch(request(
                3,
                "turn/start",
                json!({
                    "threadId": thread["id"],
                    "turnId": "turn-clarify-1",
                    "prompt": "clarify",
                    "useRegisteredClarifyHandler": false,
                }),
            ))
            .await
            .expect("turn");
        connection
            .dispatch(request(4, "turn/wait", json!({"turnId": turn["turnId"]})))
            .await
            .expect("wait");
        assert!(clarify_enabled.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn repeated_turn_resume_uses_one_event_relay_per_connection() {
        let (temp, connection, mut events) = test_connection().await;
        initialize(&connection).await;
        let thread = connection
            .dispatch(request(
                2,
                "thread/start",
                json!({"cwd": temp.path(), "source": "python"}),
            ))
            .await
            .expect("thread");
        let turn = connection
            .dispatch(request(
                3,
                "turn/start",
                json!({
                    "threadId": thread["id"],
                    "turnId": "turn-resume-once",
                    "prompt": "once",
                }),
            ))
            .await
            .expect("turn");
        let turn_id = turn["turnId"].as_str().expect("turn id");
        connection
            .dispatch(request(4, "turn/wait", json!({"turnId": turn_id})))
            .await
            .expect("wait");
        for id in 5..9 {
            connection
                .dispatch(request(id, "turn/resume", json!({"turnId": turn_id})))
                .await
                .expect("resume");
        }
        for _ in 0..64 {
            tokio::task::yield_now().await;
        }
        let mut accepted = 0;
        let mut completed = 0;
        while let Ok(notification) = events.try_recv() {
            if notification["method"] != "turn/event" {
                continue;
            }
            match notification["params"]["event"]["type"].as_str() {
                Some("accepted") => accepted += 1,
                Some("completed") => completed += 1,
                _ => {}
            }
        }
        assert_eq!(accepted, 1);
        assert_eq!(completed, 1);
    }

    #[tokio::test]
    async fn terminal_relay_releases_the_live_turn_handle_but_keeps_its_tombstone() {
        let (temp, connection, _events) = test_connection().await;
        initialize(&connection).await;
        let thread = connection
            .dispatch(request(
                2,
                "thread/start",
                json!({"cwd": temp.path(), "source": "python"}),
            ))
            .await
            .expect("thread");
        let turn = connection
            .dispatch(request(
                3,
                "turn/start",
                json!({
                    "threadId": thread["id"],
                    "turnId": "turn-relay-cleanup",
                    "prompt": "once",
                }),
            ))
            .await
            .expect("turn");
        let turn_id = turn["turnId"].as_str().expect("turn id").to_string();
        connection
            .dispatch(request(4, "turn/wait", json!({"turnId": turn_id.clone()})))
            .await
            .expect("wait");

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if !connection.turns.read().await.contains_key(&turn_id) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("terminal relay releases live handle");
        assert!(
            connection.relays.lock().await.contains(&turn_id),
            "the connection-local tombstone prevents replaying a second relay"
        );
    }

    #[test]
    fn completed_relay_tombstones_evict_in_fifo_order() {
        let mut relays = RelayRegistry::with_capacity(2);
        for turn_id in ["turn-1", "turn-2", "turn-3"] {
            assert!(relays.start(turn_id.to_string()));
            relays.complete(turn_id);
        }

        assert!(!relays.contains("turn-1"));
        assert!(relays.contains("turn-2"));
        assert!(relays.contains("turn-3"));
        assert_eq!(relays.completed.len(), 2);
    }

    #[tokio::test]
    async fn disconnect_cancels_a_quiet_relay_without_cancelling_the_durable_turn() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(tokio::sync::Notify::new());
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(BlockingAdapter {
                started: Arc::clone(&started),
            }))
            .build()
            .await
            .expect("application");
        let client = application.client();
        let (output, _events) = mpsc::channel(32);
        let connection = AppServerConnection::new(application, output);
        initialize(&connection).await;
        let thread = connection
            .dispatch(request(
                2,
                "thread/start",
                json!({"cwd": temp.path(), "source": "python"}),
            ))
            .await
            .expect("thread");
        let turn = connection
            .dispatch(request(
                3,
                "turn/start",
                json!({
                    "threadId": thread["id"],
                    "turnId": "turn-disconnect-wait",
                    "prompt": "wait",
                }),
            ))
            .await
            .expect("turn");
        let turn_id = turn["turnId"].as_str().expect("turn id").to_string();
        started.notified().await;

        tokio::time::timeout(Duration::from_millis(250), connection.disconnect())
            .await
            .expect("quiet relay disconnect is bounded");

        let handle = client
            .resume_turn(turn_id)
            .await
            .expect("durable active turn survives connection");
        handle.interrupt();
        assert_eq!(
            handle.wait().await.expect("interrupted turn").outcome,
            TurnOutcome::Interrupted
        );
    }

    #[test]
    fn overload_response_exposes_the_connection_limit() {
        let error = RpcError::overloaded();
        assert_eq!(error.code, -32001);
        assert_eq!(
            error.data,
            Some(json!({
                "limit": CONNECTION_REQUEST_LIMIT,
                "ordinaryLimit": CONNECTION_ORDINARY_REQUEST_LIMIT,
                "controlReserve": CONNECTION_CONTROL_RESERVE,
            }))
        );
    }

    #[test]
    fn callback_response_bypasses_saturated_request_capacity() {
        let response = json!({
            "jsonrpc": "2.0",
            "id": "server:callback-1",
            "result": {"outcome": "allow_once"},
        });

        assert!(request_capacity_available(
            CONNECTION_REQUEST_LIMIT,
            &response
        ));
    }

    #[tokio::test]
    async fn completed_turn_and_thread_reattach_on_a_new_connection() {
        let (temp, connection, _rx) = test_connection().await;
        initialize(&connection).await;
        let thread = connection
            .dispatch(request(
                2,
                "thread/start",
                json!({ "cwd": temp.path(), "source": "python" }),
            ))
            .await
            .expect("thread start");
        let thread_id = thread["id"].as_str().expect("thread id").to_string();
        let receipt = connection
            .dispatch(request(
                3,
                "turn/start",
                json!({
                    "threadId": thread_id,
                    "turnId": "turn-reconnect-complete",
                    "prompt": "hello",
                }),
            ))
            .await
            .expect("turn start");
        let turn_id = receipt["turnId"].as_str().expect("turn id").to_string();
        connection
            .dispatch(request(4, "turn/wait", json!({ "turnId": turn_id })))
            .await
            .expect("turn wait");

        let (output, _events) = mpsc::channel(32);
        let reconnected = AppServerConnection::new(connection.application.clone(), output);
        initialize(&reconnected).await;
        let resumed_thread = reconnected
            .dispatch(request(
                5,
                "thread/resume",
                json!({ "threadId": thread_id }),
            ))
            .await
            .expect("thread resume");
        assert_eq!(resumed_thread["id"], thread_id);
        let resumed_turn = reconnected
            .dispatch(request(6, "turn/resume", json!({ "turnId": turn_id })))
            .await
            .expect("turn resume");
        assert_eq!(resumed_turn["turnId"], turn_id);
        let result = reconnected
            .dispatch(request(7, "turn/wait", json!({ "turnId": turn_id })))
            .await
            .expect("durable wait");
        assert_eq!(result["finalAnswer"], "app server answer");
    }

    #[tokio::test]
    async fn active_turn_reattaches_and_accepts_controls_on_a_new_connection() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(tokio::sync::Notify::new());
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(BlockingAdapter {
                started: Arc::clone(&started),
            }))
            .build()
            .await
            .expect("application");
        let (first_output, _first_events) = mpsc::channel(32);
        let first = AppServerConnection::new(application, first_output);
        initialize(&first).await;
        let thread = first
            .dispatch(request(
                2,
                "thread/start",
                json!({ "cwd": temp.path(), "source": "python" }),
            ))
            .await
            .expect("thread start");
        let thread_id = thread["id"].as_str().expect("thread id").to_string();
        let receipt = first
            .dispatch(request(
                3,
                "turn/start",
                json!({
                    "threadId": thread_id,
                    "turnId": "turn-reconnect-active",
                    "prompt": "wait",
                }),
            ))
            .await
            .expect("turn start");
        let turn_id = receipt["turnId"].as_str().expect("turn id").to_string();
        started.notified().await;

        let (second_output, _second_events) = mpsc::channel(32);
        let second = AppServerConnection::new(first.application.clone(), second_output);
        initialize(&second).await;
        let resumed_thread = second
            .dispatch(request(
                4,
                "thread/resume",
                json!({ "threadId": thread_id }),
            ))
            .await
            .expect("active thread resume");
        assert_eq!(resumed_thread["activeTurnId"], turn_id);
        let resumed_turn = second
            .dispatch(request(5, "turn/resume", json!({ "turnId": turn_id })))
            .await
            .expect("active turn resume");
        assert_eq!(resumed_turn["turnId"], turn_id);
        let steer = second
            .dispatch(request(
                6,
                "turn/steer",
                json!({ "turnId": turn_id, "input": "additional input" }),
            ))
            .await
            .expect("steer");
        assert_eq!(steer["accepted"], true);
        second
            .dispatch(request(7, "turn/interrupt", json!({ "turnId": turn_id })))
            .await
            .expect("interrupt");
        let result = second
            .dispatch(request(8, "turn/wait", json!({ "turnId": turn_id })))
            .await
            .expect("interrupted wait");
        assert_eq!(result["outcome"], "interrupted");
    }

    #[tokio::test]
    async fn custom_tool_callback_is_correlated_to_the_captured_connection() {
        let (_temp, connection, mut rx) = test_connection().await;
        let (context_tx, context_rx) =
            watch::channel(Some(("thread-1".to_string(), "turn-1".to_string())));
        let _context_tx = context_tx;
        let tool = RemoteTool {
            definition: wire::AppToolDefinition {
                name: "echo".to_string(),
                description: "Echo input".to_string(),
                parameters: json!({"type": "object"}),
                execution_mode: wire::AppToolExecutionMode::Parallel,
                timeout_ms: 1_000,
            },
            validator: object_validator(),
            context: CapturedTurnContext {
                receiver: context_rx,
            },
            callbacks: connection.callbacks.clone(),
        };
        let (_control, receivers) = psychevo::__agent_core::ControlHandle::new();
        let execution = tokio::spawn(tool.execute(
            "call-1".to_string(),
            json!({"text": "hello"}),
            receivers.abort_signal(),
        ));
        let callback = rx.recv().await.expect("callback request");
        assert_eq!(callback["method"], "tool/call");
        assert_eq!(callback["params"]["threadId"], "thread-1");
        assert_eq!(callback["params"]["turnId"], "turn-1");
        let callback_id = callback["id"].clone();
        connection
            .handle_value(json!({
                "jsonrpc": "2.0",
                "id": callback_id,
                "result": {
                    "result": {"text": "hello"},
                    "isError": false,
                },
            }))
            .await;
        let output = execution.await.expect("execution");
        assert_eq!(output.json, json!({"text": "hello"}));
        assert!(!output.is_error);
    }

    #[test]
    fn custom_tool_registration_rejects_an_invalid_json_schema() {
        let error = validate_registrations(&wire::AppToolRegisterParams {
            tools: vec![wire::AppToolDefinition {
                name: "broken".to_string(),
                description: "Broken schema".to_string(),
                parameters: json!({"type": 7}),
                execution_mode: wire::AppToolExecutionMode::Parallel,
                timeout_ms: 1_000,
            }],
            approval_handler: false,
            clarify_handler: false,
        })
        .expect_err("invalid JSON Schema");
        assert!(error.message.contains("not a valid JSON Schema"));
    }

    #[tokio::test]
    async fn custom_tool_invalid_arguments_fail_before_client_callback() {
        let (_temp, connection, mut rx) = test_connection().await;
        let (_context_tx, context_rx) =
            watch::channel(Some(("thread-1".to_string(), "turn-1".to_string())));
        let schema = json!({
            "type": "object",
            "required": ["text"],
            "properties": {"text": {"type": "string"}},
            "additionalProperties": false
        });
        let tool = RemoteTool {
            definition: wire::AppToolDefinition {
                name: "echo".to_string(),
                description: "Echo input".to_string(),
                parameters: schema.clone(),
                execution_mode: wire::AppToolExecutionMode::Parallel,
                timeout_ms: 1_000,
            },
            validator: Arc::new(jsonschema::validator_for(&schema).expect("valid schema")),
            context: CapturedTurnContext {
                receiver: context_rx,
            },
            callbacks: connection.callbacks.clone(),
        };
        let (_control, receivers) = psychevo::__agent_core::ControlHandle::new();
        let output = tool
            .execute(
                "call-invalid".to_string(),
                json!({"text": 42}),
                receivers.abort_signal(),
            )
            .await;
        assert!(output.is_error);
        assert!(
            output.json["error"]
                .as_str()
                .is_some_and(|error| error.contains("JSON Schema validation"))
        );
        assert!(
            matches!(
                rx.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "invalid arguments must not reach the client callback"
        );
    }

    #[tokio::test]
    async fn approval_callback_is_correlated_and_decoded() {
        let (_temp, connection, mut rx) = test_connection().await;
        let (_context_tx, context_rx) =
            watch::channel(Some(("thread-1".to_string(), "turn-1".to_string())));
        let handler = RemoteApprovalHandler {
            context: CapturedTurnContext {
                receiver: context_rx,
            },
            callbacks: connection.callbacks.clone(),
        };
        let approval = tokio::spawn(async move {
            handler
                .request_permission(PermissionApprovalRequest {
                    tool_call_id: "tool-call-1".to_string(),
                    tool_name: "exec_command".to_string(),
                    summary: "Run a command".to_string(),
                    reason: "test".to_string(),
                    matched_rule: None,
                    suggested_rule: Some("allow exec_command".to_string()),
                    allow_always: true,
                    filesystem: None,
                    timeout_secs: 1,
                })
                .await
        });
        let callback = rx.recv().await.expect("approval callback");
        assert_eq!(callback["method"], "approval/request");
        assert_eq!(callback["params"]["threadId"], "thread-1");
        assert_eq!(callback["params"]["turnId"], "turn-1");
        connection
            .handle_value(json!({
                "jsonrpc": "2.0",
                "id": callback["id"],
                "result": {
                    "outcome": "allow_turn",
                    "filesystemDirectory": null,
                },
            }))
            .await;
        assert_eq!(
            approval.await.expect("approval result").outcome,
            PermissionApprovalOutcome::AllowTurn
        );

        let (_context_tx, context_rx) =
            watch::channel(Some(("thread-2".to_string(), "turn-2".to_string())));
        let handler = RemoteApprovalHandler {
            context: CapturedTurnContext {
                receiver: context_rx,
            },
            callbacks: connection.callbacks.clone(),
        };
        let disconnected = tokio::spawn(async move {
            handler
                .request_permission(PermissionApprovalRequest {
                    tool_call_id: "tool-call-2".to_string(),
                    tool_name: "exec_command".to_string(),
                    summary: "Run another command".to_string(),
                    reason: "test".to_string(),
                    matched_rule: None,
                    suggested_rule: None,
                    allow_always: false,
                    filesystem: None,
                    timeout_secs: 60,
                })
                .await
        });
        let _ = rx.recv().await.expect("disconnect approval callback");
        connection.callbacks.disconnect().await;
        assert_eq!(
            disconnected.await.expect("denied result").outcome,
            PermissionApprovalOutcome::Deny
        );
    }

    #[tokio::test]
    async fn custom_tool_disconnect_and_timeout_fail_the_call() {
        let (_temp, connection, mut rx) = test_connection().await;
        let (_context_tx, context_rx) =
            watch::channel(Some(("thread-1".to_string(), "turn-1".to_string())));
        let mut definition = wire::AppToolDefinition {
            name: "echo".to_string(),
            description: "Echo input".to_string(),
            parameters: json!({"type": "object"}),
            execution_mode: wire::AppToolExecutionMode::Parallel,
            timeout_ms: 5,
        };
        let tool = RemoteTool {
            definition: definition.clone(),
            validator: object_validator(),
            context: CapturedTurnContext {
                receiver: context_rx.clone(),
            },
            callbacks: connection.callbacks.clone(),
        };
        let (_control, receivers) = psychevo::__agent_core::ControlHandle::new();
        let output = tool
            .execute(
                "call-timeout".to_string(),
                json!({}),
                receivers.abort_signal(),
            )
            .await;
        assert!(output.is_error);
        assert!(
            output.json["error"]
                .as_str()
                .is_some_and(|error| error.contains("timed out"))
        );
        let _ = rx.recv().await;

        definition.timeout_ms = 10_000;
        let tool = RemoteTool {
            definition,
            validator: object_validator(),
            context: CapturedTurnContext {
                receiver: context_rx,
            },
            callbacks: connection.callbacks.clone(),
        };
        let (_control, receivers) = psychevo::__agent_core::ControlHandle::new();
        let execution = tokio::spawn(tool.execute(
            "call-disconnect".to_string(),
            json!({}),
            receivers.abort_signal(),
        ));
        let _ = rx.recv().await.expect("disconnect callback");
        connection.callbacks.disconnect().await;
        let output = execution.await.expect("execution");
        assert!(output.is_error);
        assert!(
            output.json["error"]
                .as_str()
                .is_some_and(|error| error.contains("closed"))
        );
    }

    #[tokio::test]
    async fn stdio_transport_runs_the_negotiated_framework_flow() {
        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(ImmediateAdapter))
            .build()
            .await
            .expect("application");
        let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);
        let (client_read, mut client_write) = tokio::io::split(client_stream);
        let (server_read, server_write) = tokio::io::split(server_stream);
        let server = tokio::spawn(run_stdio_streams(application, server_read, server_write));
        let mut responses = BufReader::new(client_read).lines();

        async fn send(writer: &mut (impl AsyncWrite + Unpin), value: Value) {
            writer
                .write_all(value.to_string().as_bytes())
                .await
                .expect("write request");
            writer.write_all(b"\n").await.expect("write newline");
            writer.flush().await.expect("flush request");
        }

        async fn response<R>(lines: &mut tokio::io::Lines<BufReader<R>>, id: i64) -> Value
        where
            R: AsyncRead + Unpin,
        {
            loop {
                let line = lines
                    .next_line()
                    .await
                    .expect("read response")
                    .expect("response line");
                let value: Value = serde_json::from_str(&line).expect("response json");
                if value["id"] == id {
                    return value;
                }
            }
        }

        send(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "client": {"name": "stdio-test", "version": "0"},
                    "protocolMin": 1,
                    "protocolMax": 1,
                    "capabilities": {},
                },
            }),
        )
        .await;
        assert_eq!(
            response(&mut responses, 1).await["result"]["protocolVersion"],
            1
        );
        send(
            &mut client_write,
            json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
        )
        .await;
        send(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tool/register",
                "params": {
                    "tools": [],
                    "approvalHandler": false,
                    "clarifyHandler": false,
                },
            }),
        )
        .await;
        assert_eq!(
            response(&mut responses, 2).await["result"]["registered"],
            true
        );
        send(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "thread/start",
                "params": {"cwd": temp.path(), "source": "stdio-test"},
            }),
        )
        .await;
        let thread = response(&mut responses, 3).await;
        let thread_id = thread["result"]["id"].as_str().expect("thread id");
        send(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "turn/start",
                "params": {
                    "threadId": thread_id,
                    "turnId": "turn-stdio-1",
                    "prompt": "hello"
                },
            }),
        )
        .await;
        let receipt = response(&mut responses, 4).await;
        let turn_id = receipt["result"]["turnId"].as_str().expect("turn id");
        send(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "turn/wait",
                "params": {"turnId": turn_id},
            }),
        )
        .await;
        assert_eq!(
            response(&mut responses, 5).await["result"]["finalAnswer"],
            "app server answer"
        );

        client_write.shutdown().await.expect("close stdio input");
        drop(client_write);
        server
            .await
            .expect("stdio server task")
            .expect("stdio server");
    }

    #[tokio::test]
    async fn stdio_transport_bounds_a_real_flood_of_blocked_requests() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(tokio::sync::Notify::new());
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(BlockingAdapter {
                started: Arc::clone(&started),
            }))
            .build()
            .await
            .expect("application");
        let (client_stream, server_stream) = tokio::io::duplex(1024 * 1024);
        let (client_read, mut client_write) = tokio::io::split(client_stream);
        let (server_read, server_write) = tokio::io::split(server_stream);
        let server = tokio::spawn(run_stdio_streams(application, server_read, server_write));
        let mut responses = BufReader::new(client_read).lines();

        async fn send(writer: &mut (impl AsyncWrite + Unpin), value: Value) {
            writer
                .write_all(value.to_string().as_bytes())
                .await
                .expect("write request");
            writer.write_all(b"\n").await.expect("write newline");
            writer.flush().await.expect("flush request");
        }

        async fn response<R>(lines: &mut tokio::io::Lines<BufReader<R>>, id: i64) -> Value
        where
            R: AsyncRead + Unpin,
        {
            loop {
                let line = lines
                    .next_line()
                    .await
                    .expect("read response")
                    .expect("response line");
                let value: Value = serde_json::from_str(&line).expect("response json");
                if value["id"] == id {
                    return value;
                }
            }
        }

        send(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "client": {"name": "flood-test", "version": "0"},
                    "protocolMin": 1,
                    "protocolMax": 1,
                    "capabilities": {},
                },
            }),
        )
        .await;
        assert_eq!(
            response(&mut responses, 1).await["result"]["protocolVersion"],
            1
        );
        send(
            &mut client_write,
            json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
        )
        .await;
        send(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "thread/start",
                "params": {"cwd": temp.path(), "source": "flood-test"},
            }),
        )
        .await;
        let thread_id = response(&mut responses, 2).await["result"]["id"]
            .as_str()
            .expect("thread id")
            .to_string();
        send(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "turn/start",
                "params": {
                    "threadId": thread_id,
                    "turnId": "turn-stdio-block",
                    "prompt": "block"
                },
            }),
        )
        .await;
        let turn_id = response(&mut responses, 3).await["result"]["turnId"]
            .as_str()
            .expect("turn id")
            .to_string();
        started.notified().await;

        for offset in 0..=CONNECTION_ORDINARY_REQUEST_LIMIT {
            send(
                &mut client_write,
                json!({
                    "jsonrpc": "2.0",
                    "id": 100 + offset,
                    "method": "turn/wait",
                    "params": {"turnId": turn_id},
                }),
            )
            .await;
        }
        let overloaded = tokio::time::timeout(
            Duration::from_secs(2),
            response(
                &mut responses,
                100 + i64::try_from(CONNECTION_ORDINARY_REQUEST_LIMIT).expect("limit"),
            ),
        )
        .await
        .expect("flood receives bounded overload response");
        assert_eq!(overloaded["error"]["code"], -32001);
        assert_eq!(
            overloaded["error"]["data"]["limit"],
            CONNECTION_REQUEST_LIMIT
        );
        assert_eq!(
            overloaded["error"]["data"]["ordinaryLimit"],
            CONNECTION_ORDINARY_REQUEST_LIMIT
        );
        send(
            &mut client_write,
            json!({
                "jsonrpc": "2.0",
                "id": 200,
                "method": "turn/interrupt",
                "params": {"turnId": turn_id},
            }),
        )
        .await;
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), response(&mut responses, 200))
                .await
                .expect("reserved control request is admitted")["result"]["interrupted"],
            true
        );
        for id in
            100..100 + i64::try_from(CONNECTION_ORDINARY_REQUEST_LIMIT).expect("ordinary limit")
        {
            assert_eq!(
                response(&mut responses, id).await["result"]["outcome"],
                "interrupted"
            );
        }
        client_write.shutdown().await.expect("close stdio input");
        drop(client_write);
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .expect("stdio flood server shuts down")
            .expect("stdio server task")
            .expect("stdio server");
    }

    #[tokio::test]
    async fn stdio_transport_preserves_handshake_order_across_many_clients() {
        let mut clients = JoinSet::new();
        for index in 0..96 {
            clients.spawn(async move {
                let temp = tempfile::tempdir().expect("tempdir");
                let application = Application::builder()
                    .home(temp.path())
                    .database_path(":memory:")
                    .agent_session_adapter(Arc::new(ImmediateAdapter))
                    .build()
                    .await
                    .expect("application");
                let (client_stream, server_stream) = tokio::io::duplex(16 * 1024);
                let (client_read, mut client_write) = tokio::io::split(client_stream);
                let (server_read, server_write) = tokio::io::split(server_stream);
                let server =
                    tokio::spawn(run_stdio_streams(application, server_read, server_write));
                let input = [
                    json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "method": "initialize",
                        "params": {
                            "client": {
                                "name": format!("ordered-client-{index}"),
                                "version": "0",
                            },
                            "protocolMin": 1,
                            "protocolMax": 1,
                            "capabilities": {},
                        },
                    }),
                    json!({
                        "jsonrpc": "2.0",
                        "method": "initialized",
                        "params": {},
                    }),
                    json!({
                        "jsonrpc": "2.0",
                        "id": 2,
                        "method": "tool/register",
                        "params": {
                            "tools": [],
                            "approvalHandler": false,
                            "clarifyHandler": false,
                        },
                    }),
                ]
                .into_iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join("\n");
                client_write
                    .write_all(format!("{input}\n").as_bytes())
                    .await
                    .expect("write handshake");
                client_write.flush().await.expect("flush handshake");

                let mut lines = BufReader::new(client_read).lines();
                let mut initialized = false;
                let mut registered = false;
                while !(initialized && registered) {
                    let line = lines
                        .next_line()
                        .await
                        .expect("read response")
                        .expect("response");
                    let response: Value = serde_json::from_str(&line).expect("response json");
                    match response["id"].as_i64() {
                        Some(1) => {
                            assert_eq!(response["result"]["protocolVersion"], 1);
                            initialized = true;
                        }
                        Some(2) => {
                            assert_eq!(response["result"]["registered"], true);
                            registered = true;
                        }
                        _ => {}
                    }
                }
                client_write.shutdown().await.expect("close input");
                server.await.expect("server task").expect("server");
            });
        }
        tokio::time::timeout(std::time::Duration::from_secs(20), async {
            while let Some(result) = clients.join_next().await {
                result.expect("ordered client");
            }
        })
        .await
        .expect("ordered clients complete");
    }

    #[tokio::test]
    async fn websocket_uses_the_same_negotiating_dispatcher_and_requires_bearer_auth() {
        use tungstenite::client::IntoClientRequest;

        let temp = tempfile::tempdir().expect("tempdir");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(ImmediateAdapter))
            .build()
            .await
            .expect("application");
        let server = bind_websocket(
            application,
            "127.0.0.1:0".parse().expect("address"),
            "test-token",
        )
        .await
        .expect("websocket server");
        let uri = server.uri();
        let cwd = temp.path().to_string_lossy().to_string();
        let result = tokio::task::spawn_blocking(move || {
            fn send<S>(socket: &mut tungstenite::WebSocket<S>, value: Value)
            where
                S: std::io::Read + std::io::Write,
            {
                socket
                    .send(tungstenite::Message::Text(value.to_string().into()))
                    .expect("send websocket request");
            }

            fn response<S>(socket: &mut tungstenite::WebSocket<S>, id: i64) -> Value
            where
                S: std::io::Read + std::io::Write,
            {
                loop {
                    let message = socket.read().expect("websocket response");
                    let value: Value =
                        serde_json::from_str(message.to_text().expect("text")).expect("json");
                    if value["id"] == id {
                        return value;
                    }
                }
            }

            let unauthorized =
                tungstenite::connect(uri.as_str()).expect_err("missing bearer token must fail");
            assert!(matches!(
                unauthorized,
                tungstenite::Error::Http(response)
                    if response.status() == tungstenite::http::StatusCode::UNAUTHORIZED
            ));

            let mut request = uri.into_client_request().expect("request");
            request.headers_mut().insert(
                tungstenite::http::header::AUTHORIZATION,
                tungstenite::http::HeaderValue::from_static("Bearer test-token"),
            );
            let (mut socket, _) = tungstenite::connect(request).expect("connect");
            send(
                &mut socket,
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "client": {"name": "test", "version": "0"},
                        "protocolMin": 1,
                        "protocolMax": 1,
                        "capabilities": {},
                    }
                }),
            );
            let value = response(&mut socket, 1);
            assert_eq!(value["result"]["protocolVersion"], 1);
            assert_eq!(value["result"]["capabilities"]["customTools"], true);
            send(
                &mut socket,
                json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
            );
            send(
                &mut socket,
                json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tool/register",
                    "params": {
                        "tools": [],
                        "approvalHandler": false,
                        "clarifyHandler": false,
                    },
                }),
            );
            assert_eq!(response(&mut socket, 2)["result"]["registered"], true);
            send(
                &mut socket,
                json!({
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "thread/start",
                    "params": {"cwd": cwd, "source": "websocket-test"},
                }),
            );
            let thread = response(&mut socket, 3);
            let thread_id = thread["result"]["id"]
                .as_str()
                .expect("thread id")
                .to_string();
            send(
                &mut socket,
                json!({
                    "jsonrpc": "2.0",
                    "id": 4,
                    "method": "turn/start",
                    "params": {
                        "threadId": thread_id,
                        "turnId": "turn-websocket-1",
                        "prompt": "hello"
                    },
                }),
            );
            let receipt = response(&mut socket, 4);
            let turn_id = receipt["result"]["turnId"]
                .as_str()
                .expect("turn id")
                .to_string();
            send(
                &mut socket,
                json!({
                    "jsonrpc": "2.0",
                    "id": 5,
                    "method": "turn/wait",
                    "params": {"turnId": turn_id},
                }),
            );
            assert_eq!(
                response(&mut socket, 5)["result"]["finalAnswer"],
                "app server answer"
            );
            socket.close(None).expect("close");
        })
        .await;
        result.expect("websocket client");
        server.shutdown().await.expect("server shutdown");
    }
}
