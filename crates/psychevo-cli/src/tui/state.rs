use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

const TUI_STATE_VERSION: u64 = 3;
const RECENT_MODEL_LIMIT: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TuiState {
    #[serde(default = "current_version")]
    pub(crate) version: u64,
    #[serde(default = "default_thinking_visible")]
    pub(crate) thinking_visible: bool,
    #[serde(default)]
    pub(crate) sidebar_visible: bool,
    #[serde(default)]
    pub(crate) workdirs: BTreeMap<String, TuiWorkdirState>,
    #[serde(default)]
    pub(crate) recent_models: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TuiWorkdirState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) mode: Option<String>,
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            version: TUI_STATE_VERSION,
            thinking_visible: true,
            sidebar_visible: false,
            workdirs: BTreeMap::new(),
            recent_models: Vec::new(),
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
        state.trim_recent();
        Ok(state)
    }

    pub(crate) fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_string_pretty(self)? + "\n")?;
        Ok(())
    }

    pub(crate) fn model_for(&self, workdir: &str) -> Option<String> {
        self.workdirs
            .get(workdir)
            .and_then(|entry| entry.model.clone())
    }

    pub(crate) fn variant_for(&self, workdir: &str) -> Option<String> {
        self.workdirs
            .get(workdir)
            .and_then(|entry| entry.variant.clone())
    }

    pub(crate) fn mode_for(&self, workdir: &str) -> Option<String> {
        self.workdirs
            .get(workdir)
            .and_then(|entry| entry.mode.clone())
    }

    pub(crate) fn set_model(&mut self, workdir: &str, model: String) {
        self.workdirs.entry(workdir.to_string()).or_default().model = Some(model.clone());
        self.push_recent_model(model);
    }

    pub(crate) fn set_variant(&mut self, workdir: &str, variant: String) {
        self.workdirs
            .entry(workdir.to_string())
            .or_default()
            .variant = Some(variant);
    }

    pub(crate) fn clear_variant(&mut self, workdir: &str) {
        if let Some(entry) = self.workdirs.get_mut(workdir) {
            entry.variant = None;
        }
    }

    pub(crate) fn set_mode(&mut self, workdir: &str, mode: String) {
        self.workdirs.entry(workdir.to_string()).or_default().mode = Some(mode);
    }

    pub(crate) fn set_thinking_visible(&mut self, visible: bool) {
        self.thinking_visible = visible;
    }

    pub(crate) fn set_sidebar_visible(&mut self, visible: bool) {
        self.sidebar_visible = visible;
    }

    fn push_recent_model(&mut self, model: String) {
        self.recent_models.retain(|entry| entry != &model);
        self.recent_models.insert(0, model);
        self.trim_recent();
    }

    fn trim_recent(&mut self) {
        self.recent_models.truncate(RECENT_MODEL_LIMIT);
    }
}

fn current_version() -> u64 {
    TUI_STATE_VERSION
}

fn default_thinking_visible() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn state_round_trips_per_workdir_model_and_variant() {
        let temp = tempdir().expect("temp");
        let path = temp.path().join("tui-state.json");
        let mut state = TuiState::default();
        state.set_model("/repo", "mock/model".to_string());
        state.set_variant("/repo", "high".to_string());
        state.set_mode("/repo", "plan".to_string());
        state.set_thinking_visible(false);
        state.set_sidebar_visible(true);
        state.save(&path).expect("save");

        let loaded = TuiState::load(&path).expect("load");
        assert_eq!(loaded.model_for("/repo").as_deref(), Some("mock/model"));
        assert_eq!(loaded.variant_for("/repo").as_deref(), Some("high"));
        assert_eq!(loaded.mode_for("/repo").as_deref(), Some("plan"));
        assert!(!loaded.thinking_visible);
        assert!(loaded.sidebar_visible);
        assert_eq!(loaded.version, 3);
    }

    #[test]
    fn state_can_clear_variant_override_without_schema_change() {
        let mut state = TuiState::default();
        state.set_model("/repo", "mock/model".to_string());
        state.set_variant("/repo", "high".to_string());
        state.clear_variant("/repo");

        assert_eq!(state.model_for("/repo").as_deref(), Some("mock/model"));
        assert_eq!(state.variant_for("/repo"), None);
    }

    #[test]
    fn state_tolerates_future_versions_and_bounds_recent_models() {
        let temp = tempdir().expect("temp");
        let path = temp.path().join("tui-state.json");
        std::fs::write(
            &path,
            r#"{
              "version": 99,
              "recent_models": ["a/1","b/2","c/3","d/4","e/5","f/6","g/7","h/8","i/9"]
            }"#,
        )
        .expect("state");

        let loaded = TuiState::load(&path).expect("load");
        assert_eq!(loaded.version, 3);
        assert!(loaded.thinking_visible);
        assert!(!loaded.sidebar_visible);
        assert_eq!(loaded.recent_models.len(), 8);
    }
}
