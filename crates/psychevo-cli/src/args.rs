mod admin_commands;
mod core_commands;

pub(crate) use admin_commands::profile_args::{
    AgentIdArgs, AgentInspectArgs, AgentLogsArgs, AgentSendArgs, AgentStatusArgs, AgentWaitArgs,
    AuthArgs, AuthCommand, AuthSetArgs, AuthSetupArgs, AuthStatusArgs, ConfigArgs, ConfigCommand,
    ConfigEditArgs, ConfigJsonArgs, ConfigPermissionRemoveArgs, ConfigPermissionsArgs,
    ConfigPermissionsCommand, ConfigProviderAddArgs, ConfigProviderArgs, ConfigProviderCommand,
    ConfigSetArgs, ConfigShowArgs, ModelArgs, ModelCommand, ModelFetchArgs, ModelJsonArgs,
    ModelListArgs, ModelSetArgs, PermissionModeArg, RunFormatArg, SessionArgs, SessionCommand,
    SessionExportArgs, SessionExportFormatArg, SessionIdArgs, SessionListArgs, SessionRenameArgs,
    SessionShareArgs,
};
pub(crate) use core_commands::command_variants::{
    AgentArgs, AgentBackendAddArgs, AgentBackendArgs, AgentBackendCommand, AgentBackendDoctorArgs,
    AgentBackendListArgs, AgentCommand, AgentListArgs, AgentNameArgs, AgentRunArgs, ToolArgs,
    ToolCommand, ToolCreateArgs, ToolListArgs, ToolModeMutationArgs, ToolRemoveArgs, ToolShowArgs,
    TuiArgs,
};
pub(crate) use core_commands::global_args::basic_profile::{
    DoctorArgs, InitArgs, McpArgs, McpCommand, McpServeArgs, ProfileAliasArgs, ProfileArgs,
    ProfileCommand, ProfileCreateArgs, ProfileDeleteArgs, ProfileListArgs, ProfileRenameArgs,
    ProfileShowArgs, ProfileUseArgs, ServeArgs, SetupArgs,
};
pub(crate) use core_commands::global_args::desktop::DesktopArgs;
#[cfg(feature = "native-channels")]
pub(crate) use core_commands::global_args::gateway::GatewaySetupArgs;
pub(crate) use core_commands::global_args::gateway::{
    GatewayArgs, GatewayCommand, GatewayOpenArgs, GatewayStartArgs, WebArgs, WebCommand,
};
pub(crate) use core_commands::global_args::plugins_hooks::{
    HookKeyArgs, HooksArgs, HooksCommand, HooksListArgs, PluginArgs, PluginCommand,
    PluginDoctorArgs, PluginInspectArgs, PluginInstallArgs, PluginListArgs, PluginMarketplaceArgs,
    PluginMarketplaceCommand, PluginViewArgs,
};
pub(crate) use core_commands::global_args::run_stats_context::{ContextArgs, RunArgs, StatsArgs};
pub(crate) use core_commands::global_args::skills::{
    SkillsAuditArgs, SkillsBundleArgs, SkillsBundleCommand, SkillsBundleCreateArgs,
    SkillsBundleDeleteArgs, SkillsBundleNameArgs, SkillsCommand, SkillsConfigArgs,
    SkillsConfigCommand, SkillsConfigSetArgs, SkillsInspectArgs, SkillsInstallArgs, SkillsListArgs,
    SkillsNameArgs, SkillsNameScopeArgs, SkillsPublishArgs, SkillsQueryArgs, SkillsViewArgs,
};
pub(crate) use core_commands::global_args::skills_entry::SkillsArgs;
pub(crate) use core_commands::global_args::{Cli, Commands};
