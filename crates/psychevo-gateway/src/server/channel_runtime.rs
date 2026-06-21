use super::*;
use crate::im::adapters::{
    FeishuLarkDomain, FeishuLarkLongConnectionAdapter, FeishuLarkLongConnectionConfig,
    TelegramPollingAdapter, TelegramPollingConfig, WECHAT_ILINK_BASE_URL, WechatIlinkAdapter,
    WechatIlinkConfig, is_wechat_ilink_session_expired_error, wechat_ilink_error_code_from_message,
};
use crate::im::{
    ChannelAdapterBinding, ChannelAllowlist, ChannelGateway, ImInboundMessage, ImOutboundMessage,
    gateway_source_for_im,
};
use psychevo_runtime::{ChannelRuntimeConnection, channel_runtime_connections};
use tokio_util::sync::CancellationToken;

const CHANNEL_POLL_BACKOFF_MS: u64 = 5_000;
const CHANNEL_IDLE_SLEEP_MS: u64 = 1_000;
const WECHAT_LOGIN_GRACE_MS: i64 = 60_000;

#[derive(Clone)]
pub(super) struct ChannelRuntimeState {
    inner: Arc<Mutex<ChannelRuntimeInner>>,
    status_path: PathBuf,
}

#[derive(Debug, Default)]
struct ChannelRuntimeInner {
    records: BTreeMap<String, ChannelRunnerRecord>,
    active: BTreeMap<String, CancellationToken>,
    wechat_login_grace_until_ms: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelRunnerRecord {
    state: String,
    reason: Option<String>,
    last_poll_at_ms: Option<i64>,
    last_healthy_poll_at_ms: Option<i64>,
    last_inbound_at_ms: Option<i64>,
    last_outbound_at_ms: Option<i64>,
    last_ilink_errcode: Option<i64>,
    last_error: Option<String>,
}

impl Default for ChannelRuntimeState {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ChannelRuntimeInner::default())),
            status_path: PathBuf::new(),
        }
    }
}

