import {
  RpcNotificationSchema,
  RpcResponseSchema,
  ThreadSnapshotSchema,
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
  type PermissionRespondParams,
  type RpcNotification,
  type SettingsReadParams,
  type SettingsReadResult,
  type ShellStartParams,
  type ShellStartResult,
  type SourceResetParams,
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
  type TurnControlResult,
  type TurnStartParams,
  type TurnStartResult,
  type TurnSteerParams,
  type WorkspaceDiffParams,
  type WorkspaceDiffResult,
  type WorkspaceFileReadParams,
  type WorkspaceFileReadResult,
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
  "agent/list": { scope?: GatewayRequestScope | null };
  "backend/list": { scope?: GatewayRequestScope | null };
  "clarify/respond": ClarifyRespondParams;
  "command/execute": CommandExecuteParams;
  "command/list": CommandListParams;
  "completion/list": CompletionListParams;
  "context/read": ContextReadParams;
  "initialize": InitializeParams;
  "permission/respond": PermissionRespondParams;
  "settings/read": SettingsReadParams;
  "shell/start": ShellStartParams;
  "source/reset": SourceResetParams;
  "thread/archive": ThreadIdParams;
  "thread/delete": ThreadIdParams;
  "thread/list": ThreadListParams;
  "thread/read": ThreadReadParams;
  "thread/rename": ThreadRenameParams;
  "thread/restore": ThreadIdParams;
  "thread/resume": ThreadResumeParams;
  "thread/start": ThreadStartParams;
  "turn/interrupt": { sourceKey?: string | null; threadId?: string | null };
  "turn/start": TurnStartParams;
  "turn/steer": TurnSteerParams;
  "workspace/diff": WorkspaceDiffParams;
  "workspace/file/read": WorkspaceFileReadParams;
  "workspace/files": WorkspaceFilesParams;
}

export interface GatewayRequestResults {
  "agent/list": unknown;
  "backend/list": unknown;
  "clarify/respond": InteractionRespondResult;
  "command/execute": CommandExecuteResult;
  "command/list": CommandListResult;
  "completion/list": CompletionListResult;
  "context/read": ContextReadResult;
  "initialize": InitializeResult;
  "permission/respond": InteractionRespondResult;
  "settings/read": SettingsReadResult;
  "shell/start": ShellStartResult;
  "source/reset": ThreadSnapshot;
  "thread/archive": ThreadMutationResult;
  "thread/delete": ThreadDeleteResult;
  "thread/list": ThreadListResult;
  "thread/read": ThreadSnapshot;
  "thread/rename": ThreadMutationResult;
  "thread/restore": ThreadMutationResult;
  "thread/resume": ThreadSnapshot;
  "thread/start": ThreadSnapshot;
  "turn/interrupt": TurnControlResult;
  "turn/start": TurnStartResult;
  "turn/steer": TurnControlResult;
  "workspace/diff": WorkspaceDiffResult;
  "workspace/file/read": WorkspaceFileReadResult;
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
