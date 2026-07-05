export interface GatewayEndpoint {
  httpBase: string;
  wsUrl: string;
}

export interface BrowserLocationLike {
  host: string;
  origin: string;
  protocol: string;
  search: string;
}

export function browserGatewayEndpoint(location: BrowserLocationLike): GatewayEndpoint {
  const params = new URLSearchParams(location.search);
  const explicitGateway = params.get("gateway");
  const httpBase = trimTrailingSlash(explicitGateway ?? location.origin);
  const wsUrl = new URL("/ws", httpBase);
  wsUrl.protocol = wsUrl.protocol === "https:" ? "wss:" : "ws:";
  return { httpBase, wsUrl: wsUrl.toString() };
}

export type SessionDownloadKind = "export" | "share";

export interface SessionDownloadOptions {
  filename?: string | null;
  format?: "markdown" | "json" | string | null;
  include?: string[] | null;
}

export function downloadUrl(
  endpoint: GatewayEndpoint,
  sessionId: string,
  kind: SessionDownloadKind,
  options: SessionDownloadOptions = {}
): string {
  const url = new URL(`/download/session/${encodeURIComponent(sessionId)}/${kind}`, endpoint.httpBase);
  if (options.format) {
    url.searchParams.set("format", options.format);
  }
  if (options.include && options.include.length > 0) {
    url.searchParams.set("include", options.include.join(","));
  }
  if (options.filename) {
    url.searchParams.set("filename", options.filename);
  }
  return url.toString();
}

export type HostCapabilityFailureReason =
  | "unsupported"
  | "unavailable"
  | "permissionDenied"
  | "canceled"
  | "failed";

export type HostCapabilityResult<T> =
  | { ok: true; value: T }
  | {
      capability: string;
      message?: string;
      ok: false;
      reason: HostCapabilityFailureReason;
    };

export interface HostCapabilitySnapshot {
  available: boolean;
  message?: string;
  reason?: HostCapabilityFailureReason;
}

export interface DesktopPlatformCapabilities {
  capture: {
    pointer: HostCapabilitySnapshot;
    portalScreenshot: HostCapabilitySnapshot;
    regionScreenshot: HostCapabilitySnapshot;
    selection: HostCapabilitySnapshot;
  };
  displayVariables: string[];
  os: "macos" | "windows" | "linux";
  session: "x11" | "wayland" | "unknown";
}

export interface HostRect {
  height: number;
  width: number;
  x: number;
  y: number;
}

export interface HostStorage {
  getJson<T>(key: string, fallback: T): T;
  remove(key: string): void;
  setJson<T>(key: string, value: T): void;
}

export class LocalHostStorage implements HostStorage {
  constructor(private readonly storage: Storage) {}

  getJson<T>(key: string, fallback: T): T {
    const raw = this.storage.getItem(key);
    if (!raw) {
      return fallback;
    }
    try {
      return JSON.parse(raw) as T;
    } catch {
      return fallback;
    }
  }

  setJson<T>(key: string, value: T): void {
    this.storage.setItem(key, JSON.stringify(value));
  }

  remove(key: string): void {
    this.storage.removeItem(key);
  }
}

export class MemoryHostStorage implements HostStorage {
  private readonly values = new Map<string, string>();

