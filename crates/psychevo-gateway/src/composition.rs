use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use psychevo::{Application, Client, Error};

#[cfg(test)]
use crate::FrameworkNativeTestExecutor;
use crate::gateway::agent_session::AgentSessionHost;
use crate::gateway::framework_adapter::GatewayAgentSessionAdapter;
use crate::gateway::{Gateway, GatewayLimits};

/// Owns the in-process Framework and Gateway wiring for one product process.
///
/// The state runtime stays inside the owned Application. Product surfaces use
/// the typed Application, Client, and Gateway accessors and close the
/// composition explicitly when their process-local surface stops.
pub struct GatewayApplication {
    application: Application,
    client: Client,
    gateway: Gateway,
    home: PathBuf,
    config_path: Option<PathBuf>,
    inherited_env: BTreeMap<String, String>,
}

impl fmt::Debug for GatewayApplication {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayApplication")
            .field("application", &self.application)
            .field("gateway", &self.gateway)
            .field("home", &self.home)
            .field("config_path", &self.config_path)
            .finish_non_exhaustive()
    }
}

impl GatewayApplication {
    pub async fn open(
        home: PathBuf,
        database_path: PathBuf,
        config_path: Option<PathBuf>,
        inherited_env: BTreeMap<String, String>,
    ) -> psychevo::Result<Self> {
        Self::open_with_limits(
            home,
            database_path,
            config_path,
            inherited_env,
            GatewayLimits::default(),
        )
        .await
    }

    pub async fn open_with_limits(
        home: PathBuf,
        database_path: PathBuf,
        config_path: Option<PathBuf>,
        inherited_env: BTreeMap<String, String>,
        limits: GatewayLimits,
    ) -> psychevo::Result<Self> {
        Self::open_inner(
            home,
            database_path,
            config_path,
            inherited_env,
            limits,
            #[cfg(test)]
            None,
        )
        .await
    }

    async fn open_inner(
        home: PathBuf,
        database_path: PathBuf,
        config_path: Option<PathBuf>,
        inherited_env: BTreeMap<String, String>,
        limits: GatewayLimits,
        #[cfg(test)] native_test_executor: Option<FrameworkNativeTestExecutor>,
    ) -> psychevo::Result<Self> {
        let limits = limits.validate()?;
        let agent_sessions = AgentSessionHost::new();
        #[cfg(test)]
        let adapter = match native_test_executor {
            Some(executor) => GatewayAgentSessionAdapter::with_native_test_executor(
                agent_sessions.clone(),
                home.clone(),
                inherited_env.clone(),
                executor,
            ),
            None => GatewayAgentSessionAdapter::new(
                agent_sessions.clone(),
                home.clone(),
                inherited_env.clone(),
            ),
        };
        #[cfg(not(test))]
        let adapter = GatewayAgentSessionAdapter::new(
            agent_sessions.clone(),
            home.clone(),
            inherited_env.clone(),
        );
        let mut builder = Application::builder()
            .home(home.clone())
            .database_path(database_path)
            .database_connection_limit(limits.database_connection_limit)
            .limits(limits.application)
            .inherited_environment(inherited_env.clone())
            .agent_session_adapter(Arc::new(adapter));
        if let Some(path) = config_path.clone() {
            builder = builder.config_path(path);
        }
        let application = builder.build().await?;
        let client = application.client();
        let gateway = Gateway::from_composition(
            application.gateway_durability(),
            agent_sessions,
            client.clone(),
            limits,
        );
        Ok(Self {
            application,
            client,
            gateway,
            home,
            config_path,
            inherited_env,
        })
    }

