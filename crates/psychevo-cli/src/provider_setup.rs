use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Result, anyhow};
use psychevo_runtime::{custom_provider_api_key_env, remove_config_value, set_config_value};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderSetupPresetId {
    DeepSeek,
    Zai,
    XiaomiTokenPlan,
    OpenCodeZen,
    Custom,
}

impl ProviderSetupPresetId {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek",
            Self::Zai => "zai",
            Self::XiaomiTokenPlan => "xiaomi-token-plan",
            Self::OpenCodeZen => "opencode-zen",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ProviderSetupBaseUrl {
    pub(crate) label: &'static str,
    pub(crate) url: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ProviderSetupPreset {
    pub(crate) id: ProviderSetupPresetId,
    pub(crate) label: &'static str,
    pub(crate) provider_id: Option<&'static str>,
    pub(crate) default_model: &'static str,
    pub(crate) base_urls: &'static [ProviderSetupBaseUrl],
    pub(crate) api_key_env_candidates: &'static [&'static str],
}

const DEEPSEEK_BASE_URLS: &[ProviderSetupBaseUrl] = &[ProviderSetupBaseUrl {
    label: "Default",
    url: "https://api.deepseek.com/v1",
}];

const ZAI_BASE_URLS: &[ProviderSetupBaseUrl] = &[
    ProviderSetupBaseUrl {
        label: "General API",
        url: "https://api.z.ai/api/paas/v4",
    },
    ProviderSetupBaseUrl {
        label: "Coding Plan",
        url: "https://api.z.ai/api/coding/paas/v4",
    },
];

const XIAOMI_TOKEN_PLAN_BASE_URLS: &[ProviderSetupBaseUrl] = &[
    ProviderSetupBaseUrl {
        label: "China Cluster",
        url: "https://token-plan-cn.xiaomimimo.com/v1",
    },
    ProviderSetupBaseUrl {
        label: "Singapore Cluster",
        url: "https://token-plan-sgp.xiaomimimo.com/v1",
    },
    ProviderSetupBaseUrl {
        label: "Europe Cluster",
        url: "https://token-plan-ams.xiaomimimo.com/v1",
    },
];

const OPENCODE_ZEN_BASE_URLS: &[ProviderSetupBaseUrl] = &[ProviderSetupBaseUrl {
    label: "Default",
    url: "https://opencode.ai/zen/v1",
}];

const CUSTOM_BASE_URLS: &[ProviderSetupBaseUrl] = &[ProviderSetupBaseUrl {
    label: "Custom",
    url: "http://127.0.0.1:1234/v1",
}];

const PROVIDER_SETUP_PRESETS: &[ProviderSetupPreset] = &[
    ProviderSetupPreset {
        id: ProviderSetupPresetId::DeepSeek,
        label: "DeepSeek",
        provider_id: Some("deepseek"),
        default_model: "deepseek-chat",
        base_urls: DEEPSEEK_BASE_URLS,
        api_key_env_candidates: &["DEEPSEEK_API_KEY"],
    },
    ProviderSetupPreset {
        id: ProviderSetupPresetId::Zai,
        label: "Z.AI / GLM",
        provider_id: Some("zai"),
        default_model: "glm-5.2",
        base_urls: ZAI_BASE_URLS,
        api_key_env_candidates: &["GLM_API_KEY", "ZAI_API_KEY", "Z_AI_API_KEY"],
    },
    ProviderSetupPreset {
        id: ProviderSetupPresetId::XiaomiTokenPlan,
        label: "Xiaomi Token Plan",
        provider_id: Some("xiaomi-token-plan"),
        default_model: "mimo-v2.5-pro",
        base_urls: XIAOMI_TOKEN_PLAN_BASE_URLS,
        api_key_env_candidates: &[
            "XIAOMI_TOKEN_PLAN_API_KEY",
            "XIAOMI_TOKEN_PLAN_CN_API_KEY",
            "XIAOMI_API_KEY",
        ],
    },
    ProviderSetupPreset {
        id: ProviderSetupPresetId::OpenCodeZen,
        label: "OpenCode Zen",
        provider_id: Some("opencode-zen"),
        default_model: "mimo-v2.5-free",
        base_urls: OPENCODE_ZEN_BASE_URLS,
        api_key_env_candidates: &["OPENCODE_ZEN_API_KEY"],
    },
    ProviderSetupPreset {
        id: ProviderSetupPresetId::Custom,
        label: "Custom OpenAI-compatible",
        provider_id: None,
        default_model: "",
        base_urls: CUSTOM_BASE_URLS,
        api_key_env_candidates: &[],
    },
];

pub(crate) fn provider_setup_presets() -> &'static [ProviderSetupPreset] {
    PROVIDER_SETUP_PRESETS
}

pub(crate) fn provider_setup_preset(id: ProviderSetupPresetId) -> &'static ProviderSetupPreset {
    PROVIDER_SETUP_PRESETS
        .iter()
        .find(|preset| preset.id == id)
        .expect("provider setup preset is defined")
}

