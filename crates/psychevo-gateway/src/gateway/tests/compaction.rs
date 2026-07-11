#[derive(Debug, Clone, Default)]
struct DirectCompactionRuntime {
    requests: Arc<Mutex<Vec<psychevo_runtime_host::RuntimeCompactionRequest>>>,
    fail: bool,
}

impl psychevo_runtime_host::RuntimeModule for DirectCompactionRuntime {
    fn snapshot(
        &self,
        _query: psychevo_runtime_host::SnapshotQuery,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::RuntimeSnapshot> {
        Box::pin(async {
            Err(psychevo_runtime_host::RuntimeError::new(
                "unsupported",
                psychevo_runtime_host::RuntimeErrorStage::Discovery,
                psychevo_runtime_host::RetryClass::UserAction,
                "snapshot is outside this direct compaction test",
            ))
        })
    }

    fn execute(
        &self,
        request: psychevo_runtime_host::ExecuteRequest,
        observer: psychevo_runtime_host::RuntimeObserver,
        _control: psychevo_runtime_host::RuntimeControl,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::ExecuteResult> {
        let requests = Arc::clone(&self.requests);
        let fail = self.fail;
        Box::pin(async move {
            let psychevo_runtime_host::RuntimeIntent::Compaction(compaction) = request.intent else {
                return Err(psychevo_runtime_host::RuntimeError::new(
                    "unsupported",
                    psychevo_runtime_host::RuntimeErrorStage::Control,
                    psychevo_runtime_host::RetryClass::UserAction,
                    "test runtime accepts only compaction",
                ));
            };
            requests
                .lock()
                .expect("direct compaction requests poisoned")
                .push(compaction.clone());
            observer.emit(
                psychevo_runtime_host::RuntimeObservation::CompactionChanged(
                    psychevo_runtime_host::RuntimeCompactionChange {
                        runtime_ref: request.profile.id.clone(),
                        thread_id: compaction.thread_id.clone(),
                        turn_id: None,
                        item_id: Some("native-compaction-item-secret".to_string()),
                        status: psychevo_runtime_host::RuntimeCompactionStatus::Started,
                    },
                ),
            );
            if fail {
                return Err(psychevo_runtime_host::RuntimeError::new(
                    "codex_protocol_eof",
                    psychevo_runtime_host::RuntimeErrorStage::Transport,
                    psychevo_runtime_host::RetryClass::Reconnect,
                    "Codex exited before native compaction completed",
                ));
            }
            observer.emit(
                psychevo_runtime_host::RuntimeObservation::CompactionChanged(
                    psychevo_runtime_host::RuntimeCompactionChange {
                        runtime_ref: request.profile.id,
                        thread_id: compaction.thread_id.clone(),
                        turn_id: None,
                        item_id: Some("native-compaction-item-secret".to_string()),
                        status: psychevo_runtime_host::RuntimeCompactionStatus::Completed,
                    },
                ),
            );
            Ok(psychevo_runtime_host::ExecuteResult::Compaction(
                psychevo_runtime_host::RuntimeCompactionResult {
                    thread_id: compaction.thread_id,
                    native_session_id: compaction.native_session_id,
                    item_id: "native-compaction-item-secret".to_string(),
                    compacted: true,
                    process_epoch: 7,
                },
            ))
        })
    }

    fn shutdown(
        &self,
        _mode: psychevo_runtime_host::ShutdownMode,
    ) -> psychevo_runtime_host::RuntimeFuture<()> {
        Box::pin(async { Ok(()) })
    }
}

fn bind_direct_codex_compaction_thread(
    state: &StateRuntime,
    cwd: &Path,
) -> (String, psychevo_runtime::GatewayRuntimeBindingRecord) {
    let thread_id = state
        .store()
        .create_session_with_metadata(cwd, "web", "fake-codex", "codex", None)
        .expect("session");
    state
        .store()
        .append_message(
            &thread_id,
            &Message::User {
                content: vec![UserContentBlock::text("native context")],
                timestamp_ms: 1,
            },
        )
        .expect("message");
    let profile = generated_gateway_runtime_profiles()
        .into_iter()
        .find(|profile| profile.id == "codex")
        .expect("generated Codex profile");
    let fingerprint = runtime_profile_config_fingerprint(&profile);
    let revision = runtime_profile_config_revision(&fingerprint);
    let binding = ensure_gateway_runtime_binding(
        state,
        &thread_id,
        &profile,
        revision,
        &fingerprint,
    )
    .expect("runtime binding");
    let binding = state
        .store()
        .attach_gateway_runtime_native_session(
            &thread_id,
            binding.binding_revision,
            "codex-native-compaction-thread",
        )
        .expect("native session binding");
    (thread_id, binding)
}

#[tokio::test]
async fn direct_codex_compaction_uses_immutable_binding_and_persists_projection_only_marker() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let (thread_id, _) = bind_direct_codex_compaction_thread(&state, &cwd);
    let runtime = DirectCompactionRuntime::default();
    let requests = Arc::clone(&runtime.requests);
    let host = RuntimeHost::new();
    host.register(RuntimeKind::Codex, Arc::new(runtime));
    let gateway = Gateway::with_backend_and_runtime_host(
        state.clone(),
        Arc::new(FakeBackend::default()),
        host,
    );

    let result = gateway
        .compact_session(SendCompactRequest {
            thread_id: Some(thread_id.clone()),
            source: None,
            // Client assertions are not execution identity. The immutable
            // binding must still select direct Codex instead of local mirror compaction.
            runtime_ref: Some("native".to_string()),
            cwd: temp.path().join("forged-cwd"),
            config_path: None,
            model: None,
            reasoning_effort: None,
            instructions: None,
            force: true,
            reason: psychevo_runtime::CompactionReason::Manual,
            inherited_env: None,
            event_sink: None,
        })
        .await
        .expect("direct Codex compaction");

    assert!(result.compacted);
    assert_eq!(result.summary_provider.as_deref(), Some("codex"));
    let captured = requests.lock().expect("direct compaction requests");
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].thread_id, thread_id);
    assert_eq!(
        captured[0].native_session_id,
        "codex-native-compaction-thread"
    );
    assert_eq!(captured[0].cwd, cwd.canonicalize().expect("canonical cwd"));
    drop(captured);

    let checkpoints = state
        .store()
        .list_valid_session_compactions(&thread_id)
        .expect("projection markers");
    assert_eq!(checkpoints.len(), 1);
    assert_eq!(checkpoints[0].summary_provider, "codex");
    assert_eq!(checkpoints[0].summary_model, "runtime-owned");
    assert_eq!(checkpoints[0].metadata.as_ref().unwrap()["projection_only"], true);
    assert!(
        state
            .store()
            .latest_valid_session_compaction(&thread_id)
            .expect("context checkpoint")
            .is_none(),
        "projection-only native marker must never drive local context assembly"
    );
    let serialized = format!("{result:?}{checkpoints:?}");
    assert!(!serialized.contains("native-compaction-item-secret"));
    assert!(!serialized.contains("codex-native-compaction-thread"));
}