    #[cfg(test)]
    pub(crate) async fn open_with_native_test_executor(
        home: PathBuf,
        database_path: PathBuf,
        config_path: Option<PathBuf>,
        inherited_env: BTreeMap<String, String>,
        native_test_executor: FrameworkNativeTestExecutor,
    ) -> psychevo::Result<Self> {
        Self::open_with_native_test_executor_and_limits(
            home,
            database_path,
            config_path,
            inherited_env,
            GatewayLimits::default(),
            native_test_executor,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn open_with_native_test_executor_and_limits(
        home: PathBuf,
        database_path: PathBuf,
        config_path: Option<PathBuf>,
        inherited_env: BTreeMap<String, String>,
        limits: GatewayLimits,
        native_test_executor: FrameworkNativeTestExecutor,
    ) -> psychevo::Result<Self> {
        Self::open_inner(
            home,
            database_path,
            config_path,
            inherited_env,
            limits,
            Some(native_test_executor),
        )
        .await
    }

    pub fn application(&self) -> &Application {
        &self.application
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub fn gateway(&self) -> &Gateway {
        &self.gateway
    }

    pub(crate) fn home(&self) -> &PathBuf {
        &self.home
    }

    pub(crate) fn config_path(&self) -> Option<&PathBuf> {
        self.config_path.as_ref()
    }

    pub(crate) fn inherited_env(&self) -> &BTreeMap<String, String> {
        &self.inherited_env
    }

    pub async fn shutdown(&self) -> psychevo::Result<()> {
        let gateway = self.gateway.shutdown_activity_runtime(false).await;
        let application = self
            .application
            .shutdown()
            .await
            .and_then(psychevo::ShutdownReport::require_clean)
            .map(|_| ());
        combine_shutdown_results(gateway, application)
    }

    pub async fn shutdown_force(&self) -> psychevo::Result<()> {
        let gateway = self.gateway.shutdown_activity_runtime(true).await;
        let application = self
            .application
            .shutdown_force()
            .await
            .and_then(psychevo::ShutdownReport::require_clean)
            .map(|_| ());
        combine_shutdown_results(gateway, application)
    }

    pub(crate) async fn shutdown_with_deadline(
        &self,
        graceful_timeout: Duration,
    ) -> psychevo::Result<()> {
        let gateway = match tokio::time::timeout(
            graceful_timeout,
            self.gateway.shutdown_activity_runtime(false),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                let graceful_failure = format!(
                    "graceful Gateway shutdown exceeded {} ms",
                    graceful_timeout.as_millis()
                );
                match tokio::time::timeout(
                    graceful_timeout,
                    self.gateway.shutdown_activity_runtime(true),
                )
                .await
                {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(error)) => Err(Error::Message(format!(
                        "{graceful_failure}; forced Gateway shutdown failed: {error}"
                    ))),
                    Err(_) => Err(Error::Message(format!(
                        "{graceful_failure}; forced Gateway shutdown also exceeded {} ms",
                        graceful_timeout.as_millis()
                    ))),
                }
            }
        };
        let graceful = tokio::time::timeout(graceful_timeout, self.application.shutdown()).await;
        let application = match graceful {
            Ok(Ok(report)) if report.is_clean() => Ok(()),
            outcome => {
                let graceful_failure = match outcome {
                    Ok(Ok(report)) => format!(
                        "graceful Application shutdown failed: {}",
                        report
                            .require_clean()
                            .expect_err("non-clean report must fail")
                    ),
                    Ok(Err(error)) => {
                        format!("graceful Application shutdown failed: {error}")
                    }
                    Err(_) => format!(
                        "graceful Application shutdown exceeded {} ms",
                        graceful_timeout.as_millis()
                    ),
                };
                match self.application.shutdown_force().await {
                    Ok(report) => report.require_clean().map(|_| ()).map_err(|error| {
                        Error::Message(format!(
                            "{graceful_failure}; forced Application shutdown failed: {error}"
                        ))
                    }),
                    Err(error) => Err(Error::Message(format!(
                        "{graceful_failure}; forced Application shutdown failed: {error}"
                    ))),
                }
            }
        };
        combine_shutdown_results(gateway, application)
    }
}

fn combine_shutdown_results(
    gateway: psychevo::Result<()>,
    application: psychevo::Result<()>,
) -> psychevo::Result<()> {
    match (gateway, application) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Err(gateway), Err(application)) => Err(Error::Message(format!(
            "Gateway shutdown failed: {gateway}; Application shutdown also failed: {application}"
        ))),
    }
}

#[cfg(test)]
mod crash_restart_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::activity::{SendShellRequest, ShellExecutionIntent};

    #[tokio::test]
    async fn composition_applies_the_configured_application_and_storage_limits() {
        let temp = tempfile::tempdir().expect("tempdir");
        let limits = GatewayLimits {
            event_ingress_capacity: 7,
            shell_activity_limit: 3,
            shell_queue_limit: 2,
            application: psychevo::ApplicationLimits {
                max_operations: 6,
                max_thread_operations: 2,
            },
            database_connection_limit: 2,
        };
        let runtime = GatewayApplication::open_with_limits(
            temp.path().to_path_buf(),
            temp.path().join("state.db"),
            None,
            BTreeMap::new(),
            limits,
        )
        .await
        .expect("bounded composition");

        let snapshot = runtime.application().operational_snapshot();
        assert_eq!(snapshot.limits, limits.application);
        assert_eq!(snapshot.storage.connection_limit, 2);
        assert_eq!(
            runtime.gateway().event_ingress_diagnostics().limit,
            limits.event_ingress_capacity
        );
        runtime.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn composition_connects_one_state_owner_and_closes_without_a_reference_cycle() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::write(home.join("config.toml"), "\n").expect("config");
        let runtime =
            GatewayApplication::open(home.clone(), home.join("state.db"), None, BTreeMap::new())
                .await
                .expect("composition");
        let thread = runtime
            .client()
            .start_thread(psychevo::StartThreadRequest::new(temp.path()))
            .await
            .expect("typed Client shares the composition state");
        assert!(
            runtime
                .gateway()
                .thread_transcript(thread.id())
                .await
                .expect("Gateway reads the same Thread")
                .is_empty()
        );
        tokio::time::timeout(Duration::from_secs(2), runtime.shutdown())
            .await
            .expect("explicit composition shutdown must not cycle")
            .expect("clean composition shutdown");
        let error = runtime
            .client()
            .start_thread(psychevo::StartThreadRequest::new(temp.path()))
            .await
            .expect_err("closed composition rejects new Application work");
        assert!(error.to_string().contains("shutting down"));
        let error = runtime
            .gateway()
            .send_shell(SendShellRequest {
                thread_id: None,
                source: None,
                bind_source: None,
                cwd: temp.path().to_path_buf(),
                command: "true".to_string(),
                execution: ShellExecutionIntent::new("composition-test"),
                event_sink: None,
                lineage: None,
            })
            .await
            .expect_err("closed composition rejects new Gateway work");
        assert!(error.to_string().contains("shutting down"));
        runtime.shutdown().await.expect("shutdown is idempotent");
    }
}
