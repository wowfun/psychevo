use super::*;

impl ShutdownReport {
    pub fn is_clean(&self) -> bool {
        matches!(self.adapter, ShutdownAdapterStatus::Completed)
            && self.task_panics == 0
            && self.aborted_tasks == 0
            && self.pending_terminal_failures.is_empty()
    }

    pub fn require_clean(self) -> Result<Self> {
        if self.is_clean() {
            return Ok(self);
        }
        let details = serde_json::to_string(&self).unwrap_or_else(|_| format!("{self:?}"));
        Err(Error::Message(format!(
            "Application shutdown was not clean: {details}"
        )))
    }
}

impl fmt::Debug for Application {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Application")
            .field("home", &self.inner.home)
            .field("event_capacity", &self.inner.event_capacity)
            .finish_non_exhaustive()
    }
}

impl Application {
    pub fn builder() -> ApplicationBuilder {
        ApplicationBuilder::default()
    }

    pub fn client(&self) -> Client {
        Client {
            inner: Arc::clone(&self.inner),
        }
    }

    pub fn agent_control(&self) -> crate::agents::AgentControl {
        crate::agents::AgentControl::new(
            self.inner.runtime.agent_supervisor.clone(),
            Some(self.inner.state.clone()),
        )
    }

    pub async fn shutdown(&self) -> Result<ShutdownReport> {
        self.shutdown_inner(false).await
    }

