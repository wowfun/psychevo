use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use futures::StreamExt;
use reqwest::Url;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, broadcast, mpsc, oneshot};

use psychevo_runtime::{
    ExecutableResolveOptions, HostPlatform, ProcessEnvOptions, apply_tokio_process_env,
    resolve_executable_path, terminate_tokio_child_tree,
};

use crate::{RetryClass, RuntimeError, RuntimeErrorStage, RuntimeProfile};

use super::http::OpenCodeHttp;
use super::sse::{EventDeduper, SseDecoder, decode_native_event};
use super::types::{AgentInfo, NativeEvent, SessionInfo};

pub(crate) const ADAPTER_VERSION: &str = "opencode-direct-v1";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECTED_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_STARTUP_OUTPUT: usize = 64 * 1024;
const EVENT_CHANNEL_CAPACITY: usize = 2048;

#[derive(Debug, Clone)]
pub(crate) enum GenerationSignal {
    Event(NativeEvent),
    StreamClosed(String),
    ProcessExited(Option<i32>),
}

#[derive(Debug, Clone, Default)]
struct InstanceCache {
    epoch: u64,
    agents: Vec<AgentInfo>,
    sessions: HashMap<String, SessionInfo>,
    timeline_http_hydrated: bool,
    todo_sse_reconciled: bool,
    diff_sse_reconciled: bool,
}

#[derive(Debug)]
pub(crate) struct Generation {
    pub(crate) runtime_ref: String,
    pub(crate) process_epoch: u64,
    pub(crate) runtime_version: String,
    pub(crate) http: OpenCodeHttp,
    signal: broadcast::Sender<GenerationSignal>,
    process: mpsc::Sender<ProcessCommand>,
    dead: Arc<AtomicBool>,
    stream_alive: Arc<AtomicBool>,
    instances: Mutex<HashMap<String, InstanceCache>>,
}

impl Generation {
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<GenerationSignal> {
        self.signal.subscribe()
    }

    pub(crate) fn is_usable(&self) -> bool {
        !self.dead.load(Ordering::SeqCst) && self.stream_alive.load(Ordering::SeqCst)
    }

    pub(crate) async fn instance_epoch(&self, cwd: &Path) -> u64 {
        let key = directory_key(cwd);
        let mut instances = self.instances.lock().await;
        let cache = instances.entry(key).or_insert_with(|| InstanceCache {
            epoch: 1,
            agents: Vec::new(),
            sessions: HashMap::new(),
            timeline_http_hydrated: false,
            todo_sse_reconciled: false,
            diff_sse_reconciled: false,
        });
        cache.epoch
    }

    pub(crate) async fn bump_instance_epoch(&self, cwd: &Path) -> u64 {
        let key = directory_key(cwd);
        let mut instances = self.instances.lock().await;
        let cache = instances.entry(key).or_default();
        cache.epoch = cache.epoch.saturating_add(1).max(1);
        cache.agents.clear();
        cache.sessions.clear();
        cache.timeline_http_hydrated = false;
        cache.todo_sse_reconciled = false;
        cache.diff_sse_reconciled = false;
        cache.epoch
    }

    pub(crate) async fn set_agents(&self, cwd: &Path, agents: Vec<AgentInfo>) {
        let key = directory_key(cwd);
        let mut instances = self.instances.lock().await;
        let cache = instances.entry(key).or_insert_with(|| InstanceCache {
            epoch: 1,
            agents: Vec::new(),
            sessions: HashMap::new(),
            timeline_http_hydrated: false,
            todo_sse_reconciled: false,
            diff_sse_reconciled: false,
        });
        cache.agents = agents;
    }

    pub(crate) async fn cached_agents(&self, cwd: &Path) -> Vec<AgentInfo> {
        self.instances
            .lock()
            .await
            .get(&directory_key(cwd))
            .map(|cache| cache.agents.clone())
            .unwrap_or_default()
    }

