import { useEffect, useRef, useState } from "react";

export const MCP_APP_MAX_DOCUMENT_BYTES = 1_048_576;
export const MCP_APP_MAX_MESSAGE_BYTES = 65_536;
const MIN_FRAME_HEIGHT = 120;
const MAX_FRAME_HEIGHT = 1_200;
const DEFAULT_FRAME_HEIGHT = 420;

export type McpAppDisplayMode = "inline" | "fullscreen" | "picture_in_picture";

export interface McpAppFrameDescriptor {
  id: string;
  resourceUri: string;
  resourceUrl: string;
  resourceDomains: string[];
  connectDomains: string[];
  allowedTools: string[];
  fallback: string;
}

export interface McpAppFrameProps {
  activeLease: boolean;
  descriptor: McpAppFrameDescriptor;
  displayMode?: McpAppDisplayMode;
  onDisplayMode?(mode: McpAppDisplayMode): Promise<McpAppDisplayMode> | McpAppDisplayMode;
  onElicit?(request: unknown): Promise<unknown> | unknown;
  onToolCall?(name: string, argumentsValue: Record<string, unknown>): Promise<unknown> | unknown;
}

type BridgeRequest =
  | { id: string | number | null; method: "tools/call"; params: { name: string; arguments: Record<string, unknown> } }
  | { id: string | number | null; method: "ui/elicitation/create"; params: { request: unknown } }
  | { id: string | number | null; method: "ui/request-display-mode"; params: { mode: McpAppDisplayMode } }
  | { id: string | number | null; method: "ui/notifications/size-changed"; params: { height: number } };

export function McpAppFrame({
  activeLease,
  descriptor,
  displayMode = "inline",
  onDisplayMode,
  onElicit,
  onToolCall
}: McpAppFrameProps) {
  const frameRef = useRef<HTMLIFrameElement>(null);
  const leaseRef = useRef(activeLease);
  const [token] = useState(secureBridgeToken);
  const [document, setDocument] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [height, setHeight] = useState(DEFAULT_FRAME_HEIGHT);

  useEffect(() => {
    leaseRef.current = activeLease;
  }, [activeLease]);

  useEffect(() => {
    if (!activeLease) {
      setDocument(null);
      return;
    }
    const controller = new AbortController();
    setError(null);
    void loadMcpAppDocument(descriptor, controller.signal)
      .then((value) => setDocument(value))
      .catch((reason) => {
        if (!controller.signal.aborted) setError(errorMessage(reason));
      });
    return () => controller.abort();
  }, [activeLease, descriptor]);

  useEffect(() => {
    const handleMessage = (event: MessageEvent) => {
      if (!leaseRef.current || event.source !== frameRef.current?.contentWindow || event.origin !== "null") return;
      const request = parseMcpAppBridgeMessage(event.data, token);
      if (!request) return;
      if (request.method === "ui/notifications/size-changed") {
        setHeight(Math.min(MAX_FRAME_HEIGHT, Math.max(MIN_FRAME_HEIGHT, Math.ceil(request.params.height))));
        return;
      }
      const target = event.source as WindowProxy;
      const respond = (result?: unknown, errorValue?: string) => {
        if (!leaseRef.current || target !== frameRef.current?.contentWindow || request.id === null) return;
        target.postMessage({
          jsonrpc: "2.0",
          id: request.id,
          _psychevoToken: token,
          ...(errorValue
            ? { error: { code: -32_000, message: boundedError(errorValue) } }
            : { result: result ?? null })
        }, "*");
      };
      if (request.method === "tools/call") {
        if (!descriptor.allowedTools.includes(request.params.name) || !onToolCall) {
          respond(undefined, `Tool ${request.params.name} is not available to this MCP App`);
          return;
        }
        void Promise.resolve(onToolCall(request.params.name, request.params.arguments))
          .then((result) => respond(result), (reason) => respond(undefined, errorMessage(reason)));
        return;
      }
      if (request.method === "ui/elicitation/create") {
        if (!onElicit) {
          respond(undefined, "Elicitation is not available on this surface");
          return;
        }
        void Promise.resolve(onElicit(request.params.request))
          .then((result) => respond(result), (reason) => respond(undefined, errorMessage(reason)));
        return;
      }
      if (!onDisplayMode) {
        respond(displayMode);
        return;
      }
      void Promise.resolve(onDisplayMode(request.params.mode))
        .then((result) => respond(result), (reason) => respond(undefined, errorMessage(reason)));
    };
    window.addEventListener("message", handleMessage);
    return () => window.removeEventListener("message", handleMessage);
  }, [descriptor.allowedTools, displayMode, onDisplayMode, onElicit, onToolCall, token]);

  if (!activeLease) {
    return <div className="mcpAppFallback">{descriptor.fallback}</div>;
  }
  if (error) {
    return <div className="capabilityBanner is-error">MCP App unavailable · {error}. {descriptor.fallback}</div>;
  }
  if (!document) return <div className="capabilityEmpty">Loading MCP App</div>;

  return (
    <div className="mcpAppFrame" data-display-mode={displayMode}>
      <iframe
        ref={frameRef}
        onLoad={() => frameRef.current?.contentWindow?.postMessage({
          jsonrpc: "2.0",
          id: "host-initialize",
          method: "ui/initialize",
          params: {
            appId: descriptor.id,
            resourceUri: descriptor.resourceUri,
            displayMode,
            allowedTools: descriptor.allowedTools
          },
          _psychevoToken: token
        }, "*")}
        referrerPolicy="no-referrer"
        sandbox="allow-scripts"
        srcDoc={document}
        style={{ height }}
        title={descriptor.id}
      />
    </div>
  );
}

