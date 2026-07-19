import {
  RpcNotificationSchema,
  RpcResponseSchema,
  ThreadSnapshotSchema,
  type AgentDeleteParams,
  type AgentDeleteResult,
  type AgentControlParams,
  type AgentControlResult,
  type AgentListParams,
  type AgentListResult,
  type AgentReadParams,
  type AgentReadResult,
  type AgentSetEnabledParams,
  type AgentSetEnabledResult,
  type AgentStatusParams,
  type AgentStatusResult,
  type AgentWriteParams,
  type AgentWriteResult,
  type TeamDeleteParams,
  type TeamDeleteResult,
  type TeamListParams,
  type TeamListResult,
  type TeamReadParams,
  type TeamReadResult,
  type TeamSetEnabledParams,
  type TeamSetEnabledResult,
  type TeamStatusParams,
  type TeamStatusResult,
  type TeamWriteParams,
  type TeamWriteResult,
  type AutomationDeleteResult,
  type AutomationDraftParams,
  type AutomationDraftResult,
  type AutomationIdParams,
  type AutomationListParams,
  type AutomationListResult,
  type AutomationMutationResult,
  type AutomationRunParams,
  type AutomationRunResult,
  type AutomationWriteParams,
  type BackendDeleteParams,
  type BackendDeleteResult,
  type BackendDoctorParams,
  type BackendDoctorResult,
  type BackendListParams,
  type BackendListResult,
  type BackendManageParams,
  type BackendManageResult,
  type BackendWriteParams,
  type BackendWriteResult,
  type ChannelDoctorParams,
  type ChannelDoctorResult,
  type ChannelEnableParams,
  type ChannelEnableResult,
  type ChannelIdParams,
  type ChannelListParams,
  type ChannelListResult,
  type ChannelSourceListResult,
  type ChannelUpdateParams,
  type ChannelWechatQrPollParams,
  type ChannelWechatQrPollResult,
  type ChannelWechatQrStartParams,
  type ChannelWechatQrStartResult,
  type CommandExecuteParams,
  type CommandExecuteResult,
  type CommandListParams,
  type CommandListResult,
  type CompletionListParams,
  type CompletionListResult,
  type ContextReadParams,
  type ContextReadResult,
  type GatewayRequestScope,
  type InitializeParams,
  type InitializeResult,
  type ModelAssignmentSetParams,
  type ModelAssignmentSetResult,
  type ModelProviderCatalogParams,
  type ModelProviderCatalogResult,
  type ModelProviderSaveParams,
  type ModelSettingsReadParams,
  type ModelSettingsResult,
  type ModelStateReadParams,
  type ModelStateResult,
  type ModelStateSetParams,
  type McpListParams,
  type McpNameParams,
  type McpOAuthStartParams,
  type McpOAuthStatusParams,
  type McpReadParams,
  type McpSetEnabledParams,
  type McpSetToolPolicyParams,
  type McpUpsertParams,
  type ObservabilityReadParams,
  type ObservabilityReadResult,
  type PluginCatalogAddParams,
  type PluginCatalogListParams,
  type PluginCatalogRemoveParams,
  type PluginCatalogUpgradeParams,
  type PluginAuthorityRefreshParams,
  type PluginAuthorityWriteParams,
  type PluginConnectStartParams,
  type PluginConnectStatusParams,
  type PluginDoctorParams,
  type PluginInspectParams,
  type PluginInstallParams,
  type PluginListParams,
  type PluginReadParams,
  type PluginSetEnabledParams,
  type PluginSetTrustParams,
  type PluginUninstallParams,
  type RpcNotification,
  type ThreadContextReadParams,
  type ThreadContextReadResult,
  type ThreadDraftOpenParams,
  type ThreadDraftOpenResult,
  type ThreadDraftPrepareParams,
  type ThreadDraftPrepareResult,
  type ThreadControlSetParams,
  type ThreadControlSetResult,
  type ThreadActionRunParams,
  type ThreadActionRunResult,
  type ThreadInteractionRespondParams,
  type ThreadInteractionRespondResult,
  type ThreadHistoryReadParams,
  type ThreadHistoryReadResult,
  type ThreadImportListParams,
  type ThreadImportListResult,
  type ThreadImportParams,
  type ThreadImportResult,
  type RuntimeProfileDeleteParams,
  type RuntimeProfileDeleteResult,
  type RuntimeProfileListParams,
  type RuntimeProfileListResult,
  type RuntimeProfileReadParams,
  type RuntimeProfileReadResult,
  type RuntimeProfileSetEnabledParams,
  type RuntimeProfileWriteParams,
  type RuntimeProfileWriteResult,
  type SettingsReadParams,
  type SettingsReadResult,
  type SettingsUpdateParams,
  type WebSearchSettingsReadParams,
  type WebSearchSettingsUpdateParams,
  type WebSearchSettingsView,
  type ShellStartParams,
  type SkillInstallParams,
  type SkillListParams,
  type SkillReadParams,
  type SkillSetEnabledParams,
  type SkillUninstallParams,
  type SkillWriteParams,
  type SkillWriteResult,
  type ShellStartResult,
  type SlashSettingsReadParams,
  type SlashSettingsResult,
  type SlashSettingsUpdateParams,
  type SourceResetParams,
  type TerminalMutationResult,
  type TerminalResizeParams,
  type TerminalStartParams,
  type TerminalStartResult,
  type TerminalTerminateParams,
  type TerminalWriteParams,
  type ThreadRealtimeAppendAudioParams,
  type ThreadRealtimeAppendSpeechParams,
  type ThreadRealtimeAppendTextParams,
  type ThreadRealtimeListVoicesResult,
  type ThreadRealtimeMutationResult,
  type ThreadRealtimeSessionParams,
  type ThreadRealtimeStartParams,
  type ThreadRealtimeStartResult,
  type ToolCreateParams,
  type ToolListParams,
  type ToolReadParams,
  type ToolRemoveParams,
  type ToolSetEnabledParams,
  type ThreadBrowserParams,
  type ThreadBrowserResult,
  type ThreadDeleteResult,
  type ThreadIdParams,
  type ThreadListParams,
  type ThreadListResult,
  type ThreadMutationResult,
  type ThreadReadParams,
  type ThreadRenameParams,
  type ThreadResumeParams,
  type ThreadSnapshot,
  type ThreadTraceParams,
  type ThreadTraceResult,
  type TurnStartParams,
  type TurnStartResult,
  type UsageReadParams,
  type UsageReadResult,
  type VoiceAsrTranscribeParams,
  type VoiceAsrTranscribeResult,
  type VoicePolicyReadParams,
  type VoicePolicyResult,
  type VoicePolicyUpdateParams,
  type VoiceTtsSynthesizeParams,
  type VoiceTtsSynthesizeResult,
  type WorkspaceChangeFileParams,
  type WorkspaceChangeMutationResult,
  type WorkspaceChangesParams,
  type WorkspaceChangesResult,
  type WorkspaceDiffParams,
  type WorkspaceDiffResult,
  type WorkspaceCreateParams,
  type WorkspaceCreateResult,
  type WorkspaceFolderListParams,
  type WorkspaceFolderListResult,
  type WorkspaceGitBranchesParams,
  type WorkspaceGitBranchesResult,
  type WorkspaceGitCheckoutParams,
  type WorkspaceFileExternalActionsParams,
  type WorkspaceFileExternalActionsResult,
  type WorkspaceFileOpenExternalParams,
  type WorkspaceFileOpenExternalResult,
  type WorkspaceFileReadParams,
  type WorkspaceFileReadResult,
  type WorkspaceFileWriteParams,
  type WorkspaceFileWriteResult,
  type WorkspaceFilesParams,
  type WorkspaceFilesResult
} from "@psychevo/protocol";
import type { GatewayEndpoint } from "@psychevo/host";