#[tokio::test]
async fn direct_codex_compaction_rejects_custom_instructions_before_adapter_and_eof_has_no_marker() {
    for (runtime_ref, instructions, fail, expected) in [
        (
            None,
            Some("write a custom summary".to_string()),
            false,
            "does not accept custom instructions",
        ),
        (
            None,
            None,
            true,
            "exited before native compaction completed",
        ),
        (
            Some("opencode".to_string()),
            None,
            false,
            "bound to Runtime Profile `codex`",
        ),
    ] {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
        let (thread_id, _) = bind_direct_codex_compaction_thread(&state, &cwd);
        let runtime = DirectCompactionRuntime {
            fail,
            ..DirectCompactionRuntime::default()
        };
        let requests = Arc::clone(&runtime.requests);
        let host = RuntimeHost::new();
        host.register(RuntimeKind::Codex, Arc::new(runtime));
        let gateway = Gateway::with_backend_and_runtime_host(
            state.clone(),
            Arc::new(FakeBackend::default()),
            host,
        );
        let error = gateway
            .compact_session(SendCompactRequest {
                thread_id: Some(thread_id.clone()),
                source: None,
                runtime_ref,
                cwd,
                config_path: None,
                model: None,
                reasoning_effort: None,
                instructions,
                force: true,
                reason: psychevo_runtime::CompactionReason::Manual,
                inherited_env: None,
                event_sink: None,
            })
            .await
            .expect_err("direct compaction must fail closed");
        assert!(error.to_string().contains(expected), "{error}");
        assert_eq!(requests.lock().expect("requests").len(), usize::from(fail));
        assert!(
            state
                .store()
                .list_valid_session_compactions(&thread_id)
                .expect("projection markers")
                .is_empty()
        );
    }
}