pub(crate) fn default_provider_setup_api_key_env(
    candidates: &[&str],
    env_map: &BTreeMap<String, String>,
    provider_id: &str,
) -> String {
    candidates
        .iter()
        .find(|candidate| {
            env_map
                .get(**candidate)
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
        })
        .map(|candidate| (*candidate).to_string())
        .or_else(|| candidates.first().map(|candidate| (*candidate).to_string()))
        .unwrap_or_else(|| custom_provider_api_key_env(provider_id))
}

pub(crate) fn upsert_provider_options(
    config_dir: &Path,
    provider_id: &str,
    label: &str,
    base_url: &str,
    api_key_env: &str,
) -> Result<()> {
    let base_url = validate_base_url(base_url)?;
    let api_key_env = validate_api_key_env(api_key_env)?;
    let config_dir = config_dir.to_path_buf();
    let _ = remove_config_value(config_dir.clone(), &format!("provider.{provider_id}.label"))?;
    let _ = remove_config_value(
        config_dir.clone(),
        &format!("provider.{provider_id}.options"),
    )?;
    set_config_value(
        config_dir.clone(),
        &format!("provider.{provider_id}.name"),
        json!(label.trim()),
    )?;
    set_config_value(
        config_dir.clone(),
        &format!("provider.{provider_id}.api"),
        json!(base_url),
    )?;
    set_config_value(
        config_dir.clone(),
        &format!("provider.{provider_id}.api_key_env"),
        json!(api_key_env),
    )?;
    let _ = remove_config_value(config_dir, &format!("provider.{provider_id}.no_auth"))?;
    Ok(())
}

pub(crate) fn validate_base_url(value: &str) -> Result<String> {
    let value = value.trim().trim_end_matches('/').to_string();
    if value.starts_with("http://") || value.starts_with("https://") {
        Ok(value)
    } else {
        Err(anyhow!("base url must start with http:// or https://"))
    }
}

pub(crate) fn validate_api_key_env(value: &str) -> Result<String> {
    let value = value.trim();
    if valid_env_name(value) {
        Ok(value.to_string())
    } else {
        Err(anyhow!(
            "api_key_env must be a valid environment variable name"
        ))
    }
}

pub(crate) fn validate_custom_setup_provider_id(provider_id: &str) -> Result<()> {
    if !valid_provider_id(provider_id) {
        return Err(anyhow!(
            "must use lowercase letters, numbers, hyphens, or underscores"
        ));
    }
    let normalized = normalize_setup_provider_id(provider_id);
    if normalized != provider_id || SETUP_BUILT_IN_PROVIDER_IDS.contains(&provider_id) {
        return Err(anyhow!("collides with a built-in provider or alias"));
    }
    Ok(())
}

pub(crate) fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('A'..='Z' | 'a'..='z' | '_'))
        && chars.all(|ch| matches!(ch, 'A'..='Z' | 'a'..='z' | '0'..='9' | '_'))
}

pub(crate) fn looks_like_api_key(value: &str) -> bool {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    if ["sk-", "sk-proj-", "sk-live-", "sk-ant-", "sk-or-"]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        return true;
    }

    let len = value.chars().count();
    if len < 32 {
        return false;
    }
    let token_like = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '='));
    if !token_like {
        return false;
    }
    let all_upper_env_like = valid_env_name(value)
        && value.contains('_')
        && value
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_digit() || ch.is_ascii_uppercase());
    if all_upper_env_like {
        return false;
    }
    value.chars().any(|ch| ch.is_ascii_lowercase())
        || value.chars().filter(|ch| ch.is_ascii_digit()).count() >= 6
}

pub(crate) fn is_loopback_base_url(value: &str) -> bool {
    value.starts_with("http://127.0.0.1")
        || value.starts_with("http://localhost")
        || value.starts_with("http://[::1]")
}

fn valid_provider_id(provider_id: &str) -> bool {
    let mut chars = provider_id.chars();
    matches!(chars.next(), Some('a'..='z' | '0'..='9'))
        && chars.all(|ch| matches!(ch, 'a'..='z' | '0'..='9' | '-' | '_'))
}

fn normalize_setup_provider_id(provider: &str) -> String {
    match provider.trim().to_lowercase().as_str() {
        "z.ai" | "z-ai" | "glm" => "zai".to_string(),
        "alibaba" | "qwen" => "dashscope".to_string(),
        "mimo" => "xiaomi".to_string(),
        "x-ai" | "x.ai" | "grok" => "xai".to_string(),
        "lm-studio" | "lm_studio" => "lmstudio".to_string(),
        other => other.to_string(),
    }
}

const SETUP_BUILT_IN_PROVIDER_IDS: &[&str] = &[
    "openrouter",
    "openai",
    "xai",
    "zai",
    "deepseek",
    "dashscope",
    "xiaomi",
    "xiaomi-token-plan",
    "lmstudio",
    "custom",
];
