use std::path::Path;

use psychevo::{Configuration, ConfigurationQuery, Error, config::set_channel_enabled};
use psychevo_gateway_protocol as wire;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::gateway_now_ms;
use crate::im::adapters::{
    WECHAT_ILINK_BASE_URL, WechatQrPoll, check_wechat_ilink_health, fetch_wechat_qr_code,
    poll_wechat_qr_code,
};
use psychevo_gateway_protocol::source::GatewayThreadSelector;

use super::agents::active_profile_config_dir;
use super::binding::WebState;
use super::channel_runtime;
use super::scope_session::ResolvedScope;

const WECHAT_QR_INTERVAL_MS: u64 = 3_000;
const WECHAT_QR_EXPIRES_MS: i64 = 120_000;

#[derive(Clone)]
pub(super) struct WechatQrSetupSession {
    pub id: String,
    pub label: Option<String>,
    pub qrcode: String,
    pub base_url: String,
    pub expires_at_ms: i64,
}

#[derive(Debug, Deserialize)]
struct RuntimeChannelConfigRow {
    id: String,
    channel: String,
    domain: Option<String>,
    enabled: bool,
    label: String,
    transport: String,
    cwd: Option<String>,
    runtime_ref: Option<String>,
    model: Option<String>,
    permission_mode: Option<String>,
    require_mention: bool,
    credential: RuntimeChannelCredential,
    #[serde(default)]
    account: Option<RuntimeChannelCredential>,
    #[serde(default)]
    base_url: Option<RuntimeChannelCredential>,
    #[serde(default)]
    app_id: Option<RuntimeChannelCredential>,
    allowlist: RuntimeChannelAllowlist,
    runtime_status: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeChannelCredential {
    env: Option<String>,
    status: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeChannelAllowlist {
    users: Vec<String>,
    groups: Vec<String>,
    status: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeChannelDoctorRow {
    id: String,
    channel: String,
    enabled: bool,
    runtime_status: String,
    checks: Vec<RuntimeChannelDoctorCheck>,
}

#[derive(Debug, Deserialize)]
struct RuntimeChannelDoctorCheck {
    name: String,
    status: String,
    message: String,
}

pub(super) fn channel_list_result_for_scope(
    state: &WebState,
    scope: &ResolvedScope,
) -> psychevo::Result<wire::channels::ChannelListResult> {
    channel_list_result_for_cwd(state, &scope.cwd)
}

pub(super) fn channel_list_result_for_cwd(
    state: &WebState,
    cwd: &Path,
) -> psychevo::Result<wire::channels::ChannelListResult> {
    let mut query = ConfigurationQuery::new(cwd);
    query.inherited_env = Some(state.inner.inherited_env.clone());
    let configuration = state.inner.framework.configuration(query)?;
    let value = configuration.channels()?;
    channel_list_result_from_value(state, value)
}

pub(super) fn channel_show_result(
    state: &WebState,
    scope: &ResolvedScope,
    id: &str,
) -> psychevo::Result<wire::channels::ChannelEnableResult> {
    let mut query = ConfigurationQuery::new(&scope.cwd);
    query.inherited_env = Some(state.inner.inherited_env.clone());
    let configuration = state.inner.framework.configuration(query)?;
    let value = configuration.channel(id)?;
    let row = value
        .get("channel")
        .cloned()
        .ok_or_else(|| Error::Message("channel show returned no channel".to_string()))?;
    Ok(wire::channels::ChannelEnableResult {
        channel: channel_config_view_from_runtime(state, serde_json::from_value(row)?)?,
    })
}

pub(super) fn channel_enable_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::channels::ChannelEnableParams,
) -> psychevo::Result<wire::channels::ChannelEnableResult> {
    let config_dir = active_profile_config_dir(state, scope);
    set_channel_enabled(config_dir, &params.id, params.enabled)?;
    channel_runtime::reconcile(state.clone());
    channel_show_result(state, scope, &params.id)
}

pub(super) async fn channel_update_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::channels::ChannelUpdateParams,
) -> psychevo::Result<wire::channels::ChannelEnableResult> {
    let config_dir = active_profile_config_dir(state, scope);
    let requested_cwd = params.cwd.clone();
    let previous_cwd = if requested_cwd.is_some() {
        channel_show_result(state, scope, &params.id)?.channel.cwd
    } else {
        None
    };
    let normalized_requested_cwd = requested_cwd
        .as_deref()
        .map(|cwd| normalized_channel_update_cwd(cwd, &state.inner.cwd))
        .transpose()?;
    let normalized_cwd_value = normalized_requested_cwd.clone().flatten();
    let update_cwd = if requested_cwd.is_some() {
        Some(normalized_cwd_value.clone().unwrap_or_default())
    } else {
        None
    };
    psychevo::config::update_channel_connection(psychevo::config::ChannelUpdateInput {
        config_dir,
        id: params.id.clone(),
        label: params.label,
        enabled: params.enabled,
        cwd: update_cwd,
        runtime_ref: params.runtime_ref,
        model: params.model,
        permission_mode: params.permission_mode,
        require_mention: params.require_mention,
        credential_env: params.credential_env,
        account_env: params.account_env,
        base_url_env: params.base_url_env,
        app_id_env: params.app_id_env,
        allow_users: params.allow_users,
        allow_groups: params.allow_groups,
    })?;
    if requested_cwd.is_some() && previous_cwd != normalized_cwd_value {
        state
            .inner
            .gateway
            .rotate_channel_connection_sources(&params.id)
            .await?;
        state.inner.channel_runtime.restart(&params.id);
    }
    channel_runtime::reconcile(state.clone());
    channel_show_result(state, scope, &params.id)
}

fn normalized_channel_update_cwd(value: &str, cwd: &Path) -> psychevo::Result<Option<String>> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let resolved = psychevo::host_paths::resolve_input_path(value, cwd)?;
    Ok(Some(
        psychevo::paths::canonicalize_cwd(&resolved)?
            .display()
            .to_string(),
    ))
}