export type { GatewayEndpoint } from "@psychevo/host";
export {
  appendOptimisticPrompt,
  applyLiveTranscriptEvent,
  reconcileThreadSnapshot
} from "./transcript";
export {
  acceptThreadTurn,
  applyGatewayEventToThreadSnapshot,
  bindThreadSnapshot,
  emptyThreadSnapshot,
  latestAssistantTranscriptText,
  prepareThreadTurn,
  threadTurnStartParams,
  ThreadController
} from "./thread-controller";
export type {
  ThreadGatewayEventApplication,
  ThreadTurnAdmission,
  ThreadTurnAcceptance,
  ThreadTurnControls,
  ThreadTurnPreparation,
  ThreadTurnStartInput,
  ThreadTurnStartPlan
} from "./thread-controller";

export type NotificationHandler = (notification: RpcNotification) => void;
export type GatewayRawMessageHandler = (data: unknown) => void;
export type GatewayJsonResult = Record<string, unknown>;

export interface GatewayTransport {
  close(): void;
  connect(): Promise<void>;
  onDisconnect(handler: (message: string) => void): () => void;
  onMessage(handler: GatewayRawMessageHandler): () => void;
  send(data: string): void;
}

export interface GatewayRequestParams {
  "agent/delete": AgentDeleteParams;
  "agent/list": AgentListParams;
  "agent/read": AgentReadParams;
  "agent/setEnabled": AgentSetEnabledParams;
  "agent/status": AgentStatusParams;
  "agent/write": AgentWriteParams;
  "agent/control": AgentControlParams;
  "team/delete": TeamDeleteParams;
  "team/list": TeamListParams;
  "team/read": TeamReadParams;
  "team/setEnabled": TeamSetEnabledParams;
  "team/status": TeamStatusParams;
  "team/write": TeamWriteParams;
  "automation/delete": AutomationIdParams;
  "automation/draft": AutomationDraftParams;
  "automation/list": AutomationListParams;
  "automation/pause": AutomationIdParams;
  "automation/resume": AutomationIdParams;
  "automation/run": AutomationRunParams;
  "automation/write": AutomationWriteParams;
  "backend/delete": BackendDeleteParams;
  "backend/doctor": BackendDoctorParams;
  "backend/install": BackendManageParams;
  "backend/list": BackendListParams;
  "backend/repair": BackendManageParams;
  "backend/upgrade": BackendManageParams;
  "backend/write": BackendWriteParams;
  "channel/doctor": ChannelDoctorParams;
  "channel/delete": ChannelIdParams;
  "channel/enable": ChannelEnableParams;
  "channel/list": ChannelListParams;
  "channel/show": ChannelIdParams;
  "channel/source/list": ChannelIdParams;
  "channel/update": ChannelUpdateParams;
  "channel/wechat-qr/poll": ChannelWechatQrPollParams;
  "channel/wechat-qr/start": ChannelWechatQrStartParams;
  "command/execute": CommandExecuteParams;
  "command/list": CommandListParams;
  "completion/list": CompletionListParams;
  "context/read": ContextReadParams;
  "model/assignment/set": ModelAssignmentSetParams;
  "model/provider/catalog": ModelProviderCatalogParams;
  "model/provider/save": ModelProviderSaveParams;
  "model/settings/read": ModelSettingsReadParams;
  "model/state/read": ModelStateReadParams;
  "model/state/set": ModelStateSetParams;
  "observability/read": ObservabilityReadParams;
  "usage/read": UsageReadParams;
  "initialize": InitializeParams;
  "plugin/catalog/add": PluginCatalogAddParams;
  "plugin/catalog/list": PluginCatalogListParams;
  "plugin/catalog/remove": PluginCatalogRemoveParams;
  "plugin/catalog/upgrade": PluginCatalogUpgradeParams;
  "plugin/authority/refresh": PluginAuthorityRefreshParams;
  "plugin/authority/write": PluginAuthorityWriteParams;
  "plugin/connect/start": PluginConnectStartParams;
  "plugin/connect/status": PluginConnectStatusParams;
  "plugin/doctor": PluginDoctorParams;
  "plugin/import/inspect": PluginInspectParams;
  "plugin/install": PluginInstallParams;
  "plugin/list": PluginListParams;
  "plugin/read": PluginReadParams;
  "plugin/setEnabled": PluginSetEnabledParams;
  "plugin/setTrust": PluginSetTrustParams;
  "plugin/uninstall": PluginUninstallParams;
  "skill/install": SkillInstallParams;
  "skill/list": SkillListParams;
  "skill/read": SkillReadParams;
  "skill/setEnabled": SkillSetEnabledParams;
  "skill/uninstall": SkillUninstallParams;
  "skill/write": SkillWriteParams;
  "tool/create": ToolCreateParams;
  "tool/list": ToolListParams;
  "tool/read": ToolReadParams;
  "tool/remove": ToolRemoveParams;
  "tool/setEnabled": ToolSetEnabledParams;
  "mcp/list": McpListParams;
  "mcp/oauth/logout": McpNameParams;
  "mcp/oauth/start": McpOAuthStartParams;
  "mcp/oauth/status": McpOAuthStatusParams;
  "mcp/read": McpReadParams;
  "mcp/remove": McpNameParams;
  "mcp/setEnabled": McpSetEnabledParams;
  "mcp/setToolPolicy": McpSetToolPolicyParams;
  "mcp/test": McpNameParams;
  "mcp/upsert": McpUpsertParams;
  "runtime/profile/delete": RuntimeProfileDeleteParams;
  "runtime/profile/list": RuntimeProfileListParams;
  "runtime/profile/read": RuntimeProfileReadParams;
  "runtime/profile/setEnabled": RuntimeProfileSetEnabledParams;
  "runtime/profile/write": RuntimeProfileWriteParams;
  "settings/read": SettingsReadParams;
  "settings/update": SettingsUpdateParams;
  "web/search/settings/read": WebSearchSettingsReadParams;
  "web/search/settings/update": WebSearchSettingsUpdateParams;
  "shell/start": ShellStartParams;
  "slash/settings/read": SlashSettingsReadParams;
  "slash/settings/update": SlashSettingsUpdateParams;
  "source/reset": SourceResetParams;
  "terminal/resize": TerminalResizeParams;
  "terminal/start": TerminalStartParams;
  "terminal/terminate": TerminalTerminateParams;
  "terminal/write": TerminalWriteParams;
  "thread/archive": ThreadIdParams;
  "thread/browser": ThreadBrowserParams;
  "thread/context/read": ThreadContextReadParams;
  "thread/draft/open": ThreadDraftOpenParams;
  "thread/draft/prepare": ThreadDraftPrepareParams;
  "thread/control/set": ThreadControlSetParams;
  "thread/action/run": ThreadActionRunParams;
  "thread/interaction/respond": ThreadInteractionRespondParams;
  "thread/history/read": ThreadHistoryReadParams;
  "thread/import/list": ThreadImportListParams;
  "thread/import": ThreadImportParams;
  "thread/delete": ThreadIdParams;
  "thread/list": ThreadListParams;
  "thread/read": ThreadReadParams;
  "thread/rename": ThreadRenameParams;
  "thread/restore": ThreadIdParams;
  "thread/resume": ThreadResumeParams;
  "thread/trace": ThreadTraceParams;
  "thread/realtime/appendAudio": ThreadRealtimeAppendAudioParams;
  "thread/realtime/appendSpeech": ThreadRealtimeAppendSpeechParams;
  "thread/realtime/appendText": ThreadRealtimeAppendTextParams;
  "thread/realtime/listVoices": ThreadRealtimeSessionParams;
  "thread/realtime/start": ThreadRealtimeStartParams;
  "thread/realtime/stop": ThreadRealtimeSessionParams;
  "turn/start": TurnStartParams;
  "voice/asr/transcribe": VoiceAsrTranscribeParams;
  "voice/policy/read": VoicePolicyReadParams;
  "voice/policy/update": VoicePolicyUpdateParams;
  "voice/tts/synthesize": VoiceTtsSynthesizeParams;
  "workspace/change/accept": WorkspaceChangeFileParams;
  "workspace/change/reject": WorkspaceChangeFileParams;
  "workspace/changes": WorkspaceChangesParams;
  "workspace/create": WorkspaceCreateParams;
  "workspace/diff": WorkspaceDiffParams;
  "workspace/file/externalActions": WorkspaceFileExternalActionsParams;
  "workspace/file/openExternal": WorkspaceFileOpenExternalParams;
  "workspace/file/read": WorkspaceFileReadParams;
  "workspace/file/write": WorkspaceFileWriteParams;
  "workspace/files": WorkspaceFilesParams;
  "workspace/folders": WorkspaceFolderListParams;
  "workspace/git/branches": WorkspaceGitBranchesParams;
  "workspace/git/checkout": WorkspaceGitCheckoutParams;
}