#[tokio::test]
async fn compact_session_rejects_omitted_or_forged_native_identity_for_peer_binding() {
    let backend = Arc::new(FakeBackend::default());
    let harness = harness(backend);
    let source = GatewaySource::new("web", "peer-compaction").persistent();
    let thread_id = harness
        .state
        .store()
        .create_session_with_metadata(
            &harness.cwd,
            "web",
            "fake-model",
            "fake-provider",
            None,
        )
        .expect("session");
    harness
        .gateway
        .bind_source_thread(
            &source,
            &thread_id,
            &GatewayBackendInfo {
                kind: BackendKind::PeerAgent,
                runtime_ref: Some("opencode".to_string()),
                native_id: Some("peer-native-1".to_string()),
            },
            Some(json!({"runtimeRef": "opencode"})),
        )
        .expect("peer binding");
    let unrelated_scope_source =
        GatewaySource::new("web", "unrelated-compaction-scope").persistent();

    for runtime_ref in [None, Some("native".to_string())] {
        let result = harness
            .gateway
            .compact_session(SendCompactRequest {
                thread_id: Some(thread_id.clone()),
                source: Some(unrelated_scope_source.clone()),
                runtime_ref,
                cwd: harness.cwd.clone(),
                config_path: None,
                model: None,
                reasoning_effort: None,
                instructions: None,
                force: true,
                reason: psychevo_runtime::CompactionReason::Manual,
                inherited_env: None,
                event_sink: None,
            })
            .await
            .expect("bounded unavailable result");

        assert!(!result.compacted);
        assert!(result.message.contains("unavailable"), "{result:?}");
        assert!(result.message.contains("opencode"), "{result:?}");
    }
    assert!(
        harness
            .state
            .store()
            .list_valid_session_compactions(&thread_id)
            .expect("checkpoints")
            .is_empty()
    );
}

#[test]
fn thread_transcript_merges_checkpoint_at_session_sequence_boundary_despite_clock_skew() {
    let backend = Arc::new(FakeBackend::default());
    let harness = harness(backend);
    let thread_id = harness
        .state
        .store()
        .create_session_with_metadata(
            &harness.cwd,
            "web",
            "fake-model",
            "fake-provider",
            None,
        )
        .expect("session");
    let store = harness.state.store();
    store
        .append_message(
            &thread_id,
            &Message::User {
                content: vec![UserContentBlock::text("first")],
                timestamp_ms: 9_000_000_000_000,
            },
        )
        .expect("first message");
    store
        .append_message(
            &thread_id,
            &Message::User {
                content: vec![UserContentBlock::text("second")],
                timestamp_ms: 1,
            },
        )
        .expect("second message");
    let checkpoint = store
        .append_session_compaction(psychevo_runtime::SessionCompactionInput {
            session_id: thread_id.clone(),
            reason: "manual".to_string(),
            summary_text: "summary".to_string(),
            first_kept_session_seq: 2,
            created_after_session_seq: 1,
            tokens_before: Some(100),
            tokens_after: Some(40),
            summary_provider: "fake-provider".to_string(),
            summary_model: "fake-model".to_string(),
            instructions: None,
            metadata: None,
        })
        .expect("checkpoint");
    store
        .upsert_gateway_turn_terminal(psychevo_runtime::GatewayTurnTerminalInput {
            turn_id: "failed-turn",
            thread_id: &thread_id,
            status: "failed",
            outcome: Some("failed"),
            error_message: Some("failed after seq 2"),
            started_at_ms: Some(0),
            completed_at_ms: 0,
            metadata: Some(json!({
                "firstCommittedSeq": 2,
                "lastCommittedSeq": 2,
            })),
        })
        .expect("terminal");

    let entries = harness
        .gateway
        .thread_transcript(&thread_id)
        .expect("transcript");

    assert_eq!(
        entries
            .iter()
            .map(|entry| (entry.message_seq, entry.id.clone()))
            .collect::<Vec<_>>(),
        vec![
            (Some(1), "message:1".to_string()),
            (None, format!("compaction:{}", checkpoint.id)),
            (Some(2), "message:2".to_string()),
            (None, "turn:failed-turn:terminal".to_string()),
        ]
    );
}

