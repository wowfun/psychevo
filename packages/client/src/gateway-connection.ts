import {
  GatewayClientError,
  errorMessage,
  type GatewayClientErrorCode
} from "./gateway-errors";
import type { GatewayTransport } from "./gateway-transport";

export type GatewayConnectionState =
  | "idle"
  | "connecting"
  | "connected"
  | "reconnecting"
  | "error"
  | "closed";

export interface GatewayConnectionSnapshot {
  state: GatewayConnectionState;
  generation: number;
  attempt: number;
  nextRetryAtMs: number | null;
  lastError: string | null;
}

export interface GatewayConnectionOwner {
  rejectPending(error: GatewayClientError): void;
  reportDiagnostic(kind: "connection_handler" | "transport", message: string): void;
}

const CONNECT_TIMEOUT_MS = 15_000;
const RECONNECT_DELAYS_MS = [250, 500, 1_000, 2_000, 4_000, 8_000, 15_000] as const;

export class GatewayConnectionController {
  private readonly handlers = new Set<(snapshot: GatewayConnectionSnapshot) => void>();
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

  constructor(
    private readonly transport: GatewayTransport,
    private readonly owner: GatewayConnectionOwner
  ) {
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
    this.connectPromise = null;
    this.transport.close();
    this.owner.rejectPending(
      new GatewayClientError("disconnected", "unknown", "Gateway connection closed")
    );
    this.update({
      state: "closed",
      attempt: 0,
      nextRetryAtMs: null,
      lastError: null
    });
  }

  subscribe(handler: (snapshot: GatewayConnectionSnapshot) => void): () => void {
    this.handlers.add(handler);
    this.notify(handler, this.snapshot());
    return () => this.handlers.delete(handler);
  }

  snapshot(): GatewayConnectionSnapshot {
    return { ...this.connection };
  }

  isConnected(): boolean {
    return this.connection.state === "connected";
  }

  generation(): number {
    return this.connection.generation;
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

  protocolFault(message: string): void {
    this.owner.rejectPending(
      new GatewayClientError("protocol_fault", "unknown", message)
    );
    this.transport.close();
    this.handleDisconnect(message, "protocol_fault");
  }

  private startConnectAttempt(reconnecting: boolean): Promise<void> {
    const epoch = ++this.connectEpoch;
    const attempt = reconnecting ? Math.max(1, this.reconnectAttempt) : 1;
    this.update({
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
        this.update({
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
            this.update({
              state: "reconnecting",
              nextRetryAtMs: null,
              lastError: failure.message
            });
            this.scheduleReconnect();
          } else {
            this.update({
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
    this.owner.rejectPending(error);
    this.rejectConnectAttempt?.(error);
    this.rejectConnectAttempt = null;
    this.connectEpoch += 1;
    this.connectPromise = null;
    this.owner.reportDiagnostic("transport", message);
    if (!this.hasConnected) {
      this.update({
        state: "error",
        attempt: Math.max(1, this.connection.attempt),
        nextRetryAtMs: null,
        lastError: message
      });
      return;
    }
    this.update({
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
    this.update({
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

  private update(patch: Partial<GatewayConnectionSnapshot>): void {
    this.connection = { ...this.connection, ...patch };
    const snapshot = this.snapshot();
    for (const handler of this.handlers) {
      this.notify(handler, snapshot);
    }
  }

  private notify(
    handler: (snapshot: GatewayConnectionSnapshot) => void,
    snapshot: GatewayConnectionSnapshot
  ): void {
    try {
      handler(snapshot);
    } catch (error) {
      this.owner.reportDiagnostic("connection_handler", errorMessage(error));
    }
  }
}