export interface GatewayRequestResults {
  "agent/delete": AgentDeleteResult;
  "agent/list": AgentListResult;
  "agent/read": AgentReadResult;
  "agent/setEnabled": AgentSetEnabledResult;
  "agent/status": AgentStatusResult;
  "agent/write": AgentWriteResult;
  "agent/control": AgentControlResult;
  "team/delete": TeamDeleteResult;
  "team/list": TeamListResult;
  "team/read": TeamReadResult;
  "team/setEnabled": TeamSetEnabledResult;
  "team/status": TeamStatusResult;
  "team/write": TeamWriteResult;
  "automation/delete": AutomationDeleteResult;
  "automation/draft": AutomationDraftResult;
  "automation/list": AutomationListResult;
  "automation/pause": AutomationMutationResult;
  "automation/resume": AutomationMutationResult;
  "automation/run": AutomationRunResult;
  "automation/write": AutomationMutationResult;
  "backend/delete": BackendDeleteResult;
  "backend/doctor": BackendDoctorResult;
  "backend/install": BackendManageResult;
  "backend/list": BackendListResult;
  "backend/repair": BackendManageResult;
  "backend/upgrade": BackendManageResult;
  "backend/write": BackendWriteResult;
  "channel/doctor": ChannelDoctorResult;
  "channel/delete": ChannelListResult;
  "channel/enable": ChannelEnableResult;
  "channel/list": ChannelListResult;
  "channel/show": ChannelEnableResult;
  "channel/source/list": ChannelSourceListResult;
  "channel/update": ChannelEnableResult;
  "channel/wechat-qr/poll": ChannelWechatQrPollResult;
  "channel/wechat-qr/start": ChannelWechatQrStartResult;
  "command/execute": CommandExecuteResult;
  "command/list": CommandListResult;
  "completion/list": CompletionListResult;
  "context/read": ContextReadResult;
  "model/assignment/set": ModelAssignmentSetResult;
  "model/provider/catalog": ModelProviderCatalogResult;
  "model/provider/save": ModelSettingsResult;
  "model/settings/read": ModelSettingsResult;
  "model/state/read": ModelStateResult;
  "model/state/set": ModelStateResult;
  "observability/read": ObservabilityReadResult;
  "usage/read": UsageReadResult;
  "initialize": InitializeResult;
  "plugin/catalog/add": GatewayJsonResult;
  "plugin/catalog/list": GatewayJsonResult;
  "plugin/catalog/remove": GatewayJsonResult;
  "plugin/catalog/upgrade": GatewayJsonResult;
  "plugin/authority/refresh": GatewayJsonResult;
  "plugin/authority/write": GatewayJsonResult;
  "plugin/connect/start": GatewayJsonResult;
  "plugin/connect/status": GatewayJsonResult;
  "plugin/doctor": GatewayJsonResult;
  "plugin/import/inspect": GatewayJsonResult;
  "plugin/install": GatewayJsonResult;
  "plugin/list": GatewayJsonResult;
  "plugin/read": GatewayJsonResult;
  "plugin/setEnabled": GatewayJsonResult;
  "plugin/setTrust": GatewayJsonResult;
  "plugin/uninstall": GatewayJsonResult;
  "skill/install": GatewayJsonResult;
  "skill/list": GatewayJsonResult;
  "skill/read": GatewayJsonResult;
  "skill/setEnabled": GatewayJsonResult;
  "skill/uninstall": GatewayJsonResult;
  "skill/write": SkillWriteResult;
  "tool/create": GatewayJsonResult;
  "tool/list": GatewayJsonResult;
  "tool/read": GatewayJsonResult;
  "tool/remove": GatewayJsonResult;
  "tool/setEnabled": GatewayJsonResult;
  "mcp/list": GatewayJsonResult;
  "mcp/oauth/logout": GatewayJsonResult;
  "mcp/oauth/start": GatewayJsonResult;
  "mcp/oauth/status": GatewayJsonResult;
  "mcp/read": GatewayJsonResult;
  "mcp/remove": GatewayJsonResult;
  "mcp/setEnabled": GatewayJsonResult;
  "mcp/setToolPolicy": GatewayJsonResult;
  "mcp/test": GatewayJsonResult;
  "mcp/upsert": GatewayJsonResult;
  "runtime/profile/delete": RuntimeProfileDeleteResult;
  "runtime/profile/list": RuntimeProfileListResult;
  "runtime/profile/read": RuntimeProfileReadResult;
  "runtime/profile/setEnabled": RuntimeProfileWriteResult;
  "runtime/profile/write": RuntimeProfileWriteResult;
  "settings/read": SettingsReadResult;
  "settings/update": SettingsReadResult;
  "web/search/settings/read": WebSearchSettingsView;
  "web/search/settings/update": WebSearchSettingsView;
  "shell/start": ShellStartResult;
  "slash/settings/read": SlashSettingsResult;
  "slash/settings/update": SlashSettingsResult;
  "source/reset": ThreadSnapshot;
  "terminal/resize": TerminalMutationResult;
  "terminal/start": TerminalStartResult;
  "terminal/terminate": TerminalMutationResult;
  "terminal/write": TerminalMutationResult;
  "thread/archive": ThreadMutationResult;
  "thread/browser": ThreadBrowserResult;
  "thread/context/read": ThreadContextReadResult;
  "thread/draft/open": ThreadDraftOpenResult;
  "thread/draft/prepare": ThreadDraftPrepareResult;
  "thread/control/set": ThreadControlSetResult;
  "thread/action/run": ThreadActionRunResult;
  "thread/interaction/respond": ThreadInteractionRespondResult;
  "thread/history/read": ThreadHistoryReadResult;
  "thread/import/list": ThreadImportListResult;
  "thread/import": ThreadImportResult;
  "thread/delete": ThreadDeleteResult;
  "thread/list": ThreadListResult;
  "thread/read": ThreadSnapshot;
  "thread/rename": ThreadMutationResult;
  "thread/restore": ThreadMutationResult;
  "thread/resume": ThreadSnapshot;
  "thread/trace": ThreadTraceResult;
  "thread/realtime/appendAudio": ThreadRealtimeMutationResult;
  "thread/realtime/appendSpeech": ThreadRealtimeMutationResult;
  "thread/realtime/appendText": ThreadRealtimeMutationResult;
  "thread/realtime/listVoices": ThreadRealtimeListVoicesResult;
  "thread/realtime/start": ThreadRealtimeStartResult;
  "thread/realtime/stop": ThreadRealtimeMutationResult;
  "turn/start": TurnStartResult;
  "voice/asr/transcribe": VoiceAsrTranscribeResult;
  "voice/policy/read": VoicePolicyResult;
  "voice/policy/update": VoicePolicyResult;
  "voice/tts/synthesize": VoiceTtsSynthesizeResult;
  "workspace/change/accept": WorkspaceChangeMutationResult;
  "workspace/change/reject": WorkspaceChangeMutationResult;
  "workspace/changes": WorkspaceChangesResult;
  "workspace/create": WorkspaceCreateResult;
  "workspace/diff": WorkspaceDiffResult;
  "workspace/file/externalActions": WorkspaceFileExternalActionsResult;
  "workspace/file/openExternal": WorkspaceFileOpenExternalResult;
  "workspace/file/read": WorkspaceFileReadResult;
  "workspace/file/write": WorkspaceFileWriteResult;
  "workspace/files": WorkspaceFilesResult;
  "workspace/folders": WorkspaceFolderListResult;
  "workspace/git/branches": WorkspaceGitBranchesResult;
  "workspace/git/checkout": WorkspaceGitBranchesResult;
}

