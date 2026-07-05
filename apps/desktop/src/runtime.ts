import {
  threadTurnControlsFromWorkbenchControls
} from "@psychevo/client";
import type { FloatingRuntime } from "@psychevo/floating";
import { LocalHostStorage, capabilityFailure, unsupported, type GatewayEndpoint, type HostCapabilityResult, type HostRect, type PsychevoHost } from "@psychevo/host";
import { SettingsReadResultSchema } from "@psychevo/protocol";
import type { WorkbenchRuntime } from "@psychevo/workbench/runtime";
import {
  desktopFallbackCwd,
  desktopGatewayClient,
  desktopGatewayEndpoint,
  downloadSessionArtifact,
  type DesktopDownloadSessionResult,
  floatingBeginRegionPicker,
  floatingCaptureRegion,
  floatingCaptureSelection,
  floatingInitialActivation,
  listenOpenThreadInWorkbench,
  openThreadInWorkbench
} from "./bridge";
import { createDesktopFloatingWindowControls } from "./windowControls";

interface DesktopDownloadAnchor {
  download: string;
  href: string;
  style: { display: string };
  click(): void;
  remove(): void;
}

interface DesktopDownloadDocument {
  body: {
    append(element: DesktopDownloadAnchor): void;
  };
  createElement(tagName: "a"): DesktopDownloadAnchor;
}

interface DesktopDownloadUrlApi {
  createObjectURL(blob: Blob): string;
  revokeObjectURL(url: string): void;
}

interface DesktopDownloadDependencies {
  BlobCtor: typeof Blob;
  document: DesktopDownloadDocument;
  url: DesktopDownloadUrlApi;
}

export async function createDesktopWorkbenchRuntime(connectionId: string): Promise<WorkbenchRuntime> {
  const [endpoint, fallbackCwd] = await Promise.all([
    desktopGatewayEndpoint(),
    desktopFallbackCwd()
  ]);
  return {
    client: desktopGatewayClient(connectionId),
    endpoint,
    fallbackCwd,
    host: createDesktopHost(endpoint),
    onOpenThreadRequest: listenOpenThreadInWorkbench
  };
}

export function createDesktopFloatingRuntime(connectionId: string): FloatingRuntime {
  return {
    ...createDesktopFloatingWindowControls(),
    async beginRegionPicker() {
      return floatingBeginRegionPicker();
    },
    async captureRegion(bounds) {
      return floatingCaptureRegion(bounds);
    },
    async captureSelection() {
      return floatingCaptureSelection();
    },
    async connectGateway() {
      const client = desktopGatewayClient(connectionId);
      await client.connect();
      return client;
    },
    async initialActivation() {
      return floatingInitialActivation();
    },
    async openThreadInWorkbench(threadId) {
      return openThreadInWorkbench(threadId);
    },
    async turnControls({ client, scope, threadId }) {
      const settings = SettingsReadResultSchema.parse(await client.request("settings/read", {
        cwd: scope.cwd,
        threadId
      }));
      return threadTurnControlsFromWorkbenchControls(settings.controls);
    }
  };
}

function createDesktopHost(endpoint: GatewayEndpoint): PsychevoHost {
  return {
    clipboard: {
      async readText() {
        if (!navigator.clipboard?.readText) {
          return unsupported("clipboard.readText");
        }
        return { ok: true, value: await navigator.clipboard.readText() };
      },
      async writeText(text: string) {
        if (!navigator.clipboard?.writeText) {
          return unsupported("clipboard.writeText");
        }
        await navigator.clipboard.writeText(text);
        return { ok: true, value: undefined };
      }
    },
    endpoint,
    files: {
      async pickFile() {
        return pickFile();
      },
      async pickImage() {
        return pickFile("image/*");
      }
    },
    floating: {
      async beginRegionPicker() {
        return floatingBeginRegionPicker();
      },
      async captureRegion(_bounds: HostRect) {
        return floatingCaptureRegion(_bounds);
      },
      async currentSelection() {
        try {
          const activation = await floatingCaptureSelection();
          const text = activation.attachments.find((attachment) => attachment.kind === "textSelection");
          return {
            ok: true,
            value: {
              bounds: activation.anchor,
              sourceApp: text?.sourceApp ?? null,
              text: text?.kind === "textSelection" ? text.text : null
            }
          };
        } catch (error) {
          return {
            capability: "floating.currentSelection",
            message: error instanceof Error ? error.message : String(error),
            ok: false,
            reason: "failed"
          };
        }
      },
      async showSelectionToolbar() {
        return { ok: true, value: undefined };
      }
    },
    lifecycle: {
      setTitle(title: string) {
        document.title = title;
      }
    },
    notifications: {
      async notify(title: string, body?: string) {
        if (!("Notification" in window)) {
          return unsupported("notifications.notify");
        }
        if (Notification.permission === "default") {
          await Notification.requestPermission();
        }
        if (Notification.permission !== "granted") {
          return capabilityFailure("notifications.notify", "permissionDenied");
        }
        new Notification(title, body ? { body } : undefined);
        return { ok: true, value: undefined };
      }
    },
    open: {
      async downloadSession(_endpoint, threadId, kind, options) {
        try {
          const result = await downloadSessionArtifact({
            ...(options ?? {}),
            kind,
            threadId
          });
          saveDesktopDownload(result);
          return { ok: true, value: undefined };
        } catch (error) {
          return capabilityFailure(
            "open.downloadSession",
            "failed",
            error instanceof Error ? error.message : String(error)
          );
        }
      },
      async openDownload(url: string) {
        window.open(url, "_blank", "noopener");
        return { ok: true, value: undefined };
      },
      async openExternal(url: string) {
        window.open(url, "_blank", "noopener");
        return { ok: true, value: undefined };
      }
    },
    platform: {
      kind: "desktop",
      nativeFileContract: "unsupported"
    },
    storage: new LocalHostStorage(window.localStorage),
    theme: {
      colorScheme: "system"
    }
  };
}

export function saveDesktopDownload(
  result: DesktopDownloadSessionResult,
  deps?: DesktopDownloadDependencies
): void {
  const runtimeDeps = deps ?? {
    BlobCtor: Blob,
    document: document as unknown as DesktopDownloadDocument,
    url: URL
  };
  const blob = new runtimeDeps.BlobCtor([new Uint8Array(result.content)], {
    type: result.contentType || "application/octet-stream"
  });
  const objectUrl = runtimeDeps.url.createObjectURL(blob);
  const link = runtimeDeps.document.createElement("a");
  link.href = objectUrl;
  link.download = result.filename;
  link.style.display = "none";
  runtimeDeps.document.body.append(link);
  try {
    link.click();
  } finally {
    link.remove();
    runtimeDeps.url.revokeObjectURL(objectUrl);
  }
}

function pickFile(accept?: string): Promise<HostCapabilityResult<File>> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    if (accept) {
      input.accept = accept;
    }
    input.style.position = "fixed";
    input.style.left = "-9999px";
    input.style.top = "-9999px";
    input.addEventListener("change", () => {
      const file = input.files?.[0] ?? null;
      input.remove();
      resolve(file ? { ok: true, value: file } : capabilityFailure("files.pickFile", "canceled"));
    }, { once: true });
    document.body.append(input);
    input.click();
  });
}
