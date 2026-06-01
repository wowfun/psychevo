import {
  RpcNotificationSchema,
  RpcResponseSchema,
  type ClarifyRespondParams,
  type DebugEventsParams,
  type DebugEventsResult,
  type GatewayRequestScope,
  type InitializeParams,
  type InitializeResult,
  type InteractionRespondResult,
  type PermissionRespondParams,
  type RpcNotification,
  type SettingsReadParams,
  type SettingsReadResult,
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
  type TurnSteerParams
} from "@psychevo/protocol";
import type { GatewayEndpoint } from "@psychevo/host";

export type { GatewayEndpoint } from "@psychevo/host";

export type NotificationHandler = (notification: RpcNotification) => void;

export interface GatewayRequestParams {
  "clarify/respond": ClarifyRespondParams;
  "debug/events": DebugEventsParams;
  "initialize": InitializeParams;
  "permission/respond": PermissionRespondParams;
  "settings/read": SettingsReadParams;
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
}

export interface GatewayRequestResults {
  "clarify/respond": InteractionRespondResult;
  "debug/events": DebugEventsResult;
  "initialize": InitializeResult;
  "permission/respond": InteractionRespondResult;
  "settings/read": SettingsReadResult;
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