    pub(crate) async fn observe_session(&self, cwd: &Path, session: &SessionInfo) {
        let key = directory_key(cwd);
        let mut instances = self.instances.lock().await;
        let cache = instances.entry(key).or_insert_with(|| InstanceCache {
            epoch: 1,
            agents: Vec::new(),
            sessions: HashMap::new(),
            timeline_http_hydrated: false,
            todo_sse_reconciled: false,
            diff_sse_reconciled: false,
        });
        cache.sessions.insert(session.id.clone(), session.clone());
    }

    pub(crate) async fn observe_sessions(&self, cwd: &Path, sessions: &[SessionInfo]) {
        let key = directory_key(cwd);
        let mut instances = self.instances.lock().await;
        let cache = instances.entry(key).or_insert_with(|| InstanceCache {
            epoch: 1,
            agents: Vec::new(),
            sessions: HashMap::new(),
            timeline_http_hydrated: false,
            todo_sse_reconciled: false,
            diff_sse_reconciled: false,
        });
        cache.sessions.extend(
            sessions
                .iter()
                .cloned()
                .map(|session| (session.id.clone(), session)),
        );
    }

    pub(crate) async fn cached_session(&self, cwd: &Path, session_id: &str) -> Option<SessionInfo> {
        self.instances
            .lock()
            .await
            .get(&directory_key(cwd))
            .and_then(|cache| cache.sessions.get(session_id))
            .cloned()
    }

    pub(crate) async fn mark_timeline_http_hydrated(&self, cwd: &Path) {
        let key = directory_key(cwd);
        let mut instances = self.instances.lock().await;
        let cache = instances.entry(key).or_insert_with(|| InstanceCache {
            epoch: 1,
            agents: Vec::new(),
            sessions: HashMap::new(),
            timeline_http_hydrated: false,
            todo_sse_reconciled: false,
            diff_sse_reconciled: false,
        });
        cache.timeline_http_hydrated = true;
    }

    pub(crate) async fn mark_todo_sse_reconciled(&self, cwd: &Path) {
        let key = directory_key(cwd);
        let mut instances = self.instances.lock().await;
        let cache = instances.entry(key).or_insert_with(|| InstanceCache {
            epoch: 1,
            agents: Vec::new(),
            sessions: HashMap::new(),
            timeline_http_hydrated: false,
            todo_sse_reconciled: false,
            diff_sse_reconciled: false,
        });
        cache.todo_sse_reconciled = true;
    }

    pub(crate) async fn mark_diff_sse_reconciled(&self, cwd: &Path) {
        let key = directory_key(cwd);
        let mut instances = self.instances.lock().await;
        let cache = instances.entry(key).or_insert_with(|| InstanceCache {
            epoch: 1,
            agents: Vec::new(),
            sessions: HashMap::new(),
            timeline_http_hydrated: false,
            todo_sse_reconciled: false,
            diff_sse_reconciled: false,
        });
        cache.diff_sse_reconciled = true;
    }

    pub(crate) async fn timeline_validation(&self, cwd: &Path) -> (bool, bool) {
        self.instances
            .lock()
            .await
            .get(&directory_key(cwd))
            .map(|cache| {
                (
                    cache.timeline_http_hydrated && cache.todo_sse_reconciled,
                    cache.timeline_http_hydrated && cache.diff_sse_reconciled,
                )
            })
            .unwrap_or_default()
    }

    pub(crate) async fn forget_session(&self, cwd: &Path, session_id: &str) {
        if let Some(cache) = self.instances.lock().await.get_mut(&directory_key(cwd)) {
            cache.sessions.remove(session_id);
        }
    }

    pub(crate) async fn shutdown(&self, force: bool) -> bool {
        if force {
            self.dead.store(true, Ordering::SeqCst);
        } else if self.dead.swap(true, Ordering::SeqCst) {
            return false;
        }
        let (done_tx, done_rx) = oneshot::channel();
        if self
            .process
            .send(ProcessCommand::Shutdown {
                force,
                done: done_tx,
            })
            .await
            .is_ok()
        {
            let _ = tokio::time::timeout(Duration::from_secs(5), done_rx).await;
        }
        true
    }
}

#[derive(Debug)]
pub(super) enum ProcessCommand {
    Shutdown {
        force: bool,
        done: oneshot::Sender<()>,
    },
}

