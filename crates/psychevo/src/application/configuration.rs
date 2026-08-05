use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use super::Client;
#[cfg(all(test, not(feature = "native-keyring")))]
use crate::config::SystemMcpOAuthCredentialStore;
use crate::config::{
    ChannelRuntimeConnection, ConfigRemoveResult, ConfigScope, ConfigSetResult, ConfiguredModel,
    McpOAuthCredentialStore, ModelCatalogEntry, ModelCatalogProvider, PermissionRuleMutationResult,
    ToolsetMutationResult,
};
pub use crate::types::{CustomProviderResult, ModelMetadataCacheTarget};
use crate::types::{
    ProjectContextInstructionMode, RunMode, RunOptions, RunSandboxOverride,
    ScopedCustomProviderInput,
};
use crate::{Error, Result, config, hooks, plugins};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateCustomProviderRequest {
    pub provider_id: String,
    pub label: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
    pub api_key: Option<String>,
    pub require_api_key: bool,
    pub no_auth: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigureProviderRequest {
    pub provider_id: String,
    pub label: String,
    pub base_url: String,
    pub api_key_env: String,
}

#[derive(Clone)]
pub struct ConfigurationQuery {
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub inherited_env: Option<BTreeMap<String, String>>,
    pub project_context: Option<ProjectContextInstructionMode>,
    pub sandbox: Option<RunSandboxOverride>,
    profile_only: bool,
}

impl ConfigurationQuery {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            model: None,
            reasoning_effort: None,
            inherited_env: None,
            project_context: None,
            sandbox: None,
            profile_only: false,
        }
    }

    pub fn profile(cwd: impl Into<PathBuf>) -> Self {
        Self {
            profile_only: true,
            ..Self::new(cwd)
        }
    }
}

impl fmt::Debug for ConfigurationQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConfigurationQuery")
            .field("cwd", &self.cwd)
            .field("model", &self.model)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("has_inherited_env", &self.inherited_env.is_some())
            .field("project_context", &self.project_context)
            .field("sandbox", &self.sandbox)
            .field("profile_only", &self.profile_only)
            .finish()
    }
}

#[derive(Clone)]
pub struct Configuration {
    home: PathBuf,
    pub(super) options: RunOptions,
    pub(super) provider: Option<psychevo_ai::Provider>,
    mcp_oauth_credentials: Arc<dyn McpOAuthCredentialStore>,
}

impl fmt::Debug for Configuration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Configuration")
            .field("cwd", &self.options.cwd)
            .field("model", &self.options.model)
            .field("has_injected_provider", &self.provider.is_some())
            .finish_non_exhaustive()
    }
}

impl Client {
    pub fn configuration(&self, query: ConfigurationQuery) -> Result<Configuration> {
        self.ensure_open()?;
        let cwd = crate::paths::canonicalize_cwd(&query.cwd)?;
        let inherited_env = self.application_environment(query.inherited_env);
        let config_path = if query.profile_only {
            Some(self.inner.home.join(config::CONFIG_FILE_NAME))
        } else {
            self.inner.config_path.clone()
        };
        Ok(Configuration {
            home: self.inner.home.clone(),
            provider: self.inner.native_backend.provider.clone(),
            mcp_oauth_credentials: Arc::clone(&self.inner.mcp_oauth_credentials),
            options: RunOptions {
                state: self.inner.state.clone(),
                cwd,
                snapshot_root: None,
                session: None,
                continue_latest: false,
                prompt: String::new(),
                image_inputs: Vec::new(),
                extract_prompt_image_sources: false,
                prompt_display: None,
                max_context_messages: None,
                config_path,
                project_context_override: query.project_context,
                sandbox_override: query.sandbox,
                model: query.model,
                reasoning_effort: query.reasoning_effort,
                runtime_ref: None,
                runtime_session_id: None,
                runtime_options: BTreeMap::new(),
                include_reasoning: false,
                mode: RunMode::Default,
                permission_mode: None,
                approval_handler: None,
                clarify_enabled: false,
                inherited_env: Some(inherited_env),
                agent: None,
                external_agent_delegate: None,
                no_agents: false,
                no_skills: false,
                selected_capability_roots: Vec::new(),
                skill_inputs: Vec::new(),
                mcp_servers: Vec::new(),
                mcp_runtime: None,
                workspace_mutations: None,
                runtime_tools: Vec::new(),
            },
        })
    }
}

