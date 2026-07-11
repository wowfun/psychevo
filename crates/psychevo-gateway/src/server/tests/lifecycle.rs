#[derive(Debug, Default)]
struct DeadlineShutdownRuntime {
    modes: Arc<Mutex<Vec<psychevo_runtime_host::ShutdownMode>>>,
}

impl psychevo_runtime_host::RuntimeModule for DeadlineShutdownRuntime {
    fn snapshot(
        &self,
        _query: psychevo_runtime_host::SnapshotQuery,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::RuntimeSnapshot> {
        Box::pin(async {
            Err(psychevo_runtime_host::RuntimeError::new(
                "unused",
                psychevo_runtime_host::RuntimeErrorStage::Configuration,
                psychevo_runtime_host::RetryClass::Never,
                "snapshot is unused in the shutdown lifecycle test",
            ))
        })
    }

    fn execute(
        &self,
        _request: psychevo_runtime_host::ExecuteRequest,
        _observer: psychevo_runtime_host::RuntimeObserver,
        _control: psychevo_runtime_host::RuntimeControl,
    ) -> psychevo_runtime_host::RuntimeFuture<psychevo_runtime_host::ExecuteResult> {
        Box::pin(async {
            Err(psychevo_runtime_host::RuntimeError::new(
                "unused",
                psychevo_runtime_host::RuntimeErrorStage::Configuration,
                psychevo_runtime_host::RetryClass::Never,
                "execute is unused in the shutdown lifecycle test",
            ))
        })
    }

    fn shutdown(
        &self,
        mode: psychevo_runtime_host::ShutdownMode,
    ) -> psychevo_runtime_host::RuntimeFuture<()> {
        self.modes
            .lock()
            .expect("shutdown mode trace poisoned")
            .push(mode.clone());
        match mode {
            psychevo_runtime_host::ShutdownMode::Graceful => Box::pin(std::future::pending()),
            psychevo_runtime_host::ShutdownMode::Force
            | psychevo_runtime_host::ShutdownMode::Runtime { .. } => Box::pin(async { Ok(()) }),
        }
    }
}

#[tokio::test]
async fn runtime_shutdown_deadline_falls_back_to_force() {
    let temp = tempfile::tempdir().expect("tempdir");
    let state = StateRuntime::open(temp.path().join("state.db")).expect("state");
    let runtime = Arc::new(DeadlineShutdownRuntime::default());
    let modes = Arc::clone(&runtime.modes);
    let host = psychevo_runtime_host::RuntimeHost::new();
    host.register(psychevo_runtime_host::RuntimeKind::Codex, runtime);
    let gateway = Gateway::with_backend_and_runtime_host(
        state,
        Arc::new(crate::PsychevoRuntimeBackend),
        host,
    );

    tokio::time::timeout(
        Duration::from_millis(250),
        shutdown_runtimes_with_deadlines(
            &gateway,
            Duration::from_millis(10),
            Duration::from_millis(100),
        ),
    )
    .await
    .expect("bounded shutdown helper hung")
    .expect("forced fallback");

    assert_eq!(
        &*modes.lock().expect("shutdown mode trace poisoned"),
        &[
            psychevo_runtime_host::ShutdownMode::Graceful,
            psychevo_runtime_host::ShutdownMode::Force,
        ]
    );
}
