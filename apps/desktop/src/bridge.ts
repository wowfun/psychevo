import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { GatewayClient, type GatewayRawMessageHandler, type GatewayTransport } from "@psychevo/client";
import type {
  DesktopPlatformCapabilities,
  GatewayEndpoint,
  HostCapabilityResult,
  HostRect,
  SessionDownloadKind,
  SessionDownloadOptions
} from "@psychevo/host";
import type { FloatingActivation } from "@psychevo/floating";

interface GatewayBridgePayload {
  connectionId: string;
  generation: number;
  message: string;
}

export interface DesktopDownloadSessionResult {
  content: number[];
  contentType: string;
  filename: string;
}

export interface DesktopDownloadSessionRequest extends SessionDownloadOptions {
  kind: SessionDownloadKind;
  threadId: string;
}

export class DesktopGatewayTransport implements GatewayTransport {
  private connected = false;
  private connecting: Promise<void> | null = null;
  private connectEpoch = 0;
  private disconnectUnlisten: UnlistenFn | null = null;
  private generation: number | null = null;
  private listenersPromise: Promise<void> | null = null;
  private messageUnlisten: UnlistenFn | null = null;
  private disposed = false;
  private disposePromise: Promise<void> | null = null;
  private readonly disconnectHandlers = new Set<(message: string) => void>();
  private readonly messageHandlers = new Set<GatewayRawMessageHandler>();
  readonly connectionId: string;

  constructor(surfaceLabel: string) {
    this.connectionId = desktopGatewayConnectionId(surfaceLabel);
  }

