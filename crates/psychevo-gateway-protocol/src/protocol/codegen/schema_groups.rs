fn schema_group_module(name: &str) -> &'static str {
    if name.starts_with("JsonRpc") || matches!(name, "ClientRequest" | "ServerNotification") {
        return "rpc";
    }
    if name.starts_with("Source") || name.starts_with("GatewaySource") {
        return "source";
    }
    if name == "GatewayEvent" || name == "GatewaySelectedSkill" {
        return "gateway/events";
    }
    if matches!(
        name,
        "AgentListParams" | "AgentReadParams" | "AgentWriteParams" | "AgentDeleteParams"
    ) {
        return "gateway/agents/requests";
    }
    if matches!(
        name,
        "AgentListResult" | "AgentReadResult" | "AgentWriteResult" | "AgentDeleteResult"
    ) {
        return "gateway/agents/results";
    }
    if matches!(name, "AgentStatusParams" | "AgentStatusResult")
        || name.starts_with("AgentRun")
        || name == "AgentStatusControlView"
    {
        return "gateway/agents/runs";
    }
    if name.starts_with("Agent") {
        return "gateway/agents/definitions";
    }
    if name.starts_with("BackendDoctor") {
        return "gateway/backend/doctor";
    }
    if name.starts_with("Backend") {
        return "gateway/backend/config";
    }
    if matches!(
        name,
        "AutomationScheduleInput"
            | "AutomationTaskKind"
            | "AutomationTargetInput"
            | "AutomationExecutionPolicy"
            | "AutomationExecutionInput"
    ) {
        return "automations/definitions";
    }
    if matches!(
        name,
        "AutomationListParams"
            | "AutomationDraftParams"
            | "AutomationWriteParams"
            | "AutomationIdParams"
            | "AutomationRunParams"
    ) {
        return "automations/requests";
    }
    if matches!(
        name,
        "AutomationDraftView" | "AutomationTaskView" | "AutomationRunView"
    ) {
        return "automations/views";
    }
    if matches!(
        name,
        "AutomationListResult" | "AutomationDraftResult"
    ) {
        return "automations/list-results";
    }
    if matches!(
        name,
        "AutomationMutationResult" | "AutomationDeleteResult" | "AutomationRunResult"
    ) {
        return "automations/mutation-results";
    }
    if matches!(name, "BackendKind" | "GatewayBackendInfo") {
        return "gateway/backend/core";
    }
    if name.starts_with("Plugin") {
        return "gateway/plugins/requests";
    }
    if matches!(
        name,
        "ChannelDoctorParams"
            | "ChannelDoctorCheck"
            | "ChannelDoctorChannelView"
            | "ChannelDoctorResult"
    ) {
        return "gateway/channels/doctor";
    }
    if matches!(
        name,
        "ChannelListParams"
            | "ChannelIdParams"
            | "ChannelEnableParams"
            | "ChannelUpdateParams"
            | "ChannelWechatQrStartParams"
            | "ChannelWechatQrPollParams"
    ) {
        return "gateway/channels/requests";
    }
    if matches!(
        name,
        "ChannelCredentialView"
            | "ChannelAllowlistView"
            | "ChannelConfigView"
            | "ChannelRunnerView"
            | "ChannelSourceBindingView"
    ) {
        return "gateway/channels/config";
    }
    if name.starts_with("Channel") {
        return "gateway/channels/results";
    }
    if name == "PendingActionView" || name.starts_with("GatewayAction") {
        return "interaction";
    }
    if matches!(
        name,
        "GatewayRequestScope"
            | "GatewayThreadSelector"
            | "GatewayThread"
            | "GatewayTurn"
            | "GatewayTurnStatus"
            | "GatewayInputPart"
            | "GatewayImageInput"
            | "GatewaySelectedSkill"
            | "GatewayEvent"
    ) || name.starts_with("GatewayMention")
    {
        return "gateway/core";
    }
    if name.starts_with("Transcript") {
        return "transcript";
    }
    if matches!(
        name,
        "ThreadMutationResult" | "ThreadDeleteResult" | "ThreadListResult"
    ) {
        return "thread/results";
    }
    if name == "ThreadSnapshot" {
        return "thread/snapshot";
    }
    if matches!(name, "ThreadBrowserResult" | "ThreadBrowserWorkspace") {
        return "thread/browser";
    }
    if name.starts_with("ThreadTrace") {
        return "thread/trace";
    }
    if name.starts_with("Thread") {
        return "thread/controls";
    }
    if name.starts_with("Session") || name == "GatewayActivityView" {
        return "thread/session";
    }
    if name.starts_with("Completion") {
        return "completion";
    }
    if name.starts_with("Command") {
        return "command";
    }
    if name.starts_with("Shell") {
        return "shell";
    }
    if name.starts_with("Runtime") {
        return "turn/runtime";
    }
    if matches!(
        name,
        "TurnStartParams"
            | "TurnSteerParams"
            | "TurnInterruptParams"
            | "TurnTakeoverParams"
            | "TurnStartResult"
            | "TurnControlResult"
            | "TurnTakeoverResult"
    ) {
        return "turn/control";
    }
    if name == "GatewayTurnError" || name.starts_with("Turn") {
        return "turn/result";
    }
    if name.starts_with("Permission")
        || name.starts_with("Clarify")
        || name.starts_with("Pending")
        || name == "InteractionRespondResult"
    {
        return "interaction";
    }
    if name.starts_with("Settings") || name.starts_with("Workbench") {
        return "settings";
    }
    if matches!(name, "InitializeParams" | "InitializeResult" | "GatewayProfileView" | "ReadyzResult") {
        return "system/init";
    }
    if name.starts_with("Slash") {
        return "system/slash";
    }
    if name.starts_with("Terminal") {
        return "system/terminal";
    }
    if matches!(
        name,
        "ModelProviderSaveParams"
            | "ModelProviderCatalogParams"
            | "ModelProviderView"
            | "ModelCredentialStatus"
            | "ModelOptionView"
            | "AuxiliaryModelAssignmentView"
            | "ModelProviderCatalogResult"
    ) {
        return "model/catalog";
    }
    if name.starts_with("Model") {
        return "model/state";
    }
    if name.starts_with("Observability") {
        return "observability";
    }
    if name.starts_with("Usage") {
        return "usage";
    }
    if name.starts_with("WorkspaceFile")
        || matches!(
            name,
            "WorkspaceFilesParams"
                | "WorkspaceFilesResult"
                | "WorkspaceCreateParams"
                | "WorkspaceCreateResult"
        )
    {
        return "workspace/files";
    }
    if name.starts_with("WorkspaceDiff")
        || name.starts_with("WorkspaceChange")
        || matches!(name, "WorkspaceChangesParams" | "WorkspaceChangesResult")
    {
        return "workspace/review";
    }
    if name.starts_with("Context") {
        return "context";
    }
    if name.starts_with("CreateLaunch") || name == "ManagedServerState" {
        return "launch";
    }
    "system"
}