export type GatewayMethod = keyof GatewayRequestParams;
export type GatewayRequestInit<M extends GatewayMethod> =
  | GatewayRequestParams[M]
  | Partial<GatewayRequestParams[M]>;

export function scopeForCwd(cwd: string): GatewayRequestScope {
  return {
    cwd,
    source: {
      kind: "web",
      rawId: null,
      lifetime: "persistent",
      rawIdentity: null,
      visibleName: null
    }
  };
}

export function parseThreadSnapshot(value: unknown): ThreadSnapshot {
  return ThreadSnapshotSchema.parse(withThreadSnapshotDefaults(value));
}

export type ThreadInterruptTarget = {
  scope: GatewayRequestScope;
  threadId: string;
};

export function runThreadInterrupt(
  client: GatewayClient,
  target: ThreadInterruptTarget
): Promise<GatewayRequestResults["thread/action/run"]> {
  return client.request("thread/action/run", {
    ...target,
    action: { kind: "interrupt" }
  });
}

export class GatewayClient {
  private nextId = 1;
  private readonly transport: GatewayTransport;
  private readonly pending = new Map<
    string,
    { reject: (error: Error) => void; resolve: (value: unknown) => void }
  >();
  private readonly handlers = new Set<NotificationHandler>();

  readonly endpoint: GatewayEndpoint | null;

