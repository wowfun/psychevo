#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Debug, Clone, Default)]
pub(crate) struct ModelSelection {
    pub(crate) id: Option<String>,
    pub(crate) provider: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ConfigProviderEntry {
    pub(crate) label: Option<String>,
    pub(crate) options: ConfigProviderOptions,
    pub(crate) models: BTreeMap<String, ConfigModelEntry>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ConfigProviderOptions {
    pub(crate) base_url: Option<String>,
    pub(crate) api_key_env: Option<String>,
    pub(crate) no_auth: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ConfigModelEntry {
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

#[derive(Debug, Clone, Default)]
pub(crate) struct ToolSelectionConfig {
    pub(crate) modes: BTreeMap<String, ToolModeConfig>,
    pub(crate) tool_search: ToolSearchConfig,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolSearchConfig {
    pub(crate) enabled: bool,
}

impl Default for ToolSearchConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ToolModeConfig {
    pub(crate) enabled_toolsets: Option<Vec<String>>,
    pub(crate) disabled_toolsets: Vec<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChannelConnectionConfig {
    pub(crate) id: String,
    pub(crate) platform: ChannelPlatform,
    pub(crate) domain: Option<String>,
    pub(crate) enabled: bool,
    pub(crate) label: String,
    pub(crate) transport: ChannelTransport,
    pub(crate) cwd: Option<String>,
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
    pub(crate) label: &'static str,
    pub(crate) base_url: Option<&'static str>,
    pub(crate) api_key_envs: &'static [&'static str],
    pub(crate) base_url_env: Option<&'static str>,
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
        label: "OpenRouter",
        base_url: Some("https://openrouter.ai/api/v1"),
        api_key_envs: &["OPENROUTER_API_KEY", "OPENAI_API_KEY"],
        base_url_env: Some("OPENROUTER_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "openai",
        label: "OpenAI",
        base_url: Some("https://api.openai.com/v1"),
        api_key_envs: &["OPENAI_API_KEY"],
        base_url_env: Some("OPENAI_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "opencode-zen",
        label: "OpenCode Zen",
        base_url: Some("https://opencode.ai/zen/v1"),
        api_key_envs: &["OPENCODE_ZEN_API_KEY"],
        base_url_env: Some("OPENCODE_ZEN_BASE_URL"),
        allow_no_auth: true,
    },
    BuiltInProvider {
        id: "xai",
        label: "xAI",
        base_url: Some("https://api.x.ai/v1"),
        api_key_envs: &["XAI_API_KEY"],
        base_url_env: Some("XAI_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "zai",
        label: "Z.AI / GLM",
        base_url: Some("https://api.z.ai/api/paas/v4"),
        api_key_envs: &["GLM_API_KEY", "ZAI_API_KEY", "Z_AI_API_KEY"],
        base_url_env: Some("GLM_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "deepseek",
        label: "DeepSeek",
        base_url: Some("https://api.deepseek.com/v1"),
        api_key_envs: &["DEEPSEEK_API_KEY"],
        base_url_env: Some("DEEPSEEK_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "dashscope",
        label: "Alibaba Cloud DashScope",
        base_url: Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1"),
        api_key_envs: &["DASHSCOPE_API_KEY"],
        base_url_env: Some("DASHSCOPE_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "xiaomi",
        label: "Xiaomi MiMo",
        base_url: Some("https://api.xiaomimimo.com/v1"),
        api_key_envs: &["XIAOMI_API_KEY"],
        base_url_env: Some("XIAOMI_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "xiaomi-token-plan",
        label: "Xiaomi Token Plan",
        base_url: Some("https://token-plan-cn.xiaomimimo.com/v1"),
        api_key_envs: &[
            "XIAOMI_TOKEN_PLAN_API_KEY",
            "XIAOMI_TOKEN_PLAN_CN_API_KEY",
            "XIAOMI_API_KEY",
        ],
        base_url_env: Some("XIAOMI_TOKEN_PLAN_BASE_URL"),
        allow_no_auth: false,
    },
    BuiltInProvider {
        id: "lmstudio",
        label: "LM Studio",
        base_url: Some("http://127.0.0.1:1234/v1"),
        api_key_envs: &["LM_API_KEY"],
        base_url_env: Some("LM_BASE_URL"),
        allow_no_auth: true,
    },
    BuiltInProvider {
        id: "custom",
        label: "Custom",
        base_url: None,
        api_key_envs: &[],
        base_url_env: None,
        allow_no_auth: false,
    },
];

pub(crate) fn load_run_config(options: &RunOptions, cwd: &Path) -> Result<LoadedRunConfig> {
    let loaded = load_config_value(options, cwd)?;
    let mut config = parse_run_config(loaded.value)?;
    if let Some(mode) = options.project_context_override {
        config.project_context.instructions = mode;
    }
    if let Some(sandbox) = &options.sandbox_override {
        config.sandbox = crate::sandbox::SandboxConfig {
            enabled: sandbox.enabled,
            mode: match sandbox.mode {
                crate::types::RunSandboxMode::WorkspaceWrite => {
                    crate::sandbox::SandboxMode::WorkspaceWrite
                }
                crate::types::RunSandboxMode::ReadOnly => crate::sandbox::SandboxMode::ReadOnly,
            },
            writable_roots: sandbox.writable_roots.clone(),
            include_tmp: sandbox.include_tmp,
            include_common_caches: sandbox.include_common_caches,
        };
    }
    Ok(LoadedRunConfig {
        config,
        env: loaded.env,
    })
}

pub(crate) fn load_plugin_policy_config_lenient(
    options: &RunOptions,
    cwd: &Path,
) -> Result<(PluginPolicyConfig, BTreeMap<String, String>, PathBuf)> {
    let mut env_map = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| env::vars().collect());
    let home = resolve_psychevo_home(&env_map)?;
    let project_dir = cwd.join(".psychevo");
    let mut value = json!({});

    if let Some(config_path) = resolve_config_path(options, &env_map)? {
        deep_merge(&mut value, load_toml_config_file(&config_path, true)?);
        if let Some(parent) = config_path.parent() {
            load_dotenv_file(&parent.join(".env"), &mut env_map)?;
        }
    } else {
        deep_merge(
            &mut value,
            load_toml_config_file(&home.join(CONFIG_FILE_NAME), false)?,
        );
        load_dotenv_file(&home.join(".env"), &mut env_map)?;
        deep_merge(
            &mut value,
            load_toml_config_file(&project_dir.join(CONFIG_FILE_NAME), false)?,
        );
    }

    load_dotenv_file(&project_dir.join(".env"), &mut env_map)?;
    let plugins = value
        .get("plugins")
        .map(parse_plugin_policy_config)
        .transpose()?
        .unwrap_or_default();
    Ok((plugins, env_map, home))
}

pub fn resolve_workspace_root(options: &RunOptions, _cwd: &Path) -> Result<PathBuf> {
    let mut env_map = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| env::vars().collect());
    let value = if let Some(config_path) = resolve_config_path(options, &env_map)? {
        let value = load_toml_config_file(&config_path, true)?;
        if let Some(parent) = config_path.parent() {
            load_dotenv_file(&parent.join(".env"), &mut env_map)?;
        }
        value
    } else {
        let home = resolve_psychevo_home(&env_map)?;
        let home_config = home.join(CONFIG_FILE_NAME);
        if !home_config.exists() {
            return Err(Error::Config(format!(
                "Psychevo home is not initialized; run `pevo init` to create {}",
                home_config.display()
            )));
        }
        let value = load_toml_config_file(&home_config, true)?;
        load_dotenv_file(&home.join(".env"), &mut env_map)?;
        value
    };
    let root = value
        .get("workspaces")
        .map(parse_workspaces_config)
        .transpose()?
        .unwrap_or_default()
        .root;
    resolve_explicit_path(Path::new(&root), &env_map)
}

pub fn resolve_default_workspace_cwd(options: &RunOptions, cwd: &Path) -> Result<PathBuf> {
    Ok(resolve_workspace_root(options, cwd)?.join(DEFAULT_WORKSPACE_NAME))
}

pub(crate) fn load_project_context_instruction_mode(
    options: &RunOptions,
    cwd: &Path,
) -> Result<ProjectContextInstructionMode> {
    if let Some(mode) = options.project_context_override {
        return Ok(mode);
    }
    let env_map = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| env::vars().collect());
    let mut value = json!({});

    if let Some(config_path) = resolve_config_path(options, &env_map)? {
        deep_merge(&mut value, load_toml_config_file(&config_path, true)?);
    } else {
        if let Ok(home) = resolve_psychevo_home(&env_map) {
            deep_merge(
                &mut value,
                load_toml_config_file(&home.join(CONFIG_FILE_NAME), false)?,
            );
        }
        deep_merge(
            &mut value,
            load_toml_config_file(&cwd.join(".psychevo").join(CONFIG_FILE_NAME), false)?,
        );
    }

    value
        .get("project_context")
        .map(parse_project_context_config)
        .transpose()
        .map(|config| config.unwrap_or_default().instructions)
}

pub fn load_agent_backend_configs(
    home: &Path,
    cwd: &Path,
    env_map: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, AgentBackendConfig>> {
    let mut value = json!({});
    if let Some(config_path) = env_map
        .get("PSYCHEVO_CONFIG")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| resolve_explicit_path(Path::new(value), env_map))
        .transpose()?
    {
        deep_merge(&mut value, load_toml_config_file(&config_path, true)?);
    } else {
        deep_merge(
            &mut value,
            load_toml_config_file(&home.join(CONFIG_FILE_NAME), false)?,
        );
    }
    deep_merge(
        &mut value,
        load_toml_config_file(&cwd.join(".psychevo").join(CONFIG_FILE_NAME), false)?,
    );
    value
        .get("agents")
        .map(parse_agent_backend_configs)
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(crate) fn load_config_value(options: &RunOptions, cwd: &Path) -> Result<LoadedConfigValue> {
    let mut env_map = options
        .inherited_env
        .clone()
        .unwrap_or_else(|| env::vars().collect());
    let project_dir = cwd.join(".psychevo");
    let mut value = json!({});
    let mut sources = Vec::new();

    if let Some(config_path) = resolve_config_path(options, &env_map)? {
        let loaded = load_toml_config_file(&config_path, true)?;
        deep_merge(&mut value, loaded);
        sources.push(config_path.clone());
        if let Some(parent) = config_path.parent() {
            load_dotenv_file(&parent.join(".env"), &mut env_map)?;
        }
    } else {
        let home = resolve_psychevo_home(&env_map)?;
        let home_config = home.join(CONFIG_FILE_NAME);
        if !home_config.exists() {
            return Err(Error::Config(format!(
                "Psychevo home is not initialized; run `pevo init` to create {}",
                home_config.display()
            )));
        }
        let loaded = load_toml_config_file(&home_config, true)?;
        deep_merge(&mut value, loaded);
        sources.push(home_config);
        load_dotenv_file(&home.join(".env"), &mut env_map)?;
        let project_config = project_dir.join(CONFIG_FILE_NAME);
        let loaded = load_toml_config_file(&project_config, false)?;
        if project_config.exists() {
            sources.push(project_config);
        }
        deep_merge(&mut value, loaded);
    }

    load_dotenv_file(&project_dir.join(".env"), &mut env_map)?;
    Ok(LoadedConfigValue {
        value,
        env: env_map,
        sources,
    })
}
