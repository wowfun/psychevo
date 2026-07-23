import {
  RpcNotificationSchema,
  RpcResponseSchema,
  ThreadSnapshotSchema,
  type GatewayMethod,
  type GatewayRequestParams,
  type GatewayRequestResults,
  type GatewayRequestScope,
  type RpcNotification,
  type ThreadSnapshot
} from "@psychevo/protocol";
import type { GatewayEndpoint } from "@psychevo/host";

export type { GatewayEndpoint } from "@psychevo/host";
export type {
  GatewayJsonResult,
  GatewayMethod,
  GatewayRequestParams,
  GatewayRequestResults
} from "@psychevo/protocol";
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

export interface GatewayTransport {
  close(): void;
  connect(): Promise<void>;
  onDisconnect(handler: (message: string) => void): () => void;
  onMessage(handler: GatewayRawMessageHandler): () => void;
  send(data: string): void;
}

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

export type GatewayConnectionState =
  | "idle"
  | "connecting"
  | "connected"
  | "reconnecting"
  | "error"
  | "closed";

export type GatewayDelivery = "not_sent" | "unknown";

export interface GatewayConnectionSnapshot {
  state: GatewayConnectionState;
  generation: number;
  attempt: number;
  nextRetryAtMs: number | null;
  lastError: string | null;
}

export interface GatewayRequestOptions {
  timeoutMs?: number;
  signal?: AbortSignal;
}

export type GatewayClientErrorCode =
  | "not_connected"
  | "connect_failed"
  | "connect_timeout"
  | "disconnected"
  | "request_timeout"
  | "request_aborted"
  | "protocol_fault";

export class GatewayClientError extends Error {
  readonly code: GatewayClientErrorCode;
  readonly delivery: GatewayDelivery;

  constructor(code: GatewayClientErrorCode, delivery: GatewayDelivery, message: string) {
    super(message);
    this.name = "GatewayClientError";
    this.code = code;
    this.delivery = delivery;
  }
}

export interface GatewayClientDiagnostic {
  kind: "protocol" | "notification_handler" | "transport";
  message: string;
  generation: number;
}

type PendingRequest = {
  generation: number;
  reject: (error: Error) => void;
  resolve: (value: unknown) => void;
  timeout: ReturnType<typeof setTimeout> | null;
  removeAbort: (() => void) | null;
};

const CONNECT_TIMEOUT_MS = 15_000;
const REQUEST_TIMEOUT_MS = 120_000;
const RECONNECT_DELAYS_MS = [250, 500, 1_000, 2_000, 4_000, 8_000, 15_000] as const;