  constructor(endpointOrTransport: GatewayEndpoint | GatewayTransport) {
    if (isGatewayTransport(endpointOrTransport)) {
      this.endpoint = null;
      this.transport = endpointOrTransport;
    } else {
      this.endpoint = endpointOrTransport;
      this.transport = new BrowserWebSocketTransport(endpointOrTransport);
    }
    this.transport.onMessage((data) => this.handleMessage(data));
    this.transport.onDisconnect((message) => this.rejectPending(message));
  }

  connect(): Promise<void> {
    return this.transport.connect();
  }

  close(): void {
    this.transport.close();
    this.rejectPending("Gateway WebSocket closed");
  }

  subscribe(handler: NotificationHandler): () => void {
    this.handlers.add(handler);
    return () => this.handlers.delete(handler);
  }

  request<M extends GatewayMethod>(
    method: M,
    params?: GatewayRequestInit<M>
  ): Promise<GatewayRequestResults[M]> {
    const id = String(this.nextId++);
    const payload =
      params === undefined
        ? { jsonrpc: "2.0", id, method }
        : { jsonrpc: "2.0", id, method, params };
    const promise = new Promise<GatewayRequestResults[M]>((resolve, reject) => {
      this.pending.set(id, {
        resolve: (value) => resolve(value as GatewayRequestResults[M]),
        reject
      });
    });
    try {
      this.transport.send(JSON.stringify(payload));
    } catch (error) {
      this.pending.delete(id);
      return Promise.reject(error);
    }
    return promise;
  }

