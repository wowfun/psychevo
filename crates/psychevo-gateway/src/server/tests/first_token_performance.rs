#[cfg(unix)]
#[tokio::test]
async fn initialized_gui_first_token_overhead_stays_close_to_direct_gateway_dispatch() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");
    let script = temp.path().join("fake-codex.py");
    let log = temp.path().join("broker.log");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(home.join("config.toml"), "# isolated test profile\n").expect("config");
    std::fs::write(
        &script,
        format!(
            r#"#!/usr/bin/env python3
import json, sys, time
LOG = {log}
thread_count = 0
for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    if method == "initialize":
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{"codexHome":"/fake","platformFamily":"unix","platformOs":"linux","userAgent":"fake"}}}}), flush=True)
    elif method == "initialized":
        pass
    elif method == "plugin/installed":
        with open(LOG, "a", encoding="utf-8") as handle:
            handle.write("plugin-installed\n")
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{"marketplaces":[{{"name":"openai","path":None,"plugins":[{{"id":"review@openai","name":"review","installed":True,"enabled":True}}]}}],"marketplaceLoadErrors":[]}}}}), flush=True)
    elif method == "plugin/read":
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{"plugin":{{"summary":{{"id":"review@openai","name":"review","installed":True,"enabled":True}},"skills":[],"hooks":[],"apps":[{{"id":"review-app"}}],"mcpServers":[]}}}}}}), flush=True)
    elif method == "plugin/list":
        time.sleep(2)
        with open(LOG, "a", encoding="utf-8") as handle:
            handle.write("plugin-list\n")
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{"marketplaces":[],"marketplaceLoadErrors":[],"featuredPluginIds":[]}}}}), flush=True)
    elif method == "thread/start":
        thread_count += 1
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{"thread":{{"id":"codex-thread-" + str(thread_count)}}}}}}), flush=True)
    elif method == "mcpServerStatus/list":
        with open(LOG, "a", encoding="utf-8") as handle:
            handle.write("mcp-status\n")
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{"data":[{{"name":"codex_apps","tools":{{"review":{{"description":"Review app","inputSchema":{{"type":"object","properties":{{}}}}}}}}}}],"nextCursor":None}}}}), flush=True)
    elif method == "thread/archive":
        print(json.dumps({{"jsonrpc":"2.0","id":msg["id"],"result":{{}}}}), flush=True)
"#,
            log = serde_json::to_string(&log).expect("log json"),
        ),
    )
    .expect("script");
    let mut permissions = std::fs::metadata(&script)
        .expect("script metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).expect("chmod");

    let backend = Arc::new(AutomationFakeBackend::default());
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
    let runtime = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let gateway = Gateway::with_backend(runtime, backend.clone());
    let state = WebState::new(GatewayWebServerConfig::new(
        gateway,
        home,
        cwd.clone(),
        None,
        env,
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
    state
        .inner
        .gateway
        .run_turn(direct_turn(&state))
        .await
        .expect("direct warmup");
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
    let mut warm_request = direct_turn(&state);
    warm_request
        .policy
        .selected_capability_roots
        .extend(warm_contributions.capability_roots);
    warm_request.extend_runtime_tools(warm_contributions.runtime_tools);
    state
        .inner
        .gateway
        .run_turn(warm_request)
        .await
        .expect("GUI warmup");

    let mut direct_samples = Vec::new();
    let mut gui_samples = Vec::new();
    let mut direct_create_to_result_samples = Vec::new();
    let mut gui_create_to_result_samples = Vec::new();
    let mut gui_threads = Vec::new();
    for sample in 0..9 {
        let started = Instant::now();
        state
            .inner
            .gateway
            .run_turn(direct_turn(&state))
            .await
            .expect("direct fake-provider turn");
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
        let mut request = direct_turn(&state);
        request
            .policy
            .selected_capability_roots
            .extend(contributions.capability_roots);
        request.extend_runtime_tools(contributions.runtime_tools);
        state
            .inner
            .gateway
            .run_turn(request)
            .await
            .expect("GUI fake-provider turn");
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
    assert!(
        extra <= Duration::from_millis(150),
        "initialized GUI pre-provider overhead {extra:?} exceeded 150ms; direct median {direct_median:?}, GUI median {gui_median:?}"
    );
    let direct_create_to_result_median =
        direct_create_to_result_samples[direct_create_to_result_samples.len() / 2];
    let gui_create_to_result_median =
        gui_create_to_result_samples[gui_create_to_result_samples.len() / 2];
    let create_to_result_extra =
        gui_create_to_result_median.saturating_sub(direct_create_to_result_median);
    assert!(
        create_to_result_extra <= Duration::from_millis(150),
        "GUI create-to-first-result overhead {create_to_result_extra:?} exceeded 150ms; direct median {direct_create_to_result_median:?}, GUI median {gui_create_to_result_median:?}"
    );
    let broker_log = std::fs::read_to_string(&log).expect("broker log");
    assert_eq!(broker_log.matches("plugin-installed\n").count(), 1);
    assert_eq!(broker_log.matches("mcp-status\n").count(), 10);
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