#[cfg(test)]
pub(super) fn shutdown_test_generation(
    runtime_ref: &str,
    process_epoch: u64,
) -> (Arc<Generation>, mpsc::Receiver<ProcessCommand>) {
    let (signal, _) = broadcast::channel(1);
    let (process, commands) = mpsc::channel(1);
    let http = OpenCodeHttp::new(
        reqwest::Client::new(),
        reqwest::Url::parse("http://127.0.0.1:1/").expect("test URL"),
    );
    (
        Arc::new(Generation {
            runtime_ref: runtime_ref.to_string(),
            process_epoch,
            runtime_version: "test".to_string(),
            http,
            signal,
            process,
            dead: Arc::new(AtomicBool::new(false)),
            stream_alive: Arc::new(AtomicBool::new(true)),
            instances: Mutex::new(HashMap::new()),
        }),
        commands,
    )
}

pub(crate) struct PreparedLaunch {
    pub(crate) key: String,
    pub(crate) program: PathBuf,
    pub(crate) args: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) neutral_cwd: PathBuf,
    username: String,
    password: String,
}

impl std::fmt::Debug for PreparedLaunch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedLaunch")
            .field("key", &self.key)
            .field("program", &self.program)
            .field("args", &self.args)
            .field("env_keys", &self.env.keys().collect::<Vec<_>>())
            .field("neutral_cwd", &self.neutral_cwd)
            .finish_non_exhaustive()
    }
}