  private handleMessage(data: unknown): void {
    const raw = typeof data === "string" ? data : String(data);
    const value = JSON.parse(raw) as unknown;
    const notification = RpcNotificationSchema.safeParse(value);
    if (notification.success && !("id" in (value as Record<string, unknown>))) {
      for (const handler of this.handlers) {
        handler(notification.data);
      }
      return;
    }

    const response = RpcResponseSchema.parse(value);
    const key = String(response.id);
    const pending = this.pending.get(key);
    if (!pending) {
      return;
    }
    this.pending.delete(key);
    if ("error" in response) {
      pending.reject(new Error(response.error.message));
    } else {
      pending.resolve(response.result);
    }
  }

  private rejectPending(message: string): void {
    for (const pending of this.pending.values()) {
      pending.reject(new Error(message));
    }
    this.pending.clear();
  }
}

function isGatewayTransport(value: GatewayEndpoint | GatewayTransport): value is GatewayTransport {
  return typeof (value as GatewayTransport).send === "function"
    && typeof (value as GatewayTransport).onDisconnect === "function"
    && typeof (value as GatewayTransport).onMessage === "function";
}

class BrowserWebSocketTransport implements GatewayTransport {
  private socket: WebSocket | null = null;
  private readonly disconnectHandlers = new Set<(message: string) => void>();
  private readonly handlers = new Set<GatewayRawMessageHandler>();