impl ChannelRuntimeState {
    pub(super) fn new(home: &Path) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ChannelRuntimeInner::default())),
            status_path: home.join("gateway").join("channels-status.json"),
        }
    }

    pub(super) fn runner_view(&self, id: &str) -> wire::ChannelRunnerView {
        let record = self
            .inner
            .lock()
            .expect("channel runtime state poisoned")
            .records
            .get(id)
            .cloned()
            .unwrap_or_else(ChannelRunnerRecord::stopped);
        wire::ChannelRunnerView {
            state: record.state,
            reason: record.reason,
            last_poll_at_ms: record.last_poll_at_ms,
            last_healthy_poll_at_ms: record.last_healthy_poll_at_ms,
            last_inbound_at_ms: record.last_inbound_at_ms,
            last_outbound_at_ms: record.last_outbound_at_ms,
            last_ilink_errcode: record.last_ilink_errcode,
            last_error: record.last_error,
        }
    }

    fn activate(&self, id: &str) -> Option<CancellationToken> {
        let mut inner = self.inner.lock().expect("channel runtime state poisoned");
        if inner.active.contains_key(id) {
            return None;
        }
        let token = CancellationToken::new();
        inner.active.insert(id.to_string(), token.clone());
        Some(token)
    }

    fn reconcile_active(&self, desired: &std::collections::BTreeSet<String>) {
        let cancelled = {
            let mut inner = self.inner.lock().expect("channel runtime state poisoned");
            let stale = inner
                .active
                .keys()
                .filter(|id| !desired.contains(*id))
                .cloned()
                .collect::<Vec<_>>();
            stale
                .into_iter()
                .filter_map(|id| {
                    inner.wechat_login_grace_until_ms.remove(&id);
                    inner.active.remove(&id).map(|token| (id, token))
                })
                .collect::<Vec<_>>()
        };
        for (id, token) in cancelled {
            token.cancel();
            self.update(&id, |record| {
                record.state = "stopped".to_string();
                record.reason = None;
                record.last_error = None;
            });
        }
    }

    fn deactivate(&self, id: &str) {
        let token = self
            .inner
            .lock()
            .expect("channel runtime state poisoned")
            .active
            .remove(id);
        if let Some(token) = token {
            token.cancel();
        }
    }

    pub(super) fn restart(&self, id: &str) {
        self.deactivate(id);
    }

    pub(super) fn start_wechat_login_grace(&self, id: &str) {
        let until_ms = gateway_now_ms().saturating_add(WECHAT_LOGIN_GRACE_MS);
        let snapshot = {
            let mut inner = self.inner.lock().expect("channel runtime state poisoned");
            inner
                .wechat_login_grace_until_ms
                .insert(id.to_string(), until_ms);
            let record = inner
                .records
                .entry(id.to_string())
                .or_insert_with(ChannelRunnerRecord::stopped);
            record.state = "running".to_string();
            record.reason = Some("qr_login_pending".to_string());
            record.last_error = None;
            record.last_ilink_errcode = None;
            inner.records.clone()
        };
        self.write_status_snapshot(snapshot);
        eprintln!(
            "channel runner grace started: id={} channel=wechat reason=qr_login_pending grace_ms={}",
            id, WECHAT_LOGIN_GRACE_MS
        );
    }

    fn wechat_login_grace_active(&self, id: &str) -> bool {
        let now_ms = gateway_now_ms();
        self.inner
            .lock()
            .expect("channel runtime state poisoned")
            .wechat_login_grace_until_ms
            .get(id)
            .is_some_and(|until_ms| *until_ms > now_ms)
    }

    fn clear_wechat_login_grace(&self, id: &str) {
        self.inner
            .lock()
            .expect("channel runtime state poisoned")
            .wechat_login_grace_until_ms
            .remove(id);
    }

    fn mark_stopped(&self, id: &str) {
        if self.wechat_login_grace_active(id) {
            self.mark_wechat_qr_login_pending(id, None);
            return;
        }
        self.update(id, |record| {
            record.state = "stopped".to_string();
            record.reason = None;
            record.last_error = None;
        });
    }

    fn mark_blocked(&self, id: &str, message: impl Into<String>) {
        self.mark_blocked_with_reason(id, None, message, None);
    }

    fn mark_blocked_with_reason(
        &self,
        id: &str,
        reason: Option<&str>,
        message: impl Into<String>,
        ilink_errcode: Option<i64>,
    ) {
        self.clear_wechat_login_grace(id);
        self.update(id, |record| {
            record.state = "blocked".to_string();
            record.reason = reason.map(str::to_string);
            record.last_error = Some(redact_channel_error(&message.into()));
            record.last_ilink_errcode = ilink_errcode;
        });
    }

    fn mark_running(&self, id: &str) {
        let keep_pending = self.wechat_login_grace_active(id);
        self.update(id, |record| {
            record.state = "running".to_string();
            if !keep_pending {
                record.reason = None;
                record.last_ilink_errcode = None;
            }
            record.last_error = None;
        });
    }

    fn mark_poll(&self, id: &str, reason: Option<&str>) {
        self.clear_wechat_login_grace(id);
        self.update(id, |record| {
            record.state = "running".to_string();
            record.reason = reason.map(str::to_string);
            record.last_poll_at_ms = Some(gateway_now_ms());
            record.last_healthy_poll_at_ms = record.last_poll_at_ms;
            record.last_error = None;
            record.last_ilink_errcode = None;
        });
    }

    fn mark_wechat_qr_login_pending(&self, id: &str, ilink_errcode: Option<i64>) {
        self.update(id, |record| {
            record.state = "running".to_string();
            record.reason = Some("qr_login_pending".to_string());
            record.last_ilink_errcode = ilink_errcode;
            record.last_error = None;
        });
    }

    fn mark_inbound(&self, id: &str) {
        self.update(id, |record| {
            record.state = "running".to_string();
            record.reason = Some("running".to_string());
            record.last_inbound_at_ms = Some(gateway_now_ms());
        });
    }

    fn mark_outbound(&self, id: &str) {
        self.update(id, |record| {
            record.state = "running".to_string();
            record.reason = Some("running".to_string());
            record.last_outbound_at_ms = Some(gateway_now_ms());
            record.last_error = None;
        });
    }

    fn mark_error(&self, id: &str, error: &dyn std::fmt::Display) {
        let message = error.to_string();
        self.update(id, |record| {
            record.state = "error".to_string();
            record.reason = Some("error".to_string());
            record.last_ilink_errcode = wechat_ilink_error_code_from_message(&message);
            record.last_error = Some(redact_channel_error(&message));
        });
    }

    fn update(&self, id: &str, mutate: impl FnOnce(&mut ChannelRunnerRecord)) {
        let snapshot = {
            let mut inner = self.inner.lock().expect("channel runtime state poisoned");
            let record = inner
                .records
                .entry(id.to_string())
                .or_insert_with(ChannelRunnerRecord::stopped);
            mutate(record);
            inner.records.clone()
        };
        self.write_status_snapshot(snapshot);
    }

    fn write_status_snapshot(&self, records: BTreeMap<String, ChannelRunnerRecord>) {
        if self.status_path.as_os_str().is_empty() {
            return;
        }
        if let Some(parent) = self.status_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let value = json!({ "channels": records });
        let _ = std::fs::write(
            &self.status_path,
            serde_json::to_vec_pretty(&value).unwrap_or_default(),
        );
    }
}

