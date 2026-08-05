import type { GatewayEndpoint } from "@psychevo/host";

export type GatewayRawMessageHandler = (data: unknown) => void;

export interface GatewayTransport {
  close(): void;
  connect(): Promise<void>;
  onDisconnect(handler: (message: string) => void): () => void;
  onMessage(handler: GatewayRawMessageHandler): () => void;
  send(data: string): void;
}

export function isGatewayTransport(
  value: GatewayEndpoint | GatewayTransport
): value is GatewayTransport {
  return typeof (value as GatewayTransport).send === "function"
    && typeof (value as GatewayTransport).onDisconnect === "function"
    && typeof (value as GatewayTransport).onMessage === "function";
}

export class BrowserWebSocketTransport implements GatewayTransport {
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
