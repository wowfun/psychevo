use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use psychevo_gateway::im::ImAdapter;
use psychevo_gateway::im::adapters::{
    WECHAT_ILINK_BASE_URL, WechatIlinkAdapter, WechatIlinkConfig, WechatQrCode, WechatQrPoll,
    fetch_wechat_qr_code, poll_wechat_qr_code,
};
use psychevo_runtime::{
    ChannelSetupInput, channel_doctor_value, channel_list_value, channel_summary_value,
    set_channel_enabled, setup_channel_connection, upsert_channel_connection,
};
use serde_json::{Value, json};

use crate::args::GatewaySetupArgs;
use crate::commands::common::{print_json_error, read_secret_from_stdin};
use crate::commands::serve::resolve_static_dir_diagnostic;

use super::context::GatewayContext;
use super::managed::{ManagedBindPolicy, ensure_started, managed_status, stop_managed};
use super::output::{print_json, workbench_dist_missing};

pub(super) async fn gateway_setup(args: GatewaySetupArgs) -> Result<ExitCode> {
    match gateway_setup_inner(args.clone()).await {
        Ok(code) => Ok(code),
        Err(err) if args.json => {
            print_json_error(&err)?;
            Ok(ExitCode::from(1))
        }
        Err(err) => Err(err),
    }
}

async fn gateway_setup_inner(mut args: GatewaySetupArgs) -> Result<ExitCode> {
    let ctx = GatewayContext::load_for_setup()?;
    if args.channel.is_none() {
        args = gateway_setup_wizard(&ctx)?;
    }
    let channel = args
        .channel
        .clone()
        .ok_or_else(|| anyhow!("pevo gateway setup requires --channel in non-interactive mode"))?;
    let id = args.id.clone().unwrap_or_else(|| channel.clone());
    let mut credential = read_secret_from_stdin(args.credential_stdin)?;
    let mut account_id = args.account_id.clone();
    let mut ilink_base_url = args.ilink_base_url.clone();
    if args.qr {
        if channel != "wechat" {
            return Err(anyhow!("--qr is only supported for --channel wechat"));
        }
        let qr = run_wechat_qr_login(
            args.ilink_base_url
                .as_deref()
                .unwrap_or(WECHAT_ILINK_BASE_URL),
        )
        .await?;
        credential = Some(qr.token);
        account_id = Some(qr.account_id);
        ilink_base_url = Some(qr.base_url);
        if args.allow_users.is_empty() {
            if let Some(user_id) = qr.user_id {
                eprintln!("Adding WeChat QR login user to allowlist: {user_id}");
                args.allow_users.push(user_id);
            } else if io::stdin().is_terminal()
                && prompt_yes_no_default(
                    "No WeChat user id was returned. Pair first direct-message sender now? [Y/n]: ",
                    true,
                )?
                && let Some(user_id) = discover_wechat_dm_sender(
                    credential.as_deref().unwrap_or_default(),
                    account_id.as_deref().unwrap_or_default(),
                    ilink_base_url.as_deref().unwrap_or(WECHAT_ILINK_BASE_URL),
                    &id,
                )
                .await?
            {
                args.allow_users.push(user_id);
            }
        }
    }
    let setup_input = ChannelSetupInput {
        config_dir: ctx.home.clone(),
        id: id.clone(),
        channel: channel.clone(),
        label: args.label.clone(),
        credential_env: args.credential_env.clone(),
        credential,
        account_env: args.account_env.clone(),
        account_id,
        base_url_env: matches!(args.channel.as_deref(), Some("wechat"))
            .then(|| "WECHAT_ILINK_BASE_URL".to_string()),
        base_url: ilink_base_url,
        allow_users: args.allow_users.clone(),
        allow_groups: args.allow_groups.clone(),
    };
    let setup = if args.qr && channel == "wechat" {
        upsert_channel_connection(setup_input)?
    } else {
        setup_channel_connection(setup_input)?
    };
    if setup
        .get("wrote_env")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || args.credential_stdin
        || args.qr
    {
        crate::profiles::protect_env_file(&ctx.home.join(".env"))?;
    }
    let enabled = if args.enable || args.disable {
        Some(set_channel_enabled(ctx.home.clone(), &id, args.enable)?)
    } else {
        None
    };
    let options = ctx.run_options(ctx.cwd.clone())?;
    let doctor = channel_doctor_value(&options, Some(&id), false)?;
    let summary = channel_summary_value(&options)?;
    let gateway = setup_gateway_action(&ctx, &args).await?;
    let output = json!({
        "ok": true,
        "channel": setup,
        "enabled": enabled,
        "doctor": doctor,
        "summary": summary,
        "gateway": gateway,
        "profile": ctx.profile_name,
        "profileHome": ctx.home,
    });
    if args.json {
        print_json(output)
    } else {
        print_gateway_setup_human(&output);
        Ok(ExitCode::SUCCESS)
    }
}