impl ChannelRunnerRecord {
    fn stopped() -> Self {
        Self {
            state: "stopped".to_string(),
            reason: None,
            last_poll_at_ms: None,
            last_healthy_poll_at_ms: None,
            last_inbound_at_ms: None,
            last_outbound_at_ms: None,
            last_ilink_errcode: None,
            last_error: None,
        }
    }
}

pub(super) fn reconcile(state: WebState) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    let _handle = tokio::spawn(async move {
        if let Err(err) = reconcile_inner(state.clone()).await {
            eprintln!(
                "channel runtime reconcile failed: {}",
                redact_channel_error(&err.to_string())
            );
        }
    });
}

async fn reconcile_inner(state: WebState) -> psychevo_runtime::Result<()> {
    let options = state.run_options(state.inner.workdir.clone(), None);
    let connections = channel_runtime_connections(&options, &state.inner.workdir)?;
    if !channel_runtime_enabled(&state.inner.inherited_env) {
        state
            .inner
            .channel_runtime
            .reconcile_active(&std::collections::BTreeSet::new());
        for connection in connections {
            state
                .inner
                .channel_runtime
                .clear_wechat_login_grace(&connection.id);
            state.inner.channel_runtime.mark_stopped(&connection.id);
        }
        return Ok(());
    }
    let mut desired = std::collections::BTreeSet::new();
    for connection in &connections {
        if connection.enabled && connection.config_status == "ready" {
            desired.insert(connection.id.clone());
        }
    }
    state.inner.channel_runtime.reconcile_active(&desired);

    for connection in connections {
        if !connection.enabled {
            state
                .inner
                .channel_runtime
                .clear_wechat_login_grace(&connection.id);
            state.inner.channel_runtime.mark_stopped(&connection.id);
            continue;
        }
        if connection.config_status != "ready" {
            state.inner.channel_runtime.mark_blocked(
                &connection.id,
                format!("config status is {}", connection.config_status),
            );
            continue;
        }
        let Some(cancel) = state.inner.channel_runtime.activate(&connection.id) else {
            continue;
        };
        match build_channel_gateway(&state, &connection).await {
            Ok(channel_gateway) => {
                let runtime = state.inner.channel_runtime.clone();
                let worker_state = state.clone();
                let worker_connection = connection.clone();
                let _handle = tokio::spawn(async move {
                    run_channel_loop(
                        worker_state,
                        runtime,
                        worker_connection,
                        channel_gateway,
                        cancel,
                    )
                    .await;
                });
            }
            Err(err) => {
                state.inner.channel_runtime.deactivate(&connection.id);
                state.inner.channel_runtime.mark_error(&connection.id, &err);
            }
        }
    }
    Ok(())
}

fn channel_runtime_enabled(env: &BTreeMap<String, String>) -> bool {
    !env.get("PSYCHEVO_CHANNEL_RUNTIME")
        .map(|value| matches!(value.as_str(), "0" | "false" | "off"))
        .unwrap_or(false)
}

