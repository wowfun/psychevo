use super::*;
use crate::im::adapters::{
    FeishuLarkDomain, FeishuLarkLongConnectionAdapter, FeishuLarkLongConnectionConfig,
    TelegramPollingAdapter, TelegramPollingConfig, WECHAT_ILINK_BASE_URL, WechatIlinkAdapter,
    WechatIlinkConfig, is_wechat_ilink_session_expired_error, wechat_ilink_error_code_from_message,
};
use crate::im::{
    ChannelAdapterBinding, ChannelAllowlist, ChannelGateway, ImIdentity, ImInboundMessage,
    ImOutboundMessage, gateway_input_parts_for_im, gateway_source_for_im,
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
    mut message: ImInboundMessage,
) -> psychevo_runtime::Result<()> {
    let source = gateway_source_for_im(&message);
    if let Some(action) = route_channel_command(state, runtime, connection, &message, &source)? {
        match action {
            ChannelCommandAction::Reply(reply) => {
                channel_gateway
                    .send(ImOutboundMessage {
                        identity: message.identity,
                        thread_id: channel_reply_thread_id(state, &source),
                        text: reply,
                    })
                    .await?;
                runtime.mark_outbound(&connection.id);
                return Ok(());
            }
            ChannelCommandAction::SubmitPrompt(prompt) => {
                message.text = prompt;
            }
        }
    }
    let turn_state = state.clone();
    let turn_runtime = runtime.clone();
    let turn_connection = connection.clone();
    let turn_gateway = channel_gateway.clone();
    let _handle = tokio::spawn(async move {
        if let Err(err) = run_channel_inbound_turn(
            turn_state,
            turn_runtime.clone(),
            turn_connection.clone(),
            turn_gateway,
            message,
            source,
        )
        .await
        {
            turn_runtime.mark_error(&turn_connection.id, &err);
            eprintln!(
                "channel turn failed: id={} channel={} error={}",
                turn_connection.id,
                turn_connection.channel,
                redact_channel_error(&err.to_string())
            );
        }
    });
    Ok(())
}

