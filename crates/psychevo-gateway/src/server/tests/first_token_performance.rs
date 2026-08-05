use std::collections::BTreeMap;
use std::sync::Arc;

use uuid::Uuid;

use crate::composition::GatewayApplication;
use crate::server::GatewayWebServerConfig;
use crate::server::binding::WebState;
use crate::server::tests::automations::helpers::AutomationTurnProbe;
use psychevo_gateway_protocol::source::GatewayInputPart;

async fn run_profiled_framework_turn(
    state: &WebState,
    caller: crate::gateway::activity::ThreadCallerContext,
    mut intent: crate::gateway::activity::ThreadTurnIntent,
) {
    let mut start = psychevo::StartThreadRequest::new(&caller.cwd);
    start.source = caller.runtime_source.clone();
    let thread = state
        .inner
        .framework
        .start_thread(start)
        .await
        .expect("profile Thread");
    intent.thread_id = Some(thread.id().to_string());
    intent.turn_id = Some(Uuid::now_v7().to_string());
    let submission = intent
        .into_framework_request(caller)
        .expect("profile Framework request");
    let handle = thread
        .start_turn(submission.request)
        .await
        .expect("accept profile Turn");
    handle.wait().await.expect("profile Turn");
}

#[cfg(unix)]
#[tokio::test]
async fn initialized_gui_first_token_overhead_stays_close_to_direct_gateway_dispatch() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    let budgets: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../non-functional-budgets.json"
    )))
    .expect("non-functional budgets");
    let max_overhead_ms = budgets
        .pointer("/gateway/maximum/initializedGuiOverheadMs")
        .and_then(serde_json::Value::as_u64)
        .expect("initialized GUI overhead budget");
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    let script = temp.path().join("fake-codex.py");
    let log = temp.path().join("broker.log");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        &script,
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codex_app_server.py"
        )),
    )
    .expect("script");
    std::fs::write(
        script.with_extension("json"),
        serde_json::to_vec(&serde_json::json!({
            "scenario": "first_token_performance",
            "log": &log,
        }))
        .expect("fixture config json"),
    )
    .expect("fixture config");
    let mut permissions = std::fs::metadata(&script)
        .expect("script metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).expect("chmod");
    std::fs::write(
        home.join("config.toml"),
        format!(
            "[codex_plugins]\nenabled = true\nbinary = {:?}\n",
            script.display().to_string()
        ),
    )
    .expect("config");

    let backend = Arc::new(AutomationTurnProbe::default());
    let env = BTreeMap::from([
        (
            "HOME".to_string(),
            temp.path().to_string_lossy().to_string(),
        ),
        (
            "PSYCHEVO_HOME".to_string(),
            home.to_string_lossy().to_string(),
        ),
        (
            "PSYCHEVO_CODEX_BIN".to_string(),
            script.to_string_lossy().to_string(),
        ),
        (
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        ),
    ]);
    let runtime = GatewayApplication::open_with_native_test_executor(
        home,
        temp.path().join("state.db"),
        None,
        env,
        backend.executor(),
    )
    .await
    .expect("test composition");
    let state = WebState::new(GatewayWebServerConfig::with_static(
        runtime,
        cwd.clone(),
        temp.path().join("static"),
    ));
    state
        .inner
        .codex_capability_broker
        .prepare_runtime_inventory(&cwd)
        .await
        .expect("initialized inventory");

    let direct_turn = |state: &WebState| {
        state.thread_turn_request(
            cwd.clone(),
            None,
            vec![GatewayInputPart::Text {
                text: "say hi".to_string(),
            }],
        )
    };
    let (caller, intent) = direct_turn(&state);
    run_profiled_framework_turn(&state, caller, intent).await;
    let warm_thread = "gui-warmup";
    let warm_contributions = state
        .inner
        .codex_capability_broker
        .runtime_contributions(
            state.clone(),
            &cwd,
            warm_thread,
            Some("warmup-turn".to_string()),
            None,
        )
        .await
        .expect("GUI warmup contributions");
    let warm_lease = warm_contributions.lease_id.clone();
    let (mut warm_caller, mut warm_intent) = direct_turn(&state);
    warm_intent
        .policy
        .selected_capability_roots
        .extend(warm_contributions.capability_roots);
    warm_caller.extend_runtime_tools(warm_contributions.runtime_tools);
    run_profiled_framework_turn(&state, warm_caller, warm_intent).await;
    if let Some(lease_id) = warm_lease.as_deref() {
        state
            .inner
            .codex_capability_broker
            .release_turn_lease(lease_id)
            .await;
    }

    let mut direct_samples = Vec::new();
    let mut gui_samples = Vec::new();
    let mut direct_create_to_result_samples = Vec::new();
    let mut gui_create_to_result_samples = Vec::new();
    let mut gui_threads = Vec::new();
    for sample in 0..9 {
        let started = Instant::now();
        let (caller, intent) = direct_turn(&state);
        run_profiled_framework_turn(&state, caller, intent).await;
        let completed = started.elapsed();
        let dispatched = backend
            .dispatch_times
            .lock()
            .expect("dispatch times")
            .last()
            .copied()
            .expect("direct provider dispatch")
            .duration_since(started);
        direct_samples.push(dispatched);
        direct_create_to_result_samples.push(completed);

        let psychevo_thread_id = format!("gui-thread-{sample}");
        gui_threads.push(psychevo_thread_id.clone());
        let started = Instant::now();
        let contributions = state
            .inner
            .codex_capability_broker
            .runtime_contributions(
                state.clone(),
                &cwd,
                &psychevo_thread_id,
                Some(format!("gui-turn-{sample}")),
                None,
            )
            .await
            .expect("GUI runtime contributions");
        let lease_id = contributions.lease_id.clone();
        let (mut caller, mut intent) = direct_turn(&state);
        intent
            .policy
            .selected_capability_roots
            .extend(contributions.capability_roots);
        caller.extend_runtime_tools(contributions.runtime_tools);
        run_profiled_framework_turn(&state, caller, intent).await;
        if let Some(lease_id) = lease_id.as_deref() {
            state
                .inner
                .codex_capability_broker
                .release_turn_lease(lease_id)
                .await;
        }
        let completed = started.elapsed();
        let dispatched = backend
            .dispatch_times
            .lock()
            .expect("dispatch times")
            .last()
            .copied()
            .expect("GUI provider dispatch")
            .duration_since(started);
        gui_samples.push(dispatched);
        gui_create_to_result_samples.push(completed);
    }

    direct_samples.sort_unstable();
    gui_samples.sort_unstable();
    direct_create_to_result_samples.sort_unstable();
    gui_create_to_result_samples.sort_unstable();
    let direct_median = direct_samples[direct_samples.len() / 2];
    let gui_median = gui_samples[gui_samples.len() / 2];
    let extra = gui_median.saturating_sub(direct_median);
    let direct_create_to_result_median =
        direct_create_to_result_samples[direct_create_to_result_samples.len() / 2];
    let gui_create_to_result_median =
        gui_create_to_result_samples[gui_create_to_result_samples.len() / 2];
    let create_to_result_extra =
        gui_create_to_result_median.saturating_sub(direct_create_to_result_median);
    if let Some(root) = std::env::var_os("PSYCHEVO_CI_ARTIFACT_ROOT") {
        let output = std::path::PathBuf::from(root).join("non-functional");
        std::fs::create_dir_all(&output).expect("non-functional evidence directory");
        let micros = |duration: Duration| u64::try_from(duration.as_micros()).unwrap_or(u64::MAX);
        let report = serde_json::json!({
            "schemaVersion": 1,
            "scope": "gateway-first-result",
            "maximumOverheadMs": max_overhead_ms,
            "directDispatchMicros": direct_samples.iter().copied().map(micros).collect::<Vec<_>>(),
            "guiDispatchMicros": gui_samples.iter().copied().map(micros).collect::<Vec<_>>(),
            "directCreateToResultMicros": direct_create_to_result_samples.iter().copied().map(micros).collect::<Vec<_>>(),
            "guiCreateToResultMicros": gui_create_to_result_samples.iter().copied().map(micros).collect::<Vec<_>>(),
            "median": {
                "directDispatchMicros": micros(direct_median),
                "guiDispatchMicros": micros(gui_median),
                "dispatchOverheadMicros": micros(extra),
                "directCreateToResultMicros": micros(direct_create_to_result_median),
                "guiCreateToResultMicros": micros(gui_create_to_result_median),
                "createToResultOverheadMicros": micros(create_to_result_extra),
            }
        });
        std::fs::write(
            output.join("gateway-first-result.json"),
            serde_json::to_vec_pretty(&report).expect("serialize first-result evidence"),
        )
        .expect("write first-result evidence");
    }
    assert!(
        extra <= Duration::from_millis(max_overhead_ms),
        "initialized GUI pre-provider overhead {extra:?} exceeded {max_overhead_ms}ms; direct median {direct_median:?}, GUI median {gui_median:?}"
    );
    assert!(
        create_to_result_extra <= Duration::from_millis(max_overhead_ms),
        "GUI create-to-first-result overhead {create_to_result_extra:?} exceeded {max_overhead_ms}ms; direct median {direct_create_to_result_median:?}, GUI median {gui_create_to_result_median:?}"
    );
    let broker_log = std::fs::read_to_string(&log).expect("broker log");
    assert_eq!(broker_log.matches("plugin-installed\n").count(), 1);
    assert_eq!(broker_log.matches("mcp-status\n").count(), 0);
    assert!(
        !broker_log.contains("plugin-list"),
        "provider dispatch must not enumerate the marketplace catalog: {broker_log}"
    );

    state
        .inner
        .codex_capability_broker
        .archive_ephemeral_thread(warm_thread)
        .await;
    for thread_id in gui_threads {
        state
            .inner
            .codex_capability_broker
            .archive_ephemeral_thread(&thread_id)
            .await;
    }
    state.inner.codex_capability_broker.stop().await;
}