fn schema_group_const(module: &str) -> &'static str {
    match module {
        "automations/definitions" => "automationDefinitionSchemas",
        "automations/list-results" => "automationListResultSchemas",
        "automations/mutation-results" => "automationMutationResultSchemas",
        "automations/requests" => "automationRequestSchemas",
        "automations/views" => "automationViewSchemas",
        "command" => "commandSchemas",
        "completion" => "completionSchemas",
        "context" => "contextSchemas",
        "gateway/agents/definitions" => "gatewayAgentDefinitionSchemas",
        "gateway/agents/requests" => "gatewayAgentRequestSchemas",
        "gateway/agents/results" => "gatewayAgentResultSchemas",
        "gateway/agents/runs" => "gatewayAgentRunSchemas",
        "gateway/backend/config" => "gatewayBackendConfigSchemas",
        "gateway/backend/core" => "gatewayBackendCoreSchemas",
        "gateway/backend/doctor" => "gatewayBackendDoctorSchemas",
        "gateway/channels/config" => "gatewayChannelConfigSchemas",
        "gateway/channels/doctor" => "gatewayChannelDoctorSchemas",
        "gateway/channels/requests" => "gatewayChannelRequestSchemas",
        "gateway/channels/results" => "gatewayChannelResultSchemas",
        "gateway/core" => "gatewayCoreSchemas",
        "gateway/events" => "gatewayEventSchemas",
        "gateway/plugins/requests" => "gatewayPluginRequestSchemas",
        "interaction" => "interactionSchemas",
        "launch" => "launchSchemas",
        "model/catalog" => "modelCatalogSchemas",
        "model/state" => "modelStateSchemas",
        "observability" => "observabilitySchemas",
        "rpc" => "rpcSchemas",
        "settings" => "settingsSchemas",
        "shell" => "shellSchemas",
        "source" => "sourceSchemas",
        "system/init" => "systemInitSchemas",
        "system/slash" => "systemSlashSchemas",
        "system/terminal" => "systemTerminalSchemas",
        "thread/browser" => "threadBrowserSchemas",
        "thread/controls" => "threadControlSchemas",
        "thread/results" => "threadResultSchemas",
        "thread/session" => "threadSessionSchemas",
        "thread/snapshot" => "threadSnapshotSchemas",
        "thread/trace" => "threadTraceSchemas",
        "transcript" => "transcriptSchemas",
        "turn/control" => "turnControlSchemas",
        "turn/result" => "turnResultSchemas",
        "turn/runtime" => "turnRuntimeSchemas",
        "usage" => "usageSchemas",
        "workspace/files" => "workspaceFileSchemas",
        "workspace/review" => "workspaceReviewSchemas",
        _ => unreachable!("unknown schema group module"),
    }
}