pub(super) fn channel_delete_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::channels::ChannelIdParams,
) -> psychevo::Result<wire::channels::ChannelListResult> {
    let config_dir = active_profile_config_dir(state, scope);
    psychevo::config::delete_channel_connection(config_dir, &params.id)?;
    channel_runtime::reconcile(state.clone());
    channel_list_result_for_scope(state, scope)
}

pub(super) async fn channel_source_list_result(
    state: &WebState,
    _scope: &ResolvedScope,
    params: wire::channels::ChannelIdParams,
) -> psychevo::Result<wire::channels::ChannelSourceListResult> {
    let bindings = state
        .inner
        .durability
        .gateway_source_bindings_for_connection_id(&params.id)
        .await?;
    let mut sources = Vec::new();
    for binding in bindings {
        let raw = &binding.raw_identity;
        let platform = raw
            .get("platform")
            .and_then(Value::as_str)
            .unwrap_or("channel")
            .to_string();
        let domain = raw
            .get("domain")
            .and_then(Value::as_str)
            .map(str::to_string);
        let chat_type = raw
            .get("chatType")
            .and_then(Value::as_str)
            .map(str::to_string);
        let chat_label = raw
            .get("chatId")
            .and_then(Value::as_str)
            .map(redacted_remote_label);
        let user_label = raw
            .get("userId")
            .and_then(Value::as_str)
            .map(redacted_remote_label);
        let summary = state
            .inner
            .framework
            .thread_summary(&binding.thread_id)
            .await?;
        let mut activity = state
            .inner
            .gateway
            .activity_for_selector(GatewayThreadSelector::thread_id(&binding.thread_id))
            .await;
        if let Ok(thread) = state
            .inner
            .framework
            .resume_thread(&binding.thread_id)
            .await
        {
            let framework_activity = thread.activity();
            activity.framework_revision = Some(framework_activity.revision.to_string());
            activity.running |= framework_activity.running;
            if framework_activity.active_turn_id.is_some() {
                activity.active_turn_id = framework_activity.active_turn_id.clone();
            }
            if let Some(turn_id) = framework_activity.active_turn_id {
                let kind = summary
                    .as_ref()
                    .filter(|summary| summary.parent_thread_id.is_some())
                    .map_or(wire::events_transcript::FrameworkTurnKind::Root, |_| {
                        wire::events_transcript::FrameworkTurnKind::DelegatedChild
                    });
                activity.activities.insert(
                    0,
                    wire::events_transcript::ThreadActivityView::FrameworkTurn {
                        activity_id: turn_id.clone(),
                        turn_id,
                        kind,
                        queued_turns: framework_activity.queued_turns,
                    },
                );
            }
            activity.queued_turns = framework_activity.queued_turns;
        }
        let activity_status = if activity.running {
            "running"
        } else if activity.queued_turns > 0 {
            "queued"
        } else {
            "idle"
        }
        .to_string();
        let thread_title = summary.as_ref().and_then(|summary| summary.title.clone());
        let cwd = summary
            .as_ref()
            .map(|summary| summary.cwd.clone())
            .unwrap_or_else(|| state.inner.cwd.display().to_string());
        let visible_name = Some(redacted_channel_source_name(
            &platform,
            chat_type.as_deref(),
            chat_label.as_deref(),
            user_label.as_deref(),
        ));
        sources.push(wire::channels::ChannelSourceBindingView {
            source_key: binding.source_key,
            connection_id: params.id.clone(),
            platform,
            domain,
            chat_type,
            chat_label,
            user_label,
            visible_name,
            thread_id: binding.thread_id,
            thread_title,
            cwd,
            activity_status,
            queued_turns: activity.queued_turns,
            updated_at_ms: binding.updated_at_ms,
        });
    }
    Ok(wire::channels::ChannelSourceListResult { sources })
}

