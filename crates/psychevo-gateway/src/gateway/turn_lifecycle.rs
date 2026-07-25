#[derive(Clone)]
struct GatewayTurnLifecycle {
    inner: Arc<Mutex<GatewayTurnLifecycleState>>,
}

struct GatewayTurnLifecycleState {
    turn_id: String,
    thread_id: Option<String>,
    sink: Option<GatewayEventEmitter>,
    delivery_error: Option<GatewayEventEmitError>,
    started: bool,
    completed: bool,
}

impl GatewayTurnLifecycle {
    fn new(
        turn_id: String,
        thread_id: Option<String>,
        sink: Option<GatewayEventEmitter>,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(GatewayTurnLifecycleState {
                turn_id,
                thread_id,
                sink,
                delivery_error: None,
                started: false,
                completed: false,
            })),
        }
    }

    fn start(&self) -> Result<(), GatewayEventEmitError> {
        let event = {
            let state = self.inner.lock().expect("gateway turn lifecycle poisoned");
            GatewayEvent::TurnStarted {
                thread_id: state.thread_id.clone(),
                turn_id: state.turn_id.clone(),
                selected_skills: Vec::new(),
            }
        };
        self.emit(event)
    }

    fn sink(&self) -> GatewayEventEmitter {
        let lifecycle = self.clone();
        GatewayEventEmitter::try_new(move |event| lifecycle.emit(event))
    }

    fn emit(&self, event: GatewayEvent) -> Result<(), GatewayEventEmitError> {
        let sink = {
            let mut state = self.inner.lock().expect("gateway turn lifecycle poisoned");
            match &event {
                GatewayEvent::TurnStarted { turn_id, .. } if turn_id == &state.turn_id => {
                    if state.started || state.completed {
                        return Ok(());
                    }
                    state.started = true;
                }
                GatewayEvent::TurnCompleted { turn_id, .. } if turn_id == &state.turn_id => {
                    // Adapter terminals are projection fences only. The
                    // application-owned async terminal path is the sole owner
                    // of durable terminal admission and flush.
                    return Ok(());
                }
                _ => {}
            }
            state.sink.clone()
        };
        let result = match sink {
            Some(sink) => sink.emit(event),
            None => Ok(()),
        };
        if let Err(error) = &result {
            let mut state = self.inner.lock().expect("gateway turn lifecycle poisoned");
            state.delivery_error.get_or_insert_with(|| error.clone());
        }
        result
    }

    fn delivery_error(&self) -> Option<GatewayEventEmitError> {
        self.inner
            .lock()
            .expect("gateway turn lifecycle poisoned")
            .delivery_error
            .clone()
    }

    async fn complete(&self, event: GatewayEvent) -> Result<(), GatewayEventEmitError> {
        let sink = {
            let mut state = self.inner.lock().expect("gateway turn lifecycle poisoned");
            if state.completed {
                return state.delivery_error.clone().map_or(Ok(()), Err);
            }
            state.completed = true;
            state.sink.clone()
        };
        let terminal_result = match sink {
            Some(sink) => sink.emit_wait(event).await,
            None => Ok(()),
        };
        if let Err(error) = &terminal_result {
            let mut state = self.inner.lock().expect("gateway turn lifecycle poisoned");
            state.delivery_error.get_or_insert_with(|| error.clone());
        }
        self.delivery_error().map_or(terminal_result, Err)
    }
}

#[cfg(test)]
mod turn_lifecycle_tests {
    use super::*;

    #[test]
    fn lifecycle_sink_propagates_and_remembers_ingress_rejection() {
        let rejected = GatewayEventEmitter::try_new(|_| {
            Err(GatewayEventEmitError::new("durability ingress rejected"))
        });
        let lifecycle = GatewayTurnLifecycle::new(
            "turn-1".to_string(),
            Some("thread-1".to_string()),
            Some(rejected),
        );

        let error = lifecycle
            .sink()
            .emit(GatewayEvent::Warning {
                kind: "test".to_string(),
                message: "test".to_string(),
                source_path: None,
                suggestion: None,
            })
            .expect_err("lifecycle must propagate sink rejection");

        assert_eq!(error.to_string(), "durability ingress rejected");
        assert_eq!(
            lifecycle
                .delivery_error()
                .expect("remembered lifecycle error")
                .to_string(),
            "durability ingress rejected"
        );
    }
}