export class GatewayClient {
  private nextId = 1;
  private readonly transport: GatewayTransport;
  private readonly pending = new Map<string, PendingRequest>();
  private readonly handlers = new Set<NotificationHandler>();
  private readonly connectionHandlers = new Set<(snapshot: GatewayConnectionSnapshot) => void>();
  private readonly diagnosticHandlers = new Set<(diagnostic: GatewayClientDiagnostic) => void>();
  private connection: GatewayConnectionSnapshot = {
    state: "idle",
    generation: 0,
    attempt: 0,
    nextRetryAtMs: null,
    lastError: null
  };
  private connectPromise: Promise<void> | null = null;
  private rejectConnectAttempt: ((error: Error) => void) | null = null;
  private connectEpoch = 0;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectAttempt = 0;
  private hasConnected = false;
  private closedByUser = false;

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
    this.transport.onDisconnect((message) => this.handleDisconnect(message));
  }

  connect(): Promise<void> {
    if (this.connection.state === "connected") {
      return Promise.resolve();
    }
    if (this.connectPromise) {
      return this.connectPromise;
    }
    this.closedByUser = false;
    this.clearReconnectTimer();
    return this.startConnectAttempt(this.hasConnected);
  }

  close(): void {
    if (this.connection.state === "closed") {
      return;
    }
    this.closedByUser = true;
    this.connectEpoch += 1;
    this.clearReconnectTimer();
    this.rejectConnectAttempt?.(
      new GatewayClientError("connect_failed", "not_sent", "Gateway connection closed")
    );
    this.rejectConnectAttempt = null;
    this.transport.close();
    this.rejectPending(
      new GatewayClientError("disconnected", "unknown", "Gateway connection closed")
    );
    this.updateConnection({
      state: "closed",
      attempt: 0,
      nextRetryAtMs: null,
      lastError: null
    });
  }

  subscribe(handler: NotificationHandler): () => void {
    this.handlers.add(handler);
    return () => this.handlers.delete(handler);
  }

  subscribeConnectionState(
    handler: (snapshot: GatewayConnectionSnapshot) => void
  ): () => void {
    this.connectionHandlers.add(handler);
    handler(this.connectionSnapshot());
    return () => this.connectionHandlers.delete(handler);
  }

  subscribeDiagnostics(
    handler: (diagnostic: GatewayClientDiagnostic) => void
  ): () => void {
    this.diagnosticHandlers.add(handler);
    return () => this.diagnosticHandlers.delete(handler);
  }

  connectionSnapshot(): GatewayConnectionSnapshot {
    return { ...this.connection };
  }

  reconnectNow(): Promise<void> {
    if (this.connection.state === "connected") {
      return Promise.resolve();
    }
    this.closedByUser = false;
    this.clearReconnectTimer();
    if (this.connectPromise) {
      return this.connectPromise;
    }
    return this.startConnectAttempt(this.hasConnected);
  }

  request<M extends GatewayMethod>(
    method: M,
    params?: GatewayRequestInit<M>,
    options: GatewayRequestOptions = {}
  ): Promise<GatewayRequestResults[M]> {
    if (this.connection.state !== "connected") {
      return Promise.reject(
        new GatewayClientError(
          "not_connected",
          "not_sent",
          "Gateway is not connected"
        )
      );
    }
    if (options.signal?.aborted) {
      return Promise.reject(
        new GatewayClientError(
          "request_aborted",
          "not_sent",
          "Gateway request was aborted before send"
        )
      );
    }
    const id = String(this.nextId++);
    const generation = this.connection.generation;
    const payload =
      params === undefined
        ? { jsonrpc: "2.0", id, method }
        : { jsonrpc: "2.0", id, method, params };
    const promise = new Promise<GatewayRequestResults[M]>((resolve, reject) => {
      const pending: PendingRequest = {
        generation,
        resolve: (value) => resolve(value as GatewayRequestResults[M]),
        reject,
        timeout: null,
        removeAbort: null
      };
      const timeoutMs = options.timeoutMs ?? REQUEST_TIMEOUT_MS;
      if (timeoutMs > 0) {
        pending.timeout = setTimeout(() => {
          if (!this.takePending(id)) {
            return;
          }
          reject(
            new GatewayClientError(
              "request_timeout",
              "unknown",
              `Gateway request timed out after ${timeoutMs} ms`
            )
          );
        }, timeoutMs);
      }
      if (options.signal) {
        const onAbort = () => {
          if (!this.takePending(id)) {
            return;
          }
          reject(
            new GatewayClientError(
              "request_aborted",
              "unknown",
              "Gateway request was aborted after send"
            )
          );
        };
        options.signal.addEventListener("abort", onAbort, { once: true });
        pending.removeAbort = () => options.signal?.removeEventListener("abort", onAbort);
      }
      this.pending.set(id, pending);
    });
    try {
      this.transport.send(JSON.stringify(payload));
    } catch (error) {
      this.takePending(id);
      return Promise.reject(
        new GatewayClientError(
          "not_connected",
          "not_sent",
          error instanceof Error ? error.message : "Gateway request could not be sent"
        )
      );
    }
    return promise;
  }

  private handleMessage(data: unknown): void {
    if (this.connection.state !== "connected") {
      return;
    }
    try {
      const raw = typeof data === "string" ? data : String(data);
      const value = JSON.parse(raw) as unknown;
      const notification = RpcNotificationSchema.safeParse(value);
      if (notification.success && !("id" in (value as Record<string, unknown>))) {
        for (const handler of this.handlers) {
          try {
            handler(notification.data);
          } catch (error) {
            this.emitDiagnostic("notification_handler", errorMessage(error));
          }
        }
        return;
      }

      const response = RpcResponseSchema.parse(value);
      const key = String(response.id);
      const pending = this.pending.get(key);
      if (!pending || pending.generation !== this.connection.generation) {
        return;
      }
      this.takePending(key);
      if ("error" in response) {
        pending.reject(new Error(response.error.message));
      } else {
        pending.resolve(response.result);
      }
    } catch (error) {
      const message = `Gateway protocol fault: ${errorMessage(error)}`;
      this.emitDiagnostic("protocol", message);
      this.transport.close();
      this.handleDisconnect(message, "protocol_fault");
    }
  }

  private startConnectAttempt(reconnecting: boolean): Promise<void> {
    const epoch = ++this.connectEpoch;
    const attempt = reconnecting ? Math.max(1, this.reconnectAttempt) : 1;
    this.updateConnection({
      state: reconnecting ? "reconnecting" : "connecting",
      attempt,
      nextRetryAtMs: null,
      lastError: null
    });

    let timeout: ReturnType<typeof setTimeout> | null = null;
    const interrupted = new Promise<never>((_resolve, reject) => {
      this.rejectConnectAttempt = reject;
    });
    const deadline = new Promise<never>((_resolve, reject) => {
      timeout = setTimeout(() => {
        reject(
          new GatewayClientError(
            "connect_timeout",
            "not_sent",
            `Gateway connection timed out after ${CONNECT_TIMEOUT_MS} ms`
          )
        );
        this.transport.close();
      }, CONNECT_TIMEOUT_MS);
    });

    const promise = Promise.race([this.transport.connect(), deadline, interrupted])
      .then(() => {
        if (epoch !== this.connectEpoch || this.closedByUser) {
          throw new GatewayClientError(
            "connect_failed",
            "not_sent",
            "Stale Gateway connection attempt"
          );
        }
        this.hasConnected = true;
        this.reconnectAttempt = 0;
        this.updateConnection({
          state: "connected",
          generation: this.connection.generation + 1,
          attempt,
          nextRetryAtMs: null,
          lastError: null
        });
      })
      .catch((error: unknown) => {
        const failure = error instanceof GatewayClientError
          ? error
          : new GatewayClientError("connect_failed", "not_sent", errorMessage(error));
        if (epoch === this.connectEpoch && !this.closedByUser) {
          if (this.hasConnected) {
            this.updateConnection({
              state: "reconnecting",
              nextRetryAtMs: null,
              lastError: failure.message
            });
            this.scheduleReconnect();
          } else {
            this.updateConnection({
              state: "error",
              nextRetryAtMs: null,
              lastError: failure.message
            });
          }
        }
        throw failure;
      })
      .finally(() => {
        if (timeout) {
          clearTimeout(timeout);
        }
        if (epoch === this.connectEpoch) {
          this.connectPromise = null;
          this.rejectConnectAttempt = null;
        }
      });
    this.connectPromise = promise;
    return promise;
  }

  private handleDisconnect(
    message: string,
    code: GatewayClientErrorCode = "disconnected"
  ): void {
    if (this.closedByUser || this.connection.state === "closed") {
      return;
    }
    const error = new GatewayClientError(code, "unknown", message);
    this.rejectPending(error);
    this.rejectConnectAttempt?.(error);
    this.rejectConnectAttempt = null;
    this.connectEpoch += 1;
    this.connectPromise = null;
    this.emitDiagnostic("transport", message);
    if (!this.hasConnected) {
      this.updateConnection({
        state: "error",
        attempt: Math.max(1, this.connection.attempt),
        nextRetryAtMs: null,
        lastError: message
      });
      return;
    }
    this.updateConnection({
      state: "reconnecting",
      nextRetryAtMs: null,
      lastError: message
    });
    this.scheduleReconnect();
  }

  private scheduleReconnect(): void {
    if (this.closedByUser || this.reconnectTimer) {
      return;
    }
    this.reconnectAttempt += 1;
    const delay = RECONNECT_DELAYS_MS[
      Math.min(this.reconnectAttempt - 1, RECONNECT_DELAYS_MS.length - 1)
    ]!;
    this.updateConnection({
      state: "reconnecting",
      attempt: this.reconnectAttempt,
      nextRetryAtMs: Date.now() + delay
    });
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      void this.startConnectAttempt(true).catch(() => undefined);
    }, delay);
  }

  private clearReconnectTimer(): void {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }

  private takePending(id: string): PendingRequest | null {
    const pending = this.pending.get(id) ?? null;
    if (!pending) {
      return null;
    }
    this.pending.delete(id);
    if (pending.timeout) {
      clearTimeout(pending.timeout);
    }
    pending.removeAbort?.();
    return pending;
  }

  private rejectPending(error: Error): void {
    for (const pending of this.pending.values()) {
      if (pending.timeout) {
        clearTimeout(pending.timeout);
      }
      pending.removeAbort?.();
      pending.reject(error);
    }
    this.pending.clear();
  }

  private updateConnection(patch: Partial<GatewayConnectionSnapshot>): void {
    this.connection = { ...this.connection, ...patch };
    const snapshot = this.connectionSnapshot();
    for (const handler of this.connectionHandlers) {
      handler(snapshot);
    }
  }

  private emitDiagnostic(kind: GatewayClientDiagnostic["kind"], message: string): void {
    const diagnostic: GatewayClientDiagnostic = {
      kind,
      message: message.slice(0, 1_000),
      generation: this.connection.generation
    };
    for (const handler of this.diagnosticHandlers) {
      try {
        handler(diagnostic);
      } catch {
        // Diagnostics must never become a second client failure path.
      }
    }
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function isGatewayTransport(value: GatewayEndpoint | GatewayTransport): value is GatewayTransport {
  return typeof (value as GatewayTransport).send === "function"
    && typeof (value as GatewayTransport).onDisconnect === "function"
    && typeof (value as GatewayTransport).onMessage === "function";
}

class BrowserWebSocketTransport implements GatewayTransport {
  private socket: WebSocket | null = null;
  private connecting: Promise<void> | null = null;
  private readonly disconnectHandlers = new Set<(message: string) => void>();
  private readonly handlers = new Set<GatewayRawMessageHandler>();

  constructor(private readonly endpoint: GatewayEndpoint) {}

  connect(): Promise<void> {
    if (this.socket?.readyState === WebSocket.OPEN) {
      return Promise.resolve();
    }
    if (this.connecting) {
      return this.connecting;
    }

    const connecting = new Promise<void>((resolve, reject) => {
      const socket = new WebSocket(this.endpoint.wsUrl);
      this.socket = socket;
      let settled = false;
      socket.addEventListener("open", () => {
        if (this.socket !== socket) {
          reject(new Error("Gateway WebSocket connection was replaced"));
          return;
        }
        settled = true;
        resolve();
      }, { once: true });
      socket.addEventListener(
        "error",
        () => {
          if (!settled) {
            reject(new Error("Gateway WebSocket connection failed"));
          }
        },
        { once: true }
      );
      socket.addEventListener("message", (event) => {
        if (this.socket !== socket) {
          return;
        }
        for (const handler of this.handlers) {
          handler(event.data);
        }
      });
      socket.addEventListener("close", () => {
        if (this.socket !== socket) {
          if (!settled) {
            reject(new Error("Gateway WebSocket connection was replaced"));
          }
          return;
        }
        this.socket = null;
        if (!settled) {
          reject(new Error("Gateway WebSocket closed before connecting"));
        }
        for (const handler of this.disconnectHandlers) {
          handler("Gateway WebSocket closed");
        }
      });
    });
    const wrapped = connecting.finally(() => {
      if (this.connecting === wrapped) {
        this.connecting = null;
      }
    });
    this.connecting = wrapped;
    return wrapped;
  }

  close(): void {
    const socket = this.socket;
    this.socket = null;
    socket?.close();
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
    turnStartReceipts: Array.isArray(record.turnStartReceipts) ? record.turnStartReceipts : [],
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