impl Configuration {
    /// Replaces the MCP OAuth credential store for this configuration handle.
    ///
    /// The override is instance-local. It is intended for embedding hosts and
    /// deterministic tests that must not access the user's native keyring.
    pub fn with_mcp_oauth_credential_store(
        mut self,
        credential_store: Arc<dyn McpOAuthCredentialStore>,
    ) -> Self {
        self.mcp_oauth_credentials = credential_store;
        self
    }

    fn mutation_directory(&self, scope: ConfigScope) -> Result<PathBuf> {
        match scope {
            ConfigScope::Global => Ok(self.home.clone()),
            ConfigScope::Local => Ok(self.options.cwd.join(".psychevo")),
            ConfigScope::Effective => Err(Error::Config(
                "configuration mutations require global or local scope".to_string(),
            )),
        }
    }

    pub fn configured_models(&self) -> Result<Vec<ConfiguredModel>> {
        config::configured_models(&self.options)
    }

    pub async fn resolve_mcp_server_handoffs(
        &self,
        names: &BTreeSet<String>,
    ) -> Result<Vec<crate::types::ResolvedMcpServerInput>> {
        let resolution = crate::extensions::McpServerResolution::new(
            self.home.clone(),
            Arc::clone(&self.mcp_oauth_credentials),
            self.options.cwd.clone(),
            self.options.config_path.clone(),
            self.options.inherited_env.clone().ok_or_else(|| {
                Error::Message("Configuration has no captured environment".to_string())
            })?,
            self.options.selected_capability_roots.clone(),
            self.options.mcp_servers.clone(),
        );
        crate::extensions::resolve_mcp_server_handoffs(&resolution, names).await
    }

    pub fn default_workspace_cwd(&self) -> Result<PathBuf> {
        config::resolve_default_workspace_cwd(&self.options, &self.options.cwd)
    }

    pub fn workspace_root(&self) -> Result<PathBuf> {
        config::resolve_workspace_root(&self.options, &self.options.cwd)
    }

    pub fn channels(&self) -> Result<Value> {
        config::channel_list_value(&self.options)
    }

    pub fn channel(&self, id: &str) -> Result<Value> {
        config::channel_show_value(&self.options, id)
    }

    pub fn diagnose_channels(&self, id: Option<&str>, live: bool) -> Result<Value> {
        config::channel_doctor_value(&self.options, id, live)
    }

    pub fn channel_summary(&self) -> Result<Value> {
        config::channel_summary_value(&self.options)
    }

    pub fn channel_runtime_connections(&self) -> Result<Vec<ChannelRuntimeConnection>> {
        config::channel_runtime_connections(&self.options, &self.options.cwd)
    }

    pub fn web_search_settings(&self) -> Result<Value> {
        config::web_search_settings_value(&self.options, &self.options.cwd)
    }

    pub fn voice_settings(&self) -> Result<Value> {
        config::voice_config_value(&self.options)
    }

    pub fn image_generation_settings(&self) -> Result<Value> {
        config::image_generation_config_value(&self.options)
    }

    pub fn model_catalog_providers(&self) -> Result<Vec<ModelCatalogProvider>> {
        config::model_catalog_providers(&self.options)
    }

    pub fn model_catalog_provider(&self, provider: &str) -> Result<Option<ModelCatalogProvider>> {
        config::model_catalog_provider(&self.options, provider)
    }

    pub async fn fetch_and_cache_model_catalog(
        &self,
        provider: &ModelCatalogProvider,
    ) -> Result<Vec<ModelCatalogEntry>> {
        config::fetch_and_cache_model_catalog(&self.home, provider).await
    }

    pub async fn fetch_model_catalog(
        &self,
        provider: &ModelCatalogProvider,
    ) -> Result<Vec<ModelCatalogEntry>> {
        config::fetch_model_catalog(provider).await
    }

    pub fn cached_model_catalog(
        &self,
        provider: &ModelCatalogProvider,
    ) -> Option<Vec<ModelCatalogEntry>> {
        config::read_cached_model_catalog(&self.home, provider)
    }