async fn build_channel_gateway(
    state: &WebState,
    connection: &ChannelRuntimeConnection,
) -> psychevo_runtime::Result<ChannelGateway> {
    let adapter: Arc<dyn crate::im::ImAdapter> = match connection.channel.as_str() {
        "wechat" => Arc::new(WechatIlinkAdapter::new(WechatIlinkConfig {
            connection_id: Some(connection.id.clone()),
            token: connection.credential.clone().unwrap_or_default(),
            account_id: connection.account_id.clone().unwrap_or_default(),
            base_url: connection
                .base_url
                .clone()
                .unwrap_or_else(|| WECHAT_ILINK_BASE_URL.to_string()),
            timeout_secs: 35,
            context_store_path: Some(wechat_context_store_path(&state.inner.home, &connection.id)),
        })?),
        "telegram" => Arc::new(TelegramPollingAdapter::new(TelegramPollingConfig {
            connection_id: Some(connection.id.clone()),
            token: connection.credential.clone().unwrap_or_default(),
            api_base: connection
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.telegram.org".to_string()),
            timeout_secs: 25,
        })?),
        "feishu" | "lark" => {
            let domain = FeishuLarkDomain::parse(connection.channel.as_str()).ok_or_else(|| {
                Error::Message(format!("unsupported channel `{}`", connection.channel))
            })?;
            Arc::new(
                FeishuLarkLongConnectionAdapter::connect(FeishuLarkLongConnectionConfig {
                    connection_id: Some(connection.id.clone()),
                    app_id: connection.app_id.clone().unwrap_or_default(),
                    app_secret: connection
                        .app_secret
                        .clone()
                        .or_else(|| connection.credential.clone())
                        .unwrap_or_default(),
                    domain,
                    base_url: connection.base_url.clone(),
                })
                .await?,
            )
        }
        other => {
            return Err(Error::Message(format!(
                "unsupported channel adapter `{other}`"
            )));
        }
    };
    Ok(ChannelGateway::new(vec![ChannelAdapterBinding::new(
        connection.id.clone(),
        adapter,
        ChannelAllowlist::new(
            connection.allow_users.clone(),
            connection.allow_groups.clone(),
        ),
    )]))
}

async fn run_channel_loop(
    state: WebState,
    runtime: ChannelRuntimeState,
    connection: ChannelRuntimeConnection,
    channel_gateway: ChannelGateway,
    cancel: CancellationToken,
) {
    runtime.mark_running(&connection.id);
    eprintln!(
        "channel runner started: id={} channel={}",
        connection.id, connection.channel
    );
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                runtime.mark_stopped(&connection.id);
                eprintln!(
                    "channel runner stopped: id={} channel={}",
                    connection.id, connection.channel
                );
                break;
            }
            result = channel_gateway.poll_once() => {
                match result {
                    Ok(messages) => {
                        let poll_reason = if messages.is_empty() {
                            Some("polling_empty")
                        } else {
                            Some("running")
                        };
                        runtime.mark_poll(&connection.id, poll_reason);
                        for message in messages {
                            runtime.mark_inbound(&connection.id);
                            if let Err(err) = handle_channel_message(
                                &state,
                                &runtime,
                                &connection,
                                &channel_gateway,
                                message,
                            )
                            .await
                            {
                                runtime.mark_error(&connection.id, &err);
                                eprintln!(
                                    "channel message failed: id={} channel={} error={}",
                                    connection.id,
                                    connection.channel,
                                    redact_channel_error(&err.to_string())
                                );
                            }
                        }
                        tokio::time::sleep(Duration::from_millis(CHANNEL_IDLE_SLEEP_MS)).await;
                    }
                    Err(err) => {
                        let message = err.to_string();
                        if connection.channel == "wechat"
                            && is_wechat_ilink_session_expired_error(&message)
                        {
                            if runtime.wechat_login_grace_active(&connection.id) {
                                runtime.mark_wechat_qr_login_pending(
                                    &connection.id,
                                    wechat_ilink_error_code_from_message(&message),
                                );
                                eprintln!(
                                    "channel runner waiting: id={} channel={} reason=qr_login_pending error={}",
                                    connection.id,
                                    connection.channel,
                                    redact_channel_error(&message)
                                );
                                tokio::time::sleep(Duration::from_millis(CHANNEL_POLL_BACKOFF_MS)).await;
                                continue;
                            }
                            runtime.deactivate(&connection.id);
                            runtime.mark_blocked_with_reason(
                                &connection.id,
                                Some("needs_qr_login"),
                                message.clone(),
                                wechat_ilink_error_code_from_message(&message),
                            );
                            eprintln!(
                                "channel runner blocked: id={} channel={} reason=needs_qr_login error={}",
                                connection.id,
                                connection.channel,
                                redact_channel_error(&message)
                            );
                            break;
                        }
                        runtime.mark_error(&connection.id, &err);
                        eprintln!(
                            "channel poll failed: id={} channel={} error={}",
                            connection.id,
                            connection.channel,
                            redact_channel_error(&err.to_string())
                        );
                        tokio::time::sleep(Duration::from_millis(CHANNEL_POLL_BACKOFF_MS)).await;
                    }
                }
            }
        }
    }
}

