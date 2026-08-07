mod admin_commands;
mod core_commands;

pub(crate) use admin_commands::profile_args::{
    AgentIdArgs, AgentInspectArgs, AgentLogsArgs, AgentSendArgs, AgentStatusArgs, AgentWaitArgs,
    AuthArgs, AuthCommand, AuthSetArgs, AuthSetupArgs, AuthStatusArgs, ConfigArgs, ConfigCommand,
    ConfigEditArgs, ConfigExtensionArgs, ConfigJsonArgs, ConfigPermissionRemoveArgs,
    ConfigPermissionsArgs, ConfigPermissionsCommand, ConfigProviderAddArgs, ConfigProviderArgs,
    ConfigProviderCommand, ConfigSetArgs, ConfigShowArgs, ModelArgs, ModelCommand, ModelFetchArgs,
    ModelJsonArgs, ModelListArgs, ModelSetArgs, PermissionModeArg, RunFormatArg, SessionArgs,
    SessionCommand, SessionExportArgs, SessionExportFormatArg, SessionIdArgs, SessionListArgs,
    SessionRenameArgs, SessionShareArgs,
};
#[cfg(feature = "gateway")]
pub(crate) use core_commands::command_variants::TuiArgs;
pub(crate) use core_commands::command_variants::{
    AgentArgs, AgentBackendAddArgs, AgentBackendArgs, AgentBackendCommand, AgentBackendDoctorArgs,
    AgentBackendListArgs, AgentCommand, AgentListArgs, AgentNameArgs, AgentRunArgs, ToolArgs,
    ToolCommand, ToolCreateArgs, ToolListArgs, ToolModeMutationArgs, ToolRemoveArgs, ToolShowArgs,
};
#[cfg(feature = "gateway")]
pub(crate) use core_commands::global_args::basic_profile::ServeArgs;
pub(crate) use core_commands::global_args::basic_profile::{
    DoctorArgs, InitArgs, McpArgs, McpCommand, McpServeArgs, ProfileAliasArgs, ProfileArgs,
    ProfileCommand, ProfileCreateArgs, ProfileDeleteArgs, ProfileListArgs, ProfileRenameArgs,
    ProfileShowArgs, ProfileUseArgs, SetupArgs,
};
#[cfg(feature = "desktop")]
pub(crate) use core_commands::global_args::desktop::DesktopArgs;
pub(crate) use core_commands::global_args::extensions::{
    ExtensionInstallArgs, ExtensionListArgs, ExtensionRemoveArgs, ExtensionUpdateArgs,
};
#[cfg(feature = "gateway")]
pub(crate) use core_commands::global_args::gateway::{
    GatewayArgs, GatewayCommand, GatewayOpenArgs, GatewaySetupArgs, GatewayStartArgs, WebArgs,
    WebCommand,
};
pub(crate) use core_commands::global_args::plugins_hooks::{
    HookKeyArgs, HooksArgs, HooksCommand, HooksListArgs, PluginAddArgs, PluginArgs, PluginCommand,
    PluginDoctorArgs, PluginInspectArgs, PluginListArgs, PluginMarketplaceArgs,
    PluginMarketplaceCommand, PluginViewArgs,
};
#[cfg(feature = "gateway")]
pub(crate) use core_commands::global_args::run_stats_context::RunArgs;
pub(crate) use core_commands::global_args::run_stats_context::{ContextArgs, StatsArgs};
pub(crate) use core_commands::global_args::skills::{
    SkillsAuditArgs, SkillsBundleArgs, SkillsBundleCommand, SkillsBundleCreateArgs,
    SkillsBundleDeleteArgs, SkillsBundleNameArgs, SkillsCommand, SkillsConfigArgs,
    SkillsConfigCommand, SkillsConfigSetArgs, SkillsInspectArgs, SkillsInstallArgs, SkillsListArgs,
    SkillsNameArgs, SkillsNameScopeArgs, SkillsPublishArgs, SkillsQueryArgs, SkillsViewArgs,
};
pub(crate) use core_commands::global_args::skills_entry::SkillsArgs;
pub(crate) use core_commands::global_args::{Cli, Commands};