fn redacted_remote_label(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return "unknown".to_string();
    }
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= 6 {
        return "***".to_string();
    }
    let prefix = chars.iter().take(2).collect::<String>();
    let suffix = chars
        .iter()
        .skip(chars.len().saturating_sub(4))
        .collect::<String>();
    format!("{prefix}...{suffix}")
}

fn redacted_channel_source_name(
    platform: &str,
    chat_type: Option<&str>,
    chat_label: Option<&str>,
    user_label: Option<&str>,
) -> String {
    let mut parts = vec![platform.to_string()];
    if let Some(chat_type) = chat_type.filter(|value| !value.trim().is_empty()) {
        parts.push(chat_type.to_string());
    }
    if let Some(chat_label) = chat_label {
        parts.push(format!("chat {chat_label}"));
    }
    if let Some(user_label) = user_label {
        parts.push(format!("user {user_label}"));
    }
    parts.join(" ")
}

pub(super) async fn channel_doctor_result_live(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::channels::ChannelDoctorParams,
) -> psychevo::Result<wire::channels::ChannelDoctorResult> {
    let mut query = ConfigurationQuery::new(&scope.cwd);
    query.inherited_env = Some(state.inner.inherited_env.clone());
    let configuration = state.inner.framework.configuration(query)?;
    let live = params.live.unwrap_or(false);
    let value = configuration.diagnose_channels(params.id.as_deref(), live)?;
    let mut result = channel_doctor_result_from_value(state, value)?;
    if live {
        enrich_wechat_live_doctor(&configuration, params.id.as_deref(), &mut result).await?;
    }
    Ok(result)
}

pub(super) async fn channel_wechat_qr_start_result(
    state: &WebState,
    _scope: &ResolvedScope,
    params: wire::channels::ChannelWechatQrStartParams,
) -> psychevo::Result<wire::channels::ChannelWechatQrStartResult> {
    let id = params
        .id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("wechat")
        .to_string();
    let base_url = params
        .ilink_base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(WECHAT_ILINK_BASE_URL)
        .to_string();
    let client = reqwest::Client::new();
    let qr = fetch_wechat_qr_code(&client, &base_url).await?;
    let session_id = Uuid::now_v7().to_string();
    let expires_at_ms = gateway_now_ms().saturating_add(WECHAT_QR_EXPIRES_MS);
    let qr_base_url = qr.base_url.clone();
    state
        .inner
        .wechat_qr_sessions
        .lock()
        .expect("wechat qr sessions poisoned")
        .insert(
            session_id.clone(),
            WechatQrSetupSession {
                id,
                label: params.label,
                qrcode: qr.qrcode,
                base_url: qr.base_url,
                expires_at_ms,
            },
        );
    eprintln!(
        "wechat qr setup started: id={} base_url={}",
        session_id, qr_base_url
    );
    Ok(wire::channels::ChannelWechatQrStartResult {
        session_id,
        qr_url: qr.qr_url,
        qr_image: qr.qr_image,
        qr_svg: qr.qr_svg,
        status: "wait".to_string(),
        message: "Scan with WeChat to connect this channel.".to_string(),
        interval_ms: WECHAT_QR_INTERVAL_MS,
        expires_at_ms,
    })
}

