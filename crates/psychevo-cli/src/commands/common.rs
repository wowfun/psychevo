use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Read, Write};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use futures::future::BoxFuture;
use psychevo::{
    Application, ApprovalHandler, Configuration, ConfigurationQuery,
    application::{PermissionApprovalDecision, PermissionApprovalRequest},
};
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

pub(crate) fn interactive_approval_handler() -> Option<Arc<dyn ApprovalHandler>> {
    (io::stdin().is_terminal() && io::stderr().is_terminal())
        .then(|| Arc::new(CliApprovalHandler) as Arc<dyn ApprovalHandler>)
}

#[derive(Debug)]
struct CliApprovalHandler;

impl ApprovalHandler for CliApprovalHandler {
    fn timeout_secs(&self) -> u64 {
        60
    }

    fn request_permission(
        &self,
        request: PermissionApprovalRequest,
    ) -> BoxFuture<'static, PermissionApprovalDecision> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || prompt_for_permission(request))
                .await
                .unwrap_or_else(|_| PermissionApprovalDecision::deny())
        })
    }
}

fn prompt_for_permission(request: PermissionApprovalRequest) -> PermissionApprovalDecision {
    let mut stderr = io::stderr();
    let _ = writeln!(stderr, "permission required: {}", request.reason);
    let _ = writeln!(stderr, "tool: {}", request.tool_name);
    let _ = writeln!(stderr, "action: {}", request.summary);
    if let Some(rule) = &request.matched_rule {
        let _ = writeln!(stderr, "matched rule: {rule}");
    }
    if let Some(filesystem) = &request.filesystem {
        for target in &filesystem.targets {
            let _ = writeln!(stderr, "requested path: {}", target.requested_path);
            if target.requested_path != target.resolved_path {
                let _ = writeln!(stderr, "resolved path:  {}", target.resolved_path);
            }
        }
    }
    if request.allow_always
        && let Some(rule) = &request.suggested_rule
    {
        let _ = writeln!(stderr, "suggested always rule: {rule}");
    }
    let prompt = if request.filesystem.is_some() {
        "Allow? [o]nce, [t]urn directory, [s]ession directory, [d]eny: "
    } else if request.allow_always {
        "Allow? [o]nce, [s]ession, [a]lways, [d]eny: "
    } else {
        "Allow? [o]nce, [s]ession, [d]eny: "
    };
    let _ = write!(stderr, "{prompt}");
    let _ = stderr.flush();
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return PermissionApprovalDecision::deny();
    }
    let choice = line.trim().to_ascii_lowercase();
    if matches!(choice.as_str(), "t" | "turn" | "s" | "session")
        && let Some(filesystem) = &request.filesystem
    {
        if filesystem.scope_candidates.is_empty() {
            return PermissionApprovalDecision::deny();
        }
        for (index, directory) in filesystem.scope_candidates.iter().enumerate() {
            let _ = writeln!(stderr, "  {}. {}", index + 1, directory);
        }
        let _ = write!(
            stderr,
            "Directory [1-{}]: ",
            filesystem.scope_candidates.len()
        );
        let _ = stderr.flush();
        let mut directory = String::new();
        if io::stdin().read_line(&mut directory).is_err() {
            return PermissionApprovalDecision::deny();
        }
        let Some(directory) = directory
            .trim()
            .parse::<usize>()
            .ok()
            .and_then(|index| index.checked_sub(1))
            .and_then(|index| filesystem.scope_candidates.get(index))
            .cloned()
        else {
            return PermissionApprovalDecision::deny();
        };
        return if matches!(choice.as_str(), "t" | "turn") {
            PermissionApprovalDecision::allow_filesystem_turn(directory)
        } else {
            PermissionApprovalDecision::allow_filesystem_session(directory)
        };
    }
    match choice.as_str() {
        "o" | "once" | "y" | "yes" => PermissionApprovalDecision::allow_once(),
        "s" | "session" => PermissionApprovalDecision::allow_session(),
        "a" | "always" if request.allow_always => PermissionApprovalDecision::allow_always(),
        _ => PermissionApprovalDecision::deny(),
    }
}