  constructor(private readonly endpoint: GatewayEndpoint) {}

  connect(): Promise<void> {
    if (this.socket?.readyState === WebSocket.OPEN) {
      return Promise.resolve();
    }

    return new Promise((resolve, reject) => {
      const socket = new WebSocket(this.endpoint.wsUrl);
      this.socket = socket;
      socket.addEventListener("open", () => resolve(), { once: true });
      socket.addEventListener(
        "error",
        () => reject(new Error("Gateway WebSocket connection failed")),
        { once: true }
      );
      socket.addEventListener("message", (event) => {
        for (const handler of this.handlers) {
          handler(event.data);
        }
      });
      socket.addEventListener("close", () => {
        for (const handler of this.disconnectHandlers) {
          handler("Gateway WebSocket closed");
        }
      });
    });
  }

  close(): void {
    this.socket?.close();
    this.socket = null;
  }

  onMessage(handler: GatewayRawMessageHandler): () => void {
    this.handlers.add(handler);
    return () => this.handlers.delete(handler);
  }

  onDisconnect(handler: (message: string) => void): () => void {
    this.disconnectHandlers.add(handler);
    return () => this.disconnectHandlers.delete(handler);
  }

  send(data: string): void {
    const socket = this.socket;
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      throw new Error("Gateway WebSocket is not connected");
    }
    socket.send(data);
  }
}

