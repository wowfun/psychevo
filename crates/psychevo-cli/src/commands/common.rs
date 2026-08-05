use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Read};
use std::path::Path;

use anyhow::{Result, anyhow};
use psychevo::{Application, Configuration, ConfigurationQuery};
use serde_json::json;

use crate::env::{env_path, resolve_state_db};

pub(crate) fn print_json_error(err: &anyhow::Error) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(&json!({
            "type": "error",
            "message": format!("{err:#}"),
        }))?
    );
    Ok(())
}

pub(crate) struct CommandConfiguration {
    application: Application,
    configuration: Configuration,
}

impl CommandConfiguration {
    pub(crate) async fn open(
        env_map: &BTreeMap<String, String>,
        home: &Path,
        cwd: &Path,
    ) -> Result<Self> {
        let db_path = resolve_state_db(env_map, home, cwd)?;
        let mut builder = Application::builder().home(home).database_path(db_path);
        if let Some(config_path) = env_path("PSYCHEVO_CONFIG", env_map, cwd)? {
            builder = builder.config_path(config_path);
        }
        let application = builder.build().await?;
        let mut query = ConfigurationQuery::new(cwd);
        query.inherited_env = Some(env_map.clone());
        let configuration = match application.client().configuration(query) {
            Ok(configuration) => configuration,
            Err(error) => {
                let _ = application.shutdown().await;
                return Err(error.into());
            }
        };
        Ok(Self {
            application,
            configuration,
        })
    }

    pub(crate) fn configuration(&self) -> &Configuration {
        &self.configuration
    }

    pub(crate) async fn finish<T>(self, result: Result<T>) -> Result<T> {
        let shutdown = self
            .application
            .shutdown()
            .await
            .and_then(|report| report.require_clean())
            .map(|_| ())
            .map_err(anyhow::Error::from);
        match result {
            Err(error) => Err(error),
            Ok(value) => {
                shutdown?;
                Ok(value)
            }
        }
    }
}

pub(crate) fn read_secret_from_stdin(required: bool) -> Result<Option<String>> {
    if !required {
        return Ok(None);
    }
    if io::stdin().is_terminal() {
        return Err(anyhow!(
            "stdin secret input requires piped stdin; interactive secret input is unavailable here"
        ));
    }
    let mut secret = String::new();
    io::stdin().read_to_string(&mut secret)?;
    let secret = secret.trim().to_string();
    if secret.is_empty() {
        return Err(anyhow!("stdin secret input requires a non-empty value"));
    }
    Ok(Some(secret))
}