pub(crate) fn prepare_launch(
    profile: &RuntimeProfile,
    resolution_cwd: &Path,
) -> Result<PreparedLaunch, RuntimeError> {
    validate_profile_args(profile)?;
    let command = profile
        .command
        .as_deref()
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .ok_or_else(|| {
            RuntimeError::new(
                "missing_runtime",
                RuntimeErrorStage::Discovery,
                RetryClass::UserAction,
                "OpenCode runtime command is not configured",
            )
        })?;
    let mut env = std::env::vars().collect::<BTreeMap<_, _>>();
    env.extend(profile.env.clone());
    let program = resolve_executable_path(
        command,
        resolution_cwd,
        &ExecutableResolveOptions {
            platform: HostPlatform::current(),
            env: &env,
        },
    )
    .ok_or_else(|| {
        RuntimeError::new(
            "missing_runtime",
            RuntimeErrorStage::Discovery,
            RetryClass::UserAction,
            "OpenCode runtime command was not found on PATH/PATHEXT",
        )
    })?;

    let mut args = profile.args.clone();
    args.extend([
        "--hostname".to_string(),
        "127.0.0.1".to_string(),
        "--port".to_string(),
        "0".to_string(),
        "--no-mdns".to_string(),
    ]);
    let key = generation_key(profile, &program, &args)?;
    let neutral_cwd = std::env::temp_dir()
        .join("psychevo-opencode")
        .join(&key[..16]);
    std::fs::create_dir_all(&neutral_cwd).map_err(|error| {
        RuntimeError::new(
            "launch_directory_failed",
            RuntimeErrorStage::Launch,
            RetryClass::UserAction,
            format!("failed to create the neutral OpenCode launch directory: {error}"),
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&neutral_cwd, std::fs::Permissions::from_mode(0o700));
    }

    let username = "opencode".to_string();
    let password = process_password()?;
    env.insert("OPENCODE_SERVER_USERNAME".to_string(), username.clone());
    env.insert("OPENCODE_SERVER_PASSWORD".to_string(), password.clone());
    Ok(PreparedLaunch {
        key,
        program,
        args,
        env,
        neutral_cwd,
        username,
        password,
    })
}

pub(crate) fn generation_lookup_key(
    profile: &RuntimeProfile,
    resolution_cwd: &Path,
) -> Result<String, RuntimeError> {
    validate_profile_args(profile)?;
    let command = profile
        .command
        .as_deref()
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .ok_or_else(|| configuration_error("OpenCode runtime command is not configured"))?;
    let mut env = std::env::vars().collect::<BTreeMap<_, _>>();
    env.extend(profile.env.clone());
    let program = resolve_executable_path(
        command,
        resolution_cwd,
        &ExecutableResolveOptions {
            platform: HostPlatform::current(),
            env: &env,
        },
    )
    .ok_or_else(|| {
        RuntimeError::new(
            "missing_runtime",
            RuntimeErrorStage::Discovery,
            RetryClass::UserAction,
            "OpenCode runtime command was not found on PATH/PATHEXT",
        )
    })?;
    let mut args = profile.args.clone();
    args.extend([
        "--hostname".to_string(),
        "127.0.0.1".to_string(),
        "--port".to_string(),
        "0".to_string(),
        "--no-mdns".to_string(),
    ]);
    generation_key(profile, &program, &args)
}

pub(crate) async fn spawn_generation(
    launch: PreparedLaunch,
    runtime_ref: String,
    process_epoch: u64,
) -> Result<Arc<Generation>, RuntimeError> {
    let mut command = Command::new(&launch.program);
    command
        .args(&launch.args)
        .current_dir(&launch.neutral_cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    apply_tokio_process_env(&mut command, &launch.env, ProcessEnvOptions::new(&[])).map_err(
        |error| {
            RuntimeError::new(
                "invalid_process_environment",
                RuntimeErrorStage::Launch,
                RetryClass::UserAction,
                format!("failed to prepare the OpenCode environment: {error}"),
            )
        },
    )?;
    let mut child = command.spawn().map_err(|error| {
        RuntimeError::new(
            "launch_failed",
            RuntimeErrorStage::Launch,
            RetryClass::UserAction,
            format!("failed to launch OpenCode: {error}"),
        )
    })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        RuntimeError::new(
            "launch_failed",
            RuntimeErrorStage::Launch,
            RetryClass::Never,
            "OpenCode stdout was unavailable",
        )
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        RuntimeError::new(
            "launch_failed",
            RuntimeErrorStage::Launch,
            RetryClass::Never,
            "OpenCode stderr was unavailable",
        )
    })?;
    let mut stdout = BufReader::new(stdout);
    let base_url = match tokio::time::timeout(STARTUP_TIMEOUT, read_startup_url(&mut stdout)).await
    {
        Ok(result) => result,
        Err(_) => Err(RuntimeError::new(
            "launch_timeout",
            RuntimeErrorStage::Launch,
            RetryClass::SafeRetry,
            "OpenCode did not report a loopback server URL before the startup timeout",
        )),
    };
    let base_url = match base_url {
        Ok(url) => url,
        Err(error) => {
            terminate_tokio_child_tree(&mut child).await;
            return Err(error);
        }
    };

    let client = match authenticated_client(&launch.username, &launch.password) {
        Ok(client) => client,
        Err(error) => {
            terminate_tokio_child_tree(&mut child).await;
            return Err(error);
        }
    };
    let http = OpenCodeHttp::new(client, base_url);
    let health = match http.health().await {
        Ok(health) if health.healthy && !health.version.trim().is_empty() => health,
        Ok(_) => {
            terminate_tokio_child_tree(&mut child).await;
            return Err(RuntimeError::new(
                "invalid_handshake",
                RuntimeErrorStage::Handshake,
                RetryClass::UserAction,
                "OpenCode health response did not contain a usable runtime version",
            ));
        }
        Err(error) => {
            terminate_tokio_child_tree(&mut child).await;
            return Err(error);
        }
    };

    tokio::spawn(drain_output(stdout));
    tokio::spawn(drain_output(BufReader::new(stderr)));

    let (signal, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    let dead = Arc::new(AtomicBool::new(false));
    let stream_alive = Arc::new(AtomicBool::new(true));
    let (process_tx, process_rx) = mpsc::channel(1);
    tokio::spawn(run_process(child, process_rx, signal.clone(), dead.clone()));

    let connected = start_event_stream(
        http.clone(),
        process_epoch,
        signal.clone(),
        stream_alive.clone(),
    );
    match tokio::time::timeout(CONNECTED_TIMEOUT, connected).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            let (done, _) = oneshot::channel();
            let _ = process_tx
                .send(ProcessCommand::Shutdown { force: true, done })
                .await;
            return Err(error);
        }
        Err(_) => {
            let (done, _) = oneshot::channel();
            let _ = process_tx
                .send(ProcessCommand::Shutdown { force: true, done })
                .await;
            return Err(RuntimeError::new(
                "event_handshake_timeout",
                RuntimeErrorStage::Handshake,
                RetryClass::Reconnect,
                "OpenCode global events did not emit server.connected before timeout",
            ));
        }
    }

    Ok(Arc::new(Generation {
        runtime_ref,
        process_epoch,
        runtime_version: health.version,
        http,
        signal,
        process: process_tx,
        dead,
        stream_alive,
        instances: Mutex::new(HashMap::new()),
    }))
}