  async connect(): Promise<void> {
    if (this.disposed) {
      throw new Error("Desktop Gateway transport is disposed");
    }
    if (this.connected) {
      return;
    }
    if (this.connecting) {
      return this.connecting;
    }
    const epoch = ++this.connectEpoch;
    const connecting = this.ensureListeners()
      .then(() => invoke<number>("gateway_connect", {
        connectionId: this.connectionId
      }))
      .then(async (generation) => {
        if (this.disposed || epoch !== this.connectEpoch) {
          await invoke("gateway_disconnect", {
            connectionId: this.connectionId,
            generation
          }).catch(() => undefined);
          throw new Error("Gateway bridge connection was replaced");
        }
        this.generation = generation;
        this.connected = true;
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
    const generation = this.generation;
    this.connectEpoch += 1;
    this.connected = false;
    this.generation = null;
    if (generation !== null) {
      void invoke("gateway_disconnect", {
        connectionId: this.connectionId,
        generation
      }).catch(() => undefined);
    }
  }

  onDisconnect(handler: (message: string) => void): () => void {
    this.disconnectHandlers.add(handler);
    return () => this.disconnectHandlers.delete(handler);
  }

  onMessage(handler: GatewayRawMessageHandler): () => void {
    this.messageHandlers.add(handler);
    return () => this.messageHandlers.delete(handler);
  }

  send(data: string): void {
    if (this.disposed) {
      throw new Error("Desktop Gateway transport is disposed");
    }
    const generation = this.generation;
    if (!this.connected || generation === null) {
      throw new Error("Gateway bridge is not connected");
    }
    void invoke("gateway_send", {
      connectionId: this.connectionId,
      generation,
      message: data
    }).catch((error) => {
      if (this.generation !== generation) {
        return;
      }
      const message = error instanceof Error ? error.message : String(error);
      this.connected = false;
      this.generation = null;
      for (const handler of this.disconnectHandlers) {
        handler(message);
      }
    });
  }

  dispose(): Promise<void> {
    if (this.disposePromise) {
      return this.disposePromise;
    }
    this.disposed = true;
    this.close();
    this.disposePromise = this.releaseListeners();
    return this.disposePromise;
  }

  private async releaseListeners(): Promise<void> {
    try {
      await this.listenersPromise;
    } catch {
      // Listener setup owns its partial-failure cleanup.
    }
    const messageUnlisten = this.messageUnlisten;
    const disconnectUnlisten = this.disconnectUnlisten;
    this.messageUnlisten = null;
    this.disconnectUnlisten = null;
    this.listenersPromise = null;
    messageUnlisten?.();
    disconnectUnlisten?.();
    this.messageHandlers.clear();
    this.disconnectHandlers.clear();
  }

  private ensureListeners(): Promise<void> {
    if (this.listenersPromise) {
      return this.listenersPromise;
    }
    this.listenersPromise = Promise.all([
      listen<GatewayBridgePayload>("gateway-message", (event) => {
        if (
          event.payload.connectionId !== this.connectionId
          || event.payload.generation !== this.generation
        ) {
          return;
        }
        for (const handler of this.messageHandlers) {
          handler(event.payload.message);
        }
      }).then((unlisten) => {
        this.messageUnlisten = unlisten;
      }),
      listen<GatewayBridgePayload>("gateway-disconnect", (event) => {
        if (
          event.payload.connectionId !== this.connectionId
          || event.payload.generation !== this.generation
        ) {
          return;
        }
        this.connected = false;
        this.generation = null;
        for (const handler of this.disconnectHandlers) {
          handler(event.payload.message || "Gateway bridge disconnected");
        }
      }).then((unlisten) => {
        this.disconnectUnlisten = unlisten;
      })
    ]).then(() => undefined).catch((error) => {
      this.messageUnlisten?.();
      this.disconnectUnlisten?.();
      this.messageUnlisten = null;
      this.disconnectUnlisten = null;
      this.listenersPromise = null;
      throw error;
    });
    return this.listenersPromise;
  }
}

let nextDesktopGatewayConnectionSeq = 1;

export function desktopGatewayConnectionId(label: string): string {
  const safeLabel = label.trim().replace(/[^A-Za-z0-9_.-]+/g, "-") || "desktop";
  return `${safeLabel}:${desktopGatewayConnectionNonce()}`;
}

function desktopGatewayConnectionNonce(): string {
  const randomUUID = globalThis.crypto?.randomUUID;
  if (typeof randomUUID === "function") {
    return randomUUID.call(globalThis.crypto);
  }
  return `${Date.now().toString(36)}-${nextDesktopGatewayConnectionSeq++}`;
}

export interface DesktopGatewayConnection {
  client: GatewayClient;
  dispose(): Promise<void>;
}

export function desktopGatewayConnection(label: string): DesktopGatewayConnection {
  const transport = new DesktopGatewayTransport(label);
  const client = new GatewayClient(transport);
  return {
    client,
    dispose: async () => {
      client.close();
      await transport.dispose();
    }
  };
}

export function desktopGatewayEndpoint(): Promise<GatewayEndpoint> {
  return invoke<GatewayEndpoint>("gateway_endpoint");
}

export function downloadSessionArtifact(request: DesktopDownloadSessionRequest): Promise<DesktopDownloadSessionResult> {
  return invoke<DesktopDownloadSessionResult>("download_session_artifact", { request });
}

export function desktopFallbackCwd(): Promise<string> {
  return invoke<string>("desktop_fallback_cwd");
}

export function desktopPlatformCapabilities(): Promise<DesktopPlatformCapabilities> {
  return invoke<DesktopPlatformCapabilities>("desktop_platform_capabilities");
}

export function floatingInitialActivation(): Promise<FloatingActivation> {
  return invoke<FloatingActivation>("floating_initial_activation");
}

export function floatingCaptureSelection(): Promise<FloatingActivation> {
  return invoke<FloatingActivation>("floating_capture_selection");
}

export function floatingBeginRegionPicker(): Promise<HostCapabilityResult<HostRect | null>> {
  return invoke<HostCapabilityResult<HostRect | null>>("floating_begin_region_picker");
}

export function floatingCaptureRegion(bounds: HostRect): Promise<HostCapabilityResult<{ dataUrl: string; name: string }>> {
  return invoke<HostCapabilityResult<{ dataUrl: string; name: string }>>("floating_capture_region", { bounds });
}

export function openThreadInWorkbench(threadId: string): Promise<void> {
  return invoke<void>("open_thread_in_workbench", { threadId });
}

export async function listenOpenThreadInWorkbench(
  handler: (threadId: string) => void
): Promise<UnlistenFn> {
  return listen<{ threadId: string }>("desktop-open-thread", (event) => {
    const threadId = event.payload.threadId?.trim();
    if (threadId) {
      handler(threadId);
    }
  });
}
