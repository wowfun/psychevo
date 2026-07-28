    fn rpc_request(method: &str, params: Value) -> RpcRequest {
        RpcRequest {
            jsonrpc: wire::JSONRPC_VERSION.to_string(),
            id: Some(json!(1)),
            method: method.to_string(),
            params: Some(params),
        }
    }

    fn write_minimal_home_config(state: &WebState) {
        std::fs::create_dir_all(&state.inner.home).expect("home");
        std::fs::write(state.inner.home.join("config.toml"), "# config\n").expect("config");
    }

    #[derive(Debug)]
    struct AcceptanceRealtimeAdapter {
        accepted: Arc<std::sync::atomic::AtomicBool>,
    }

    impl psychevo::__ai::RealtimeAdapter for AcceptanceRealtimeAdapter {
        fn connect(
            &self,
            _call: psychevo::__ai::AdapterCall<psychevo::__ai::RealtimeConnectRequest>,
        ) -> psychevo::__ai::AdapterFuture<'_, psychevo::__ai::RealtimeAdapterTransport> {
            let accepted = Arc::clone(&self.accepted);
            Box::pin(async move {
                Ok(psychevo::__ai::RealtimeAdapterTransport {
                    commands: Arc::new(AcceptanceRealtimeSink { accepted }),
                    events: Box::pin(futures::stream::pending()),
                })
            })
        }
    }

    struct AcceptanceRealtimeSink {
        accepted: Arc<std::sync::atomic::AtomicBool>,
    }

    impl psychevo::__ai::RealtimeCommandSink for AcceptanceRealtimeSink {
        fn send(
            &self,
            _command: psychevo::__ai::RealtimeCommand,
        ) -> psychevo::__ai::AdapterFuture<'_, ()> {
            self.accepted
                .store(true, std::sync::atomic::Ordering::Release);
            Box::pin(async { Ok(()) })
        }

        fn close(&self) -> psychevo::__ai::AdapterFuture<'_, ()> {
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn voice_fake_asr_and_tts_rpc_use_deterministic_providers() {
        let (_temp, state) = web_state().await;
        write_minimal_home_config(&state);
        let (out_tx, _out_rx) = mpsc::unbounded_channel();
        let asr = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            out_tx.clone(),
            rpc_request(
                "voice/asr/transcribe",
                json!({
                    "provider": "fake",
                    "model": "fake-asr",
                    "audio": {
                        "data": "UklGRg==",
                        "format": "wav",
                        "mimeType": "audio/wav"
                    }
                }),
            ),
        )
        .await
        .expect("asr");
        assert_eq!(asr["transcript"], "fake transcript");
        assert_eq!(asr["provider"], "fake");
        assert_eq!(asr["model"], "fake-asr");

        let tts = handle_rpc(
            state,
            AuthContext::Bearer,
            out_tx,
            rpc_request(
                "voice/tts/synthesize",
                json!({
                    "provider": "fake",
                    "model": "fake-tts",
                    "voice": "fake",
                    "format": "wav",
                    "text": "hello"
                }),
            ),
        )
        .await
        .expect("tts");
        assert_eq!(tts["provider"], "fake");
        assert_eq!(tts["model"], "fake-tts");
        assert_eq!(tts["audio"]["data"], "UklGRg==");
        assert_eq!(tts["audio"]["format"], "wav");
    }

    #[tokio::test]
    async fn voice_asr_rejects_declared_pcm16_even_with_wav_mime() {
        let (_temp, state) = web_state().await;
        write_minimal_home_config(&state);
        let (out_tx, _out_rx) = mpsc::unbounded_channel();

        let error = handle_rpc(
            state,
            AuthContext::Bearer,
            out_tx,
            rpc_request(
                "voice/asr/transcribe",
                json!({
                    "provider": "fake",
                    "model": "fake-asr",
                    "audio": {
                        "data": "UklGRg==",
                        "format": "pcm16",
                        "mimeType": "audio/wav"
                    }
                }),
            ),
        )
        .await
        .expect_err("pcm16 is not an ASR input format");

        assert!(
            error.to_string().contains("unsupported ASR audio format `pcm16`"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn voice_asr_rejects_format_and_mime_conflict() {
        let (_temp, state) = web_state().await;
        write_minimal_home_config(&state);
        let (out_tx, _out_rx) = mpsc::unbounded_channel();

        let error = handle_rpc(
            state,
            AuthContext::Bearer,
            out_tx,
            rpc_request(
                "voice/asr/transcribe",
                json!({
                    "provider": "fake",
                    "model": "fake-asr",
                    "audio": {
                        "data": "UklGRg==",
                        "format": "wav",
                        "mimeType": "audio/mpeg"
                    }
                }),
            ),
        )
        .await
        .expect_err("declared format and MIME must agree");

        assert!(
            error
                .to_string()
                .contains("ASR audio format `wav` conflicts with MIME type `audio/mpeg`"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn voice_policy_rpc_round_trips_source_policy() {
        let (_temp, state) = web_state().await;
        let (out_tx, _out_rx) = mpsc::unbounded_channel();
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
        let source_key = scope.source.source_key().0;

        let updated = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            out_tx.clone(),
            rpc_request(
                "voice/policy/update",
                json!({
                    "sourceKey": source_key,
                    "mode": "voiceOnly"
                }),
            ),
        )
        .await
        .expect("policy update");
        assert_eq!(updated["mode"], "voiceOnly");

        let read = handle_rpc(
            state,
            AuthContext::Bearer,
            out_tx,
            rpc_request(
                "voice/policy/read",
                json!({
                    "sourceKey": source_key
                }),
            ),
        )
        .await
        .expect("policy read");
        assert_eq!(read["mode"], "voiceOnly");
        assert_eq!(read["target"], source_key);
    }

    #[tokio::test]
    async fn model_settings_read_includes_voice_status_without_credentials() {
        let (_temp, state) = web_state().await;
        write_minimal_home_config(&state);
        let (out_tx, _out_rx) = mpsc::unbounded_channel();
        let value = handle_rpc(
            state,
            AuthContext::Bearer,
            out_tx,
            rpc_request(
                "model/settings/read",
                json!({
                    "scope": "global"
                }),
            ),
        )
        .await
        .expect("model settings");
        assert_eq!(value["voice"]["asr"]["provider"], "xiaomi-token-plan");
        assert_eq!(value["voice"]["asr"]["credentialStatus"], "missing");
        assert_eq!(value["voice"]["tts"]["provider"], "xiaomi-token-plan");
        assert_eq!(value["voice"]["realtime"], Value::Null);
    }

    #[tokio::test]
    async fn voice_fake_realtime_starts_and_stops_session() {
        let (_temp, state) = web_state().await;
        write_minimal_home_config(&state);
        let scope = default_resolved_scope(&state, &AuthContext::Bearer).expect("scope");
        let thread_id = state
            .inner
            .state

            .create_session(&scope.cwd)
            .await.expect("session");
        let (out_tx, mut out_rx) = mpsc::unbounded_channel();
        let started = handle_rpc(
            state.clone(),
            AuthContext::Bearer,
            out_tx.clone(),
            rpc_request(
                "thread/realtime/start",
                json!({
                    "threadId": thread_id,
                    "provider": "fake",
                    "model": "fake-realtime",
                    "transport": "webrtc",
                    "outputModality": "audio"
                }),
            ),
        )
        .await
        .expect("realtime start");
        let session_id = started["sessionId"].as_str().expect("session id").to_string();
        assert_eq!(started["accepted"], true);

        let notification = out_rx.recv().await.expect("started notification");
        let notification: Value = serde_json::from_str(&notification).expect("notification json");
        assert_eq!(notification["method"], "thread/realtime/started");
        assert_eq!(notification["params"]["sessionId"], session_id);

        let stopped = handle_rpc(
            state,
            AuthContext::Bearer,
            out_tx,
            rpc_request(
                "thread/realtime/stop",
                json!({
                    "sessionId": session_id
                }),
            ),
        )
        .await
        .expect("realtime stop");
        assert_eq!(stopped["accepted"], true);
    }

    #[tokio::test]
    async fn realtime_append_response_waits_for_adapter_command_acceptance() {
        let (_temp, state) = web_state().await;
        let accepted = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let provider = psychevo::__ai::Provider::builder(
            psychevo::__ai::DeploymentConfig::new("acceptance", "test", "test://realtime"),
        )
        .realtime_adapter(AcceptanceRealtimeAdapter {
            accepted: Arc::clone(&accepted),
        })
        .build()
        .expect("provider");
        let mut session = provider
            .realtime_model("test")
            .expect("model")
            .connect(psychevo::__ai::RealtimeConnectRequest {
                instructions: None,
                voice: None,
                headers: BTreeMap::new(),
                extensions: BTreeMap::new(),
            })
            .await
            .expect("session");
        state
            .inner
            .realtime_sessions
            .lock()
            .expect("sessions")
            .insert(
                "acceptance-session".to_string(),
                RealtimeSessionState {
                    provider: "test".to_string(),
                    sender: session.sender(),
                },
            );
        let (out_tx, _out_rx) = mpsc::unbounded_channel();

        let result = handle_rpc(
            state,
            AuthContext::Bearer,
            out_tx,
            rpc_request(
                "thread/realtime/appendText",
                json!({
                    "sessionId": "acceptance-session",
                    "text": "queued"
                }),
            ),
        )
        .await
        .expect("append");

        assert_eq!(result["accepted"], true);
        assert!(
            accepted.load(std::sync::atomic::Ordering::Acquire),
            "accepted response must follow Adapter command acceptance"
        );
        session.abort();
        let _ = session.next_event().await;
    }
