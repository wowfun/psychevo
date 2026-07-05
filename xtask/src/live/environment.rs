use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{Context, Result};
use clap::ValueEnum;
use serde::Serialize;

use super::registry::LiveProvider;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[value(rename_all = "kebab-case")]
pub(crate) enum LiveEnvMode {
    #[default]
    Shared,
    Isolated,
}

impl std::fmt::Display for LiveEnvMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shared => f.write_str("shared"),
            Self::Isolated => f.write_str("isolated"),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct LiveEnvironmentPlanOutput {
    pub(crate) mode: LiveEnvMode,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct LiveEnvironmentPathsOutput {
    pub(crate) mode: LiveEnvMode,
    pub(crate) home_path: String,
    pub(crate) config_path: String,
    pub(crate) db_path: String,
}

#[derive(Clone, Debug)]
pub(crate) struct LivePrerequisites {
    dev_home: PathBuf,
    config_path: PathBuf,
    dotenv: BTreeMap<String, String>,
}

impl LivePrerequisites {
    pub(crate) fn load(root: &Path) -> Result<Self, String> {
        let dev_home = root.join(".local").join(".psychevo-dev");
        let config_path = dev_home.join("config.toml");
        let env_path = dev_home.join(".env");
        if !config_path.is_file() || !env_path.is_file() {
            return Err(format!(
                "dev home is not initialized: {}; run: cargo xtask init dev-env",
                dev_home.display()
            ));
        }
        let dotenv = load_dotenv(&env_path)
            .map_err(|error| format!("failed to read {}: {error:#}", env_path.display()))?;
        Ok(Self {
            dev_home,
            config_path,
            dotenv,
        })
    }

    pub(crate) fn resolve(&self, mode: LiveEnvMode, check_dir: &Path) -> Result<LiveEnvironment> {
        let (home_path, db_path) = match mode {
            LiveEnvMode::Shared => (self.dev_home.clone(), self.dev_home.join("state.db")),
            LiveEnvMode::Isolated => (check_dir.join("home"), check_dir.join("state.db")),
        };
        fs::create_dir_all(&home_path)
            .with_context(|| format!("create live home {}", home_path.display()))?;
        Ok(LiveEnvironment {
            mode,
            home_path,
            config_path: self.config_path.clone(),
            db_path,
            dotenv: self.dotenv.clone(),
        })
    }

    pub(crate) fn provider_credentials_available(&self, provider: &LiveProvider) -> bool {
        provider.credential_env.iter().any(|key| {
            self.dotenv
                .get(*key)
                .is_some_and(|value| !value.trim().is_empty())
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LiveEnvironment {
    mode: LiveEnvMode,
    home_path: PathBuf,
    config_path: PathBuf,
    db_path: PathBuf,
    dotenv: BTreeMap<String, String>,
}

impl LiveEnvironment {
    pub(crate) fn apply_to_command(
        &self,
        command: &mut ProcessCommand,
        provider: Option<LiveProvider>,
    ) {
        command
            .envs(self.dotenv.iter())
            .env("PSYCHEVO_HOME", &self.home_path)
            .env("PSYCHEVO_CONFIG", &self.config_path)
            .env("PSYCHEVO_DB", &self.db_path);
        if let Some(provider) = provider {
            command
                .env("PSYCHEVO_INFERENCE_PROVIDER", provider.id)
                .env("PSYCHEVO_INFERENCE_MODEL", provider.model);
        }
    }

    pub(crate) fn home_path(&self) -> &Path {
        &self.home_path
    }

    pub(crate) fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub(crate) fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub(crate) fn to_output(&self) -> LiveEnvironmentPathsOutput {
        LiveEnvironmentPathsOutput {
            mode: self.mode,
            home_path: self.home_path.display().to_string(),
            config_path: self.config_path.display().to_string(),
            db_path: self.db_path.display().to_string(),
        }
    }
}

fn load_dotenv(path: &Path) -> Result<BTreeMap<String, String>> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut values = BTreeMap::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let value = value
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();
        values.insert(key.to_string(), value);
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::live::registry::XIAOMI_TOKEN_PLAN;

    use super::*;

    #[test]
    fn dotenv_parser_handles_basic_values() {
        let root = temp_dir("psychevo-xtask-live-dotenv");
        fs::create_dir_all(&root).expect("root");
        let env_path = root.join(".env");
        fs::write(
            &env_path,
            "\n# comment\nXIAOMI_API_KEY='abc'\nDEEPSEEK_API_KEY=\"def\"\nEMPTY=\n",
        )
        .expect("env");
        let parsed = load_dotenv(&env_path).expect("dotenv");
        assert_eq!(
            parsed.get("XIAOMI_API_KEY").map(String::as_str),
            Some("abc")
        );
        assert_eq!(
            parsed.get("DEEPSEEK_API_KEY").map(String::as_str),
            Some("def")
        );
        assert_eq!(parsed.get("EMPTY").map(String::as_str), Some(""));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_dev_home_is_blocked_prerequisite() {
        let root = temp_dir("psychevo-xtask-live-missing-home");
        fs::create_dir_all(&root).expect("root");
        let err = LivePrerequisites::load(&root).expect_err("blocked");
        assert!(err.contains("cargo xtask init dev-env"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn shared_environment_uses_dev_home_and_state_db() {
        let root = live_dev_home("psychevo-xtask-live-shared-env");
        let prerequisites = LivePrerequisites::load(&root).expect("prerequisites");
        let env = prerequisites
            .resolve(LiveEnvMode::Shared, &root.join("artifact/live/check"))
            .expect("shared env");
        assert_eq!(env.home_path(), root.join(".local/.psychevo-dev"));
        assert_eq!(
            env.config_path(),
            root.join(".local/.psychevo-dev/config.toml")
        );
        assert_eq!(env.db_path(), root.join(".local/.psychevo-dev/state.db"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn isolated_environment_uses_check_home_and_db_with_dev_config() {
        let root = live_dev_home("psychevo-xtask-live-isolated-env");
        let check_dir = root.join("artifact/live/check");
        let prerequisites = LivePrerequisites::load(&root).expect("prerequisites");
        let env = prerequisites
            .resolve(LiveEnvMode::Isolated, &check_dir)
            .expect("isolated env");
        assert_eq!(env.home_path(), check_dir.join("home"));
        assert_eq!(
            env.config_path(),
            root.join(".local/.psychevo-dev/config.toml")
        );
        assert_eq!(env.db_path(), check_dir.join("state.db"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn provider_command_environment_includes_provider_and_model() {
        let root = live_dev_home("psychevo-xtask-live-provider-env");
        let prerequisites = LivePrerequisites::load(&root).expect("prerequisites");
        let env = prerequisites
            .resolve(LiveEnvMode::Shared, &root.join("artifact/live/check"))
            .expect("shared env");
        let mut command = ProcessCommand::new("pevo");

        env.apply_to_command(&mut command, Some(XIAOMI_TOKEN_PLAN));

        let envs = command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().to_string(),
                    value.map(|value| value.to_string_lossy().to_string()),
                )
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            envs.get("PSYCHEVO_INFERENCE_PROVIDER"),
            Some(&Some("xiaomi-token-plan".to_string()))
        );
        assert_eq!(
            envs.get("PSYCHEVO_INFERENCE_MODEL"),
            Some(&Some("xiaomi-token-plan/mimo-v2.5-pro".to_string()))
        );
        let _ = fs::remove_dir_all(root);
    }

    fn live_dev_home(prefix: &str) -> PathBuf {
        let root = temp_dir(prefix);
        let dev_home = root.join(".local/.psychevo-dev");
        fs::create_dir_all(&dev_home).expect("dev home");
        fs::write(dev_home.join("config.toml"), "# config\n").expect("config");
        fs::write(dev_home.join(".env"), "XIAOMI_API_KEY=secret\n").expect("env");
        root
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{now}", std::process::id()))
    }
}