  getJson<T>(key: string, fallback: T): T {
    const raw = this.values.get(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  }

  setJson<T>(key: string, value: T): void {
    this.values.set(key, JSON.stringify(value));
  }

  remove(key: string): void {
    this.values.delete(key);
  }
}

export interface ClipboardHost {
  readText(): Promise<HostCapabilityResult<string>>;
  writeText(text: string): Promise<HostCapabilityResult<void>>;
}

export interface FilePickerHost {
  pickImage(): Promise<HostCapabilityResult<File>>;
  pickFile(): Promise<HostCapabilityResult<File>>;
}

export interface FloatingSelectionSnapshot {
  bounds?: HostRect | null;
  sourceApp?: string | null;
  text?: string | null;
}

export interface FloatingCaptureHost {
  beginRegionPicker(): Promise<HostCapabilityResult<HostRect | null>>;
  captureRegion(bounds: HostRect): Promise<HostCapabilityResult<{ dataUrl: string; name: string }>>;
  currentSelection(): Promise<HostCapabilityResult<FloatingSelectionSnapshot>>;
  showSelectionToolbar(anchor?: HostRect | null): Promise<HostCapabilityResult<void>>;
}

export interface OpenHost {
  downloadSession(
    endpoint: GatewayEndpoint,
    sessionId: string,
    kind: SessionDownloadKind,
    options?: SessionDownloadOptions
  ): Promise<HostCapabilityResult<void>>;
  openExternal(url: string): Promise<HostCapabilityResult<void>>;
  openDownload(url: string): Promise<HostCapabilityResult<void>>;
}

export interface NotificationHost {
  notify(title: string, body?: string): Promise<HostCapabilityResult<void>>;
}

export interface ThemeHost {
  readonly colorScheme: "light" | "dark" | "system";
}

export interface PlatformHost {
  readonly kind: "browser" | "managedWeb" | "desktop" | "mobile";
  readonly nativeFileContract: "unsupported" | "path" | "bookmark";
}

export interface WindowLifecycleHost {
  setTitle(title: string): void;
}

export interface PsychevoHost {
  clipboard: ClipboardHost;
  endpoint: GatewayEndpoint;
  files: FilePickerHost;
  floating: FloatingCaptureHost;
  lifecycle: WindowLifecycleHost;
  notifications: NotificationHost;
  open: OpenHost;
  platform: PlatformHost;
  storage: HostStorage;
  theme: ThemeHost;
}

export function createBrowserHost(location: BrowserLocationLike, storage: Storage): PsychevoHost {
  return {
    clipboard: browserClipboard(),
    endpoint: browserGatewayEndpoint(location),
    files: browserFiles(),
    floating: browserFloating(),
    lifecycle: browserLifecycle(),
    notifications: browserNotifications(),
    open: browserOpen(),
    platform: {
      kind: "browser",
      nativeFileContract: "unsupported"
    },
    storage: new LocalHostStorage(storage),
    theme: {
      colorScheme: "system"
    }
  };
}

function browserFloating(): FloatingCaptureHost {
  return {
    async beginRegionPicker() {
      return unsupported("floating.beginRegionPicker");
    },
    async captureRegion() {
      return unsupported("floating.captureRegion");
    },
    async currentSelection() {
      return unsupported("floating.currentSelection");
    },
    async showSelectionToolbar() {
      return unsupported("floating.showSelectionToolbar");
    }
  };
}

function browserClipboard(): ClipboardHost {
  return {
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
  };
}

function browserFiles(): FilePickerHost {
  return {
    async pickFile() {
      return pickBrowserFile();
    },
    async pickImage() {
      return pickBrowserFile("image/*");
    }
  };
}

function pickBrowserFile(accept?: string): Promise<HostCapabilityResult<File>> {
  if (typeof document === "undefined") {
    return Promise.resolve(unsupported("files.pickFile"));
  }
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
      if (!file) {
        resolve(capabilityFailure("files.pickFile", "canceled"));
        return;
      }
      resolve({ ok: true, value: file });
    }, { once: true });
    document.body.append(input);
    input.click();
  });
}

function browserOpen(): OpenHost {
  const openDownload = async (url: string): Promise<HostCapabilityResult<void>> => {
    window.open(url, "_blank", "noopener");
    return { ok: true, value: undefined };
  };
  return {
    async downloadSession(endpoint, sessionId, kind, options) {
      return openDownload(downloadUrl(endpoint, sessionId, kind, options));
    },
    openDownload,
    async openExternal(url: string) {
      window.open(url, "_blank", "noopener");
      return { ok: true, value: undefined };
    }
  };
}

function browserNotifications(): NotificationHost {
  return {
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
  };
}

function browserLifecycle(): WindowLifecycleHost {
  return {
    setTitle(title: string) {
      document.title = title;
    }
  };
}

export function capabilityFailure(
  capability: string,
  reason: HostCapabilityFailureReason,
  message?: string
): HostCapabilityResult<never> {
  return message
    ? { capability, message, ok: false, reason }
    : { capability, ok: false, reason };
}

export function unsupported(capability: string): HostCapabilityResult<never> {
  return capabilityFailure(capability, "unsupported");
}

function trimTrailingSlash(value: string): string {
  return value.replace(/\/+$/, "");
}