async fn read_startup_url<R>(reader: &mut R) -> Result<Url, RuntimeError>
where
    R: AsyncBufRead + Unpin,
{
    let mut consumed = 0usize;
    loop {
        let mut line = String::new();
        let count = reader.read_line(&mut line).await.map_err(|error| {
            RuntimeError::new(
                "launch_output_failed",
                RuntimeErrorStage::Launch,
                RetryClass::SafeRetry,
                format!("failed to read OpenCode startup output: {error}"),
            )
        })?;
        if count == 0 {
            return Err(RuntimeError::new(
                "process_exit",
                RuntimeErrorStage::Launch,
                RetryClass::SafeRetry,
                "OpenCode exited before reporting a loopback server URL",
            ));
        }
        consumed = consumed.saturating_add(count);
        if consumed > MAX_STARTUP_OUTPUT {
            return Err(RuntimeError::new(
                "launch_output_limit",
                RuntimeErrorStage::Launch,
                RetryClass::Never,
                "OpenCode startup output exceeded the bounded diagnostic limit",
            ));
        }
        let line = line.trim_end_matches(['\r', '\n']);
        let Some(port) = line.strip_prefix("opencode server listening on http://127.0.0.1:") else {
            if line.contains("opencode server listening on http://") {
                return Err(RuntimeError::new(
                    "unsafe_server_address",
                    RuntimeErrorStage::Handshake,
                    RetryClass::Never,
                    "OpenCode reported a non-loopback server address",
                ));
            }
            continue;
        };
        let port = port
            .parse::<u16>()
            .ok()
            .filter(|port| *port > 0)
            .ok_or_else(|| {
                RuntimeError::new(
                    "invalid_server_address",
                    RuntimeErrorStage::Handshake,
                    RetryClass::Never,
                    "OpenCode reported an invalid loopback server port",
                )
            })?;
        return Url::parse(&format!("http://127.0.0.1:{port}/")).map_err(|error| {
            RuntimeError::new(
                "invalid_server_address",
                RuntimeErrorStage::Handshake,
                RetryClass::Never,
                format!("OpenCode reported an invalid server URL: {error}"),
            )
        });
    }
}

async fn drain_output<R>(mut reader: R)
where
    R: AsyncBufRead + Unpin,
{
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                if line.len() > MAX_STARTUP_OUTPUT {
                    line.clear();
                }
            }
        }
    }
}

async fn run_process(
    mut child: Child,
    mut commands: mpsc::Receiver<ProcessCommand>,
    signal: broadcast::Sender<GenerationSignal>,
    dead: Arc<AtomicBool>,
) {
    let code = tokio::select! {
        result = child.wait() => result.ok().and_then(|status| status.code()),
        command = commands.recv() => {
            match command {
                Some(ProcessCommand::Shutdown { force, done }) => {
                    let _ = force;
                    terminate_tokio_child_tree(&mut child).await;
                    let code = tokio::time::timeout(Duration::from_secs(3), child.wait())
                        .await
                        .ok()
                        .and_then(Result::ok)
                        .and_then(|status| status.code());
                    let _ = done.send(());
                    code
                }
                None => child.wait().await.ok().and_then(|status| status.code()),
            }
        }
    };
    dead.store(true, Ordering::SeqCst);
    let _ = signal.send(GenerationSignal::ProcessExited(code));
}