fn gateway_setup_wizard(ctx: &GatewayContext) -> Result<GatewaySetupArgs> {
    if !io::stdin().is_terminal() {
        return Err(anyhow!(
            "pevo gateway setup requires --channel in non-interactive mode"
        ));
    }
    let options = ctx.run_options(ctx.cwd.clone())?;
    let existing = channel_list_value(&options)?;
    println!("Configured channels:");
    if existing
        .get("channels")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
    {
        println!("  none");
    } else {
        for channel in existing
            .get("channels")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            println!(
                "  {}  {}  {}",
                channel.get("id").and_then(Value::as_str).unwrap_or("?"),
                channel
                    .get("channel")
                    .and_then(Value::as_str)
                    .unwrap_or("?"),
                channel
                    .get("runtime_status")
                    .and_then(Value::as_str)
                    .unwrap_or("?")
            );
        }
    }
    let channel = prompt_line("Channel (wechat, telegram, feishu, lark): ")?;
    if channel.trim().is_empty() {
        return Err(anyhow!("channel is required"));
    }
    let id = prompt_line("Connection id [same as channel]: ")?;
    let label = prompt_line("Label [default]: ")?;
    let use_wechat_qr =
        channel.trim() == "wechat" && prompt_yes_no_default("Use WeChat QR login? [Y/n]: ", true)?;
    let credential_env = if use_wechat_qr {
        String::new()
    } else {
        prompt_line("Credential env [default]: ")?
    };
    let allow_user = prompt_line("Allowed user id [optional]: ")?;
    let allow_group = prompt_line("Allowed group/chat id [optional]: ")?;
    let enable = prompt_yes_no("Enable after setup? [y/N]: ")?;
    Ok(GatewaySetupArgs {
        channel: Some(channel.trim().to_string()),
        id: non_empty(id),
        label: non_empty(label),
        credential_env: non_empty(credential_env),
        credential_stdin: false,
        qr: use_wechat_qr,
        account_id: None,
        account_env: None,
        ilink_base_url: None,
        allow_users: non_empty(allow_user).into_iter().collect(),
        allow_groups: non_empty(allow_group).into_iter().collect(),
        enable,
        disable: !enable,
        start: false,
        restart: false,
        json: false,
    })
}

fn prompt_line(prompt: &str) -> Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    Ok(value.trim().to_string())
}