pub(super) async fn channel_wechat_qr_poll_result(
    state: &WebState,
    scope: &ResolvedScope,
    params: wire::channels::ChannelWechatQrPollParams,
) -> psychevo::Result<wire::channels::ChannelWechatQrPollResult> {
    let session_id = params.session_id.trim();
    let session = {
        let sessions = state
            .inner
            .wechat_qr_sessions
            .lock()
            .expect("wechat qr sessions poisoned");
        sessions
            .get(session_id)
            .map(|session| WechatQrSetupSession {
                id: session.id.clone(),
                label: session.label.clone(),
                qrcode: session.qrcode.clone(),
                base_url: session.base_url.clone(),
                expires_at_ms: session.expires_at_ms,
            })
    }
    .ok_or_else(|| Error::Message("WeChat QR session not found".to_string()))?;
    if gateway_now_ms() > session.expires_at_ms {
        state
            .inner
            .wechat_qr_sessions
            .lock()
            .expect("wechat qr sessions poisoned")
            .remove(session_id);
        return Ok(wire::channels::ChannelWechatQrPollResult {
            done: false,
            status: "expired".to_string(),
            message: "WeChat QR session expired. Generate a new code.".to_string(),
            channel: None,
            expires_at_ms: Some(session.expires_at_ms),
        });
    }
    let client = reqwest::Client::new();
    match poll_wechat_qr_code(&client, &session.base_url, &session.qrcode).await? {
        WechatQrPoll::Waiting {
            status,
            message,
            base_url,
        } => {
            if base_url != session.base_url {
                eprintln!(
                    "wechat qr setup redirect followed: id={} base_url={}",
                    session.id, base_url
                );
                state
                    .inner
                    .wechat_qr_sessions
                    .lock()
                    .expect("wechat qr sessions poisoned")
                    .entry(session_id.to_string())
                    .and_modify(|session| session.base_url = base_url);
            }
            Ok(wire::channels::ChannelWechatQrPollResult {
                done: false,
                status,
                message,
                channel: None,
                expires_at_ms: Some(session.expires_at_ms),
            })
        }
        WechatQrPoll::Expired { message } => {
            state
                .inner
                .wechat_qr_sessions
                .lock()
                .expect("wechat qr sessions poisoned")
                .remove(session_id);
            Ok(wire::channels::ChannelWechatQrPollResult {
                done: false,
                status: "expired".to_string(),
                message,
                channel: None,
                expires_at_ms: Some(session.expires_at_ms),
            })
        }
        WechatQrPoll::Confirmed {
            account_id,
            token,
            base_url,
            user_id,
        } => {
            eprintln!(
                "wechat qr setup confirmed: id={} base_url={} allow_user_present={}",
                session.id,
                base_url,
                user_id.is_some()
            );
            state
                .inner
                .wechat_qr_sessions
                .lock()
                .expect("wechat qr sessions poisoned")
                .remove(session_id);
            let config_dir = active_profile_config_dir(state, scope);
            let allow_users = user_id.into_iter().collect::<Vec<_>>();
            psychevo::config::upsert_channel_connection(psychevo::config::ChannelSetupInput {
                config_dir: config_dir.clone(),
                id: session.id.clone(),
                channel: "wechat".to_string(),
                label: session.label.clone(),
                credential_env: Some("WECHAT_BOT_TOKEN".to_string()),
                credential: Some(token),
                account_env: Some("WECHAT_ACCOUNT_ID".to_string()),
                account_id: Some(account_id),
                base_url_env: Some("WECHAT_ILINK_BASE_URL".to_string()),
                base_url: Some(base_url.clone()),
                allow_users,
                allow_groups: Vec::new(),
            })?;
            protect_channel_env_file(&config_dir.join(".env"))?;
            if params.enable.unwrap_or(true) {
                set_channel_enabled(config_dir, &session.id, true)?;
            }
            state.inner.channel_runtime.restart(&session.id);
            state
                .inner
                .channel_runtime
                .start_wechat_login_grace(&session.id);
            eprintln!(
                "wechat qr credentials persisted: id={} base_url={} enabled={}",
                session.id,
                base_url,
                params.enable.unwrap_or(true)
            );
            channel_runtime::reconcile(state.clone());
            let shown = channel_show_result(state, scope, &session.id)?;
            Ok(wire::channels::ChannelWechatQrPollResult {
                done: true,
                status: "qr_login_pending".to_string(),
                message: "WeChat credentials saved. Gateway is starting polling.".to_string(),
                channel: Some(shown.channel),
                expires_at_ms: None,
            })
        }
    }
}

pub(super) fn channel_list_result_from_value(
    state: &WebState,
    value: Value,
) -> psychevo::Result<wire::channels::ChannelListResult> {
    let rows = value
        .get("channels")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let rows: Vec<RuntimeChannelConfigRow> = serde_json::from_value(rows)?;
    Ok(wire::channels::ChannelListResult {
        channels: rows
            .into_iter()
            .map(|row| channel_config_view_from_runtime(state, row))
            .collect::<psychevo::Result<Vec<_>>>()?,
    })
}