async fn start_event_stream(
    http: OpenCodeHttp,
    process_epoch: u64,
    signal: broadcast::Sender<GenerationSignal>,
    stream_alive: Arc<AtomicBool>,
) -> Result<(), RuntimeError> {
    let response = http.event_stream().await?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !content_type
        .to_ascii_lowercase()
        .starts_with("text/event-stream")
    {
        return Err(RuntimeError::new(
            "invalid_event_stream",
            RuntimeErrorStage::Handshake,
            RetryClass::Reconnect,
            "OpenCode global events did not return text/event-stream",
        ));
    }
    let mut stream = response.bytes_stream();
    let mut decoder = SseDecoder::default();
    let mut deduper = EventDeduper::default();
    let mut buffered = Vec::new();
    let connected = loop {
        let chunk = stream
            .next()
            .await
            .ok_or_else(|| {
                RuntimeError::new(
                    "event_stream_closed",
                    RuntimeErrorStage::Handshake,
                    RetryClass::Reconnect,
                    "OpenCode global events closed before server.connected",
                )
            })?
            .map_err(|error| {
                RuntimeError::new(
                    "event_stream_closed",
                    RuntimeErrorStage::Handshake,
                    RetryClass::Reconnect,
                    format!("OpenCode global event handshake failed: {error}"),
                )
            })?;
        let events = decoder.push(&chunk)?;
        let mut found = false;
        for data in events {
            let event = decode_native_event(&data)?;
            if event.event_type == "server.connected" {
                found = true;
                continue;
            }
            if deduper.accept(process_epoch, event.id.as_deref()) {
                buffered.push(event);
            }
        }
        if found {
            break true;
        }
    };
    if !connected {
        return Err(RuntimeError::new(
            "event_handshake_failed",
            RuntimeErrorStage::Handshake,
            RetryClass::Reconnect,
            "OpenCode global events did not connect",
        ));
    }
    for event in buffered {
        let _ = signal.send(GenerationSignal::Event(event));
    }
    tokio::spawn(async move {
        let result: Result<(), RuntimeError> = async {
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|error| {
                    RuntimeError::new(
                        "event_stream_closed",
                        RuntimeErrorStage::Transport,
                        RetryClass::Reconnect,
                        format!("OpenCode global event stream failed: {error}"),
                    )
                })?;
                for data in decoder.push(&chunk)? {
                    let event = decode_native_event(&data)?;
                    if event.event_type == "server.heartbeat"
                        || event.event_type == "server.connected"
                    {
                        continue;
                    }
                    if deduper.accept(process_epoch, event.id.as_deref()) {
                        let _ = signal.send(GenerationSignal::Event(event));
                    }
                }
            }
            Err(RuntimeError::new(
                "event_stream_closed",
                RuntimeErrorStage::Transport,
                RetryClass::Reconnect,
                "OpenCode global event stream closed",
            ))
        }
        .await;
        stream_alive.store(false, Ordering::SeqCst);
        let message = result
            .err()
            .map(|error| error.message)
            .unwrap_or_else(|| "OpenCode global event stream closed".to_string());
        let _ = signal.send(GenerationSignal::StreamClosed(message));
    });
    Ok(())
}

fn authenticated_client(username: &str, password: &str) -> Result<reqwest::Client, RuntimeError> {
    let encoded = STANDARD.encode(format!("{username}:{password}"));
    let mut value = HeaderValue::from_str(&format!("Basic {encoded}")).map_err(|_| {
        RuntimeError::new(
            "authentication_failed",
            RuntimeErrorStage::Authentication,
            RetryClass::Never,
            "failed to construct OpenCode process credentials",
        )
    })?;
    value.set_sensitive(true);
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, value);
    reqwest::Client::builder()
        .default_headers(headers)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|error| {
            RuntimeError::new(
                "transport_initialization_failed",
                RuntimeErrorStage::Launch,
                RetryClass::Never,
                format!("failed to create the OpenCode HTTP client: {error}"),
            )
        })
}

