use serde::{Deserialize, Serialize};

/// The Codex plugin behavior snapshot covered by Psychevo's conformance suite.
pub const CODEX_PLUGIN_COMPATIBILITY_PROFILE: &str = "codex-plugin/8604689e";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginAuthority {
    Psychevo { selector: String },
    Codex { plugin: String, marketplace: String },
}

impl PluginAuthority {
    pub fn canonical_id(&self) -> String {
        match self {
            Self::Psychevo { selector } => selector.clone(),
            Self::Codex {
                plugin,
                marketplace,
            } => format!("{plugin}@{marketplace}"),
        }
    }

    pub fn owner(&self) -> &'static str {
        match self {
            Self::Psychevo { .. } => "psychevo",
            Self::Codex { .. } => "codex",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginComponentKind {
    Skills,
    McpServers,
    Hooks,
    Apps,
    Interface,
    Runtime,
    Agents,
    Toolsets,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCompatibilityLevel {
    Parse,
    Inspect,
    Install,
    Project,
    Execute,
    Delegate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginExecutionOwner {
    PsychevoNative,
    PsychevoWorker,
    CodexBroker,
    MetadataOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginReadiness {
    Ready,
    Disabled,
    NeedsTrust,
    NeedsAuth,
    NeedsSetup,
    Unavailable,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginComponentStatus {
    pub component: PluginComponentKind,
    pub compatibility_profile: String,
    pub highest_level: PluginCompatibilityLevel,
    pub execution_owner: PluginExecutionOwner,
    pub readiness: PluginReadiness,
    pub reason: String,
}

impl PluginComponentStatus {
    pub(crate) fn new(
        component: PluginComponentKind,
        highest_level: PluginCompatibilityLevel,
        execution_owner: PluginExecutionOwner,
        readiness: PluginReadiness,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            component,
            compatibility_profile: CODEX_PLUGIN_COMPATIBILITY_PROFILE.to_string(),
            highest_level,
            execution_owner,
            readiness,
            reason: reason.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_identity_is_owner_qualified() {
        let psychevo = PluginAuthority::Psychevo {
            selector: "profile:review@local-source".to_string(),
        };
        let codex = PluginAuthority::Codex {
            plugin: "review".to_string(),
            marketplace: "openai".to_string(),
        };

        assert_eq!(psychevo.owner(), "psychevo");
        assert_eq!(psychevo.canonical_id(), "profile:review@local-source");
        assert_eq!(codex.owner(), "codex");
        assert_eq!(codex.canonical_id(), "review@openai");
        assert_ne!(psychevo.canonical_id(), codex.canonical_id());
    }
}
