use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

pub(crate) const TUI_STATE_VERSION: u64 = 5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TuiState {
    #[serde(default = "current_version")]
    pub(crate) version: u64,
    #[serde(default = "default_thinking_visible")]
    pub(crate) thinking_visible: bool,
    #[serde(default)]
    pub(crate) raw_visible: bool,
    #[serde(default)]
    pub(crate) sidebar_visible: bool,
    #[serde(default)]
    pub(crate) workdirs: BTreeMap<String, TuiWorkdirState>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TuiWorkdirState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) permission_mode: Option<String>,
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            version: TUI_STATE_VERSION,
            thinking_visible: true,
            raw_visible: false,
            sidebar_visible: false,
            workdirs: BTreeMap::new(),
        }
    }
}

impl TuiState {
    pub(crate) fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut state: Self = serde_json::from_str(&fs::read_to_string(path)?)?;
        state.version = TUI_STATE_VERSION;
        Ok(state)
    }

    pub(crate) fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_string_pretty(self)? + "\n")?;
        Ok(())
    }

    pub(crate) fn mode_for(&self, workdir: &str) -> Option<String> {
        self.workdirs
            .get(workdir)
            .and_then(|entry| entry.mode.clone())
    }

    pub(crate) fn permission_mode_for(&self, workdir: &str) -> Option<String> {
        self.workdirs
            .get(workdir)
            .and_then(|entry| entry.permission_mode.clone())
    }

    pub(crate) fn set_mode(&mut self, workdir: &str, mode: String) {
        self.workdirs.entry(workdir.to_string()).or_default().mode = Some(mode);
    }

    pub(crate) fn set_permission_mode(&mut self, workdir: &str, mode: String) {
        self.workdirs
            .entry(workdir.to_string())
            .or_default()
            .permission_mode = Some(mode);
    }

    pub(crate) fn set_thinking_visible(&mut self, visible: bool) {
        self.thinking_visible = visible;
    }

    pub(crate) fn set_raw_visible(&mut self, visible: bool) {
        self.raw_visible = visible;
    }

    pub(crate) fn set_sidebar_visible(&mut self, visible: bool) {
        self.sidebar_visible = visible;
    }
}

pub(crate) fn current_version() -> u64 {
    TUI_STATE_VERSION
}

pub(crate) fn default_thinking_visible() -> bool {
    true
}

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;
    use tempfile::tempdir;

    #[test]
    fn state_round_trips_per_workdir_mode_and_permission() {
        let temp = tempdir().expect("temp");
        let path = temp.path().join("tui-state.json");
        let mut state = TuiState::default();
        state.set_mode("/repo", "plan".to_string());
        state.set_permission_mode("/repo", "acceptEdits".to_string());
        state.set_thinking_visible(false);
        state.set_raw_visible(true);
        state.set_sidebar_visible(true);
        state.save(&path).expect("save");

        let loaded = TuiState::load(&path).expect("load");
        assert_eq!(loaded.mode_for("/repo").as_deref(), Some("plan"));
        assert_eq!(
            loaded.permission_mode_for("/repo").as_deref(),
            Some("acceptEdits")
        );
        assert!(!loaded.thinking_visible);
        assert!(loaded.raw_visible);
        assert!(loaded.sidebar_visible);
        assert_eq!(loaded.version, 5);
    }

    #[test]
    fn state_tolerates_future_versions_and_legacy_model_fields() {
        let temp = tempdir().expect("temp");
        let path = temp.path().join("tui-state.json");
        std::fs::write(
            &path,
            r#"{
              "version": 99,
              "recent_models": ["a/1","b/2"],
              "workdirs": {
                "/repo": {
                  "model": "mock/model",
                  "variant": "high",
                  "mode": "plan"
                }
              }
            }"#,
        )
        .expect("state");

        let loaded = TuiState::load(&path).expect("load");
        assert_eq!(loaded.version, 5);
        assert!(loaded.thinking_visible);
        assert!(!loaded.raw_visible);
        assert!(!loaded.sidebar_visible);
        assert_eq!(loaded.mode_for("/repo").as_deref(), Some("plan"));
    }
}
