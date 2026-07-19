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
  message: string;
}

interface GatewayBridgeBroadcastPayload {
  message: string;
  originConnectionId: string;
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
  private broadcastUnlisten: UnlistenFn | null = null;
  private connected = false;
  private disconnectUnlisten: UnlistenFn | null = null;
  private messageUnlisten: UnlistenFn | null = null;
  private readonly disconnectHandlers = new Set<(message: string) => void>();
  private readonly messageHandlers = new Set<GatewayRawMessageHandler>();
  readonly connectionId: string;

  constructor(label: string) {
    this.connectionId = desktopGatewayConnectionId(label);
  }

  async connect(): Promise<void> {
    if (this.connected) {
      return;
    }
    this.messageUnlisten = await listen<GatewayBridgePayload>("gateway-message", (event) => {
      if (event.payload.connectionId !== this.connectionId) {
        return;
      }
      for (const handler of this.messageHandlers) {
        handler(event.payload.message);
      }
    });
    this.broadcastUnlisten = await listen<GatewayBridgeBroadcastPayload>("gateway-broadcast", (event) => {
      if (event.payload.originConnectionId === this.connectionId) {
        return;
      }
      if (!isBroadcastGatewayNotification(event.payload.message)) {
        return;
      }
      for (const handler of this.messageHandlers) {
        handler(event.payload.message);
      }
    });
    this.disconnectUnlisten = await listen<GatewayBridgePayload>("gateway-disconnect", (event) => {
      if (event.payload.connectionId !== this.connectionId) {
        return;
      }
      this.connected = false;
      for (const handler of this.disconnectHandlers) {
        handler(event.payload.message || "Gateway bridge disconnected");
      }
    });
    try {
      await invoke("gateway_connect", { connectionId: this.connectionId });
      this.connected = true;
    } catch (error) {
      this.messageUnlisten?.();
      this.broadcastUnlisten?.();
      this.disconnectUnlisten?.();
      this.messageUnlisten = null;
      this.broadcastUnlisten = null;
      this.disconnectUnlisten = null;
      throw error;
    }
  }

  close(): void {
    this.connected = false;
    void invoke("gateway_disconnect", { connectionId: this.connectionId }).catch(() => undefined);
    this.messageUnlisten?.();
    this.broadcastUnlisten?.();
    this.disconnectUnlisten?.();
    this.messageUnlisten = null;
    this.broadcastUnlisten = null;
    this.disconnectUnlisten = null;
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
    if (!this.connected) {
      throw new Error("Gateway bridge is not connected");
    }
    void invoke("gateway_send", { connectionId: this.connectionId, message: data }).catch((error) => {
      const message = error instanceof Error ? error.message : String(error);
      this.connected = false;
      for (const handler of this.disconnectHandlers) {
        handler(message);
      }
    });
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

export function desktopGatewayClient(label: string): GatewayClient {
  return new GatewayClient(new DesktopGatewayTransport(label));
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

const BROADCAST_GATEWAY_NOTIFICATIONS = new Set([
  "gateway/event"
]);

function isBroadcastGatewayNotification(message: string): boolean {
  try {
    const parsed = JSON.parse(message) as Record<string, unknown>;
    return parsed?.jsonrpc === "2.0" &&
      !Object.prototype.hasOwnProperty.call(parsed, "id") &&
      typeof parsed.method === "string" &&
      BROADCAST_GATEWAY_NOTIFICATIONS.has(parsed.method);
  } catch {
    return false;
  }
}
