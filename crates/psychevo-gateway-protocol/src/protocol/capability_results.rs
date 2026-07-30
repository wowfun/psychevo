#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(tag = "kind", rename_all = "snake_case")]
pub enum PluginAuthorityIdentityView {
    Psychevo { selector: String },
    Codex { plugin: String, marketplace: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginComponentStatusView {
    pub component: String,
    pub compatibility_profile: String,
    pub highest_level: String,
    pub execution_owner: String,
    pub readiness: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginDiagnosticView {
    pub kind: String,
    pub message: String,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginPolicyView {
    pub profile_enabled: bool,
    #[serde(default)]
    pub project_override: Option<bool>,
    pub effective_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginTrustView {
    pub required: bool,
    pub status: String,
    pub fingerprint: String,
    #[serde(default)]
    pub trusted_fingerprint: Option<String>,
    #[serde(
        default,
        serialize_with = "option_json_safe_i64::serialize",
        deserialize_with = "option_json_safe_i64::deserialize"
    )]
    #[schemars(with = "Option<JsonSafeI64>")]
    #[ts(type = "number | null")]
    pub trusted_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PsychevoPluginView {
    pub name: String,
    pub selector: String,
    pub scope_name: String,
    pub enablement_scope_name: String,
    pub removable: bool,
    pub package_mutable: bool,
    pub enablement_mutable: bool,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub source_id: String,
    pub source: String,
    pub source_kind: String,
    pub scope: String,
    pub manifest_kind: String,
    pub enabled: bool,
    #[serde(default)]
    pub authority: Option<PluginAuthorityIdentityView>,
    #[serde(default)]
    pub canonical_id: Option<String>,
    #[serde(default)]
    pub npm_registry: Option<String>,
    #[serde(default)]
    pub package_root: Option<String>,
    #[serde(default)]
    pub data_root: Option<String>,
    #[serde(default)]
    pub manifest_path: Option<String>,
    #[serde(default)]
    pub compatibility_profile: Option<String>,
    #[serde(default)]
    pub component_statuses: Vec<PluginComponentStatusView>,
    #[serde(default)]
    pub manifest_resources: Vec<String>,
    #[serde(default)]
    pub psychevo_extensions: Vec<String>,
    #[serde(default)]
    pub readiness: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub installed: Option<bool>,
    #[serde(default)]
    #[ts(type = "unknown")]
    pub interface: Option<Value>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    #[ts(type = "unknown")]
    pub contributions: Option<Value>,
    #[serde(default)]
    pub diagnostics: Vec<PluginDiagnosticView>,
    #[serde(default)]
    pub policy: Option<PluginPolicyView>,
    #[serde(default)]
    pub trust: Option<PluginTrustView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct CodexPluginView {
    pub name: String,
    pub selector: String,
    pub canonical_id: String,
    pub authority: PluginAuthorityIdentityView,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub source_id: String,
    pub scope_name: String,
    pub manifest_kind: String,
    #[serde(default)]
    pub compatibility_profile: Option<String>,
    #[serde(default)]
    pub component_statuses: Vec<PluginComponentStatusView>,
    #[serde(default)]
    pub installed: Option<bool>,
    pub enabled: bool,
    #[serde(default)]
    pub readiness: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    #[ts(type = "unknown")]
    pub interface: Option<Value>,
    #[serde(default)]
    pub policy: Option<PluginPolicyView>,
    #[serde(default)]
    pub trust: Option<PluginTrustView>,
    #[serde(default)]
    pub enablement_mutable: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(untagged)]
pub enum PluginView {
    Psychevo(Box<PsychevoPluginView>),
    Codex(Box<CodexPluginView>),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginAuthorityRuntimeView {
    pub kind: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(default)]
    pub auth: Option<String>,
    #[serde(default)]
    pub readiness: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub resolved_binary: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub compatibility_profile: Option<String>,
    #[serde(default)]
    pub private_home: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(
        default,
        serialize_with = "option_json_safe_u64::serialize",
        deserialize_with = "option_json_safe_u64::deserialize"
    )]
    #[schemars(with = "Option<JsonSafeU64>")]
    #[ts(type = "number | null")]
    pub generation: Option<u64>,
    #[serde(default)]
    pub inventory_ready: Option<bool>,
    #[serde(default)]
    pub security_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginListResult {
    pub plugins: Vec<PluginView>,
    pub count: usize,
    pub codex_authority: PluginAuthorityRuntimeView,
    pub authorities: Vec<PluginAuthorityRuntimeView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginReadResult {
    pub plugin: PluginView,
    #[ts(type = "unknown")]
    pub manifest: Value,
    #[ts(type = "unknown")]
    pub inspection: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginDoctorEntryView {
    pub plugin: PluginView,
    #[ts(type = "unknown")]
    pub manifest: Value,
    #[ts(type = "unknown")]
    pub inspection: Value,
    #[serde(default)]
    #[ts(type = "unknown")]
    pub worker: Option<Value>,
    #[serde(default)]
    #[ts(type = "unknown")]
    pub sandbox: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginDoctorResult {
    pub plugins: Vec<PluginDoctorEntryView>,
    #[serde(default)]
    #[ts(type = "unknown")]
    pub apps: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginStageDiagnosticView {
    pub stage: String,
    pub status: String,
    pub message: String,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginInterfaceView {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub short_description: Option<String>,
    #[serde(default)]
    pub long_description: Option<String>,
    #[serde(default)]
    pub developer_name: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub default_prompt: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub website_url: Option<String>,
    #[serde(default)]
    pub privacy_policy_url: Option<String>,
    #[serde(default)]
    pub terms_of_service_url: Option<String>,
    #[serde(default)]
    pub brand_color: Option<String>,
    #[serde(default)]
    pub composer_icon: Option<String>,
    #[serde(default)]
    pub logo: Option<String>,
    #[serde(default)]
    pub logo_dark: Option<String>,
    #[serde(default)]
    pub screenshots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginInspectionView {
    pub source_kind: String,
    pub source_id: String,
    pub framework: String,
    pub canonical_id: String,
    #[serde(default)]
    pub compatibility_profile: Option<String>,
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub manifest_path: String,
    pub package_root: String,
    pub support: String,
    pub declared_lanes: Vec<String>,
    #[serde(default)]
    pub component_statuses: Vec<PluginComponentStatusView>,
    pub unsupported_lanes: Vec<String>,
    pub diagnostics: Vec<PluginDiagnosticView>,
    pub stages: Vec<PluginStageDiagnosticView>,
    #[serde(default)]
    pub interface: Option<PluginInterfaceView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginInspectResult {
    pub success: bool,
    pub inspection: PluginInspectionView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginInstallRecordView {
    pub name: String,
    pub version: String,
    pub description: String,
    pub source_id: String,
    pub source_slug: String,
    pub source_kind: String,
    #[serde(default)]
    pub npm_registry: Option<String>,
    #[serde(default)]
    pub resolved_revision: Option<String>,
    pub scope: String,
    pub package_root: String,
    pub data_root: String,
    pub manifest_path: String,
    pub manifest_kind: String,
    pub compatibility_profile: String,
    #[serde(default)]
    pub component_statuses: Vec<PluginComponentStatusView>,
    pub manifest_resources: Vec<String>,
    pub psychevo_extensions: Vec<String>,
    pub diagnostics: Vec<PluginDiagnosticView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginInstallResult {
    #[serde(default)]
    pub success: Option<bool>,
    #[serde(default)]
    pub plugin: Option<PluginInstallRecordView>,
    #[serde(default)]
    pub authority: Option<PluginAuthorityIdentityView>,
    #[serde(default)]
    pub partial: Option<bool>,
    #[serde(default, rename = "completedSteps")]
    #[ts(rename = "completedSteps")]
    pub completed_steps: Vec<String>,
    #[serde(default, rename = "failedStep")]
    #[ts(rename = "failedStep")]
    pub failed_step: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default, rename = "safeState")]
    #[ts(rename = "safeState")]
    pub safe_state: Option<String>,
    #[serde(default)]
    #[ts(type = "unknown")]
    pub materialization: Option<Value>,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub policy: Option<PluginPolicyView>,
    #[serde(default)]
    pub trust: Option<PluginTrustView>,
    #[serde(
        default,
        serialize_with = "option_json_safe_u64::serialize",
        deserialize_with = "option_json_safe_u64::deserialize"
    )]
    #[schemars(with = "Option<JsonSafeU64>")]
    #[ts(type = "number | null")]
    pub generation: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginUninstallResult {
    pub success: bool,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub plugin: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub authority: Option<PluginAuthorityIdentityView>,
    #[serde(default)]
    #[ts(type = "unknown")]
    pub result: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginSetEnabledResult {
    pub success: bool,
    pub scope: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub plugin: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub manifest_resources: Vec<String>,
    #[serde(default)]
    pub psychevo_extensions: Vec<String>,
    #[serde(default)]
    pub policy: Option<PluginPolicyView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginAuthorityWriteResult {
    pub success: bool,
    #[ts(type = "unknown")]
    pub write: Value,
    pub authority: PluginAuthorityRuntimeView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginAuthorityRefreshResult {
    pub success: bool,
    pub authority: PluginAuthorityRuntimeView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginAuthoritySetTrustResult {
    pub success: bool,
    pub selector: String,
    pub trusted: bool,
    pub trust: PluginTrustView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginMarketplaceView {
    pub name: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub git_ref: Option<String>,
    #[serde(default)]
    pub npm_version: Option<String>,
    #[serde(default)]
    pub npm_registry: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    #[ts(type = "Array<unknown>")]
    pub plugins: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginCatalogListResult {
    #[serde(default)]
    pub scope: Option<String>,
    pub marketplaces: Vec<PluginMarketplaceView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginCatalogPsychevoAddResult {
    pub success: bool,
    pub scope: String,
    pub marketplace: PluginMarketplaceView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginCatalogPsychevoRemoveResult {
    pub success: bool,
    pub scope: String,
    pub removed: bool,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginCatalogCodexAddResult {
    pub marketplace_name: String,
    pub installed_root: String,
    pub already_added: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginCatalogCodexRemoveResult {
    pub marketplace_name: String,
    #[schemars(required)]
    #[schemars(with = "JsonNullableString")]
    #[ts(type = "string | null")]
    pub installed_root: Option<String>,
}

struct JsonNullableString;

impl JsonSchema for JsonNullableString {
    fn schema_name() -> String {
        "JsonNullableString".to_string()
    }

    fn json_schema(
        _generator: &mut schemars::r#gen::SchemaGenerator,
    ) -> schemars::schema::Schema {
        schemars::schema::SchemaObject {
            instance_type: Some(
                vec![
                    schemars::schema::InstanceType::String,
                    schemars::schema::InstanceType::Null,
                ]
                .into(),
            ),
            ..Default::default()
        }
        .into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginCatalogUpgradeError {
    pub marketplace_name: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginCatalogUpgradeResult {
    pub selected_marketplaces: Vec<String>,
    pub upgraded_roots: Vec<String>,
    pub errors: Vec<PluginCatalogUpgradeError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(untagged)]
pub enum PluginCatalogAddResult {
    Psychevo(PluginCatalogPsychevoAddResult),
    Codex(PluginCatalogCodexAddResult),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(untagged)]
pub enum PluginCatalogRemoveResult {
    Psychevo(PluginCatalogPsychevoRemoveResult),
    Codex(PluginCatalogCodexRemoveResult),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginConnectStartResult {
    pub session_id: String,
    pub status: String,
    #[serde(default)]
    pub install_url: Option<String>,
    #[serde(default)]
    pub authorization_url: Option<String>,
    #[serde(with = "json_safe_u64")]
    #[schemars(with = "JsonSafeU64")]
    #[ts(type = "number")]
    pub expires_in_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct PluginConnectStatusResult {
    pub session_id: String,
    pub status: String,
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub component_id: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub authorization_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct SkillDiagnosticView {
    pub kind: String,
    pub message: String,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct SkillRequiredEnvironmentVariableView {
    pub name: String,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub help: Option<String>,
    #[serde(default)]
    pub required_for: Option<String>,
    #[serde(default)]
    pub optional: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct SkillSummaryView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub location: String,
    pub source: String,
    pub source_label: String,
    #[serde(default)]
    pub category: Option<String>,
    pub enabled: bool,
    pub prompt_visible: bool,
    pub readiness_status: String,
    pub supported_on_current_platform: bool,
    pub disable_model_invocation: bool,
    pub issues: Vec<String>,
    #[serde(default)]
    pub collision_group: Vec<String>,
    #[serde(default)]
    pub skill_dir: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub related_skills: Vec<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub required_environment_variables: Vec<SkillRequiredEnvironmentVariableView>,
    #[serde(default)]
    pub missing_required_environment_variables: Vec<String>,
    #[serde(default)]
    pub missing_credential_files: Vec<String>,
    #[serde(default)]
    pub compatibility: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub required_tools: Vec<String>,
    #[serde(default)]
    pub fallback_for_tools: Vec<String>,
    #[serde(default)]
    pub required_toolsets: Vec<String>,
    #[serde(default)]
    pub fallback_for_toolsets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct SkillListResult {
    pub success: bool,
    pub skills: Vec<SkillSummaryView>,
    pub diagnostics: Vec<SkillDiagnosticView>,
    pub collisions: BTreeMap<String, Vec<String>>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct SkillReadResult {
    pub success: bool,
    pub name: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub prompt_visible: Option<bool>,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub skill_dir: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub source_label: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub related_skills: Vec<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub platform_status: Option<String>,
    #[serde(default)]
    pub issues: Vec<String>,
    #[serde(default)]
    pub collision_group: Vec<String>,
    #[serde(default)]
    pub required_environment_variables: Vec<SkillRequiredEnvironmentVariableView>,
    #[serde(default)]
    pub missing_required_environment_variables: Vec<String>,
    #[serde(default)]
    pub missing_credential_files: Vec<String>,
    #[serde(default)]
    pub setup_needed: Option<bool>,
    #[serde(default)]
    pub readiness_status: Option<String>,
    #[serde(default)]
    pub setup_help: Option<String>,
    #[serde(default)]
    pub compatibility: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub required_tools: Vec<String>,
    #[serde(default)]
    pub fallback_for_tools: Vec<String>,
    #[serde(default)]
    pub required_toolsets: Vec<String>,
    #[serde(default)]
    pub fallback_for_toolsets: Vec<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub preview_content: Option<String>,
    #[serde(default)]
    pub linked_files: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub available_files: Vec<String>,
    #[serde(default)]
    pub is_binary: Option<bool>,
    #[serde(default)]
    pub size: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct SkillScanFindingView {
    pub category: String,
    pub severity: String,
    pub file: String,
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct SkillScanResultView {
    pub verdict: String,
    pub findings: Vec<SkillScanFindingView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct SkillInstalledView {
    pub name: String,
    pub path: String,
    pub scan: SkillScanResultView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct SkillInstallResult {
    pub success: bool,
    pub installed: Vec<SkillInstalledView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct SkillUninstallResult {
    pub success: bool,
    pub name: String,
    pub scope: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct SkillSetEnabledResult {
    pub success: bool,
    pub name: String,
    pub enabled: bool,
    pub scope: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct ToolModeView {
    #[serde(default)]
    pub enabled_toolsets: Option<Vec<String>>,
    pub disabled_toolsets: Vec<String>,
    pub effective_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct ToolsetView {
    pub name: String,
    pub source: String,
    #[serde(default)]
    pub description: Option<String>,
    pub tools: Vec<String>,
    pub includes: Vec<String>,
    pub unknown_tools: Vec<String>,
    pub mode_mutable: bool,
    pub removable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct ToolListResult {
    pub scope: String,
    #[serde(default)]
    pub path: Option<String>,
    #[ts(type = "Array<unknown>")]
    pub sources: Vec<Value>,
    pub default_enabled_toolsets: Vec<String>,
    pub modes: BTreeMap<String, ToolModeView>,
    pub toolsets: Vec<ToolsetView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct ToolReadResult {
    pub toolset: ToolsetView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct ToolMutationResult {
    pub success: bool,
    pub changed: bool,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind")]
#[ts(tag = "kind")]
pub enum McpTransportView {
    #[serde(rename = "stdio")]
    #[ts(rename = "stdio")]
    Stdio {
        command: String,
        args: Vec<String>,
        #[serde(rename = "envKeys")]
        #[ts(rename = "envKeys")]
        env_keys: Vec<String>,
        #[serde(default)]
        cwd: Option<String>,
    },
    #[serde(rename = "streamable_http")]
    #[ts(rename = "streamable_http")]
    StreamableHttp {
        url: String,
        headers: BTreeMap<String, String>,
        auth: McpAuthView,
    },
    #[serde(rename = "unsupported")]
    #[ts(rename = "unsupported")]
    Unsupported {
        #[serde(rename = "unsupportedKind")]
        #[ts(rename = "unsupportedKind")]
        unsupported_kind: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct McpAuthView {
    #[serde(default)]
    pub bearer_token_env_var: Option<String>,
    pub scopes: Vec<String>,
    #[serde(default)]
    pub oauth_resource: Option<String>,
    #[serde(default)]
    pub oauth_client_id: Option<String>,
    pub oauth_configured: bool,
    #[serde(rename = "storedOAuthToken")]
    #[ts(rename = "storedOAuthToken")]
    pub stored_oauth_token: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct McpPolicyView {
    #[serde(default)]
    pub enabled_tools: Option<Vec<String>>,
    pub disabled_tools: Vec<String>,
    pub supports_parallel_tool_calls: bool,
    #[serde(
        default,
        serialize_with = "option_json_safe_u64::serialize",
        deserialize_with = "option_json_safe_u64::deserialize"
    )]
    #[schemars(with = "Option<JsonSafeU64>")]
    #[ts(type = "number | null")]
    pub startup_timeout_secs: Option<u64>,
    #[serde(
        default,
        serialize_with = "option_json_safe_u64::serialize",
        deserialize_with = "option_json_safe_u64::deserialize"
    )]
    #[schemars(with = "Option<JsonSafeU64>")]
    #[ts(type = "number | null")]
    pub tool_timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct McpServerView {
    pub name: String,
    pub source_id: String,
    pub source_kind: String,
    pub enabled: bool,
    pub required: bool,
    pub transport: McpTransportView,
    pub policy: McpPolicyView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpListResult {
    pub scope: String,
    #[serde(default)]
    pub path: Option<String>,
    #[ts(type = "Array<unknown>")]
    pub sources: Vec<Value>,
    pub servers: Vec<McpServerView>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpReadResult {
    pub server: McpServerView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpConfigView {
    pub name: String,
    #[ts(type = "Record<string, unknown>")]
    pub config: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpMutationResult {
    pub success: bool,
    pub changed: bool,
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub server: Option<McpConfigView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpToolSummaryView {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpTestResult {
    pub ok: bool,
    pub name: String,
    pub transport: String,
    #[serde(default)]
    pub tools: Vec<McpToolSummaryView>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "status", rename_all = "snake_case")]
#[ts(tag = "status", rename_all = "snake_case")]
pub enum McpOAuthStartResult {
    Pending {
        #[serde(rename = "sessionId")]
        #[ts(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "authorizationUrl")]
        #[ts(rename = "authorizationUrl")]
        authorization_url: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "status", rename_all = "snake_case")]
#[ts(tag = "status", rename_all = "snake_case")]
pub enum McpOAuthStatusResult {
    Pending {
        #[serde(rename = "sessionId")]
        #[ts(rename = "sessionId")]
        session_id: String,
    },
    Succeeded {
        #[serde(rename = "sessionId")]
        #[ts(rename = "sessionId")]
        session_id: String,
    },
    Failed {
        #[serde(rename = "sessionId")]
        #[ts(rename = "sessionId")]
        session_id: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpOAuthLogoutResult {
    pub success: bool,
    pub name: String,
    pub removed: bool,
}