fn prompt_yes_no(prompt: &str) -> Result<bool> {
    Ok(matches!(
        prompt_line(prompt)?.to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn prompt_yes_no_default(prompt: &str, default: bool) -> Result<bool> {
    let value = prompt_line(prompt)?;
    if value.trim().is_empty() {
        return Ok(default);
    }
    Ok(matches!(value.to_ascii_lowercase().as_str(), "y" | "yes"))
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

#[derive(Debug)]
struct WechatQrLoginCredential {
    token: String,
    account_id: String,
    base_url: String,
    user_id: Option<String>,
}

async fn run_wechat_qr_login(base_url: &str) -> Result<WechatQrLoginCredential> {
    let client = reqwest::Client::new();
    let mut qr = request_and_print_wechat_qr(&client, base_url).await?;
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut refresh_count = 0;
    loop {
        if Instant::now() >= deadline {
            return Err(anyhow!("WeChat QR login timed out"));
        }
        match poll_wechat_qr_code(&client, &qr.base_url, &qr.qrcode).await? {
            WechatQrPoll::Confirmed {
                account_id,
                token,
                base_url,
                user_id,
            } => {
                eprintln!("WeChat connected: account_id={account_id}");
                return Ok(WechatQrLoginCredential {
                    token,
                    account_id,
                    base_url,
                    user_id,
                });
            }
            WechatQrPoll::Expired { message } => {
                refresh_count += 1;
                if refresh_count > 3 {
                    return Err(anyhow!("{message}"));
                }
                eprintln!("{message}; refreshing QR ({refresh_count}/3).");
                qr = request_and_print_wechat_qr(&client, base_url).await?;
            }
            WechatQrPoll::Waiting {
                status,
                message,
                base_url,
            } => {
                if base_url != qr.base_url {
                    qr.base_url = base_url;
                }
                if status == "scaned" || status == "scaned_but_redirect" {
                    eprintln!("{message}");
                } else {
                    eprint!(".");
                    io::stderr().flush()?;
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn request_and_print_wechat_qr(
    client: &reqwest::Client,
    base_url: &str,
) -> Result<WechatQrCode> {
    let qr = fetch_wechat_qr_code(client, base_url).await?;
    eprintln!("Scan this QR with WeChat to connect Psychevo:");
    eprintln!("{}", qr.qr_url);
    if qr.qr_image.is_some() {
        eprintln!(
            "iLink returned an image QR payload; use Workbench if your terminal cannot render the data URL."
        );
    } else if let Some(rendered) = render_wechat_terminal_qr(&qr.qr_url) {
        eprintln!("{rendered}");
    }
    Ok(qr)
}

fn render_wechat_terminal_qr(value: &str) -> Option<String> {
    let code = qrcode::QrCode::new(value.as_bytes()).ok()?;
    Some(
        code.render::<qrcode::render::unicode::Dense1x2>()
            .quiet_zone(true)
            .build(),
    )
}

async fn discover_wechat_dm_sender(
    token: &str,
    account_id: &str,
    base_url: &str,
    connection_id: &str,
) -> Result<Option<String>> {
    eprintln!("Ask the WeChat user to send one direct message to the iLink bot now.");
    let adapter = WechatIlinkAdapter::new(WechatIlinkConfig {
        connection_id: Some(connection_id.to_string()),
        token: token.to_string(),
        account_id: account_id.to_string(),
        base_url: base_url.to_string(),
        timeout_secs: 5,
        context_store_path: None,
    })
    .map_err(|err| anyhow!(err.to_string()))?;
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        let messages = adapter
            .poll()
            .await
            .map_err(|err| anyhow!(err.to_string()))?;
        if let Some(user_id) = messages
            .into_iter()
            .find(|message| message.identity.chat_type.as_deref() == Some("dm"))
            .and_then(|message| message.identity.user_id)
        {
            eprintln!("Adding WeChat direct-message sender to allowlist: {user_id}");
            return Ok(Some(user_id));
        }
        eprint!(".");
        io::stderr().flush()?;
    }
    eprintln!("No WeChat direct message was received before pairing timed out.");
    Ok(None)
}

async fn setup_gateway_action(ctx: &GatewayContext, args: &GatewaySetupArgs) -> Result<Value> {
    if !args.start && !args.restart {
        return managed_status(&ctx.paths);
    }
    if args.restart {
        let _ = stop_managed(&ctx.paths)?;
    }
    let static_dir = resolve_static_dir_diagnostic(None, &ctx.env_map, &ctx.cwd)?;
    if !static_dir.found() {
        return Ok(workbench_dist_missing(&static_dir));
    }
    let state =
        ensure_started(ctx, ManagedBindPolicy::new(None), static_dir.path.as_path()).await?;
    Ok(json!({
        "ok": true,
        "running": true,
        "pid": state.pid,
        "baseUrl": state.base_url,
        "readyzUrl": state.readyz_url,
        "startedAtMs": state.started_at_ms,
        "restarted": args.restart,
    }))
}

fn print_gateway_setup_human(output: &Value) {
    let channel = output
        .get("channel")
        .and_then(|value| value.get("channel"))
        .and_then(Value::as_str)
        .unwrap_or("channel");
    let id = output
        .get("channel")
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("?");
    println!("Configured {channel} channel `{id}`.");
    if let Some(summary) = output.get("summary") {
        println!(
            "Channels: configured={} enabled={} ready={} blocked={}",
            summary
                .get("configured")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            summary.get("enabled").and_then(Value::as_u64).unwrap_or(0),
            summary.get("ready").and_then(Value::as_u64).unwrap_or(0),
            summary.get("blocked").and_then(Value::as_u64).unwrap_or(0)
        );
    }
    println!("Run `pevo gateway start` or open Settings > Channels to continue.");
}