#[tokio::test]
async fn auto_compaction_projects_only_new_checkpoint_in_live_completion_with_transient_status() {
    use std::io::{Read as _, Write as _};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("summary server");
    listener.set_nonblocking(true).expect("nonblocking server");
    let summary_base_url = format!("http://{}/v1", listener.local_addr().expect("server addr"));
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        return false;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(err) => panic!("summary accept failed: {err}"),
            }
        };
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .expect("server read timeout");
        let mut request = [0u8; 32 * 1024];
        let _ = stream.read(&mut request).expect("summary request");
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"new compacted summary\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).expect("summary response");
        true
    });

    let backend = Arc::new(FakeBackend::default());
    backend.set_context_snapshot(psychevo_runtime::ContextSnapshot {
        event_type: "context_snapshot".to_string(),
        scope: psychevo_runtime::ContextScope::LastProviderRequest,
        status: "estimated".to_string(),
        session_id: None,
        provider: "mock".to_string(),
        model: "summary".to_string(),
        mode: Some("default".to_string()),
        context_limit: Some(10),
        tokenizer: psychevo_runtime::ContextTokenizer {
            encoding: "o200k_base".to_string(),
            source: "test".to_string(),
            fallback: true,
        },
        total: psychevo_runtime::ContextTotal {
            tokens: 9,
            estimated_tokens: 9,
            estimated: true,
            source: "test".to_string(),
            percent: Some(90.0),
        },
        categories: BTreeMap::new(),
        advice: Vec::new(),
    });
    let harness = harness(backend);
    let home = harness._temp.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(
        home.join("config.toml"),
        format!(
            r#"model = "mock/summary"

[provider.mock]
api = "{summary_base_url}"
no_auth = true

[provider.mock.models.summary]

[provider.mock.models.summary.limit]
context = 10

[compression]
threshold_percent = 10
reserve_tokens = 1
keep_recent_tokens = 1
model = "mock/summary"
"#
        ),
    )
    .expect("config");
    let thread_id = harness
        .state
        .store()
        .create_session_with_metadata(&harness.cwd, "web", "summary", "mock", None)
        .expect("session");
    let store = harness.state.store();
    for (index, text) in [
        "old user context one with enough words",
        "old assistant context two with enough words",
        "new user context three with enough words",
        "new assistant context four with enough words",
        "latest user task five with enough words",
    ]
    .into_iter()
    .enumerate()
    {
        store
            .append_message(
                &thread_id,
                &Message::User {
                    content: vec![UserContentBlock::text(text)],
                    timestamp_ms: index as i64 + 1,
                },
            )
            .expect("message");
    }
    let old_checkpoint = store
        .append_session_compaction(psychevo_runtime::SessionCompactionInput {
            session_id: thread_id.clone(),
            reason: "manual".to_string(),
            summary_text: "old compacted summary".to_string(),
            first_kept_session_seq: 3,
            created_after_session_seq: 2,
            tokens_before: Some(40),
            tokens_after: Some(20),
            summary_provider: "mock".to_string(),
            summary_model: "summary".to_string(),
            instructions: None,
            metadata: None,
        })
        .expect("old checkpoint");
    let events = Arc::new(Mutex::new(Vec::<GatewayEvent>::new()));
    let event_log = Arc::clone(&events);
    let mut turn_request = request(
        &harness,
        GatewaySource::new("web", "auto-compaction").persistent(),
        "continue",
    );
    turn_request.thread_id = Some(thread_id.clone());
    turn_request.options.session = Some(thread_id.clone());
    turn_request.options.model = Some("mock/summary".to_string());
    turn_request.options.inherited_env = Some(BTreeMap::from([
        ("HOME".to_string(), home.display().to_string()),
        ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
    ]));
    turn_request.event_sink = Some(Arc::new(move |event| {
        event_log.lock().expect("event log poisoned").push(event);
    }));

    let turn = harness
        .gateway
        .send_turn(turn_request)
        .await
        .expect("turn with auto compaction");
    let summary_requested = server.join().expect("summary server join");
    let terminal = store
        .gateway_turn_terminal(&turn.turn.id)
        .expect("terminal lookup")
        .expect("persisted terminal");
    assert_eq!(
        terminal
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("lastCommittedSeq"))
            .and_then(Value::as_i64),
        Some(5)
    );

    let checkpoint_entries = turn
        .committed_entries
        .iter()
        .filter(|entry| {
            entry
                .blocks
                .iter()
                .any(|block| block.kind == TranscriptBlockKind::Compaction)
        })
        .collect::<Vec<_>>();
    assert!(summary_requested, "events={:#?}", events.lock().expect("event log poisoned"));
    assert_eq!(checkpoint_entries.len(), 1, "{:#?}", turn.committed_entries);
    assert_ne!(
        checkpoint_entries[0].id,
        format!("compaction:{}", old_checkpoint.id)
    );
    let events = events.lock().expect("event log poisoned");
    assert!(events.iter().any(|event| matches!(
        event,
        GatewayEvent::EntryStarted { entry, .. }
            if entry.blocks.iter().any(|block|
                block.kind == TranscriptBlockKind::Status
                    && block.title.as_deref() == Some("Summarizing thread")
                    && block.status == TranscriptBlockStatus::Running)
    )), "{events:#?}");
    assert!(events.iter().any(|event| matches!(
        event,
        GatewayEvent::EntryCompleted { entry, .. }
            if entry.blocks.iter().any(|block|
                block.title.as_deref() == Some("Summarizing thread")
                    && block.status == TranscriptBlockStatus::Completed)
    )), "{events:#?}");
    assert!(events.iter().any(|event| matches!(
        event,
        GatewayEvent::TurnCompleted { committed_entries, .. }
            if committed_entries.iter().filter(|entry|
                entry.blocks.iter().any(|block| block.kind == TranscriptBlockKind::Compaction)
            ).count() == 1
    )), "{events:#?}");
}