export async function loadMcpAppDocument(
  descriptor: McpAppFrameDescriptor,
  signal?: AbortSignal,
  fetcher: typeof fetch = fetch
): Promise<string> {
  const allowedOrigins = validateDescriptorOrigins(descriptor);
  const response = await fetcher(descriptor.resourceUrl, {
    credentials: "omit",
    headers: { Accept: "text/html, application/xhtml+xml;q=0.9" },
    redirect: "follow",
    referrerPolicy: "no-referrer",
    ...(signal ? { signal } : {})
  });
  if (!response.ok) throw new Error(`resource request failed with HTTP ${response.status}`);
  const finalUrl = new URL(response.url || descriptor.resourceUrl);
  if (!allowedOrigins.has(finalUrl.origin)) throw new Error("resource redirect left declared resource domains");
  const contentType = response.headers.get("content-type")?.split(";", 1)[0]?.trim().toLowerCase();
  if (contentType !== "text/html" && contentType !== "application/xhtml+xml") {
    throw new Error("resource content type must be HTML");
  }
  const declaredSize = Number(response.headers.get("content-length"));
  if (Number.isFinite(declaredSize) && declaredSize > MCP_APP_MAX_DOCUMENT_BYTES) {
    throw new Error("resource document exceeds the 1 MiB limit");
  }
  const bytes = await readBoundedBody(response, MCP_APP_MAX_DOCUMENT_BYTES);
  const html = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  return injectMcpAppPolicy(html, descriptor);
}

