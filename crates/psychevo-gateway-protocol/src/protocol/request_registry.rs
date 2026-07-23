#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequestContract {
    pub(crate) method: &'static str,
    pub(crate) params_ts: String,
    pub(crate) result_ts: String,
}

macro_rules! request_result_ts {
    (json_object) => {
        "GatewayJsonResult".to_string()
    };
    ($result:ty) => {
        <$result as TS>::name()
    };
}

macro_rules! request_registry {
    (
        $(
            $variant:ident => $method:literal {
                params: $params:ty,
                result: $result:tt
            }
        ),* $(,)?
    ) => {
        #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
        #[serde(tag = "method", content = "params")]
        pub enum ClientRequest {
            $(
                #[serde(rename = $method)]
                $variant($params),
            )*
        }

        pub(crate) fn request_contracts() -> Vec<RequestContract> {
            vec![
                $(
                    RequestContract {
                        method: $method,
                        params_ts: <$params as TS>::name(),
                        result_ts: request_result_ts!($result),
                    },
                )*
            ]
        }
    };
}

request_registry! {
    Initialize => "initialize" { params: InitializeParams, result: InitializeResult },
    ThreadDraftOpen => "thread/draft/open" { params: ThreadDraftOpenParams, result: ThreadDraftOpenResult },
    ThreadResume => "thread/resume" { params: ThreadResumeParams, result: ThreadSnapshot },
    ThreadRead => "thread/read" { params: ThreadReadParams, result: ThreadSnapshot },
    ThreadTrace => "thread/trace" { params: ThreadTraceParams, result: ThreadTraceResult },
    ThreadList => "thread/list" { params: ThreadListParams, result: ThreadListResult },
    ThreadBrowser => "thread/browser" { params: ThreadBrowserParams, result: ThreadBrowserResult },
    ThreadRename => "thread/rename" { params: ThreadRenameParams, result: ThreadMutationResult },
    ThreadArchive => "thread/archive" { params: ThreadIdParams, result: ThreadMutationResult },
    ThreadRestore => "thread/restore" { params: ThreadIdParams, result: ThreadMutationResult },
    ThreadDelete => "thread/delete" { params: ThreadIdParams, result: ThreadDeleteResult },
    TurnStart => "turn/start" { params: TurnStartParams, result: TurnStartResult },
    ThreadContextRead => "thread/context/read" { params: ThreadContextReadParams, result: ThreadContextReadResult },
    ThreadDraftPrepare => "thread/draft/prepare" { params: ThreadDraftPrepareParams, result: ThreadDraftPrepareResult },
    ThreadControlSet => "thread/control/set" { params: ThreadControlSetParams, result: ThreadControlSetResult },
    ThreadActionRun => "thread/action/run" { params: ThreadActionRunParams, result: ThreadActionRunResult },
    ThreadInteractionRespond => "thread/interaction/respond" { params: ThreadInteractionRespondParams, result: ThreadInteractionRespondResult },
    ThreadHistoryRead => "thread/history/read" { params: ThreadHistoryReadParams, result: ThreadHistoryReadResult },
    ThreadHistoryDraftRead => "thread/history/draft/read" { params: ThreadHistoryDraftReadParams, result: ThreadHistoryDraftReadResult },
    ThreadImportList => "thread/import/list" { params: ThreadImportListParams, result: ThreadImportListResult },
    ThreadImport => "thread/import" { params: ThreadImportParams, result: ThreadImportResult },
    RuntimeProfileList => "runtime/profile/list" { params: RuntimeProfileListParams, result: RuntimeProfileListResult },
    RuntimeProfileRead => "runtime/profile/read" { params: RuntimeProfileReadParams, result: RuntimeProfileReadResult },
    RuntimeProfileWrite => "runtime/profile/write" { params: RuntimeProfileWriteParams, result: RuntimeProfileWriteResult },
    RuntimeProfileDelete => "runtime/profile/delete" { params: RuntimeProfileDeleteParams, result: RuntimeProfileDeleteResult },
    RuntimeProfileSetEnabled => "runtime/profile/setEnabled" { params: RuntimeProfileSetEnabledParams, result: RuntimeProfileWriteResult },
    AutomationList => "automation/list" { params: AutomationListParams, result: AutomationListResult },
    AutomationDraft => "automation/draft" { params: AutomationDraftParams, result: AutomationDraftResult },
    AutomationWrite => "automation/write" { params: AutomationWriteParams, result: AutomationMutationResult },
    AutomationPause => "automation/pause" { params: AutomationIdParams, result: AutomationMutationResult },
    AutomationResume => "automation/resume" { params: AutomationIdParams, result: AutomationMutationResult },
    AutomationDelete => "automation/delete" { params: AutomationIdParams, result: AutomationDeleteResult },
    AutomationRun => "automation/run" { params: AutomationRunParams, result: AutomationRunResult },
    CompletionList => "completion/list" { params: CompletionListParams, result: CompletionListResult },
    CommandList => "command/list" { params: CommandListParams, result: CommandListResult },
    CommandExecute => "command/execute" { params: CommandExecuteParams, result: CommandExecuteResult },
    SlashSettingsRead => "slash/settings/read" { params: SlashSettingsReadParams, result: SlashSettingsResult },
    SlashSettingsUpdate => "slash/settings/update" { params: SlashSettingsUpdateParams, result: SlashSettingsResult },
    AgentList => "agent/list" { params: AgentListParams, result: AgentListResult },
    AgentRead => "agent/read" { params: AgentReadParams, result: AgentReadResult },
    AgentWrite => "agent/write" { params: AgentWriteParams, result: AgentWriteResult },
    AgentSetEnabled => "agent/setEnabled" { params: AgentSetEnabledParams, result: AgentSetEnabledResult },
    AgentDelete => "agent/delete" { params: AgentDeleteParams, result: AgentDeleteResult },
    AgentStatus => "agent/status" { params: AgentStatusParams, result: AgentStatusResult },
    TeamList => "team/list" { params: TeamListParams, result: TeamListResult },
    TeamRead => "team/read" { params: TeamReadParams, result: TeamReadResult },
    TeamWrite => "team/write" { params: TeamWriteParams, result: TeamWriteResult },
    TeamSetEnabled => "team/setEnabled" { params: TeamSetEnabledParams, result: TeamSetEnabledResult },
    TeamDelete => "team/delete" { params: TeamDeleteParams, result: TeamDeleteResult },
    TeamStatus => "team/status" { params: TeamStatusParams, result: TeamStatusResult },
    AgentControl => "agent/control" { params: AgentControlParams, result: AgentControlResult },
    BackendList => "backend/list" { params: BackendListParams, result: BackendListResult },
    BackendDoctor => "backend/doctor" { params: BackendDoctorParams, result: BackendDoctorResult },
    BackendInstall => "backend/install" { params: BackendManageParams, result: BackendManageResult },
    BackendRepair => "backend/repair" { params: BackendManageParams, result: BackendManageResult },
    BackendUpgrade => "backend/upgrade" { params: BackendManageParams, result: BackendManageResult },
    BackendWrite => "backend/write" { params: BackendWriteParams, result: BackendWriteResult },
    BackendDelete => "backend/delete" { params: BackendDeleteParams, result: BackendDeleteResult },
    PluginList => "plugin/list" { params: PluginListParams, result: json_object },
    PluginRead => "plugin/read" { params: PluginReadParams, result: json_object },
    PluginDoctor => "plugin/doctor" { params: PluginDoctorParams, result: json_object },
    PluginInspect => "plugin/import/inspect" { params: PluginInspectParams, result: json_object },
    PluginInstall => "plugin/install" { params: PluginInstallParams, result: json_object },
    PluginUninstall => "plugin/uninstall" { params: PluginUninstallParams, result: json_object },
    PluginSetEnabled => "plugin/setEnabled" { params: PluginSetEnabledParams, result: json_object },
    PluginSetTrust => "plugin/setTrust" { params: PluginSetTrustParams, result: json_object },
    PluginAuthorityWrite => "plugin/authority/write" { params: PluginAuthorityWriteParams, result: json_object },
    PluginAuthorityRefresh => "plugin/authority/refresh" { params: PluginAuthorityRefreshParams, result: json_object },
    PluginCatalogList => "plugin/catalog/list" { params: PluginCatalogListParams, result: json_object },
    PluginCatalogAdd => "plugin/catalog/add" { params: PluginCatalogAddParams, result: json_object },
    PluginCatalogRemove => "plugin/catalog/remove" { params: PluginCatalogRemoveParams, result: json_object },
    PluginCatalogUpgrade => "plugin/catalog/upgrade" { params: PluginCatalogUpgradeParams, result: json_object },
    PluginConnectStart => "plugin/connect/start" { params: PluginConnectStartParams, result: json_object },
    PluginConnectStatus => "plugin/connect/status" { params: PluginConnectStatusParams, result: json_object },
    SkillList => "skill/list" { params: SkillListParams, result: json_object },
    SkillRead => "skill/read" { params: SkillReadParams, result: json_object },
    SkillInstall => "skill/install" { params: SkillInstallParams, result: json_object },
    SkillUninstall => "skill/uninstall" { params: SkillUninstallParams, result: json_object },
    SkillSetEnabled => "skill/setEnabled" { params: SkillSetEnabledParams, result: json_object },
    SkillWrite => "skill/write" { params: SkillWriteParams, result: SkillWriteResult },
    ToolList => "tool/list" { params: ToolListParams, result: json_object },
    ToolRead => "tool/read" { params: ToolReadParams, result: json_object },
    ToolSetEnabled => "tool/setEnabled" { params: ToolSetEnabledParams, result: json_object },
    ToolCreate => "tool/create" { params: ToolCreateParams, result: json_object },
    ToolRemove => "tool/remove" { params: ToolRemoveParams, result: json_object },
    McpList => "mcp/list" { params: McpListParams, result: json_object },
    McpRead => "mcp/read" { params: McpReadParams, result: json_object },
    McpUpsert => "mcp/upsert" { params: McpUpsertParams, result: json_object },
    McpRemove => "mcp/remove" { params: McpNameParams, result: json_object },
    McpSetEnabled => "mcp/setEnabled" { params: McpSetEnabledParams, result: json_object },
    McpSetToolPolicy => "mcp/setToolPolicy" { params: McpSetToolPolicyParams, result: json_object },
    McpTest => "mcp/test" { params: McpNameParams, result: json_object },
    McpOAuthStart => "mcp/oauth/start" { params: McpOAuthStartParams, result: json_object },
    McpOAuthStatus => "mcp/oauth/status" { params: McpOAuthStatusParams, result: json_object },
    McpOAuthLogout => "mcp/oauth/logout" { params: McpNameParams, result: json_object },
    ChannelList => "channel/list" { params: ChannelListParams, result: ChannelListResult },
    ChannelShow => "channel/show" { params: ChannelIdParams, result: ChannelEnableResult },
    ChannelEnable => "channel/enable" { params: ChannelEnableParams, result: ChannelEnableResult },
    ChannelUpdate => "channel/update" { params: ChannelUpdateParams, result: ChannelEnableResult },
    ChannelDelete => "channel/delete" { params: ChannelIdParams, result: ChannelListResult },
    ChannelDoctor => "channel/doctor" { params: ChannelDoctorParams, result: ChannelDoctorResult },
    ChannelSourceList => "channel/source/list" { params: ChannelIdParams, result: ChannelSourceListResult },
    ChannelWechatQrStart => "channel/wechat-qr/start" { params: ChannelWechatQrStartParams, result: ChannelWechatQrStartResult },
    ChannelWechatQrPoll => "channel/wechat-qr/poll" { params: ChannelWechatQrPollParams, result: ChannelWechatQrPollResult },
    ShellStart => "shell/start" { params: ShellStartParams, result: ShellStartResult },
    TerminalStart => "terminal/start" { params: TerminalStartParams, result: TerminalStartResult },
    TerminalWrite => "terminal/write" { params: TerminalWriteParams, result: TerminalMutationResult },
    TerminalResize => "terminal/resize" { params: TerminalResizeParams, result: TerminalMutationResult },
    TerminalTerminate => "terminal/terminate" { params: TerminalTerminateParams, result: TerminalMutationResult },
    SourceReset => "source/reset" { params: SourceResetParams, result: ThreadSnapshot },
    SettingsUpdate => "settings/update" { params: SettingsUpdateParams, result: SettingsReadResult },
    SettingsRead => "settings/read" { params: SettingsReadParams, result: SettingsReadResult },
    WebSearchSettingsRead => "web/search/settings/read" { params: WebSearchSettingsReadParams, result: WebSearchSettingsView },
    WebSearchSettingsUpdate => "web/search/settings/update" { params: WebSearchSettingsUpdateParams, result: WebSearchSettingsView },
    ModelSettingsRead => "model/settings/read" { params: ModelSettingsReadParams, result: ModelSettingsResult },
    ModelProviderSave => "model/provider/save" { params: ModelProviderSaveParams, result: ModelSettingsResult },
    ModelProviderCatalog => "model/provider/catalog" { params: ModelProviderCatalogParams, result: ModelProviderCatalogResult },
    ModelStateRead => "model/state/read" { params: ModelStateReadParams, result: ModelStateResult },
    ModelStateSet => "model/state/set" { params: ModelStateSetParams, result: ModelStateResult },
    ModelAssignmentSet => "model/assignment/set" { params: ModelAssignmentSetParams, result: ModelAssignmentSetResult },
    VoiceAsrTranscribe => "voice/asr/transcribe" { params: VoiceAsrTranscribeParams, result: VoiceAsrTranscribeResult },
    VoiceTtsSynthesize => "voice/tts/synthesize" { params: VoiceTtsSynthesizeParams, result: VoiceTtsSynthesizeResult },
    VoicePolicyRead => "voice/policy/read" { params: VoicePolicyReadParams, result: VoicePolicyResult },
    VoicePolicyUpdate => "voice/policy/update" { params: VoicePolicyUpdateParams, result: VoicePolicyResult },
    ThreadRealtimeStart => "thread/realtime/start" { params: ThreadRealtimeStartParams, result: ThreadRealtimeStartResult },
    ThreadRealtimeAppendAudio => "thread/realtime/appendAudio" { params: ThreadRealtimeAppendAudioParams, result: ThreadRealtimeMutationResult },
    ThreadRealtimeAppendText => "thread/realtime/appendText" { params: ThreadRealtimeAppendTextParams, result: ThreadRealtimeMutationResult },
    ThreadRealtimeAppendSpeech => "thread/realtime/appendSpeech" { params: ThreadRealtimeAppendSpeechParams, result: ThreadRealtimeMutationResult },
    ThreadRealtimeStop => "thread/realtime/stop" { params: ThreadRealtimeSessionParams, result: ThreadRealtimeMutationResult },
    ThreadRealtimeListVoices => "thread/realtime/listVoices" { params: ThreadRealtimeSessionParams, result: ThreadRealtimeListVoicesResult },
    WorkspaceFiles => "workspace/files" { params: WorkspaceFilesParams, result: WorkspaceFilesResult },
    WorkspaceFolderList => "workspace/folders" { params: WorkspaceFolderListParams, result: WorkspaceFolderListResult },
    WorkspaceGitBranches => "workspace/git/branches" { params: WorkspaceGitBranchesParams, result: WorkspaceGitBranchesResult },
    WorkspaceGitCheckout => "workspace/git/checkout" { params: WorkspaceGitCheckoutParams, result: WorkspaceGitBranchesResult },
    WorkspaceFileRead => "workspace/file/read" { params: WorkspaceFileReadParams, result: WorkspaceFileReadResult },
    WorkspaceFilePreviewOpen => "workspace/file/preview/open" { params: WorkspaceFilePreviewOpenParams, result: WorkspaceFilePreviewOpenResult },
    WorkspaceFilePreviewRelease => "workspace/file/preview/release" { params: WorkspaceFilePreviewReleaseParams, result: WorkspaceFilePreviewReleaseResult },
    WorkspaceFileWrite => "workspace/file/write" { params: WorkspaceFileWriteParams, result: WorkspaceFileWriteResult },
    WorkspaceFileExternalActions => "workspace/file/externalActions" { params: WorkspaceFileExternalActionsParams, result: WorkspaceFileExternalActionsResult },
    WorkspaceFileOpenExternal => "workspace/file/openExternal" { params: WorkspaceFileOpenExternalParams, result: WorkspaceFileOpenExternalResult },
    WorkspaceDiff => "workspace/diff" { params: WorkspaceDiffParams, result: WorkspaceDiffResult },
    WorkspaceChanges => "workspace/changes" { params: WorkspaceChangesParams, result: WorkspaceChangesResult },
    WorkspaceChangeAccept => "workspace/change/accept" { params: WorkspaceChangeFileParams, result: WorkspaceChangeMutationResult },
    WorkspaceChangeReject => "workspace/change/reject" { params: WorkspaceChangeFileParams, result: WorkspaceChangeMutationResult },
    WorkspaceCreate => "workspace/create" { params: WorkspaceCreateParams, result: WorkspaceCreateResult },
    ContextRead => "context/read" { params: ContextReadParams, result: ContextReadResult },
    ObservabilityRead => "observability/read" { params: ObservabilityReadParams, result: ObservabilityReadResult },
    UsageRead => "usage/read" { params: UsageReadParams, result: UsageReadResult },
}
