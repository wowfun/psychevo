import type {
  GatewayMethod,
  GatewayRequestResults
} from "@psychevo/protocol";

import { GatewayClientError } from "./gateway-errors";

export interface GatewayRequestOptions {
  timeoutMs?: number;
  signal?: AbortSignal;
}

export interface PendingRequest {
  generation: number;
  method: GatewayMethod;
  reject(error: Error): void;
  resolve(value: unknown): void;
  timeout: ReturnType<typeof setTimeout> | null;
  removeAbort: (() => void) | null;
}

export interface PendingRequestReservation<M extends GatewayMethod> {
  id: string;
  promise: Promise<GatewayRequestResults[M]>;
}

const REQUEST_TIMEOUT_MS = 120_000;

export class RpcPendingRequests {
  private nextId = 1;
  private readonly requests = new Map<string, PendingRequest>();

  reserve<M extends GatewayMethod>(
    generation: number,
    method: M,
    options: GatewayRequestOptions
  ): PendingRequestReservation<M> {
    const id = String(this.nextId++);
    const promise = new Promise<GatewayRequestResults[M]>((resolve, reject) => {
      const pending: PendingRequest = {
        generation,
        method,
        resolve: (value) => resolve(value as GatewayRequestResults[M]),
        reject,
        timeout: null,
        removeAbort: null
      };
      const timeoutMs = options.timeoutMs ?? REQUEST_TIMEOUT_MS;
      if (timeoutMs > 0) {
        pending.timeout = setTimeout(() => {
          if (!this.take(id)) {
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
          if (!this.take(id)) {
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
      this.requests.set(id, pending);
    });
    return { id, promise };
  }

  get(id: string): PendingRequest | null {
    return this.requests.get(id) ?? null;
  }

  take(id: string): PendingRequest | null {
    const pending = this.requests.get(id) ?? null;
    if (!pending) {
      return null;
    }
    this.requests.delete(id);
    this.clean(pending);
    return pending;
  }

  reject(id: string, error: Error): boolean {
    const pending = this.take(id);
    if (!pending) {
      return false;
    }
    pending.reject(error);
    return true;
  }

  rejectAll(error: Error): void {
    const pending = [...this.requests.values()];
    this.requests.clear();
    for (const request of pending) {
      this.clean(request);
      request.reject(error);
    }
  }

  private clean(pending: PendingRequest): void {
    if (pending.timeout) {
      clearTimeout(pending.timeout);
    }
    pending.removeAbort?.();
  }
}
