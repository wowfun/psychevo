use super::adapters::build_channel_gateway;
use super::runner::run_channel_loop;
use super::*;

pub(in crate::server) fn reconcile(state: WebState) {
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
    if !channel_runtime_enabled(&state.inner.inherited_env) {
        state
            .inner
            .channel_runtime
            .reconcile_active(&std::collections::BTreeSet::new());
        return Ok(());
    }
    let options = state.run_options(state.inner.cwd.clone(), None);
    let connections = channel_runtime_connections(&options, &state.inner.cwd)?;
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
