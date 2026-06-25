import {
  RpcNotificationSchema,
  RpcResponseSchema,
  ThreadSnapshotSchema,
  type AgentDeleteParams,
  type AgentDeleteResult,
  type AgentListParams,
  type AgentListResult,
  type AgentReadParams,
  type AgentReadResult,
  type AgentStatusParams,
  type AgentStatusResult,
  type AgentWriteParams,
  type AgentWriteResult,
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
  type ClarifyRespondParams,
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
  type InteractionRespondResult,
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
  type ObservabilityReadParams,
  type ObservabilityReadResult,
  type PermissionRespondParams,
  type RpcNotification,
  type RuntimeOptionsParams,
  type RuntimeOptionsResult,
  type SettingsReadParams,
  type SettingsReadResult,
  type SettingsUpdateParams,
  type ShellStartParams,
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
  type ThreadStartParams,
  type ThreadTraceParams,
  type ThreadTraceResult,
  type TurnControlResult,
  type TurnStartParams,
  type TurnStartResult,
  type TurnSteerParams,
  type TurnTakeoverResult,
  type UsageReadParams,
  type UsageReadResult,
  type WorkspaceChangeFileParams,
  type WorkspaceChangeMutationResult,
  type WorkspaceChangesParams,
  type WorkspaceChangesResult,
  type WorkspaceDiffParams,
  type WorkspaceDiffResult,
  type WorkspaceCreateParams,
  type WorkspaceCreateResult,
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

export type NotificationHandler = (notification: RpcNotification) => void;

export interface GatewayRequestParams {
  "agent/delete": AgentDeleteParams;
  "agent/list": AgentListParams;
  "agent/read": AgentReadParams;
  "agent/status": AgentStatusParams;
  "agent/write": AgentWriteParams;
  "automation/delete": AutomationIdParams;
  "automation/draft": AutomationDraftParams;
  "automation/list": AutomationListParams;
  "automation/pause": AutomationIdParams;
  "automation/resume": AutomationIdParams;
  "automation/run": AutomationRunParams;
  "automation/write": AutomationWriteParams;
  "backend/delete": BackendDeleteParams;
  "backend/doctor": BackendDoctorParams;
  "backend/list": BackendListParams;
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
  "clarify/respond": ClarifyRespondParams;
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
  "permission/respond": PermissionRespondParams;
  "runtime/options": RuntimeOptionsParams;
  "settings/read": SettingsReadParams;
  "settings/update": SettingsUpdateParams;
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
  "thread/delete": ThreadIdParams;
  "thread/list": ThreadListParams;
  "thread/read": ThreadReadParams;
  "thread/rename": ThreadRenameParams;
  "thread/restore": ThreadIdParams;
  "thread/resume": ThreadResumeParams;
  "thread/start": ThreadStartParams;
  "thread/trace": ThreadTraceParams;
  "turn/interrupt": { sourceKey?: string | null; threadId?: string | null };
  "turn/start": TurnStartParams;
  "turn/steer": TurnSteerParams;
  "turn/takeover": { sourceKey?: string | null; threadId?: string | null };
  "workspace/change/accept": WorkspaceChangeFileParams;
  "workspace/change/reject": WorkspaceChangeFileParams;
  "workspace/changes": WorkspaceChangesParams;
  "workspace/create": WorkspaceCreateParams;
  "workspace/diff": WorkspaceDiffParams;
  "workspace/file/read": WorkspaceFileReadParams;
  "workspace/file/write": WorkspaceFileWriteParams;
  "workspace/files": WorkspaceFilesParams;
}

export interface GatewayRequestResults {
  "agent/delete": AgentDeleteResult;
  "agent/list": AgentListResult;
  "agent/read": AgentReadResult;
  "agent/status": AgentStatusResult;
  "agent/write": AgentWriteResult;
  "automation/delete": AutomationDeleteResult;
  "automation/draft": AutomationDraftResult;
  "automation/list": AutomationListResult;
  "automation/pause": AutomationMutationResult;
  "automation/resume": AutomationMutationResult;
  "automation/run": AutomationRunResult;
  "automation/write": AutomationMutationResult;
  "backend/delete": BackendDeleteResult;
  "backend/doctor": BackendDoctorResult;
  "backend/list": BackendListResult;
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
  "clarify/respond": InteractionRespondResult;
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
  "permission/respond": InteractionRespondResult;
  "runtime/options": RuntimeOptionsResult;
  "settings/read": SettingsReadResult;
  "settings/update": SettingsReadResult;
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
  "thread/delete": ThreadDeleteResult;
  "thread/list": ThreadListResult;
  "thread/read": ThreadSnapshot;
  "thread/rename": ThreadMutationResult;
  "thread/restore": ThreadMutationResult;
  "thread/resume": ThreadSnapshot;
  "thread/start": ThreadSnapshot;
  "thread/trace": ThreadTraceResult;
  "turn/interrupt": TurnControlResult;
  "turn/start": TurnStartResult;
  "turn/steer": TurnControlResult;
  "turn/takeover": TurnTakeoverResult;
  "workspace/change/accept": WorkspaceChangeMutationResult;
  "workspace/change/reject": WorkspaceChangeMutationResult;
  "workspace/changes": WorkspaceChangesResult;
  "workspace/create": WorkspaceCreateResult;
  "workspace/diff": WorkspaceDiffResult;
  "workspace/file/read": WorkspaceFileReadResult;
  "workspace/file/write": WorkspaceFileWriteResult;
  "workspace/files": WorkspaceFilesResult;
}

export type GatewayMethod = keyof GatewayRequestParams;
export type GatewayRequestInit<M extends GatewayMethod> =
  | GatewayRequestParams[M]
  | Partial<GatewayRequestParams[M]>;

export function scopeForWorkdir(workdir: string): GatewayRequestScope {
  return {
    workdir,
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

export class GatewayClient {
  private nextId = 1;
  private socket: WebSocket | null = null;
  private readonly pending = new Map<
    string,
    { reject: (error: Error) => void; resolve: (value: unknown) => void }
  >();
  private readonly handlers = new Set<NotificationHandler>();

  constructor(readonly endpoint: GatewayEndpoint) {}

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
      socket.addEventListener("message", (event) => this.handleMessage(event.data));
      socket.addEventListener("close", () => this.rejectPending("Gateway WebSocket closed"));
    });
  }

  close(): void {
    this.socket?.close();
    this.socket = null;
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
    const socket = this.socket;
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      return Promise.reject(new Error("Gateway WebSocket is not connected"));
    }

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
    socket.send(JSON.stringify(payload));
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
    pendingPermissions: Array.isArray(record.pendingPermissions) ? record.pendingPermissions : [],
    pendingClarifies: Array.isArray(record.pendingClarifies) ? record.pendingClarifies : []
  };
}

function defaultScopeFromSource(value: unknown): GatewayRequestScope {
  const source = asRecord(value);
  return {
    workdir: "",
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