fn channel_doctor_result_from_value(
    state: &WebState,
    value: Value,
) -> psychevo::Result<wire::channels::ChannelDoctorResult> {
    let live = value.get("live").and_then(Value::as_bool).unwrap_or(false);
    let rows = value
        .get("channels")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let rows: Vec<RuntimeChannelDoctorRow> = serde_json::from_value(rows)?;
    Ok(wire::channels::ChannelDoctorResult {
        live,
        channels: rows
            .into_iter()
            .map(|row| wire::channels::ChannelDoctorChannelView {
                runner: state.inner.channel_runtime.runner_view(&row.id),
                id: row.id,
                channel: row.channel,
                enabled: row.enabled,
                runtime_status: row.runtime_status,
                checks: row
                    .checks
                    .into_iter()
                    .map(|check| wire::channels::ChannelDoctorCheck {
                        name: check.name,
                        status: check.status,
                        message: check.message,
                    })
                    .collect(),
            })
            .collect(),
    })
}

async fn enrich_wechat_live_doctor(
    configuration: &Configuration,
    id: Option<&str>,
    result: &mut wire::channels::ChannelDoctorResult,
) -> psychevo::Result<()> {
    let connections = configuration.channel_runtime_connections()?;
    let client = reqwest::Client::new();
    for connection in connections
        .into_iter()
        .filter(|connection| connection.channel == "wechat")
        .filter(|connection| id.is_none_or(|id| connection.id == id))
    {
        let Some(row) = result
            .channels
            .iter_mut()
            .find(|row| row.id == connection.id)
        else {
            continue;
        };
        row.checks.retain(|check| check.name != "live");
        let check = match connection.credential.as_deref() {
            None | Some("") => wire::channels::ChannelDoctorCheck {
                name: "live".to_string(),
                status: "fail".to_string(),
                message: "WeChat token env is missing; run QR setup".to_string(),
            },
            Some(token) => {
                let base_url = connection
                    .base_url
                    .as_deref()
                    .unwrap_or(WECHAT_ILINK_BASE_URL);
                match check_wechat_ilink_health(&client, base_url, token, 3).await {
                    Ok(health) if health.ok => wire::channels::ChannelDoctorCheck {
                        name: "live".to_string(),
                        status: "ok".to_string(),
                        message: "iLink getupdates accepted the current token".to_string(),
                    },
                    Ok(health) => wire::channels::ChannelDoctorCheck {
                        name: "live".to_string(),
                        status: "fail".to_string(),
                        message: if health.reason.as_deref() == Some("needs_qr_login") {
                            "iLink reports this WeChat login is expired; reconnect with QR"
                                .to_string()
                        } else {
                            format!(
                                "iLink getupdates failed: {}",
                                health.message.as_deref().unwrap_or("unknown error")
                            )
                        },
                    },
                    Err(err) => wire::channels::ChannelDoctorCheck {
                        name: "live".to_string(),
                        status: "fail".to_string(),
                        message: format!(
                            "iLink getupdates request failed: {}",
                            channel_runtime::redact_channel_error(&err.to_string())
                        ),
                    },
                }
            }
        };
        row.checks.push(check);
    }
    Ok(())
}

fn channel_config_view_from_runtime(
    state: &WebState,
    row: RuntimeChannelConfigRow,
) -> psychevo::Result<wire::channels::ChannelConfigView> {
    let runner = state.inner.channel_runtime.runner_view(&row.id);
    Ok(wire::channels::ChannelConfigView {
        id: row.id,
        channel: row.channel,
        domain: row.domain,
        enabled: row.enabled,
        label: row.label,
        transport: row.transport,
        cwd: row.cwd,
        runtime_ref: row.runtime_ref,
        model: row.model,
        permission_mode: row.permission_mode,
        require_mention: row.require_mention,
        credential: wire::channels::ChannelCredentialView {
            env: row.credential.env,
            status: row.credential.status,
        },
        account: row
            .account
            .map(|credential| wire::channels::ChannelCredentialView {
                env: credential.env,
                status: credential.status,
            }),
        base_url: row
            .base_url
            .map(|credential| wire::channels::ChannelCredentialView {
                env: credential.env,
                status: credential.status,
            }),
        app_id: row
            .app_id
            .map(|credential| wire::channels::ChannelCredentialView {
                env: credential.env,
                status: credential.status,
            }),
        allowlist: wire::channels::ChannelAllowlistView {
            users: row.allowlist.users,
            groups: row.allowlist.groups,
            status: row.allowlist.status,
        },
        runtime_status: row.runtime_status,
        runner,
    })
}

fn protect_channel_env_file(path: &Path) -> psychevo::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