fn validate_profile_args(profile: &RuntimeProfile) -> Result<(), RuntimeError> {
    if profile
        .args
        .iter()
        .filter(|arg| arg.as_str() == "serve")
        .count()
        != 1
    {
        return Err(configuration_error(
            "OpenCode direct runtime args must contain exactly one `serve` subcommand",
        ));
    }
    const RESERVED: &[&str] = &[
        "--hostname",
        "--port",
        "--mdns",
        "--no-mdns",
        "--mdns-domain",
        "--cors",
    ];
    if profile.args.iter().any(|arg| {
        arg == "--"
            || RESERVED
                .iter()
                .any(|reserved| arg == reserved || arg.starts_with(&format!("{reserved}=")))
    }) {
        return Err(configuration_error(
            "OpenCode direct runtime network arguments are adapter-owned",
        ));
    }
    if profile.env.keys().any(|key| {
        key.eq_ignore_ascii_case("OPENCODE_SERVER_PASSWORD")
            || key.eq_ignore_ascii_case("OPENCODE_SERVER_USERNAME")
    }) {
        return Err(configuration_error(
            "OpenCode process-scoped server credentials are adapter-owned",
        ));
    }
    Ok(())
}

fn generation_key(
    profile: &RuntimeProfile,
    program: &Path,
    args: &[String],
) -> Result<String, RuntimeError> {
    let material = serde_json::to_vec(&serde_json::json!({
        "runtimeRef": profile.id,
        "program": program,
        "args": args,
        "env": profile.env,
        "options": profile.options,
        "revision": profile.revision,
        "fingerprint": profile.fingerprint,
        "adapter": ADAPTER_VERSION,
    }))
    .map_err(|error| {
        RuntimeError::new(
            "invalid_profile",
            RuntimeErrorStage::Configuration,
            RetryClass::Never,
            format!("failed to fingerprint the OpenCode profile: {error}"),
        )
    })?;
    let mut hash = Sha256::new();
    hash.update(material);
    Ok(format!("{:x}", hash.finalize()))
}

fn process_password() -> Result<String, RuntimeError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| {
        RuntimeError::new(
            "credential_generation_failed",
            RuntimeErrorStage::Launch,
            RetryClass::Never,
            format!("failed to generate OpenCode process credentials: {error}"),
        )
    })?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn configuration_error(message: impl Into<String>) -> RuntimeError {
    RuntimeError::new(
        "invalid_profile",
        RuntimeErrorStage::Configuration,
        RetryClass::UserAction,
        message,
    )
}

fn directory_key(cwd: &Path) -> String {
    std::fs::canonicalize(cwd)
        .unwrap_or_else(|_| cwd.to_path_buf())
        .to_string_lossy()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RuntimeKind;
    use serde_json::Value;

    fn profile(args: &[&str]) -> RuntimeProfile {
        RuntimeProfile {
            id: "opencode".to_string(),
            label: "OpenCode".to_string(),
            kind: RuntimeKind::OpenCode,
            enabled: true,
            command: Some("opencode".to_string()),
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
            env: BTreeMap::new(),
            backend_ref: None,
            default_model: None,
            default_mode: None,
            default_agent: None,
            approval_mode: None,
            sandbox: None,
            workspace_roots: Vec::new(),
            options: Value::Null,
            revision: 1,
            fingerprint: "fp".to_string(),
        }
    }

    #[test]
    fn direct_runtime_rejects_network_overrides_and_separator() {
        for args in [
            vec!["serve", "--hostname", "0.0.0.0"],
            vec!["serve", "--port=4096"],
            vec!["serve", "--"],
            vec!["serve", "--mdns"],
        ] {
            let error = validate_profile_args(&profile(&args)).expect_err("reserved args");
            assert_eq!(error.code, "invalid_profile");
        }
    }

    #[test]
    fn direct_runtime_requires_exactly_one_serve_subcommand() {
        assert!(validate_profile_args(&profile(&[])).is_err());
        assert!(validate_profile_args(&profile(&["serve", "serve"])).is_err());
        assert!(validate_profile_args(&profile(&["serve"])).is_ok());
    }

    #[tokio::test]
    async fn startup_parser_rejects_non_loopback_address() {
        let mut input = BufReader::new(&b"opencode server listening on http://0.0.0.0:4096\n"[..]);
        let error = read_startup_url(&mut input)
            .await
            .expect_err("unsafe address");
        assert_eq!(error.code, "unsafe_server_address");
    }
}