    pub async fn shutdown_force(&self) -> Result<ShutdownReport> {
        self.shutdown_inner(true).await
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __from_open_state(
        home: PathBuf,
        config_path: Option<PathBuf>,
        state: StateRuntime,
        agent_sessions: Arc<dyn AgentSessionAdapter>,
    ) -> Self {
        Self {
            inner: Arc::new(ApplicationInner {
                state,
                agent_sessions,
                home,
                config_path,
                event_capacity: DEFAULT_EVENT_CAPACITY,
                force_shutdown_requested: AtomicBool::new(false),
                force_shutdown_notify: Notify::new(),
                shutdown_complete: Mutex::new(None),
                shutdown_finalizer: AsyncMutex::new(()),
                runtime: Arc::new(ApplicationRuntime::new()),
            }),
        }
    }

    async fn shutdown_inner(&self, force: bool) -> Result<ShutdownReport> {
        if let Some(report) = self
            .inner
            .shutdown_complete
            .lock()
            .expect("application shutdown state poisoned")
            .clone()
        {
            return Ok(report);
        }
        self.inner.runtime.close_admission().await;
        if force {
            self.inner
                .force_shutdown_requested
                .store(true, AtomicOrdering::Release);
            self.inner.force_shutdown_notify.notify_one();
        }
        let _finalizer = self.inner.shutdown_finalizer.lock().await;
        if let Some(report) = self
            .inner
            .shutdown_complete
            .lock()
            .expect("application shutdown state poisoned")
            .clone()
        {
            return Ok(report);
        }
        let force = self
            .inner
            .force_shutdown_requested
            .load(AtomicOrdering::Acquire);
        let mut report = ShutdownReport {
            forced: force,
            adapter: ShutdownAdapterStatus::Completed,
            task_panics: 0,
            aborted_tasks: 0,
            pending_terminal_failures: Vec::new(),
        };

        if force {
            self.shutdown_force_owned(&mut report).await;
        } else {
            tokio::select! {
                biased;
                _ = self.inner.force_shutdown_notify.notified() => {
                    report.forced = true;
                    self.shutdown_force_owned(&mut report).await;
                }
                _ = self.shutdown_graceful_owned(&mut report) => {}
            }
        }
        self.inner.runtime.clear_mcp_runtimes();
        report.task_panics = self
            .inner
            .runtime
            .task_panics
            .load(AtomicOrdering::Relaxed)
            .saturating_add(self.inner.runtime.agent_supervisor.task_panics());
        *self
            .inner
            .shutdown_complete
            .lock()
            .expect("application shutdown state poisoned") = Some(report.clone());
        Ok(report)
    }

    async fn shutdown_graceful_owned(&self, report: &mut ShutdownReport) {
        self.inner.runtime.tasks.wait().await;
        self.inner
            .runtime
            .agent_supervisor
            .shutdown_graceful()
            .await;
        self.inner
            .runtime
            .agent_supervisor
            .stage_remaining_interrupted("application shutdown");
        self.flush_agent_terminals(report).await;
        if let Err(error) = self.inner.agent_sessions.shutdown(false).await {
            report.adapter = ShutdownAdapterStatus::Failed {
                message: error.to_string(),
            };
        }
        self.retry_and_settle_terminal_slots(report, None).await;
        self.inner.state.close().await;
    }

    async fn shutdown_force_owned(&self, report: &mut ShutdownReport) {
        let deadline = tokio::time::Instant::now() + FORCE_SHUTDOWN_TOTAL;
        for control in self.inner.runtime.active_controls() {
            control.abort();
        }
        self.inner.runtime.agent_supervisor.close_and_cancel();

        let adapter_deadline =
            std::cmp::min(deadline, tokio::time::Instant::now() + FORCE_ADAPTER_BUDGET);
        report.adapter = match tokio::time::timeout_at(
            adapter_deadline,
            self.inner.agent_sessions.shutdown(true),
        )
        .await
        {
            Ok(Ok(())) => ShutdownAdapterStatus::Completed,
            Ok(Err(error)) => ShutdownAdapterStatus::Failed {
                message: error.to_string(),
            },
            Err(_) => ShutdownAdapterStatus::TimedOut,
        };

        let join_deadline = std::cmp::min(
            deadline,
            tokio::time::Instant::now() + FORCE_COOPERATIVE_JOIN_BUDGET,
        );
        let wait_for_owned_tasks = async {
            tokio::join!(
                self.inner.runtime.tasks.wait(),
                self.inner.runtime.agent_supervisor.wait_background()
            );
        };
        if tokio::time::timeout_at(join_deadline, wait_for_owned_tasks)
            .await
            .is_err()
        {
            report.aborted_tasks = self.inner.runtime.abort_all_tasks()
                + self.inner.runtime.agent_supervisor.abort_background();
            let wait_for_aborted_tasks = async {
                tokio::join!(
                    self.inner.runtime.tasks.wait(),
                    self.inner.runtime.agent_supervisor.wait_background()
                );
            };
            if tokio::time::timeout_at(deadline, wait_for_aborted_tasks)
                .await
                .is_err()
            {
                report.adapter = ShutdownAdapterStatus::ContractViolation {
                    message: "tracked tasks remained live after forced abort".to_string(),
                };
            }
        }

        self.inner
            .runtime
            .agent_supervisor
            .stage_remaining_interrupted("application force shutdown");
        self.flush_agent_terminals(report).await;
        self.retry_and_settle_terminal_slots(report, Some(deadline))
            .await;
        if tokio::time::timeout_at(deadline, self.inner.state.close())
            .await
            .is_err()
        {
            report.adapter = ShutdownAdapterStatus::ContractViolation {
                message: "State close exceeded the force-shutdown deadline".to_string(),
            };
        }
    }

    async fn flush_agent_terminals(&self, report: &mut ShutdownReport) {
        report.pending_terminal_failures.extend(
            self.inner
                .runtime
                .agent_supervisor
                .flush_pending_terminals(&self.inner.state)
                .await
                .into_iter()
                .map(|(id, message)| PendingTerminalFailure {
                    turn_id: format!("agent:{id}"),
                    message,
                }),
        );
    }

    async fn retry_and_settle_terminal_slots(
        &self,
        report: &mut ShutdownReport,
        deadline: Option<tokio::time::Instant>,
    ) {
        for slot in self.inner.runtime.take_turn_slots() {
            if slot.phase == TurnPhase::PendingAcceptance {
                slot.handle.completion.settle(Err(Arc::from(
                    "Framework Turn ended before durable acceptance",
                )));
                slot.handle.control.abort();
                slot.handle.events.close();
                continue;
            }
            let terminal = slot
                .pending_terminal
                .unwrap_or_else(|| PendingTerminal::interrupted(slot.handle.receipt.clone()));
            let result = match deadline {
                Some(deadline) => {
                    match tokio::time::timeout_at(deadline, terminal.persist(&self.inner.state))
                        .await
                    {
                        Ok(result) => result,
                        Err(_) => Err(Error::TerminalPersistence {
                            turn_id: terminal.receipt.turn_id.clone(),
                            message: "force-shutdown deadline elapsed".to_string(),
                        }),
                    }
                }
                None => terminal.persist(&self.inner.state).await,
            };
            match result {
                Ok(()) => {
                    if slot.phase == TurnPhase::Active {
                        slot.handle.events.push(terminal.terminal_event.clone());
                        slot.handle.completion.settle(terminal.completion.clone());
                    }
                }
                Err(error) => {
                    report
                        .pending_terminal_failures
                        .push(PendingTerminalFailure {
                            turn_id: terminal.receipt.turn_id.clone(),
                            message: error.to_string(),
                        });
                    if slot.phase == TurnPhase::Active {
                        let message: Arc<str> = Arc::from(format!(
                            "failed to persist Framework Turn terminal: {error}"
                        ));
                        slot.handle.events.push(TurnEvent::Warning {
                            data: serde_json::json!({
                                "kind": "framework_terminal_persistence",
                                "message": message.as_ref(),
                                "turnId": terminal.receipt.turn_id,
                            }),
                        });
                        slot.handle.completion.settle(Err(message));
                    }
                }
            }
            slot.handle.control.abort();
            slot.handle.events.close();
        }
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __state_runtime(&self) -> StateRuntime {
        self.inner.state.clone()
    }
}

impl fmt::Debug for ApplicationBuilder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApplicationBuilder")
            .field("home", &self.home)
            .field("database_path", &self.database_path)
            .field("has_state_runtime", &self.state.is_some())
            .field("config_path", &self.config_path)
            .field("event_capacity", &self.event_capacity)
            .field("has_agent_session_adapter", &self.agent_sessions.is_some())
            .field("has_provider", &self.provider.is_some())
            .finish()
    }
}

