#[derive(Clone)]
struct GatewayTurnLifecycle {
    inner: Arc<Mutex<GatewayTurnLifecycleState>>,
}

struct GatewayTurnLifecycleState {
    turn_id: String,
    thread_id: Option<String>,
    sink: Option<GatewayEventSink>,
    started: bool,
    completed: bool,
}

impl GatewayTurnLifecycle {
    fn new(
        turn_id: String,
        thread_id: Option<String>,
        sink: Option<GatewayEventSink>,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(GatewayTurnLifecycleState {
                turn_id,
                thread_id,
                sink,
                started: false,
                completed: false,
            })),
        }
    }

    fn start(&self) {
        let event = {
            let state = self.inner.lock().expect("gateway turn lifecycle poisoned");
            GatewayEvent::TurnStarted {
                thread_id: state.thread_id.clone(),
                turn_id: state.turn_id.clone(),
                selected_skills: Vec::new(),
            }
        };
        self.emit(event);
    }

    fn sink(&self) -> GatewayEventSink {
        let lifecycle = self.clone();
        Arc::new(move |event| lifecycle.emit(event))
    }

    fn emit(&self, event: GatewayEvent) {
        let sink = {
            let mut state = self.inner.lock().expect("gateway turn lifecycle poisoned");
            match &event {
                GatewayEvent::TurnStarted { turn_id, .. } if turn_id == &state.turn_id => {
                    if state.started || state.completed {
                        return;
                    }
                    state.started = true;
                }
                GatewayEvent::TurnCompleted { turn_id, .. } if turn_id == &state.turn_id => {
                    if state.completed {
                        return;
                    }
                    state.completed = true;
                }
                _ => {}
            }
            state.sink.clone()
        };
        if let Some(sink) = sink {
            sink(event);
        }
    }
}
