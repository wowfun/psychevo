use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::Result;

pub const MODEL_STATE_FILE: &str = "model-state.json";
pub const MODEL_STATE_VERSION: u64 = 1;
pub const MODEL_STATE_RECENT_LIMIT: usize = 8;
pub const SESSION_COMPOSER_MODEL_METADATA_KEY: &str = "composerModel";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelState {
    #[serde(default = "current_model_state_version")]
    pub version: u64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cwds: BTreeMap<String, ModelCwdState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_models: Vec<ModelRecentEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelCwdState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelRecentEntry {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_selected_at_ms: Option<i64>,
}

impl Default for ModelState {
    fn default() -> Self {
        Self {
            version: MODEL_STATE_VERSION,
            cwds: BTreeMap::new(),
            recent_models: Vec::new(),
        }
    }
}

impl ModelState {
    pub fn path_for_home(home: &Path) -> PathBuf {
        home.join(MODEL_STATE_FILE)
    }

    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut state: Self = serde_json::from_str(&fs::read_to_string(path)?)?;
        state.version = MODEL_STATE_VERSION;
        state.normalize();
        Ok(state)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_string_pretty(self)? + "\n")?;
        Ok(())
    }

    pub fn model_for(&self, cwd: &str) -> Option<String> {
        self.cwds.get(cwd).and_then(|entry| entry.model.clone())
    }

    pub fn reasoning_effort_for(&self, cwd: &str) -> Option<String> {
        self.cwds
            .get(cwd)
            .and_then(|entry| entry.reasoning_effort.clone())
    }

    pub fn set_model(
        &mut self,
        cwd: &str,
        model: impl Into<String>,
        reasoning_effort: Option<String>,
    ) {
        let model = model.into().trim().to_string();
        if model.is_empty() {
            return;
        }
        let reasoning_effort = normalize_reasoning_effort(reasoning_effort);
        let updated_at_ms = now_ms();
        let entry = self.cwds.entry(cwd.to_string()).or_default();
        entry.model = Some(model.clone());
        entry.reasoning_effort = reasoning_effort.clone();
        entry.updated_at_ms = Some(updated_at_ms);
        self.push_recent_model_with_timestamp(model, reasoning_effort, updated_at_ms);
    }

    pub fn set_reasoning_effort(&mut self, cwd: &str, reasoning_effort: Option<String>) {
        let reasoning_effort = normalize_reasoning_effort(reasoning_effort);
        let updated_at_ms = now_ms();
        let entry = self.cwds.entry(cwd.to_string()).or_default();
        entry.reasoning_effort = reasoning_effort.clone();
        entry.updated_at_ms = Some(updated_at_ms);
        if let Some(model) = entry.model.clone() {
            self.push_recent_model_with_timestamp(model, reasoning_effort, updated_at_ms);
        }
    }

    pub fn clear_cwd_model(&mut self, cwd: &str) {
        if let Some(entry) = self.cwds.get_mut(cwd) {
            entry.model = None;
            entry.reasoning_effort = None;
            entry.updated_at_ms = Some(now_ms());
        }
    }

    pub fn push_recent_model(
        &mut self,
        model: impl Into<String>,
        reasoning_effort: Option<String>,
    ) {
        self.push_recent_model_with_timestamp(
            model.into(),
            normalize_reasoning_effort(reasoning_effort),
            now_ms(),
        );
    }

    pub fn recent_model_values(&self) -> Vec<String> {
        self.recent_models
            .iter()
            .map(|entry| entry.model.clone())
            .collect()
    }

    fn push_recent_model_with_timestamp(
        &mut self,
        model: String,
        reasoning_effort: Option<String>,
        last_selected_at_ms: i64,
    ) {
        let model = model.trim().to_string();
        if model.is_empty() {
            return;
        }
        self.recent_models.retain(|entry| entry.model != model);
        self.recent_models.insert(
            0,
            ModelRecentEntry {
                model,
                reasoning_effort,
                last_selected_at_ms: Some(last_selected_at_ms),
            },
        );
        self.trim_recent_models();
    }

    fn normalize(&mut self) {
        for entry in self.cwds.values_mut() {
            entry.model = entry
                .model
                .take()
                .map(|model| model.trim().to_string())
                .filter(|model| !model.is_empty());
            entry.reasoning_effort = normalize_reasoning_effort(entry.reasoning_effort.take());
        }
        self.cwds
            .retain(|_, entry| entry.model.is_some() || entry.reasoning_effort.is_some());
        let mut normalized_recent = Vec::new();
        for mut entry in std::mem::take(&mut self.recent_models) {
            entry.model = entry.model.trim().to_string();
            entry.reasoning_effort = normalize_reasoning_effort(entry.reasoning_effort.take());
            if entry.model.is_empty()
                || normalized_recent
                    .iter()
                    .any(|seen: &ModelRecentEntry| seen.model == entry.model)
            {
                continue;
            }
            normalized_recent.push(entry);
        }
        self.recent_models = normalized_recent;
        self.trim_recent_models();
    }

    fn trim_recent_models(&mut self) {
        self.recent_models.truncate(MODEL_STATE_RECENT_LIMIT);
    }
}

pub fn normalize_reasoning_effort(reasoning_effort: Option<String>) -> Option<String> {
    reasoning_effort
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && value != "none")
}

pub fn current_model_state_version() -> u64 {
    MODEL_STATE_VERSION
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn model_state_round_trips_per_cwd_selection() {
        let temp = tempdir().expect("temp");
        let path = temp.path().join("model-state.json");
        let mut state = ModelState::default();
        state.set_model("/repo", "mock/model", Some("high".to_string()));
        state.save(&path).expect("save");

        let loaded = ModelState::load(&path).expect("load");
        assert_eq!(loaded.model_for("/repo").as_deref(), Some("mock/model"));
        assert_eq!(
            loaded.reasoning_effort_for("/repo").as_deref(),
            Some("high")
        );
        assert_eq!(loaded.recent_model_values(), vec!["mock/model"]);
        assert_eq!(loaded.version, MODEL_STATE_VERSION);
    }

    #[test]
    fn missing_model_state_defaults_empty() {
        let temp = tempdir().expect("temp");
        let loaded = ModelState::load(&temp.path().join("missing.json")).expect("load");

        assert_eq!(loaded, ModelState::default());
    }

    #[test]
    fn none_reasoning_is_not_persisted_as_override() {
        let mut state = ModelState::default();
        state.set_model("/repo", "mock/model", Some("none".to_string()));

        assert_eq!(state.reasoning_effort_for("/repo"), None);
        assert_eq!(state.recent_models[0].reasoning_effort, None);
    }

    #[test]
    fn recent_models_are_deduped_and_bounded() {
        let mut state = ModelState::default();
        for index in 0..(MODEL_STATE_RECENT_LIMIT + 2) {
            state.push_recent_model(format!("provider/model-{index}"), None);
        }
        state.push_recent_model("provider/model-4", Some("low".to_string()));

        assert_eq!(state.recent_models.len(), MODEL_STATE_RECENT_LIMIT);
        assert_eq!(state.recent_models[0].model, "provider/model-4");
        assert_eq!(
            state.recent_models[0].reasoning_effort.as_deref(),
            Some("low")
        );
        assert_eq!(
            state
                .recent_models
                .iter()
                .filter(|entry| entry.model == "provider/model-4")
                .count(),
            1
        );
    }
}