async fn handle_channel_message(
    state: &WebState,
    runtime: &ChannelRuntimeState,
    connection: &ChannelRuntimeConnection,
    channel_gateway: &ChannelGateway,
    message: ImInboundMessage,
) -> psychevo_runtime::Result<()> {
    let source = gateway_source_for_im(&message);
    let workdir = channel_workdir(&state.inner.workdir, connection);
    let mut options = state.run_options(workdir, None);
    options.model = connection.model.clone();
    options.permission_mode = connection
        .permission_mode
        .as_deref()
        .and_then(PermissionMode::parse)
        .or(options.permission_mode);
    let result = state
        .inner
        .gateway
        .send_turn(crate::SendTurnRequest {
            thread_id: None,
            source: Some(source.clone()),
            bind_source: Some(source),
            reset_source_binding: false,
            input: vec![GatewayInputPart::Text {
                text: message.text.clone(),
            }],
            options,
            runtime_source: Some(format!("channel/{}", connection.channel)),
            continue_sources: vec![format!("channel/{}", connection.channel)],
            stream: None,
            event_sink: None,
            control_handle: None,
            control: None,
            lineage: Some(json!({
                "channel": connection.channel,
                "connectionId": connection.id,
                "messageId": message.message_id,
            })),
        })
        .await?;
    let answer = result.result.final_answer.trim().to_string();
    if answer.is_empty() {
        return Ok(());
    }
    channel_gateway
        .send(ImOutboundMessage {
            identity: message.identity,
            thread_id: result.thread.id,
            text: answer,
        })
        .await?;
    runtime.mark_outbound(&connection.id);
    Ok(())
}

fn channel_workdir(default_workdir: &Path, connection: &ChannelRuntimeConnection) -> PathBuf {
    let raw = connection.workdir.as_deref().unwrap_or("");
    if raw.trim().is_empty() {
        return default_workdir.to_path_buf();
    }
    let path = PathBuf::from(raw);
    let path = if path.is_absolute() {
        path
    } else {
        default_workdir.join(path)
    };
    psychevo_runtime::canonicalize_workdir(&path).unwrap_or(path)
}

fn wechat_context_store_path(home: &Path, id: &str) -> PathBuf {
    home.join("gateway").join("channels").join(format!(
        "{}-wechat-context.json",
        safe_channel_file_stem(id)
    ))
}

fn safe_channel_file_stem(value: &str) -> String {
    let mut out = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect::<String>();
    if out.is_empty() {
        out = "channel".to_string();
    }
    out
}