function withThreadSnapshotDefaults(value: unknown): unknown {
  const record = asRecord(value);
  if (!record) {
    return value;
  }
  return {
    ...record,
    scope: record.scope ?? defaultScopeFromSource(record.source),
    thread: Object.prototype.hasOwnProperty.call(record, "thread") ? record.thread : null,
    activity: withActivityDefaults(record.activity),
    pendingActions: Array.isArray(record.pendingActions) ? record.pendingActions : []
  };
}

function defaultScopeFromSource(value: unknown): GatewayRequestScope {
  const source = asRecord(value);
  return {
    cwd: "",
    source: {
      kind: typeof source?.kind === "string" ? source.kind : "web",
      rawId: typeof source?.rawId === "string" ? source.rawId : null,
      lifetime: source?.lifetime === "invocation" || source?.lifetime === "process" || source?.lifetime === "persistent"
        ? source.lifetime
        : "persistent",
      rawIdentity: source?.rawIdentity ?? null,
      visibleName: typeof source?.visibleName === "string" ? source.visibleName : null
    }
  };
}

function withActivityDefaults(value: unknown): Record<string, unknown> {
  const activity = asRecord(value) ?? {};
  return {
    ...activity,
    running: activity.running === true,
    activeTurnId: typeof activity.activeTurnId === "string" ? activity.activeTurnId : null,
    queuedTurns: Number.isFinite(activity.queuedTurns) ? activity.queuedTurns : 0
  };
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}