    pub fn model_catalog_cache_path(&self) -> PathBuf {
        config::provider_models_cache_path_for_home(&self.home)
    }

    pub fn sandbox_status_text(&self, mode: RunMode) -> Result<String> {
        crate::sandbox::sandbox_status_text(&self.options, mode)
    }

    pub async fn refresh_model_metadata_cache(
        &self,
        targets: Vec<ModelMetadataCacheTarget>,
    ) -> Result<()> {
        config::refresh_model_metadata_cache(
            self.home.clone(),
            self.options.inherited_env.clone().unwrap_or_default(),
            targets,
        )
        .await
    }

    pub fn selected_model(&self) -> Result<Option<ConfiguredModel>> {
        config::selected_configured_model(&self.options)
    }

    pub fn permission_rules(&self, scope: ConfigScope) -> Result<Value> {
        config::permission_rules_value(&self.options, scope)
    }

    pub fn toolsets(&self, scope: ConfigScope) -> Result<Value> {
        config::toolsets_value(&self.options, scope)
    }

    pub fn mcp_servers(&self, scope: ConfigScope) -> Result<Value> {
        config::config_mcp_management::mcp_servers_value_with_store(
            &self.options,
            scope,
            self.mcp_oauth_credentials.as_ref(),
        )
    }

    pub fn mcp_server(&self, name: &str) -> Result<Value> {
        config::config_mcp_management::mcp_server_value_with_store(
            &self.options,
            name,
            self.mcp_oauth_credentials.as_ref(),
        )
    }

    pub async fn test_mcp_server(&self, name: &str) -> Result<Value> {
        crate::mcp::mcp_test_server_value_with_store(
            &self.options,
            name,
            &self.home,
            self.mcp_oauth_credentials.as_ref(),
        )
        .await
    }

    pub fn auth_status(&self, provider: Option<&str>) -> Result<Value> {
        config::auth_status_value(&self.options, provider)
    }

    pub fn config_value(&self, scope: ConfigScope) -> Result<Value> {
        config::config_show_value(&self.options, scope)
    }

    pub fn provider_list(&self, scope: ConfigScope) -> Result<Value> {
        config::config_provider_list_value(&self.options, scope)
    }

    pub fn set_value(
        &self,
        scope: ConfigScope,
        key: &str,
        value: Value,
    ) -> Result<ConfigSetResult> {
        config::set_config_value(self.mutation_directory(scope)?, key, value)
    }

    pub fn remove_value(&self, scope: ConfigScope, key: &str) -> Result<ConfigRemoveResult> {
        config::remove_config_value(self.mutation_directory(scope)?, key)
    }

    pub fn set_provider_api_key(
        &self,
        scope: ConfigScope,
        provider: &str,
        api_key: &str,
    ) -> Result<Value> {
        config::set_provider_api_key(
            &self.options,
            self.mutation_directory(scope)?,
            provider,
            api_key,
        )
    }

    pub fn create_custom_provider(
        &self,
        scope: ConfigScope,
        request: CreateCustomProviderRequest,
    ) -> Result<CustomProviderResult> {
        config::create_scoped_custom_provider(ScopedCustomProviderInput {
            config_dir: self.mutation_directory(scope)?,
            provider_id: request.provider_id,
            label: request.label,
            base_url: request.base_url,
            api_key_env: request.api_key_env,
            api_key: request.api_key,
            require_api_key: request.require_api_key,
            no_auth: request.no_auth,
        })
    }

    pub fn configure_provider(
        &self,
        scope: ConfigScope,
        request: ConfigureProviderRequest,
    ) -> Result<()> {
        let provider_id = request.provider_id.trim();
        let label = request.label.trim();
        let base_url = request.base_url.trim().trim_end_matches('/');
        let api_key_env = request.api_key_env.trim();
        if !config::valid_provider_id(provider_id) {
            return Err(Error::Config(
                "provider id must use lowercase letters, numbers, hyphens, or underscores"
                    .to_string(),
            ));
        }
        if label.is_empty() {
            return Err(Error::Config("provider name is required".to_string()));
        }
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return Err(Error::Config(
                "base url must start with http:// or https://".to_string(),
            ));
        }
        if !config::valid_env_name(api_key_env) {
            return Err(Error::Config(
                "api_key_env must be a valid environment variable name".to_string(),
            ));
        }