impl ApplicationBuilder {
    pub fn home(mut self, home: impl Into<PathBuf>) -> Self {
        self.home = Some(home.into());
        self
    }

    pub fn database_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.database_path = Some(path.into());
        self
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __state_runtime(mut self, state: StateRuntime) -> Self {
        self.state = Some(state);
        self
    }

    pub fn config_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config_path = Some(path.into());
        self
    }

    pub fn event_capacity(mut self, capacity: usize) -> Self {
        self.event_capacity = Some(capacity);
        self
    }

    pub fn agent_session_adapter(mut self, adapter: Arc<dyn AgentSessionAdapter>) -> Self {
        self.agent_sessions = Some(adapter);
        self
    }

    pub fn provider(mut self, provider: psychevo_ai::Provider) -> Self {
        self.provider = Some(provider);
        self
    }

    pub async fn build(self) -> Result<Application> {
        let home = self.home.ok_or_else(|| {
            Error::Message(
                "ApplicationBuilder requires an explicit Psychevo home directory".to_string(),
            )
        })?;
        if self.state.is_some() && self.database_path.is_some() {
            return Err(Error::Message(
                "ApplicationBuilder accepts either database_path or an existing state runtime, not both"
                    .to_string(),
            ));
        }
        let database_path = self.database_path.unwrap_or_else(|| home.join("state.db"));
        let event_capacity = self.event_capacity.unwrap_or(DEFAULT_EVENT_CAPACITY);
        if event_capacity == 0 {
            return Err(Error::Message(
                "Application event capacity must be greater than zero".to_string(),
            ));
        }
        let state = match self.state {
            Some(state) => state,
            None => StateRuntime::open(database_path).await?,
        };
        let agent_sessions = self.agent_sessions.unwrap_or_else(|| {
            Arc::new(NativeAgentSessionAdapter {
                state: state.clone(),
                config_path: self.config_path.clone(),
                provider: self.provider,
            })
        });
        Ok(Application {
            inner: Arc::new(ApplicationInner {
                state,
                agent_sessions,
                home,
                config_path: self.config_path,
                event_capacity,
                force_shutdown_requested: AtomicBool::new(false),
                force_shutdown_notify: Notify::new(),
                shutdown_complete: Mutex::new(None),
                shutdown_finalizer: AsyncMutex::new(()),
                runtime: Arc::new(ApplicationRuntime::new()),
            }),
        })
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Client")
            .field("home", &self.inner.home)
            .finish_non_exhaustive()
    }
}

impl Client {
    pub(super) fn ensure_open(&self) -> Result<()> {
        self.inner.runtime.ensure_open()
    }

    pub(super) fn application_environment(
        &self,
        inherited: Option<BTreeMap<String, String>>,
    ) -> BTreeMap<String, String> {
        let mut environment = inherited.unwrap_or_else(|| std::env::vars().collect());
        environment.insert(
            "PSYCHEVO_HOME".to_string(),
            self.inner.home.to_string_lossy().into_owned(),
        );
        environment
    }
}