pub(super) fn redact_channel_error(value: &str) -> String {
    let mut out = value.replace("Bearer ", "Bearer [redacted] ");
    for key in ["token=", "access_token=", "bot_token="] {
        while let Some(index) = out.find(key) {
            let start = index + key.len();
            let end = out[start..]
                .find(|ch: char| ch == '&' || ch.is_whitespace())
                .map(|offset| start + offset)
                .unwrap_or(out.len());
            out.replace_range(start..end, "[redacted]");
        }
    }
    if out.len() > 240 {
        out.truncate(240);
        out.push_str("...");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::im::{FakeImAdapter, ImIdentity};
    use futures::future::BoxFuture;
    use psychevo_runtime::{Outcome, RunResult, StateRuntime};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug, Default)]
    struct TestBackend {
        prompts: Arc<Mutex<Vec<String>>>,
        runs: AtomicUsize,
    }

    #[derive(Debug)]
    struct ErrorImAdapter {
        polls: Arc<AtomicUsize>,
    }

    impl crate::im::ImAdapter for ErrorImAdapter {
        fn platform(&self) -> &str {
            "wechat"
        }

        fn poll(&self) -> BoxFuture<'static, psychevo_runtime::Result<Vec<ImInboundMessage>>> {
            let polls = Arc::clone(&self.polls);
            Box::pin(async move {
                polls.fetch_add(1, Ordering::SeqCst);
                Err(Error::Message(
                    "WeChat iLink getupdates failed: needs_qr_login errcode=-14: session timeout"
                        .to_string(),
                ))
            })
        }

        fn send(
            &self,
            _message: ImOutboundMessage,
        ) -> BoxFuture<'static, psychevo_runtime::Result<()>> {
            Box::pin(async { Ok(()) })
        }
    }

    impl crate::GatewayBackend for TestBackend {
        fn kind(&self) -> BackendKind {
            BackendKind::Psychevo
        }

        fn run_turn(
            &self,
            request: crate::BackendTurnRequest,
        ) -> BoxFuture<'static, psychevo_runtime::Result<RunResult>> {
            let prompts = Arc::clone(&self.prompts);
            let run_number = self.runs.fetch_add(1, Ordering::SeqCst) + 1;
            Box::pin(async move {
                prompts
                    .lock()
                    .expect("prompts poisoned")
                    .push(request.options.prompt.clone());
                let session_id = request.options.state.store().create_session_with_metadata(
                    &request.options.workdir,
                    &request.runtime_source,
                    "fake-model",
                    "fake-provider",
                    None,
                )?;
                Ok(RunResult {
                    session_id,
                    outcome: Outcome::Normal,
                    terminal_reason: None,
                    final_answer: format!("answer {run_number}"),
                    db_path: request.options.state.db_path().to_path_buf(),
                    workdir: request.options.workdir,
                    provider: "fake-provider".to_string(),
                    model: "fake-model".to_string(),
                    base_url: String::new(),
                    api_key_env: None,
                    reasoning_effort: None,
                    context_limit: None,
                    tool_failures: 0,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                    context_snapshot: None,
                    events: Vec::new(),
                    warnings: Vec::new(),
                })
            })
        }
    }

    #[tokio::test]
    async fn channel_message_runs_gateway_turn_and_sends_final_answer() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workdir = temp.path().join("work");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&workdir).expect("workdir");
        let backend = Arc::new(TestBackend::default());
        let prompts = Arc::clone(&backend.prompts);
        let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
        let gateway = Gateway::with_backend(state_runtime, backend);
        let state = WebState::new(GatewayWebServerConfig::new(
            gateway,
            home,
            workdir.clone(),
            None,
            BTreeMap::new(),
            temp.path().join("static"),
        ));
        let adapter = FakeImAdapter::new("wechat");
        let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
            "wechat",
            Arc::new(adapter.clone()),
            ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
        )]);
        let runtime = ChannelRuntimeState::new(temp.path());
        let connection = ChannelRuntimeConnection {
            id: "wechat".to_string(),
            channel: "wechat".to_string(),
            domain: Some("wechat".to_string()),
            enabled: true,
            label: "WeChat".to_string(),
            transport: "polling".to_string(),
            workdir: None,
            model: None,
            permission_mode: None,
            require_mention: true,
            credential: None,
            app_id: None,
            app_secret: None,
            account_id: None,
            base_url: None,
            allow_users: vec!["wx-user".to_string()],
            allow_groups: Vec::new(),
            config_status: "ready".to_string(),
        };
        let message = ImInboundMessage {
            identity: ImIdentity {
                connection_id: Some("wechat".to_string()),
                platform: "wechat".to_string(),
                domain: Some("wechat".to_string()),
                workspace_id: None,
                chat_type: Some("dm".to_string()),
                chat_id: "wx-user".to_string(),
                thread_id: None,
                user_id: Some("wx-user".to_string()),
                operator_id: None,
                reply_to: None,
            },
            message_id: "wx-message".to_string(),
            text: "ping".to_string(),
            task_key: None,
        };

        handle_channel_message(&state, &runtime, &connection, &channel_gateway, message)
            .await
            .expect("message handled");

        assert_eq!(
            prompts.lock().expect("prompts poisoned").as_slice(),
            ["ping"]
        );
        let sent = adapter.sent();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].text, "answer 1");
        assert_eq!(runtime.runner_view("wechat").state, "running");
        assert!(runtime.runner_view("wechat").last_outbound_at_ms.is_some());
    }

    #[tokio::test]
    async fn wechat_session_timeout_blocks_runner_without_retrying() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workdir = temp.path().join("work");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&workdir).expect("workdir");
        let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
        let state = WebState::new(GatewayWebServerConfig::new(
            Gateway::with_backend(state_runtime, Arc::new(TestBackend::default())),
            home,
            workdir,
            None,
            BTreeMap::new(),
            temp.path().join("static"),
        ));
        let polls = Arc::new(AtomicUsize::new(0));
        let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
            "wechat",
            Arc::new(ErrorImAdapter {
                polls: Arc::clone(&polls),
            }),
            ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
        )]);
        let runtime = ChannelRuntimeState::new(temp.path());
        let connection = ChannelRuntimeConnection {
            id: "wechat".to_string(),
            channel: "wechat".to_string(),
            domain: Some("wechat".to_string()),
            enabled: true,
            label: "WeChat".to_string(),
            transport: "polling".to_string(),
            workdir: None,
            model: None,
            permission_mode: None,
            require_mention: true,
            credential: None,
            app_id: None,
            app_secret: None,
            account_id: None,
            base_url: None,
            allow_users: vec!["wx-user".to_string()],
            allow_groups: Vec::new(),
            config_status: "ready".to_string(),
        };

        run_channel_loop(
            state,
            runtime.clone(),
            connection,
            channel_gateway,
            CancellationToken::new(),
        )
        .await;

        let runner = runtime.runner_view("wechat");
        assert_eq!(polls.load(Ordering::SeqCst), 1);
        assert_eq!(runner.state, "blocked");
        assert_eq!(runner.reason.as_deref(), Some("needs_qr_login"));
        assert_eq!(runner.last_ilink_errcode, Some(-14));
    }

    #[tokio::test]
    async fn wechat_session_timeout_during_login_grace_reports_pending() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workdir = temp.path().join("work");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&workdir).expect("workdir");
        let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
        let state = WebState::new(GatewayWebServerConfig::new(
            Gateway::with_backend(state_runtime, Arc::new(TestBackend::default())),
            home,
            workdir,
            None,
            BTreeMap::new(),
            temp.path().join("static"),
        ));
        let polls = Arc::new(AtomicUsize::new(0));
        let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
            "wechat",
            Arc::new(ErrorImAdapter {
                polls: Arc::clone(&polls),
            }),
            ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
        )]);
        let runtime = ChannelRuntimeState::new(temp.path());
        runtime.start_wechat_login_grace("wechat");
        let connection = ChannelRuntimeConnection {
            id: "wechat".to_string(),
            channel: "wechat".to_string(),
            domain: Some("wechat".to_string()),
            enabled: true,
            label: "WeChat".to_string(),
            transport: "polling".to_string(),
            workdir: None,
            model: None,
            permission_mode: None,
            require_mention: true,
            credential: None,
            app_id: None,
            app_secret: None,
            account_id: None,
            base_url: None,
            allow_users: vec!["wx-user".to_string()],
            allow_groups: Vec::new(),
            config_status: "ready".to_string(),
        };
        let cancel = CancellationToken::new();
        let handle = tokio::spawn(run_channel_loop(
            state,
            runtime.clone(),
            connection,
            channel_gateway,
            cancel.clone(),
        ));

        for _ in 0..100 {
            let runner = runtime.runner_view("wechat");
            if polls.load(Ordering::SeqCst) >= 1
                && runner.reason.as_deref() == Some("qr_login_pending")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let runner = runtime.runner_view("wechat");
        assert!(polls.load(Ordering::SeqCst) >= 1);
        assert_eq!(runner.state, "running");
        assert_eq!(runner.reason.as_deref(), Some("qr_login_pending"));
        assert_eq!(runner.last_ilink_errcode, Some(-14));
        assert!(runner.last_error.is_none());
        cancel.cancel();
        handle.abort();
    }
}
