import {
  type GatewayMethod,
  type GatewayRequestParams,
  type GatewayRequestResults,
  type GatewayRequestScope,
  type RpcNotification
} from "@psychevo/protocol";
import type { GatewayEndpoint } from "@psychevo/host";
import {
  BrowserWebSocketTransport,
  isGatewayTransport,
  type GatewayTransport
} from "./gateway-transport";
import {
  GatewayClientError,
  errorMessage
} from "./gateway-errors";
import {
  RpcPendingRequests,
  type GatewayRequestOptions
} from "./rpc-pending-requests";
import { decodeRpcEnvelope, decodeRpcResult } from "./rpc-decoder";
import { parseThreadSnapshot } from "./thread-snapshot";
import {
  GatewayConnectionController,
  type GatewayConnectionSnapshot
} from "./gateway-connection";

export type { GatewayEndpoint } from "@psychevo/host";
export type {
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
  threadTurnStartParams
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
export { ThreadSession } from "./thread-session";
export {
  CapabilitiesApplication,
  type CapabilitiesClient,
  type CapabilitiesSnapshot,
  type CapabilityDomain,
  type CapabilityOperationReceipt,
  type CapabilityPollState
} from "./capabilities-application";
export { gatewayScopeKey } from "./scope";
export type {
  ObserverDiagnostic,
  ObserverDiagnosticHandler,
  ObserverDiagnosticSource
} from "./observer";
export type {
  ThreadSessionClient,
  ThreadSessionControlInput,
  ThreadSessionLoadInput,
  ThreadSessionOptions,
  ThreadSessionSendInput,
  ThreadSessionSendOutcome,
  ThreadSessionView
} from "./thread-session";

export type { GatewayRawMessageHandler, GatewayTransport } from "./gateway-transport";
export {
  GatewayClientError,
  type GatewayClientErrorCode,
  type GatewayClientErrorDetails,
  type GatewayClientErrorKind,
  type GatewayDelivery
} from "./gateway-errors";
export type { GatewayRequestOptions } from "./rpc-pending-requests";
export { parseThreadSnapshot } from "./thread-snapshot";
export type {
  GatewayConnectionSnapshot,
  GatewayConnectionState
} from "./gateway-connection";

export type NotificationHandler = (notification: RpcNotification) => void;

export type GatewayRequestInit<M extends GatewayMethod> = GatewayRequestParams[M];

export type GatewayRequestArguments<M extends GatewayMethod> =
  Record<string, never> extends GatewayRequestParams[M]
    ? [params?: GatewayRequestParams[M], options?: GatewayRequestOptions]
    : [params: GatewayRequestParams[M], options?: GatewayRequestOptions];

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

export interface GatewayClientDiagnostic {
  kind: "connection_handler" | "notification_handler" | "protocol" | "transport";
  message: string;
  generation: number;
}

export class GatewayClient {
  private readonly transport: GatewayTransport;
  private readonly pending = new RpcPendingRequests();
  private readonly handlers = new Set<NotificationHandler>();
  private readonly diagnosticHandlers = new Set<(diagnostic: GatewayClientDiagnostic) => void>();
  private readonly connection: GatewayConnectionController;

  readonly endpoint: GatewayEndpoint | null;

  constructor(endpointOrTransport: GatewayEndpoint | GatewayTransport) {
    if (isGatewayTransport(endpointOrTransport)) {
      this.endpoint = null;
      this.transport = endpointOrTransport;
    } else {
      this.endpoint = endpointOrTransport;
      this.transport = new BrowserWebSocketTransport(endpointOrTransport);
    }
    this.connection = new GatewayConnectionController(this.transport, {
      rejectPending: (error) => this.pending.rejectAll(error),
      reportDiagnostic: (kind, message) => this.emitDiagnostic(kind, message)
    });
    this.transport.onMessage((data) => this.handleMessage(data));
  }

  connect(): Promise<void> {
    return this.connection.connect();
  }

  close(): void {
    this.connection.close();
  }

  subscribe(handler: NotificationHandler): () => void {
    this.handlers.add(handler);
    return () => this.handlers.delete(handler);
  }

  subscribeConnectionState(
    handler: (snapshot: GatewayConnectionSnapshot) => void
  ): () => void {
    return this.connection.subscribe(handler);
  }

  subscribeDiagnostics(
    handler: (diagnostic: GatewayClientDiagnostic) => void
  ): () => void {
    this.diagnosticHandlers.add(handler);
    return () => this.diagnosticHandlers.delete(handler);
  }

  connectionSnapshot(): GatewayConnectionSnapshot {
    return this.connection.snapshot();
  }

  reconnectNow(): Promise<void> {
    return this.connection.reconnectNow();
  }

  request<M extends GatewayMethod>(
    method: M,
    ...arguments_: GatewayRequestArguments<M>
  ): Promise<GatewayRequestResults[M]> {
    const [params, options = {}] = arguments_;
    if (!this.connection.isConnected()) {
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
    const generation = this.connection.generation();
    const reservation = this.pending.reserve(generation, method, options);
    const payload =
      params === undefined
        ? { jsonrpc: "2.0", id: reservation.id, method }
        : { jsonrpc: "2.0", id: reservation.id, method, params };
    try {
      this.transport.send(JSON.stringify(payload));
    } catch (error) {
      this.pending.reject(
        reservation.id,
        new GatewayClientError(
          "not_connected",
          "not_sent",
          error instanceof Error ? error.message : "Gateway request could not be sent"
        )
      );
    }
    return reservation.promise;
  }

  private handleMessage(data: unknown): void {
    if (!this.connection.isConnected()) {
      return;
    }
    try {
      const envelope = decodeRpcEnvelope(data);
      if (envelope.kind === "notification") {
        for (const handler of this.handlers) {
          try {
            handler(envelope.notification);
          } catch (error) {
            this.emitDiagnostic("notification_handler", errorMessage(error));
          }
        }
        return;
      }

      const response = envelope.response;
      const key = String(response.id);
      const pending = this.pending.get(key);
      if (!pending || pending.generation !== this.connection.generation()) {
        return;
      }
      if ("error" in response) {
        this.pending.take(key);
        pending.reject(new GatewayClientError(
          "server_error",
          "acknowledged",
          response.error.message,
          {
            data: response.error.data,
            kind: "server",
            rpcCode: response.error.code
          }
        ));
      } else {
        const result = decodeRpcResult(pending.method, response.result);
        this.pending.take(key);
        pending.resolve(result);
      }
    } catch (error) {
      const message = `Gateway protocol fault: ${errorMessage(error)}`;
      this.emitDiagnostic("protocol", message);
      this.connection.protocolFault(message);
    }
  }

  private emitDiagnostic(kind: GatewayClientDiagnostic["kind"], message: string): void {
    const diagnostic: GatewayClientDiagnostic = {
      kind,
      message: message.slice(0, 1_000),
      generation: this.connection.generation()
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
