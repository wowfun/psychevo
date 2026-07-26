#[derive(Clone)]
pub struct GatewayAgentSessionAdapter {
    gateway: Gateway,
}

impl GatewayAgentSessionAdapter {
    pub fn new(gateway: Gateway) -> Self {
        Self { gateway }
    }
}

impl fmt::Debug for GatewayAgentSessionAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayAgentSessionAdapter")
            .finish_non_exhaustive()
    }
}

impl psychevo::AgentSessionAdapter for GatewayAgentSessionAdapter {
    fn run_turn(
        &self,
        mut request: psychevo::AgentTurnRequest,
    ) -> BoxFuture<'static, psychevo::Result<psychevo::TurnResult>> {
        let gateway = self.gateway.clone();
        Box::pin(async move {
            let control = request.__take_native_control()?;
            let receipt = request.receipt.clone();
            let thread = request.thread.clone();
            let events = request.events.clone();
            let prompt = request.input.prompt.clone();
            let image_inputs = request.input.image_inputs.clone();
            let runtime_source = request.input.source.clone();
            let adapter_input = request.input.__take_adapter_input_parts();
            let run_stream_observer = request.input.__take_run_stream_observer();
            let initial_thread_preferences =
                request.input.__take_initial_thread_preferences();
            let prepared_source_key = request.input.__take_prepared_source_key();
            let mut options = request.input.__into_run_options(
                gateway.state.clone(),
                PathBuf::from(&thread.cwd),
                receipt.thread_id.clone(),
                None,
            );
            let input = if adapter_input.is_empty() {
                framework_gateway_input(prompt, image_inputs)
            } else {
                adapter_input
                    .into_iter()
                    .map(serde_json::from_value)
                    .collect::<Result<Vec<GatewayInputPart>, _>>()?
            };
            let stream: RunStreamSink = Arc::new(move |event| {
                if let Some(observer) = run_stream_observer.as_ref() {
                    observer(event.clone());
                }
                events.__emit_run_stream(event);
            });

            let bound_target =
                resolve_bound_gateway_agent_target(&options, options.runtime_ref.as_deref()).await?;
            let (profile, profile_revision, profile_fingerprint) =
                match bound_target.as_ref() {
                    Some(target) => (
                        target.profile.clone(),
                        target.revision,
                        target.fingerprint.clone(),
                    ),
                    None => resolve_gateway_runtime_profile(&options).await?,
                };
            options.runtime_ref = Some(profile.id.clone());
            let existing_binding = match bound_target.as_ref() {
                Some(target) => Some(target.binding.clone()),
                None => gateway
                    .state
                    .gateway_runtime_binding(&receipt.thread_id)
                    .await?,
            };
            let agent_binding = resolve_gateway_agent_binding_snapshot(
                &options,
                &profile,
                existing_binding.as_ref(),
                AgentEntrypoint::Peer,
            )?;
            options.agent = agent_binding.agent_ref.clone();
            let mut binding = ensure_gateway_runtime_binding(
                &gateway.state,
                &receipt.thread_id,
                &agent_binding,
                &profile,
                profile_revision,
                &profile_fingerprint,
            )
            .await?;
            if existing_binding.is_none() && !initial_thread_preferences.is_empty() {
                let preferences = initial_thread_preferences
                    .iter()
                    .map(|(control_id, value)| {
                        (control_id.clone(), Value::String(value.clone()))
                    })
                    .collect::<BTreeMap<_, _>>();
                binding = gateway
                    .state
                    .compare_and_set_gateway_runtime_control_state(
                        &binding.thread_id,
                        binding.binding_revision,
                        binding.control_revision,
                        GatewayRuntimeControlStatePatch {
                            thread_preferences: Some(&preferences),
                            runtime_observed: None,
                        },
                    )
                    .await?;
            }
            if existing_binding.is_none()
                && profile.runtime == RuntimeProfileKind::Acp
                && let Some(source_key) = prepared_source_key.as_deref()
                && let Some(native_session_id) = gateway
                    .agent_sessions
                    .promote_prepared(
                        source_key,
                        binding.agent_ref.as_deref(),
                        &profile.id,
                        &profile_fingerprint,
                        &binding.thread_id,
                    )
                    .await?
            {
                gateway
                    .state
                    .attach_gateway_runtime_native_session(
                        &binding.thread_id,
                        binding.binding_revision,
                        &native_session_id,
                    )
                    .await?;
                options.runtime_session_id = Some(native_session_id);
                binding = gateway
                    .state
                    .gateway_runtime_binding(&binding.thread_id)
                    .await?
                    .ok_or_else(|| {
                        Error::Message(
                            "promoted Agent Session binding disappeared".to_string(),
                        )
                    })?;
            }
            let peer = if let Some(target) = bound_target {
                target.peer
            } else if profile.runtime == RuntimeProfileKind::Acp {
                let mut peer_options = options.clone();
                peer_options.runtime_ref = profile.backend_ref.clone();
                resolve_peer_turn(&peer_options)?
            } else {
                None
            };
            if peer.is_none() {
                clear_acp_peer_usage_update(&gateway.state, &receipt.thread_id).await?;
                options.external_agent_delegate = Some(Arc::new(GatewayExternalAgentDelegate {
                    gateway: gateway.clone(),
                    base_options: options.clone(),
                    stream: Some(stream.clone()),
                    event_sink: None,
                }));
            }
            let attached = gateway
                .agent_sessions
                .attach(CapturedAgentSessionTarget::bound(
                    &binding,
                    profile.clone(),
                    peer,
                )?)?;
            let session_ready = (profile.runtime == RuntimeProfileKind::Acp)
                .then(|| acp_session_ready_for_binding(gateway.state.clone(), binding));
            let output = attached
                .run_turn(
                    BackendTurnRequest {
                        options,
                        input,
                        runtime_source: runtime_source.clone(),
                        continue_sources: vec![runtime_source],
                        stream: Some(stream),
                        control: Some(control),
                    },
                    receipt.turn_id.clone(),
                    session_ready,
                )
                .await?;
            let diagnostics = gateway.state.diagnostics();
            gateway_profile_mark(
                "framework_adapter_completed",
                Some(&receipt.turn_id),
                Some(&receipt.thread_id),
                GatewayProfileFields {
                    state_in_flight: Some(diagnostics.in_flight_operations),
                    state_pool_idle: Some(diagnostics.pool_idle),
                    state_pool_size: Some(diagnostics.pool_size),
                    ..GatewayProfileFields::default()
                },
            );
            Ok(psychevo::TurnResult::from(output.run))
        })
    }

    fn shutdown(&self, force: bool) -> BoxFuture<'static, psychevo::Result<()>> {
        let gateway = self.gateway.clone();
        Box::pin(async move { gateway.shutdown_application(force).await })
    }
}

fn framework_gateway_input(
    prompt: String,
    image_inputs: Vec<ImageInput>,
) -> Vec<GatewayInputPart> {
    let mut input = Vec::with_capacity(1 + image_inputs.len());
    if !prompt.is_empty() {
        input.push(GatewayInputPart::Text { text: prompt });
    }
    input.extend(image_inputs.into_iter().map(|image| {
        GatewayInputPart::Image {
            input: match image {
                ImageInput::LocalPath(path) => GatewayImageInput::LocalPath {
                    path: path.display().to_string(),
                },
                ImageInput::ImageUrl(url) => GatewayImageInput::Url { url },
            },
        }
    }));
    input
}
