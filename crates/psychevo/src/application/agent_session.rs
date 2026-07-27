use super::*;

impl fmt::Debug for AgentTurnRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentTurnRequest")
            .field("thread", &self.thread)
            .field("receipt", &self.receipt)
            .field("input", &self.input)
            .finish_non_exhaustive()
    }
}

impl AgentTurnRequest {
    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __take_native_control(&mut self) -> Result<crate::types::RunControl> {
        self.native_control.take().ok_or_else(|| {
            Error::Message("Agent Session Adapter is missing its Turn control".to_string())
        })
    }
}

impl fmt::Debug for NativeAgentSessionAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeAgentSessionAdapter")
            .field("config_path", &self.config_path)
            .finish_non_exhaustive()
    }
}

impl AgentSessionAdapter for NativeAgentSessionAdapter {
    fn run_turn(&self, mut request: AgentTurnRequest) -> BoxFuture<'static, Result<TurnResult>> {
        let state = self.state.clone();
        let application_config_path = self.config_path.clone();
        let provider = self.provider.clone();
        Box::pin(async move {
            let source = request.input.source.clone();
            let turn_id = request.receipt.turn_id.clone();
            let stream_events = request.events.clone();
            let run_stream_observer = request.input.adapter_options.run_stream_observer.take();
            let stream: RunStreamSink = Arc::new(move |event| {
                if let Some(observer) = run_stream_observer.as_ref() {
                    observer(event.clone());
                }
                if let Some(event) = TurnEvent::from_run_stream(event) {
                    stream_events.emit(event);
                }
            });
            let options = request.input.into_run_options(
                state.clone(),
                PathBuf::from(request.thread.cwd.clone()),
                request.receipt.thread_id,
                application_config_path,
            );
            let control = request.native_control.take().ok_or_else(|| {
                Error::Message("Native Agent Session is missing its Turn control".to_string())
            })?;
            state.confirm_gateway_turn_delivery(&turn_id).await?;
            match provider {
                Some(provider) => {
                    run_live_streaming_controlled_with_provider(
                        options,
                        &source,
                        &[source.as_str()],
                        stream,
                        control,
                        provider,
                    )
                    .await
                }
                None => {
                    run_live_streaming_controlled(
                        options,
                        &source,
                        &[source.as_str()],
                        stream,
                        control,
                    )
                    .await
                }
            }
            .map(TurnResult::from)
        })
    }
}
