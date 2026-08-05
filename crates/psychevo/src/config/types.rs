use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use super::{ImageGenerationFormat, RunConfig, VoiceAudioFormat, VoiceRealtimeTransport};
use crate::types::{ModelMetadata, ProjectContextInstructionMode};
use crate::{Error, Result};

#[derive(Debug, Clone, Default)]
pub(crate) struct ModelSelection {
    pub(crate) id: Option<String>,
    pub(crate) provider: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ConfigProviderEntry {
    pub(crate) name: Option<String>,
    pub(crate) api: Option<String>,
    pub(crate) api_key_env: Option<String>,
    pub(crate) no_auth: bool,
    pub(crate) inference_idle_timeout_secs: Option<u64>,
    pub(crate) models: BTreeMap<String, ConfigModelEntry>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ConfigModelEntry {
    pub(crate) name: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) metadata: ModelMetadata,
}

#[derive(Debug, Clone)]
pub(crate) struct CompressionConfig {
    pub(crate) enabled: bool,
    pub(crate) auto: bool,
    pub(crate) threshold_percent: f64,
    pub(crate) reserve_tokens: u64,
    pub(crate) keep_recent_tokens: u64,
    pub(crate) model: ModelSelection,
    pub(crate) model_configured: bool,
    pub(crate) reasoning_effort: Option<String>,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto: true,
            threshold_percent: 70.0,
            reserve_tokens: 16_384,
            keep_recent_tokens: 20_000,
            model: ModelSelection::default(),
            model_configured: false,
            reasoning_effort: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AuxiliaryConfig {
    pub(crate) title_generation: AuxiliaryTaskConfig,
    pub(crate) compression: AuxiliaryTaskConfig,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AuxiliaryTaskConfig {
    pub(crate) provider: Option<String>,
    pub(crate) model: ModelSelection,
    pub(crate) model_configured: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct LspConfig {
    pub(crate) enabled: bool,
    pub(crate) wait_mode: String,
    pub(crate) wait_timeout_secs: f64,
    pub(crate) install_strategy: String,
}

impl Default for LspConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            wait_mode: "document".to_string(),
            wait_timeout_secs: 5.0,
            install_strategy: "auto".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ProjectContextConfig {
    pub(crate) instructions: ProjectContextInstructionMode,
}

pub const DEFAULT_WORKSPACE_ROOT: &str = "~/workspaces";
pub const DEFAULT_WORKSPACE_NAME: &str = "general";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacesConfig {
    pub root: String,
}

impl Default for WorkspacesConfig {
    fn default() -> Self {
        Self {
            root: DEFAULT_WORKSPACE_ROOT.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeProfileKind {
    Native,
    Acp,
}

impl RuntimeProfileKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Acp => "acp",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "native" => Some(Self::Native),
            "acp" => Some(Self::Acp),
            _ => None,
        }
    }
}

pub fn generated_runtime_profile_id_for_backend(backend_id: &str) -> String {
    match backend_id {
        "codex" | "opencode" => backend_id.to_string(),
        _ => format!("acp:{backend_id}"),
    }
}

pub fn validate_runtime_profile_backend_ref(
    profile_id: &str,
    runtime: RuntimeProfileKind,
    backend_ref: Option<&str>,
) -> Result<()> {
    match (
        runtime,
        backend_ref.map(str::trim).filter(|value| !value.is_empty()),
    ) {
        (RuntimeProfileKind::Acp, None) => Err(Error::Config(format!(
            "runtime_profiles.{profile_id}.backend_ref is required when runtime is acp"
        ))),
        (RuntimeProfileKind::Acp, Some(_)) | (_, None) => Ok(()),
        (_, Some(_)) => Err(Error::Config(format!(
            "runtime_profiles.{profile_id}.backend_ref is only allowed when runtime is acp"
        ))),
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RuntimeProfileConfig {
    pub id: String,
    pub runtime: RuntimeProfileKind,
    pub enabled: bool,
    pub label: String,
    pub backend_ref: Option<String>,
    pub default_model: Option<String>,
    pub default_mode: Option<String>,
    pub default_agent: Option<String>,
    pub sandbox: Option<String>,
    pub workspace_roots: Vec<String>,
    pub options: Value,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ToolSelectionConfig {
    pub(crate) modes: BTreeMap<String, ToolModeConfig>,
    pub(crate) tool_search: ToolSearchConfig,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolSearchConfig {
    pub(crate) enabled: bool,
    pub(crate) default_limit: usize,
    pub(crate) max_limit: usize,
}

impl Default for ToolSearchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_limit: 8,
            max_limit: 20,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ToolModeConfig {
    pub(crate) enabled_toolsets: Option<Vec<String>>,
    pub(crate) disabled_toolsets: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchExecution {
    #[default]
    Auto,
    Local,
    Hosted,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchBackend {
    #[default]
    Auto,
    Searxng,
    Brave,
    Exa,
    Parallel,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchExternalAccess {
    #[default]
    Live,
    Cached,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchContextSize {
    Low,
    #[default]
    Medium,
    High,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchTokenBudget {
    #[default]
    Default,
    Unlimited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchContentType {
    Text,
    Image,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WebSearchLocation {
    pub country: String,
    pub region: String,
    pub city: String,
    pub timezone: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WebSearchImageConfig {
    pub max_results: usize,
    pub caption: bool,
}

impl Default for WebSearchImageConfig {
    fn default() -> Self {
        Self {
            max_results: 3,
            caption: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WebSearchConfig {
    pub execution: WebSearchExecution,
    pub backend: WebSearchBackend,
    pub external_access: WebSearchExternalAccess,
    pub context_size: WebSearchContextSize,
    pub return_token_budget: WebSearchTokenBudget,
    pub content_types: Vec<WebSearchContentType>,
    pub allowed_domains: Vec<String>,
    pub blocked_domains: Vec<String>,
    pub background_storage_acknowledged: bool,
    pub location: WebSearchLocation,
    pub image: WebSearchImageConfig,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            execution: WebSearchExecution::Local,
            backend: WebSearchBackend::Exa,
            external_access: WebSearchExternalAccess::Live,
            context_size: WebSearchContextSize::Medium,
            return_token_budget: WebSearchTokenBudget::Default,
            content_types: vec![WebSearchContentType::Text],
            allowed_domains: Vec::new(),
            blocked_domains: Vec::new(),
            background_storage_acknowledged: false,
            location: WebSearchLocation::default(),
            image: WebSearchImageConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WebConfig {
    pub search: WebSearchConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CustomToolsetConfig {
    pub(crate) description: Option<String>,
    pub(crate) tools: Vec<String>,
    pub(crate) includes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolsetContribution {
    pub(crate) source_id: String,
    pub(crate) source_kind: String,
    pub(crate) name: String,
    pub(crate) config: CustomToolsetConfig,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PluginPolicyConfig {
    pub(crate) plugins: BTreeMap<String, PluginPolicyEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodexPluginsConfig {
    pub enabled: bool,
    pub binary: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BuiltinPluginPolicyConfig {
    pub(crate) entries: BTreeMap<String, PluginPolicyEntry>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PluginPolicyEntry {
    pub(crate) enabled: Option<bool>,
}

impl PluginPolicyEntry {
    pub(crate) fn plugin_enabled(&self) -> bool {
        self.enabled.unwrap_or(false)
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ChannelsConfig {
    pub(crate) connections: Vec<ChannelConnectionConfig>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct VoiceConfig {
    pub(crate) asr: VoiceAsrConfig,
    pub(crate) tts: VoiceTtsConfig,
    pub(crate) realtime: Option<VoiceRealtimeConfig>,
}

#[derive(Debug, Clone)]
pub(crate) struct VoiceAsrConfig {
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) language: Option<String>,
}

impl Default for VoiceAsrConfig {
    fn default() -> Self {
        Self {
            provider: "xiaomi-token-plan".to_string(),
            model: "mimo-v2.5-asr".to_string(),
            language: Some("auto".to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct VoiceTtsConfig {
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) voice: String,
    pub(crate) format: VoiceAudioFormat,
}

impl Default for VoiceTtsConfig {
    fn default() -> Self {
        Self {
            provider: "xiaomi-token-plan".to_string(),
            model: "mimo-v2.5-tts".to_string(),
            voice: "mimo_default".to_string(),
            format: VoiceAudioFormat::Wav,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct VoiceRealtimeConfig {
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) transport: VoiceRealtimeTransport,
    pub(crate) voice: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ImageGenerationConfig {
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) size: String,
    pub(crate) format: ImageGenerationFormat,
}

impl Default for ImageGenerationConfig {
    fn default() -> Self {
        Self {
            provider: "openai".to_string(),
            model: "gpt-image-2".to_string(),
            size: "1024x1024".to_string(),
            format: ImageGenerationFormat::Png,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedVoiceAsrConfig {
    pub provider: String,
    pub display_label: String,
    pub model: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
    pub api_key: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedVoiceTtsConfig {
    pub provider: String,
    pub display_label: String,
    pub model: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
    pub api_key: Option<String>,
    pub voice: String,
    pub format: VoiceAudioFormat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedImageGenerationConfig {
    pub provider: String,
    pub display_label: String,
    pub model: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
    pub api_key: Option<String>,
    pub size: String,
    pub format: ImageGenerationFormat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedVoiceRealtimeConfig {
    pub provider: String,
    pub display_label: String,
    pub model: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
    pub api_key: Option<String>,
    pub transport: VoiceRealtimeTransport,
    pub voice: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChannelConnectionConfig {
    pub(crate) id: String,
    pub(crate) platform: ChannelPlatform,
    pub(crate) domain: Option<String>,
    pub(crate) enabled: bool,
    pub(crate) label: String,
    pub(crate) transport: ChannelTransport,
    pub(crate) cwd: Option<String>,
    pub(crate) runtime_ref: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) permission_mode: Option<String>,
    pub(crate) require_mention: bool,
    pub(crate) credential_env: Option<String>,
    pub(crate) app_id_env: Option<String>,
    pub(crate) app_secret_env: Option<String>,
    pub(crate) account_env: Option<String>,
    pub(crate) base_url_env: Option<String>,
    pub(crate) allow_users: Vec<String>,
    pub(crate) allow_groups: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChannelPlatform {
    Wechat,
    Telegram,
    Feishu,
    Lark,
}

impl ChannelPlatform {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "wechat" => Some(Self::Wechat),
            "telegram" => Some(Self::Telegram),
            "feishu" => Some(Self::Feishu),
            "lark" => Some(Self::Lark),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Wechat => "wechat",
            Self::Telegram => "telegram",
            Self::Feishu => "feishu",
            Self::Lark => "lark",
        }
    }

    pub(crate) fn default_domain(self) -> &'static str {
        match self {
            Self::Wechat => "wechat",
            Self::Telegram => "telegram",
            Self::Feishu => "feishu",
            Self::Lark => "lark",
        }
    }

    pub(crate) fn default_transport(self) -> ChannelTransport {
        match self {
            Self::Wechat | Self::Telegram => ChannelTransport::Polling,
            Self::Feishu | Self::Lark => ChannelTransport::LongConnection,
        }
    }

    pub(crate) fn default_label(self) -> &'static str {
        match self {
            Self::Wechat => "WeChat",
            Self::Telegram => "Telegram",
            Self::Feishu => "Feishu",
            Self::Lark => "Lark",
        }
    }

    pub(crate) fn default_credential_env(self) -> &'static str {
        match self {
            Self::Wechat => "WECHAT_BOT_TOKEN",
            Self::Telegram => "TELEGRAM_BOT_TOKEN",
            Self::Feishu => "FEISHU_APP_SECRET",
            Self::Lark => "LARK_APP_SECRET",
        }
    }

    pub(crate) fn default_app_id_env(self) -> Option<&'static str> {
        match self {
            Self::Feishu => Some("FEISHU_APP_ID"),
            Self::Lark => Some("LARK_APP_ID"),
            _ => None,
        }
    }

    pub(crate) fn default_account_env(self) -> Option<&'static str> {
        match self {
            Self::Wechat => Some("WECHAT_ACCOUNT_ID"),
            _ => None,
        }
    }

    pub(crate) fn default_base_url_env(self) -> Option<&'static str> {
        match self {
            Self::Wechat => Some("WECHAT_ILINK_BASE_URL"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChannelTransport {
    Polling,
    Webhook,
    LongConnection,
}

impl ChannelTransport {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "polling" => Some(Self::Polling),
            "webhook" => Some(Self::Webhook),
            "long_connection" => Some(Self::LongConnection),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Polling => "polling",
            Self::Webhook => "webhook",
            Self::LongConnection => "long_connection",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BuiltInProvider {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) api: Option<&'static str>,
    pub(crate) allow_no_auth: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedRunProvider {
    pub(crate) provider: String,
    pub(crate) display_label: String,
    pub(crate) model: String,
    pub(crate) base_url: String,
    pub(crate) api_key_env: Option<String>,
    pub(crate) api_key: String,
    pub(crate) inference_idle_timeout_secs: u64,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) context_limit: Option<u64>,
    pub(crate) metadata: ModelMetadata,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedCompressionConfig {
    pub(crate) model_configured: bool,
    pub(crate) provider: ResolvedRunProvider,
}

#[derive(Debug, Clone)]
pub(crate) struct LoadedRunConfig {
    pub(crate) config: RunConfig,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) sources: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct LoadedConfigValue {
    pub(crate) value: Value,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) sources: Vec<PathBuf>,
}

pub(crate) const AUTO_PROVIDER_ORDER: &[&str] = &[
    "openrouter",
    "openai",
    "anthropic",
    "opencode-zen",
    "xai",
    "zai",
    "deepseek",
    "dashscope",
    "xiaomi",
    "xiaomi-token-plan",
    "lmstudio",
    "custom",
];

pub const REASONING_EFFORT_VALUES: &[&str] =
    &["none", "minimal", "low", "medium", "high", "xhigh", "max"];
pub(crate) const MODEL_CATALOG_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) const BUILT_IN_PROVIDERS: &[BuiltInProvider] = &[
    BuiltInProvider {
        id: "openrouter",
        name: "OpenRouter",
        api: Some("https://openrouter.ai/api/v1"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "openai",
        name: "OpenAI",
        api: Some("https://api.openai.com/v1"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "anthropic",
        name: "Anthropic",
        api: Some("https://api.anthropic.com"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "opencode-zen",
        name: "OpenCode Zen",
        api: Some("https://opencode.ai/zen/v1"),
        allow_no_auth: true,
    },
    BuiltInProvider {
        id: "xai",
        name: "xAI",
        api: Some("https://api.x.ai/v1"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "zai",
        name: "Z.AI / GLM",
        api: Some("https://api.z.ai/api/paas/v4"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "deepseek",
        name: "DeepSeek",
        api: Some("https://api.deepseek.com/v1"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "dashscope",
        name: "Alibaba Cloud DashScope",
        api: Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "xiaomi",
        name: "Xiaomi MiMo",
        api: Some("https://api.xiaomimimo.com/v1"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "xiaomi-token-plan",
        name: "Xiaomi Token Plan",
        api: Some("https://token-plan-cn.xiaomimimo.com/v1"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "lmstudio",
        name: "LM Studio",
        api: Some("http://127.0.0.1:1234/v1"),
        allow_no_auth: true,
    },
    BuiltInProvider {
        id: "custom",
        name: "Custom",
        api: None,
        allow_no_auth: false,
    },
];

// Loading and source-merging are owned by config/loading.rs.