fn schema_group_refs_const(module: &str) -> &'static str {
    match module {
        "automations/definitions" => "automationDefinitionSchemaRefs",
        "automations/list-results" => "automationListResultSchemaRefs",
        "automations/mutation-results" => "automationMutationResultSchemaRefs",
        "automations/requests" => "automationRequestSchemaRefs",
        "automations/views" => "automationViewSchemaRefs",
        "command" => "commandSchemaRefs",
        "completion" => "completionSchemaRefs",
        "context" => "contextSchemaRefs",
        "gateway/agents/definitions" => "gatewayAgentDefinitionSchemaRefs",
        "gateway/agents/requests" => "gatewayAgentRequestSchemaRefs",
        "gateway/agents/results" => "gatewayAgentResultSchemaRefs",
        "gateway/agents/runs" => "gatewayAgentRunSchemaRefs",
        "gateway/backend/config" => "gatewayBackendConfigSchemaRefs",
        "gateway/backend/core" => "gatewayBackendCoreSchemaRefs",
        "gateway/backend/doctor" => "gatewayBackendDoctorSchemaRefs",
        "gateway/channels/config" => "gatewayChannelConfigSchemaRefs",
        "gateway/channels/doctor" => "gatewayChannelDoctorSchemaRefs",
        "gateway/channels/requests" => "gatewayChannelRequestSchemaRefs",
        "gateway/channels/results" => "gatewayChannelResultSchemaRefs",
        "gateway/core" => "gatewayCoreSchemaRefs",
        "gateway/events" => "gatewayEventSchemaRefs",
        "gateway/plugins/requests" => "gatewayPluginRequestSchemaRefs",
        "interaction" => "interactionSchemaRefs",
        "launch" => "launchSchemaRefs",
        "model/catalog" => "modelCatalogSchemaRefs",
        "model/state" => "modelStateSchemaRefs",
        "observability" => "observabilitySchemaRefs",
        "rpc" => "rpcSchemaRefs",
        "settings" => "settingsSchemaRefs",
        "shell" => "shellSchemaRefs",
        "source" => "sourceSchemaRefs",
        "system/init" => "systemInitSchemaRefs",
        "system/slash" => "systemSlashSchemaRefs",
        "system/terminal" => "systemTerminalSchemaRefs",
        "thread/browser" => "threadBrowserSchemaRefs",
        "thread/controls" => "threadControlSchemaRefs",
        "thread/results" => "threadResultSchemaRefs",
        "thread/session" => "threadSessionSchemaRefs",
        "thread/snapshot" => "threadSnapshotSchemaRefs",
        "thread/trace" => "threadTraceSchemaRefs",
        "transcript" => "transcriptSchemaRefs",
        "turn/control" => "turnControlSchemaRefs",
        "turn/result" => "turnResultSchemaRefs",
        "turn/runtime" => "turnRuntimeSchemaRefs",
        "usage" => "usageSchemaRefs",
        "workspace/files" => "workspaceFileSchemaRefs",
        "workspace/review" => "workspaceReviewSchemaRefs",
        _ => unreachable!("unknown schema group module"),
    }
}