async fn run_channel_inbound_turn(
    state: WebState,
    runtime: ChannelRuntimeState,
    connection: ChannelRuntimeConnection,
    channel_gateway: ChannelGateway,
    message: ImInboundMessage,
    source: GatewaySource,
) -> psychevo_runtime::Result<()> {
    let workdir = channel_workdir(&state.inner.workdir, &connection);
    let mut options = state.run_options(workdir, None);
    options.model = connection.model.clone();
    options.permission_mode = connection
        .permission_mode
        .as_deref()
        .and_then(PermissionMode::parse)
        .or(options.permission_mode);
    let event_sink = channel_event_sink(
        runtime.clone(),
        connection.id.clone(),
        channel_gateway.clone(),
        message.identity.clone(),
        source.source_key(),
    );
    let result = state
        .inner
        .gateway
        .send_turn(crate::SendTurnRequest {
            thread_id: None,
            source: Some(source.clone()),
            bind_source: Some(source),
            reset_source_binding: false,
            input: gateway_input_parts_for_im(&message),
            options,
            runtime_source: Some(format!("channel/{}", connection.channel)),
            continue_sources: vec![format!("channel/{}", connection.channel)],
            stream: None,
            event_sink: Some(event_sink),
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

enum ChannelCommandAction {
    Reply(String),
    SubmitPrompt(String),
}

struct ChannelCommandContext<'a> {
    state: &'a WebState,
    runtime: &'a ChannelRuntimeState,
    connection: &'a ChannelRuntimeConnection,
    source: &'a GatewaySource,
    scope: &'a ResolvedScope,
    raw: &'a str,
}

fn route_channel_command(
    state: &WebState,
    runtime: &ChannelRuntimeState,
    connection: &ChannelRuntimeConnection,
    message: &ImInboundMessage,
    source: &GatewaySource,
) -> psychevo_runtime::Result<Option<ChannelCommandAction>> {
    let text = message.text.trim();
    let Some((command, args)) = parse_channel_command(text) else {
        return Ok(None);
    };
    let reply = match command.as_str() {
        "stop" => {
            let interrupted = state
                .inner
                .gateway
                .interrupt_turn(GatewayThreadSelector::source(source.source_key()));
            if interrupted {
                "Stop requested for this channel thread.".to_string()
            } else {
                "No active turn is running for this channel thread.".to_string()
            }
        }
        "approve" | "allow" => {
            let request_id = args.split_whitespace().next().unwrap_or("");
            if request_id.is_empty() {
                "Usage: /approve <request_id>".to_string()
            } else if state.inner.gateway.submit_permission(
                GatewayThreadSelector::source(source.source_key()),
                request_id,
                PermissionApprovalDecision::allow_once(),
            ) {
                format!("Approved request {request_id}.")
            } else {
                format!("No matching permission request for {request_id}.")
            }
        }
        "deny" => {
            let request_id = args.split_whitespace().next().unwrap_or("");
            if request_id.is_empty() {
                "Usage: /deny <request_id>".to_string()
            } else if state.inner.gateway.submit_permission(
                GatewayThreadSelector::source(source.source_key()),
                request_id,
                PermissionApprovalDecision::deny(),
            ) {
                format!("Denied request {request_id}.")
            } else {
                format!("No matching permission request for {request_id}.")
            }
        }
        "answer" => {
            let (request_id, answer) = split_first_arg(args);
            if request_id.is_empty() || answer.is_empty() {
                "Usage: /answer <request_id> <answer>".to_string()
            } else if state.inner.gateway.submit_clarify(
                GatewayThreadSelector::source(source.source_key()),
                request_id,
                ClarifyResult::Answered(ClarifyResponse {
                    answers: vec![ClarifyAnswer {
                        answers: vec![answer.to_string()],
                    }],
                }),
            ) {
                format!("Answered request {request_id}.")
            } else {
                format!("No matching Ask request for {request_id}.")
            }
        }
        "cancel" => {
            let request_id = args.split_whitespace().next().unwrap_or("");
            if request_id.is_empty() {
                "Usage: /cancel <request_id>".to_string()
            } else if state.inner.gateway.submit_clarify(
                GatewayThreadSelector::source(source.source_key()),
                request_id,
                ClarifyResult::Cancelled,
            ) {
                format!("Cancelled request {request_id}.")
            } else {
                format!("No matching Ask request for {request_id}.")
            }
        }
        "reset" => reset_channel_source_reply(state, source)?,
        "" => return Ok(None),
        _ => {
            return route_shared_channel_command(state, runtime, connection, source, text);
        }
    };
    Ok(Some(ChannelCommandAction::Reply(reply)))
}

fn route_shared_channel_command(
    state: &WebState,
    runtime: &ChannelRuntimeState,
    connection: &ChannelRuntimeConnection,
    source: &GatewaySource,
    text: &str,
) -> psychevo_runtime::Result<Option<ChannelCommandAction>> {
    let scope = channel_resolved_scope(state, connection, source)?;
    let context = ChannelCommandContext {
        state,
        runtime,
        connection,
        source,
        scope: &scope,
        raw: text,
    };
    let thread_id = state.inner.gateway.resolve_source_thread(source)?;
    let active_turn = state.activity(source, thread_id.as_deref()).running;
    let dynamic = dynamic_slash_commands(state, &scope)?;
    let action = match parse_slash_command_line(text) {
        SlashCommandParse::Known(invocation) => {
            let action = invocation.spec.action;
            if !channel_action_visible(action) {
                return Ok(Some(ChannelCommandAction::Reply(
                    invocation
                        .spec
                        .unavailable_guidance(SlashCommandSurface::Messaging),
                )));
            }
            match slash_invocation_effect(
                &invocation,
                &channel_command_capabilities(),
                SlashCommandSurface::Messaging,
                active_turn,
            ) {
                Ok(effect) => channel_command_action_from_effect(&context, action, effect)?,
                Err(message) => ChannelCommandAction::Reply(message),
            }
        }
        SlashCommandParse::Unknown { command, args, .. } => {
            if let Some(effect) = dynamic_slash_command_effect(&command, &args, &dynamic) {
                channel_command_action_from_effect(
                    &context,
                    SlashCommandAction::SkillInvoke,
                    effect,
                )?
            } else {
                ChannelCommandAction::Reply(format!(
                    "Unsupported channel command /{}. Send /help for available commands.",
                    command
                ))
            }
        }
        SlashCommandParse::NotSlash => return Ok(None),
    };
    Ok(Some(action))
}

fn channel_command_action_from_effect(
    context: &ChannelCommandContext<'_>,
    action: SlashCommandAction,
    effect: SlashCommandEffect,
) -> psychevo_runtime::Result<ChannelCommandAction> {
    let action = match effect {
        SlashCommandEffect::LocalText => match action {
            SlashCommandAction::Help => ChannelCommandAction::Reply(channel_help_text(
                context.state,
                context.scope,
                context.source,
                context.connection,
                context.runtime,
            )?),
            SlashCommandAction::Status => ChannelCommandAction::Reply(channel_status_text(
                context.state,
                context.runtime,
                context.connection,
                context.source,
            )?),
            _ => ChannelCommandAction::Reply(format!(
                "{} is not available as channel text output yet.",
                context.raw.split_whitespace().next().unwrap_or(context.raw)
            )),
        },
        SlashCommandEffect::NewSession => {
            ChannelCommandAction::Reply(reset_channel_source_reply(context.state, context.source)?)
        }
        SlashCommandEffect::PassThroughPrompt(text)
        | SlashCommandEffect::SubmitPrompt(text)
        | SlashCommandEffect::Queue(text)
        | SlashCommandEffect::Fork(text) => ChannelCommandAction::SubmitPrompt(text),
        SlashCommandEffect::Compact { instructions } => {
            ChannelCommandAction::SubmitPrompt(compact_prompt_text(instructions))
        }
        SlashCommandEffect::Steer(text) => {
            let message = RuntimeMessage::User {
                content: vec![UserContentBlock::text(text)],
                timestamp_ms: gateway_now_ms(),
            };
            let accepted = context.state.inner.gateway.steer_foreign_turn(
                GatewayThreadSelector::source(context.source.source_key()),
                None,
                message,
            );
            ChannelCommandAction::Reply(if accepted {
                "Steer message sent to the active channel turn.".to_string()
            } else {
                "No active channel turn accepted the steer message.".to_string()
            })
        }
        SlashCommandEffect::PendingCancel => {
            let selector = GatewayThreadSelector::source(context.source.source_key());
            let cleared = context.state.inner.gateway.clear_queue(selector.clone());
            let interrupted = context.state.inner.gateway.interrupt_turn(selector);
            ChannelCommandAction::Reply(format!(
                "Pending work updated: interrupted={}, cleared queued turns={}.",
                interrupted, cleared
            ))
        }
        SlashCommandEffect::SandboxShow => {
            let thread_id = context
                .state
                .inner
                .gateway
                .resolve_source_thread(context.source)?;
            let options = context
                .state
                .run_options(context.scope.workdir.clone(), thread_id);
            ChannelCommandAction::Reply(psychevo_runtime::sandbox_status_text(
                &options,
                RunMode::Default,
            )?)
        }
        SlashCommandEffect::Skills { .. } => {
            ChannelCommandAction::Reply(channel_skills_text(context.state, context.scope)?)
        }
        SlashCommandEffect::Agents => {
            ChannelCommandAction::Reply(channel_agents_text(context.state, context.scope)?)
        }
        SlashCommandEffect::Unsupported(message) => ChannelCommandAction::Reply(message),
        SlashCommandEffect::Diff
        | SlashCommandEffect::SessionsList
        | SlashCommandEffect::ResumeSession { .. }
        | SlashCommandEffect::Btw { .. }
        | SlashCommandEffect::ShowModel
        | SlashCommandEffect::SetModel { .. }
        | SlashCommandEffect::SetVariant(_)
        | SlashCommandEffect::SetMode(_)
        | SlashCommandEffect::PermissionsShow
        | SlashCommandEffect::PermissionAdd { .. }
        | SlashCommandEffect::PermissionRemove { .. }
        | SlashCommandEffect::ToolsShow
        | SlashCommandEffect::ToolsetSet { .. }
        | SlashCommandEffect::Rename(_)
        | SlashCommandEffect::Undo
        | SlashCommandEffect::Redo
        | SlashCommandEffect::Bundles { .. }
        | SlashCommandEffect::Curator { .. }
        | SlashCommandEffect::Export { .. }
        | SlashCommandEffect::Share { .. } => ChannelCommandAction::Reply(format!(
            "{} is not available on messaging channels yet.",
            context.raw.split_whitespace().next().unwrap_or(context.raw)
        )),
    };
    Ok(action)
}

fn channel_command_capabilities() -> Vec<CommandCapability> {
    vec![
        CommandCapability::ActiveTurnControl,
        CommandCapability::Queue,
    ]
}

fn channel_action_visible(action: SlashCommandAction) -> bool {
    matches!(
        action,
        SlashCommandAction::Help
            | SlashCommandAction::Status
            | SlashCommandAction::New
            | SlashCommandAction::Steer
            | SlashCommandAction::Queue
            | SlashCommandAction::Pending
            | SlashCommandAction::Sandbox
            | SlashCommandAction::Skills
            | SlashCommandAction::Agents
            | SlashCommandAction::Compact
            | SlashCommandAction::SkillInvoke
    )
}

fn channel_help_text(
    state: &WebState,
    scope: &ResolvedScope,
    source: &GatewaySource,
    connection: &ChannelRuntimeConnection,
    runtime: &ChannelRuntimeState,
) -> psychevo_runtime::Result<String> {
    let thread_id = state.inner.gateway.resolve_source_thread(source)?;
    let active_turn = state.activity(source, thread_id.as_deref()).running;
    let dynamic = dynamic_slash_commands(state, scope)?;
    let available = available_slash_commands_for_surface(
        &channel_command_capabilities(),
        active_turn,
        &dynamic,
        32,
    );
    let mut lines = vec![format!("Channel {} commands:", connection.label.trim())];
    for command in available
        .commands
        .iter()
        .filter(|command| channel_action_visible(command.action))
        .take(16)
    {
        lines.push(format!("/{} - {}", command.name, command.summary));
    }
    if available.hidden_dynamic > 0 {
        lines.push(format!(
            "...and {} more skill commands.",
            available.hidden_dynamic
        ));
    }
    lines.push(
        "Controls: /stop, /reset, /approve <id>, /deny <id>, /answer <id> <text>, /cancel <id>."
            .to_string(),
    );
    lines.push(channel_status_text(state, runtime, connection, source)?);
    Ok(lines.join("\n"))
}

fn channel_status_text(
    state: &WebState,
    runtime: &ChannelRuntimeState,
    connection: &ChannelRuntimeConnection,
    source: &GatewaySource,
) -> psychevo_runtime::Result<String> {
    let runner = runtime.runner_view(&connection.id);
    let thread = state
        .inner
        .gateway
        .resolve_source_thread(source)?
        .unwrap_or_else(|| "none".to_string());
    Ok(format!(
        "Channel {} is {}{}; config {}; thread {}.",
        connection.label,
        runner.state,
        runner
            .reason
            .as_deref()
            .map(|reason| format!(" ({reason})"))
            .unwrap_or_default(),
        connection.config_status,
        thread
    ))
}

fn channel_skills_text(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<String> {
    let dynamic = dynamic_slash_commands(state, scope)?;
    if dynamic.is_empty() {
        return Ok("No channel-available skills found for this workspace.".to_string());
    }
    let mut lines = vec!["Channel-available skills:".to_string()];
    for command in dynamic.iter().take(20) {
        lines.push(format!("/{} - {}", command.name, command.summary));
    }
    if dynamic.len() > 20 {
        lines.push(format!("...and {} more.", dynamic.len() - 20));
    }
    Ok(lines.join("\n"))
}

fn channel_agents_text(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<String> {
    let catalog = discover_gateway_agents(state, scope)?;
    let agents = catalog
        .agents
        .into_iter()
        .filter(|agent| agent.supports_entrypoint(AgentEntrypoint::Peer))
        .collect::<Vec<_>>();
    if agents.is_empty() {
        return Ok("No peer agents found for this workspace.".to_string());
    }
    let mut lines = vec!["Channel-available agents:".to_string()];
    for agent in agents.iter().take(20) {
        lines.push(format!("@{} - {}", agent.name, agent.description));
    }
    if agents.len() > 20 {
        lines.push(format!("...and {} more.", agents.len() - 20));
    }
    Ok(lines.join("\n"))
}

fn reset_channel_source_reply(
    state: &WebState,
    source: &GatewaySource,
) -> psychevo_runtime::Result<String> {
    let previous = state.inner.gateway.reset_source_to_empty(source)?;
    Ok(if previous.is_some() {
        "Started a new channel thread. The next message will use this channel's current default workspace.".to_string()
    } else {
        "No channel thread was active. The next message will start a new channel thread."
            .to_string()
    })
}

fn channel_resolved_scope(
    state: &WebState,
    connection: &ChannelRuntimeConnection,
    source: &GatewaySource,
) -> psychevo_runtime::Result<ResolvedScope> {
    let workdir = match state.inner.gateway.resolve_source_thread(source)? {
        Some(thread_id) => state
            .inner
            .state
            .store()
            .session_summary(&thread_id)?
            .map(|summary| PathBuf::from(summary.workdir))
            .unwrap_or_else(|| channel_workdir(&state.inner.workdir, connection)),
        None => channel_workdir(&state.inner.workdir, connection),
    };
    Ok(ResolvedScope {
        workdir,
        source: source.clone(),
    })
}

fn parse_channel_command(text: &str) -> Option<(String, &str)> {
    let text = text.trim();
    let command_line = text.strip_prefix('/')?;
    let split_at = command_line
        .find(char::is_whitespace)
        .unwrap_or(command_line.len());
    let (token, args) = command_line.split_at(split_at);
    let command = token.split('@').next().unwrap_or("").to_ascii_lowercase();
    Some((command, args.trim()))
}

fn split_first_arg(value: &str) -> (&str, &str) {
    let value = value.trim();
    let split_at = value.find(char::is_whitespace).unwrap_or(value.len());
    let (first, rest) = value.split_at(split_at);
    (first, rest.trim())
}

fn channel_reply_thread_id(state: &WebState, source: &GatewaySource) -> String {
    state
        .inner
        .gateway
        .resolve_source_thread(source)
        .ok()
        .flatten()
        .unwrap_or_else(|| source.source_key().0)
}

fn channel_event_sink(
    runtime: ChannelRuntimeState,
    connection_id: String,
    channel_gateway: ChannelGateway,
    identity: ImIdentity,
    fallback_source_key: SourceKey,
) -> GatewayEventSink {
    Arc::new(move |event| {
        let Some(text) = channel_event_reply_text(&event) else {
            return;
        };
        let thread_id = channel_event_thread_id(&event, &fallback_source_key);
        let gateway = channel_gateway.clone();
        let runtime = runtime.clone();
        let connection_id = connection_id.clone();
        let identity = identity.clone();
        tokio::spawn(async move {
            let result = gateway
                .send(ImOutboundMessage {
                    identity,
                    thread_id,
                    text,
                })
                .await;
            match result {
                Ok(()) => runtime.mark_outbound(&connection_id),
                Err(err) => {
                    runtime.mark_error(&connection_id, &err);
                    eprintln!(
                        "channel event delivery failed: id={} error={}",
                        connection_id,
                        redact_channel_error(&err.to_string())
                    );
                }
            }
        });
    })
}

fn channel_event_reply_text(event: &GatewayEvent) -> Option<String> {
    match event {
        GatewayEvent::PermissionRequested {
            request_id,
            tool_name,
            summary,
            reason,
            ..
        } => {
            let detail = if !summary.trim().is_empty() {
                summary.trim()
            } else if !reason.trim().is_empty() {
                reason.trim()
            } else {
                "approval requested"
            };
            Some(format!(
                "Permission required for {tool_name}: {detail}. Reply /approve {request_id} to allow once or /deny {request_id} to deny."
            ))
        }
        GatewayEvent::ClarifyRequested {
            request_id, raw, ..
        } => {
            let question = raw
                .get("questions")
                .and_then(Value::as_array)
                .and_then(|questions| questions.first())
                .and_then(|question| question.get("question"))
                .and_then(Value::as_str)
                .filter(|question| !question.trim().is_empty())
                .unwrap_or("Please provide more information.");
            Some(format!(
                "Psychevo asks: {question}. Reply /answer {request_id} <answer> or /cancel {request_id}."
            ))
        }
        _ => None,
    }
}

fn channel_event_thread_id(event: &GatewayEvent, fallback_source_key: &SourceKey) -> String {
    match event {
        GatewayEvent::PermissionRequested {
            thread_id,
            source_key,
            ..
        }
        | GatewayEvent::ClarifyRequested {
            thread_id,
            source_key,
            ..
        } => thread_id
            .clone()
            .or_else(|| source_key.clone())
            .unwrap_or_else(|| fallback_source_key.0.clone()),
        _ => fallback_source_key.0.clone(),
    }
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
    use crate::im::FakeImAdapter;
    use futures::future::BoxFuture;
    use psychevo_runtime::{Outcome, RunResult, StateRuntime};
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Debug, Default)]
    struct TestBackend {
        prompts: Arc<Mutex<Vec<String>>>,
        runs: AtomicUsize,
        request_permission: AtomicBool,
    }

    impl TestBackend {
        fn request_permission(&self) {
            self.request_permission.store(true, Ordering::SeqCst);
        }
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
            let request_permission = self.request_permission.load(Ordering::SeqCst);
            Box::pin(async move {
                if request_permission {
                    let Some(handler) = request.options.approval_handler.clone() else {
                        return Err(Error::Message("approval handler missing".to_string()));
                    };
                    let decision = handler
                        .request_permission(psychevo_runtime::PermissionApprovalRequest {
                            tool_call_id: "permission-1".to_string(),
                            tool_name: "fake_tool".to_string(),
                            summary: "fake permission".to_string(),
                            reason: "test permission".to_string(),
                            matched_rule: None,
                            suggested_rule: None,
                            allow_always: false,
                            timeout_secs: 300,
                        })
                        .await;
                    if matches!(decision.outcome, PermissionApprovalOutcome::Deny) {
                        return Err(Error::Message("permission denied".to_string()));
                    }
                }
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

    fn ready_wechat_connection(workdir: Option<String>) -> ChannelRuntimeConnection {
        ChannelRuntimeConnection {
            id: "wechat".to_string(),
            channel: "wechat".to_string(),
            domain: Some("wechat".to_string()),
            enabled: true,
            label: "WeChat".to_string(),
            transport: "polling".to_string(),
            workdir,
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
        }
    }

    fn wechat_message(text: &str, message_id: &str) -> ImInboundMessage {
        ImInboundMessage {
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
            message_id: message_id.to_string(),
            text: text.to_string(),
            attachments: Vec::new(),
            task_key: None,
        }
    }

    async fn wait_for_sent(adapter: &FakeImAdapter, count: usize) -> Vec<ImOutboundMessage> {
        for _ in 0..100 {
            let sent = adapter.sent();
            if sent.len() >= count {
                return sent;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        adapter.sent()
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
        let connection = ready_wechat_connection(None);
        let message = wechat_message("ping", "wx-message");

        handle_channel_message(&state, &runtime, &connection, &channel_gateway, message)
            .await
            .expect("message handled");

        let sent = wait_for_sent(&adapter, 1).await;
        assert_eq!(
            prompts.lock().expect("prompts poisoned").as_slice(),
            ["ping"]
        );
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].text, "answer 1");
        assert_eq!(runtime.runner_view("wechat").state, "running");
        assert!(runtime.runner_view("wechat").last_outbound_at_ms.is_some());
    }

    #[tokio::test]
    async fn channel_help_command_replies_without_running_gateway_turn() {
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
            workdir,
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

        handle_channel_message(
            &state,
            &runtime,
            &ready_wechat_connection(None),
            &channel_gateway,
            wechat_message("/help", "wx-help"),
        )
        .await
        .expect("help handled");

        assert!(prompts.lock().expect("prompts poisoned").is_empty());
        let sent = wait_for_sent(&adapter, 1).await;
        assert_eq!(sent.len(), 1);
        assert!(sent[0].text.contains("/status"));
        assert!(sent[0].text.contains("/compact"));
        assert!(sent[0].thread_id.starts_with("im.wechat:"));
    }

    #[tokio::test]
    async fn channel_shared_compact_command_runs_gateway_turn() {
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
            workdir,
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

        handle_channel_message(
            &state,
            &runtime,
            &ready_wechat_connection(None),
            &channel_gateway,
            wechat_message("/compact keep decisions", "wx-compact"),
        )
        .await
        .expect("compact handled");

        let sent = wait_for_sent(&adapter, 1).await;
        let prompts = prompts.lock().expect("prompts poisoned");
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].contains("Compact this session"));
        assert!(prompts[0].contains("keep decisions"));
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].text, "answer 1");
    }

    #[tokio::test]
    async fn channel_dynamic_skill_command_runs_gateway_turn() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workdir = temp.path().join("work");
        let home = temp.path().join("home");
        let skill_dir = workdir.join(".psychevo/skills/reviewer");
        std::fs::create_dir_all(&skill_dir).expect("skill dir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: reviewer\ndescription: Review the current change.\n---\n\nReview carefully.\n",
        )
        .expect("skill");
        let backend = Arc::new(TestBackend::default());
        let prompts = Arc::clone(&backend.prompts);
        let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
        let gateway = Gateway::with_backend(state_runtime, backend);
        let state = WebState::new(GatewayWebServerConfig::new(
            gateway,
            home,
            workdir,
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

        handle_channel_message(
            &state,
            &runtime,
            &ready_wechat_connection(None),
            &channel_gateway,
            wechat_message("/reviewer focus security", "wx-reviewer"),
        )
        .await
        .expect("dynamic skill handled");

        let sent = wait_for_sent(&adapter, 1).await;
        let prompts = prompts.lock().expect("prompts poisoned");
        assert_eq!(prompts.as_slice(), ["$reviewer focus security"]);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].text, "answer 1");
    }

    #[tokio::test]
    async fn channel_new_command_clears_binding_for_next_default_workdir() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workdir = temp.path().join("work");
        let changed_workdir = temp.path().join("changed");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&workdir).expect("workdir");
        std::fs::create_dir_all(&changed_workdir).expect("changed workdir");
        let backend = Arc::new(TestBackend::default());
        let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
        let store_state = state_runtime.clone();
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

        handle_channel_message(
            &state,
            &runtime,
            &ready_wechat_connection(None),
            &channel_gateway,
            wechat_message("first", "wx-first"),
        )
        .await
        .expect("first handled");
        let sent = wait_for_sent(&adapter, 1).await;
        assert_eq!(sent.len(), 1);
        handle_channel_message(
            &state,
            &runtime,
            &ready_wechat_connection(Some(changed_workdir.to_string_lossy().to_string())),
            &channel_gateway,
            wechat_message("/new", "wx-new"),
        )
        .await
        .expect("new handled");
        let sent = wait_for_sent(&adapter, 2).await;
        assert_eq!(sent.len(), 2);
        handle_channel_message(
            &state,
            &runtime,
            &ready_wechat_connection(Some(changed_workdir.to_string_lossy().to_string())),
            &channel_gateway,
            wechat_message("second", "wx-second"),
        )
        .await
        .expect("second handled");

        let sent = wait_for_sent(&adapter, 3).await;
        assert_eq!(sent.len(), 3);
        assert_ne!(sent[0].thread_id, sent[2].thread_id);
        let active_sessions = store_state
            .store()
            .list_sessions_with_sources(&["channel/wechat"])
            .expect("sessions");
        let archived_sessions = store_state
            .store()
            .list_archived_sessions_with_sources(&["channel/wechat"])
            .expect("archived sessions");
        let workdirs = active_sessions
            .iter()
            .chain(archived_sessions.iter())
            .map(|session| session.workdir.as_str())
            .collect::<BTreeSet<_>>();
        assert!(workdirs.contains(workdir.to_string_lossy().as_ref()));
        assert!(workdirs.contains(changed_workdir.to_string_lossy().as_ref()));
    }

    #[tokio::test]
    async fn channel_permission_request_can_be_approved_by_command() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workdir = temp.path().join("work");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&workdir).expect("workdir");
        let backend = Arc::new(TestBackend::default());
        backend.request_permission();
        let state_runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
        let gateway = Gateway::with_backend(state_runtime, backend);
        let state = WebState::new(GatewayWebServerConfig::new(
            gateway,
            home,
            workdir,
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

        handle_channel_message(
            &state,
            &runtime,
            &ready_wechat_connection(None),
            &channel_gateway,
            wechat_message("needs approval", "wx-approval-turn"),
        )
        .await
        .expect("approval turn accepted");

        let sent = wait_for_sent(&adapter, 1).await;
        assert!(sent[0].text.contains("Permission required for fake_tool"));
        assert!(sent[0].text.contains("/approve permission-1"));

        handle_channel_message(
            &state,
            &runtime,
            &ready_wechat_connection(None),
            &channel_gateway,
            wechat_message("/approve permission-1", "wx-approve"),
        )
        .await
        .expect("approval command handled");

        let sent = wait_for_sent(&adapter, 3).await;
        assert!(
            sent.iter()
                .any(|message| message.text == "Approved request permission-1.")
        );
        assert!(sent.iter().any(|message| message.text == "answer 1"));
    }

    #[tokio::test]
    async fn channel_answer_command_reports_missing_ask_request() {
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
            workdir,
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

        handle_channel_message(
            &state,
            &runtime,
            &ready_wechat_connection(None),
            &channel_gateway,
            wechat_message("/answer ask-1 use repo root", "wx-answer"),
        )
        .await
        .expect("answer command handled");

        assert!(prompts.lock().expect("prompts poisoned").is_empty());
        let sent = wait_for_sent(&adapter, 1).await;
        assert_eq!(sent[0].text, "No matching Ask request for ask-1.");
    }

    #[tokio::test]
    async fn channel_event_sink_sends_clarify_prompt() {
        let adapter = FakeImAdapter::new("wechat");
        let channel_gateway = ChannelGateway::new(vec![ChannelAdapterBinding::new(
            "wechat",
            Arc::new(adapter.clone()),
            ChannelAllowlist::new(["wx-user".to_string()], Vec::<String>::new()),
        )]);
        let runtime = ChannelRuntimeState::default();
        let identity = wechat_message("ignored", "wx-event").identity;
        let sink = channel_event_sink(
            runtime,
            "wechat".to_string(),
            channel_gateway,
            identity,
            SourceKey("im.wechat:fallback".to_string()),
        );

        sink(GatewayEvent::ClarifyRequested {
            request_id: "ask-1".to_string(),
            raw: json!({
                "questions": [
                    { "question": "Which workspace should I use?" }
                ]
            }),
            thread_id: Some("thread-1".to_string()),
            turn_id: None,
            activity_id: None,
            source_key: None,
            owner_id: None,
            lease_expires_at_ms: None,
        });

        let sent = wait_for_sent(&adapter, 1).await;
        assert_eq!(sent[0].thread_id, "thread-1");
        assert!(sent[0].text.contains("Which workspace should I use?"));
        assert!(sent[0].text.contains("/answer ask-1 <answer>"));
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