export function parseMcpAppBridgeMessage(value: unknown, token: string): BridgeRequest | null {
  let serialized: string;
  try {
    serialized = JSON.stringify(value);
  } catch {
    return null;
  }
  if (!serialized || new TextEncoder().encode(serialized).byteLength > MCP_APP_MAX_MESSAGE_BYTES) return null;
  const message = objectValue(value);
  if (message.jsonrpc !== "2.0" || message._psychevoToken !== token) return null;
  const id = typeof message.id === "string" || typeof message.id === "number" ? message.id : null;
  const params = objectValue(message.params);
  if (message.method === "tools/call") {
    const name = typeof params.name === "string" ? params.name : "";
    if (!name) return null;
    return { id, method: "tools/call", params: { name, arguments: objectValue(params.arguments) } };
  }
  if (message.method === "ui/elicitation/create") {
    return { id, method: "ui/elicitation/create", params: { request: params.request } };
  }
  if (message.method === "ui/request-display-mode") {
    const mode = params.mode;
    if (mode !== "inline" && mode !== "fullscreen" && mode !== "picture_in_picture") return null;
    return { id, method: "ui/request-display-mode", params: { mode } };
  }
  if (message.method === "ui/notifications/size-changed") {
    const height = params.height;
    if (typeof height !== "number" || !Number.isFinite(height) || height < 1 || height > 100_000) return null;
    return { id, method: "ui/notifications/size-changed", params: { height } };
  }
  return null;
}

export function injectMcpAppPolicy(html: string, descriptor: McpAppFrameDescriptor): string {
  const resources = descriptor.resourceDomains.map(cspSource).join(" ");
  const connections = descriptor.connectDomains.map(cspSource).join(" ");
  const policy = [
    "default-src 'none'",
    `script-src 'unsafe-inline' 'unsafe-eval' blob: ${resources}`.trim(),
    `style-src 'unsafe-inline' ${resources}`.trim(),
    `img-src data: blob: ${resources}`.trim(),
    `font-src data: ${resources}`.trim(),
    `media-src data: blob: ${resources}`.trim(),
    `connect-src ${connections || "'none'"}`,
    "worker-src blob:",
    "frame-src 'none'",
    "object-src 'none'",
    "base-uri 'none'",
    "form-action 'none'"
  ].join("; ");
  const parsed = new DOMParser().parseFromString(html, "text/html");
  parsed.head.querySelectorAll('meta[http-equiv="Content-Security-Policy"], meta[name="referrer"]')
    .forEach((element) => element.remove());
  const referrer = parsed.createElement("meta");
  referrer.name = "referrer";
  referrer.content = "no-referrer";
  const csp = parsed.createElement("meta");
  csp.httpEquiv = "Content-Security-Policy";
  csp.content = policy;
  parsed.head.prepend(referrer);
  parsed.head.prepend(csp);
  return `<!doctype html>${parsed.documentElement.outerHTML}`;
}

function validateDescriptorOrigins(descriptor: McpAppFrameDescriptor): Set<string> {
  const resource = new URL(descriptor.resourceUrl);
  if (resource.protocol !== "https:" || resource.username || resource.password) {
    throw new Error("resource URL must use HTTPS without credentials");
  }
  const resourceOrigins = new Set(descriptor.resourceDomains.map(exactHttpsOrigin));
  descriptor.connectDomains.forEach(exactHttpsOrigin);
  if (!resourceOrigins.has(resource.origin)) throw new Error("resource URL origin is not declared");
  return resourceOrigins;
}

function exactHttpsOrigin(value: string): string {
  const url = new URL(value);
  if (url.protocol !== "https:" || url.username || url.password || url.pathname !== "/" || url.search || url.hash) {
    throw new Error(`invalid HTTPS origin: ${value}`);
  }
  return url.origin;
}

async function readBoundedBody(response: Response, limit: number): Promise<Uint8Array> {
  if (!response.body) {
    const bytes = new Uint8Array(await response.arrayBuffer());
    if (bytes.byteLength > limit) throw new Error("resource document exceeds the 1 MiB limit");
    return bytes;
  }
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let length = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    length += value.byteLength;
    if (length > limit) {
      await reader.cancel();
      throw new Error("resource document exceeds the 1 MiB limit");
    }
    chunks.push(value);
  }
  const output = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    output.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return output;
}

function secureBridgeToken(): string {
  if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function objectValue(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function cspSource(value: string): string {
  return exactHttpsOrigin(value).replace(/'/g, "%27");
}

function boundedError(value: string): string {
  return value.length > 1_000 ? `${value.slice(0, 1_000)}…` : value;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