        let config_dir = self.mutation_directory(scope)?;
        let _ = config::remove_config_value(
            config_dir.clone(),
            &format!("provider.{provider_id}.label"),
        )?;
        let _ = config::remove_config_value(
            config_dir.clone(),
            &format!("provider.{provider_id}.options"),
        )?;
        config::set_config_value(
            config_dir.clone(),
            &format!("provider.{provider_id}.name"),
            Value::String(label.to_string()),
        )?;
        config::set_config_value(
            config_dir.clone(),
            &format!("provider.{provider_id}.api"),
            Value::String(base_url.to_string()),
        )?;
        config::set_config_value(
            config_dir.clone(),
            &format!("provider.{provider_id}.api_key_env"),
            Value::String(api_key_env.to_string()),
        )?;
        let _ =
            config::remove_config_value(config_dir, &format!("provider.{provider_id}.no_auth"))?;
        Ok(())
    }

    pub fn set_default_model(
        &self,
        scope: ConfigScope,
        model: &str,
        reasoning_effort: Option<&str>,
    ) -> Result<Value> {
        let global = match scope {
            ConfigScope::Global => true,
            ConfigScope::Local => false,
            ConfigScope::Effective => {
                return Err(Error::Config(
                    "default-model mutation requires global or local scope".to_string(),
                ));
            }
        };
        config::set_default_model_with_reasoning(
            &self.home,
            &self.options.cwd,
            global,
            model,
            reasoning_effort,
        )
    }

    pub fn remove_local_permission_rule(
        &self,
        kind: &str,
        rule: &str,
    ) -> Result<PermissionRuleMutationResult> {
        config::remove_local_permission_rule(
            self.mutation_directory(ConfigScope::Local)?,
            kind,
            rule,
        )
    }

    pub fn set_toolset_enabled(
        &self,
        scope: ConfigScope,
        mode: RunMode,
        name: &str,
        enabled: bool,
    ) -> Result<ToolsetMutationResult> {
        config::set_local_toolset_enabled(self.mutation_directory(scope)?, mode, name, enabled)
    }

    pub fn create_toolset(
        &self,
        scope: ConfigScope,
        name: &str,
        description: Option<String>,
        tools: Vec<String>,
        includes: Vec<String>,
        force: bool,
    ) -> Result<ToolsetMutationResult> {
        config::create_local_toolset(
            self.mutation_directory(scope)?,
            name,
            description,
            tools,
            includes,
            force,
        )
    }

    pub fn remove_toolset(&self, scope: ConfigScope, name: &str) -> Result<ToolsetMutationResult> {
        config::remove_local_toolset(self.mutation_directory(scope)?, name)
    }

    pub fn hooks(&self) -> Result<Value> {
        hooks::hook_metadata_value(&self.options, &self.options.cwd)
    }

    pub fn trust_hook(&self, hook_key: &str) -> Result<Value> {
        hooks::trust_hook_in_profile(&self.options, &self.options.cwd, hook_key)
    }

    pub fn set_hook_enabled(&self, hook_key: &str, enabled: bool) -> Result<Value> {
        hooks::set_hook_enabled_in_profile(&self.options, hook_key, enabled)
    }

    pub fn plugins(&self) -> Result<Value> {
        plugins::plugin_list_value(&self.options)
    }

    pub fn plugin(&self, selector: &str) -> Result<Value> {
        plugins::plugin_view_value(&self.options, selector)
    }

    pub async fn diagnose_plugins(&self, selector: Option<&str>) -> Result<Value> {
        plugins::plugin_doctor_value(&self.options, selector).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;
    use crate::application::Application;

    #[derive(Default)]
    struct FakeMcpOAuthCredentialStore {
        tokens: Mutex<BTreeMap<String, String>>,
    }

    impl McpOAuthCredentialStore for FakeMcpOAuthCredentialStore {
        fn load_access_token(&self, account: &str) -> Result<Option<String>> {
            Ok(self
                .tokens
                .lock()
                .expect("fake MCP OAuth credentials poisoned")
                .get(account)
                .cloned())
        }

        fn save_access_token(&self, account: &str, access_token: &str) -> Result<()> {
            self.tokens
                .lock()
                .expect("fake MCP OAuth credentials poisoned")
                .insert(account.to_string(), access_token.to_string());
            Ok(())
        }

        fn clear_access_token(&self, account: &str) -> Result<bool> {
            Ok(self
                .tokens
                .lock()
                .expect("fake MCP OAuth credentials poisoned")
                .remove(account)
                .is_some())
        }
    }

    #[cfg(feature = "native-keyring")]
    #[test]
    fn supported_host_keyring_backend_is_persistent() {
        assert!(matches!(
            keyring::default::default_credential_builder().persistence(),
            keyring::credential::CredentialPersistence::UntilDelete
        ));
    }

    #[cfg(not(feature = "native-keyring"))]
    #[test]
    fn system_keyring_operations_name_the_opt_in_capability() {
        let error = SystemMcpOAuthCredentialStore
            .load_access_token("isolated-account")
            .expect_err("feature-free Framework has no ambient native credential backend");
        assert!(error.to_string().contains("`native-keyring` feature"));
    }

    #[test]
    fn mcp_oauth_credentials_use_an_injected_profile_scoped_store() {
        let store = FakeMcpOAuthCredentialStore::default();
        let profile_home = PathBuf::from("/isolated/profile");
        let url = "https://mcp.example.test/mcp";

        assert_eq!(
            config::load_mcp_oauth_access_token_with_store(&store, &profile_home, "docs", url,)
                .expect("load absent fake credential"),
            None
        );
        config::save_mcp_oauth_access_token_with_store(
            &store,
            &profile_home,
            "docs",
            url,
            "test-only-token",
        )
        .expect("save fake credential");
        assert_eq!(
            config::load_mcp_oauth_access_token_with_store(&store, &profile_home, "docs", url,)
                .expect("load fake credential")
                .as_deref(),
            Some("test-only-token")
        );
        assert_eq!(
            config::load_mcp_oauth_access_token_with_store(
                &store,
                &profile_home,
                "docs",
                "https://other.example.test/mcp",
            )
            .expect("load isolated fake credential"),
            None
        );
        assert!(
            config::clear_mcp_oauth_access_token_with_store(&store, &profile_home, "docs", url,)
                .expect("clear fake credential")
        );
        assert!(
            !config::clear_mcp_oauth_access_token_with_store(&store, &profile_home, "docs", url,)
                .expect("clear absent fake credential")
        );
    }

    #[tokio::test]
    async fn configuration_mutations_resolve_scope_from_application_context() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let cwd = temp.path().join("workspace");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let application = Application::builder()
            .home(&home)
            .build()
            .await
            .expect("application");
        let configuration = application
            .client()
            .configuration(ConfigurationQuery::new(&cwd))
            .expect("configuration");

        let global = configuration
            .set_value(ConfigScope::Global, "model", json!("fake/global"))
            .expect("global mutation");
        let local = configuration
            .set_value(ConfigScope::Local, "model", json!("fake/local"))
            .expect("local mutation");

        assert_eq!(global.path, home.join("config.toml"));
        assert_eq!(local.path, cwd.join(".psychevo/config.toml"));
        assert!(matches!(
            configuration.set_value(ConfigScope::Effective, "model", json!("invalid")),
            Err(Error::Config(message)) if message.contains("global or local")
        ));
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn configuration_reads_workspace_channels_and_web_from_the_config_owner() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let cwd = temp.path().join("workspace");
        let workspace_root = temp.path().join("shared-workspaces");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&cwd).expect("cwd");
        std::fs::write(
            home.join("config.toml"),
            format!(
                r#"
[workspaces]
root = "{}"

[[channels.connections]]
id = "release"
channel = "telegram"
label = "Release Bot"
enabled = true
credential_env = "TELEGRAM_BOT_TOKEN"
allow_users = ["42"]

[web.search]
execution = "local"
backend = "brave"

[voice.tts]
provider = "openai"
model = "gpt-4o-mini-tts"

[image_generation]
provider = "openai"
model = "gpt-image-1"

[toolsets.review]
description = "Review tools"
tools = ["read"]

[mcp_servers.docs]
transport = "streamable_http"
url = "https://mcp.example.test/mcp"

[mcp_servers.docs.oauth]
client_id = "test-client"

[mcp_servers.missing]
transport = "stdio"
command = "/definitely/missing/psychevo-mcp-test"

[audit]
owner = "global"
"#,
                workspace_root.display()
            ),
        )
        .expect("config");
        std::fs::write(
            home.join(".env"),
            "TELEGRAM_BOT_TOKEN=channel-secret\nBRAVE_SEARCH_API_KEY=search-secret\n",
        )
        .expect("environment");
        std::fs::create_dir_all(cwd.join(".psychevo")).expect("project config directory");
        std::fs::write(
            cwd.join(".psychevo/config.toml"),
            "[audit]\nowner = \"project\"\n",
        )
        .expect("project config");
        let mcp_oauth_credentials: Arc<dyn McpOAuthCredentialStore> =
            Arc::new(FakeMcpOAuthCredentialStore::default());
        config::save_mcp_oauth_access_token_with_store(
            mcp_oauth_credentials.as_ref(),
            &home,
            "docs",
            "https://mcp.example.test/mcp",
            "test-only-token",
        )
        .expect("seed fake MCP OAuth credential");
        let application = Application::builder()
            .home(&home)
            .mcp_oauth_credential_store(Arc::clone(&mcp_oauth_credentials))
            .build()
            .await
            .expect("application");
        let configuration = application
            .client()
            .configuration(ConfigurationQuery::new(&cwd))
            .expect("configuration");
        let profile_configuration = application
            .client()
            .configuration(ConfigurationQuery::profile(&cwd))
            .expect("profile configuration");

        assert_eq!(
            configuration
                .config_value(ConfigScope::Effective)
                .expect("effective")["value"]["audit"]["owner"],
            json!("project")
        );
        assert_eq!(
            profile_configuration
                .config_value(ConfigScope::Effective)
                .expect("profile effective")["value"]["audit"]["owner"],
            json!("global")
        );

        assert_eq!(
            configuration.default_workspace_cwd().expect("workspace"),
            config::resolve_default_workspace_cwd(
                &configuration.options,
                &configuration.options.cwd
            )
            .expect("owner workspace")
        );
        assert_eq!(
            configuration.workspace_root().expect("workspace root"),
            workspace_root
        );
        assert_eq!(
            configuration.channels().expect("channels"),
            config::channel_list_value(&configuration.options).expect("owner channels")
        );
        assert_eq!(
            configuration.channel("release").expect("channel"),
            config::channel_show_value(&configuration.options, "release").expect("owner channel")
        );
        assert_eq!(
            configuration
                .diagnose_channels(Some("release"), false)
                .expect("channel doctor"),
            config::channel_doctor_value(&configuration.options, Some("release"), false)
                .expect("owner channel doctor")
        );
        assert_eq!(
            configuration.channel_summary().expect("channel summary"),
            config::channel_summary_value(&configuration.options).expect("owner channel summary")
        );
        let runtime_connections = configuration
            .channel_runtime_connections()
            .expect("runtime connections");
        let owner_runtime_connections =
            config::channel_runtime_connections(&configuration.options, &configuration.options.cwd)
                .expect("owner runtime connections");
        assert_eq!(runtime_connections.len(), owner_runtime_connections.len());
        assert_eq!(runtime_connections[0].id, owner_runtime_connections[0].id);
        assert_eq!(
            runtime_connections[0].credential,
            owner_runtime_connections[0].credential
        );
        assert_eq!(
            configuration.web_search_settings().expect("web search"),
            config::web_search_settings_value(&configuration.options, &configuration.options.cwd)
                .expect("owner web search")
        );
        assert_eq!(
            configuration
                .web_search_settings()
                .expect("web search credentials")["credentials"]["brave"],
            json!("present")
        );
        assert_eq!(
            configuration.voice_settings().expect("voice"),
            config::voice_config_value(&configuration.options).expect("owner voice")
        );
        assert_eq!(
            configuration
                .image_generation_settings()
                .expect("image generation"),
            config::image_generation_config_value(&configuration.options)
                .expect("owner image generation")
        );
        let toolsets = configuration
            .toolsets(ConfigScope::Effective)
            .expect("toolsets");
        assert!(
            toolsets["toolsets"]
                .as_array()
                .expect("toolset rows")
                .iter()
                .any(|toolset| toolset["name"] == json!("review"))
        );
        let mcp_servers = configuration
            .mcp_servers(ConfigScope::Effective)
            .expect("MCP servers");
        assert_eq!(mcp_servers["count"], json!(2));
        let mcp_server = configuration.mcp_server("docs").expect("MCP server");
        assert_eq!(
            mcp_server["server"]["transport"]["url"],
            json!("https://mcp.example.test/mcp")
        );
        assert_eq!(
            mcp_server["server"]["transport"]["auth"]["storedOAuthToken"],
            json!(true)
        );
        let handoffs = configuration
            .resolve_mcp_server_handoffs(&BTreeSet::from(["docs".to_string()]))
            .await
            .expect("MCP handoff with injected OAuth credentials");
        assert_eq!(handoffs.len(), 1);
        assert_eq!(handoffs[0].bearer_token(), Some("test-only-token"));
        let test_result = configuration
            .test_mcp_server("missing")
            .await
            .expect("deterministic missing MCP server test");
        assert_eq!(test_result["ok"], json!(false));
        assert_eq!(test_result["name"], json!("missing"));

        application
            .shutdown()
            .await
            .expect("shutdown")
            .require_clean()
            .expect("clean shutdown");
    }

    #[tokio::test]
    async fn configure_provider_preserves_models_and_replaces_legacy_fields() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let cwd = temp.path().join("workspace");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&cwd).expect("cwd");
        std::fs::write(
            home.join("config.toml"),
            r#"
[provider.demo]
label = "Legacy"
no_auth = true

[provider.demo.options]
legacy = true

[provider.demo.models.keep]
name = "Keep"
"#,
        )
        .expect("config");
        let application = Application::builder()
            .home(&home)
            .build()
            .await
            .expect("application");
        let configuration = application
            .client()
            .configuration(ConfigurationQuery::new(&cwd))
            .expect("configuration");

        configuration
            .configure_provider(
                ConfigScope::Global,
                ConfigureProviderRequest {
                    provider_id: "demo".to_string(),
                    label: "Current".to_string(),
                    base_url: "https://example.test/v1/".to_string(),
                    api_key_env: "DEMO_API_KEY".to_string(),
                },
            )
            .expect("configure provider");

        let document = configuration
            .config_value(ConfigScope::Global)
            .expect("config value");
        let provider = &document["value"]["provider"]["demo"];
        assert_eq!(provider["name"], json!("Current"));
        assert_eq!(provider["api"], json!("https://example.test/v1"));
        assert_eq!(provider["api_key_env"], json!("DEMO_API_KEY"));
        assert_eq!(provider["models"]["keep"]["name"], json!("Keep"));
        assert!(provider.get("label").is_none());
        assert!(provider.get("options").is_none());
        assert!(provider.get("no_auth").is_none());
        application
            .shutdown()
            .await
            .expect("shutdown")
            .require_clean()
            .expect("clean shutdown");
    }

    #[tokio::test]
    async fn default_model_mutation_preserves_reasoning_as_one_semantic_write() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let cwd = temp.path().join("workspace");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&cwd).expect("cwd");
        std::fs::write(
            home.join("config.toml"),
            "[provider.fake]\napi = \"http://127.0.0.1:9\"\n",
        )
        .expect("config");
        let application = Application::builder()
            .home(&home)
            .build()
            .await
            .expect("application");
        let configuration = application
            .client()
            .configuration(ConfigurationQuery::new(&cwd))
            .expect("configuration");

        let value = configuration
            .set_default_model(ConfigScope::Global, "fake/model", Some("high"))
            .expect("default model");

        assert_eq!(value["model"], json!("fake/model"));
        assert_eq!(value["reasoning_effort"], json!("high"));
        let document = std::fs::read_to_string(home.join("config.toml")).expect("config");
        assert!(document.contains("[model]"));
        assert!(document.contains("id = \"fake/model\""));
        assert!(document.contains("reasoning_effort = \"high\""));
        application
            .shutdown()
            .await
            .expect("shutdown")
            .require_clean()
            .expect("clean shutdown");
    }
}
