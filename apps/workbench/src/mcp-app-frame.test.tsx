// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  MCP_APP_MAX_DOCUMENT_BYTES,
  McpAppFrame,
  injectMcpAppPolicy,
  loadMcpAppDocument,
  parseMcpAppBridgeMessage,
  type McpAppFrameDescriptor
} from "./mcp-app-frame";

const descriptor: McpAppFrameDescriptor = {
  id: "example.dashboard",
  resourceUri: "ui://example/dashboard.html",
  resourceUrl: "https://apps.example.test/dashboard.html",
  resourceDomains: ["https://apps.example.test", "https://cdn.example.test"],
  connectDomains: ["https://api.example.test"],
  allowedTools: ["example.lookup"],
  fallback: "Use the text dashboard."
};

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("McpAppFrame", () => {
  it("injects a host CSP and never grants same-origin or navigation sandbox authority", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(
      "<!doctype html><html><head><title>Dashboard</title></head><body>Ready</body></html>",
      { headers: { "content-type": "text/html; charset=utf-8" }, status: 200 }
    )));
    render(<McpAppFrame activeLease descriptor={descriptor} />);

    const frame = await screen.findByTitle("example.dashboard") as HTMLIFrameElement;
    expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
    expect(frame.getAttribute("referrerpolicy")).toBe("no-referrer");
    expect(frame.srcdoc).toContain("Content-Security-Policy");
    expect(frame.srcdoc).toContain("connect-src https://api.example.test");
    expect(frame.srcdoc).toContain("frame-src 'none'");
  });

  it("requires source, opaque origin, token, tool allowlist, and an active lease", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(
      "<!doctype html><html><head></head><body>Ready</body></html>",
      { headers: { "content-type": "text/html" }, status: 200 }
    )));
    const onToolCall = vi.fn(async () => ({ ok: true }));
    const view = render(<McpAppFrame activeLease descriptor={descriptor} onToolCall={onToolCall} />);
    const frame = await screen.findByTitle("example.dashboard") as HTMLIFrameElement;
    const postMessage = vi.spyOn(frame.contentWindow!, "postMessage");
    fireEvent.load(frame);
    const initialize = postMessage.mock.calls[0]?.[0] as Record<string, unknown>;
    const token = initialize._psychevoToken as string;
    const request = {
      jsonrpc: "2.0",
      id: 1,
      method: "tools/call",
      params: { name: "example.lookup", arguments: { query: "rust" } },
      _psychevoToken: token
    };

    dispatchFrameMessage(frame, request, "https://apps.example.test");
    dispatchFrameMessage(frame, { ...request, _psychevoToken: "wrong" }, "null");
    expect(onToolCall).not.toHaveBeenCalled();
    dispatchFrameMessage(frame, request, "null");
    await waitFor(() => expect(onToolCall).toHaveBeenCalledWith("example.lookup", { query: "rust" }));

    view.rerender(<McpAppFrame activeLease={false} descriptor={descriptor} onToolCall={onToolCall} />);
    dispatchFrameMessage(frame, { ...request, id: 2 }, "null");
    expect(onToolCall).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Use the text dashboard.")).toBeTruthy();
  });

  it("rejects undeclared redirects, non-HTML content, and over-size documents", async () => {
    const redirect = vi.fn(async () => responseWithUrl("<html></html>", "text/html", "https://evil.example/app"));
    await expect(loadMcpAppDocument(descriptor, undefined, redirect as typeof fetch))
      .rejects.toThrow("redirect left declared resource domains");

    const json = vi.fn(async () => responseWithUrl("{}", "application/json", descriptor.resourceUrl));
    await expect(loadMcpAppDocument(descriptor, undefined, json as typeof fetch))
      .rejects.toThrow("content type must be HTML");

    const huge = vi.fn(async () => responseWithUrl(
      "x".repeat(MCP_APP_MAX_DOCUMENT_BYTES + 1),
      "text/html",
      descriptor.resourceUrl
    ));
    await expect(loadMcpAppDocument(descriptor, undefined, huge as typeof fetch))
      .rejects.toThrow("exceeds the 1 MiB limit");
  });

  it("bounds and strictly parses AppBridge message families", () => {
    expect(parseMcpAppBridgeMessage({
      jsonrpc: "2.0",
      method: "ui/notifications/size-changed",
      params: { height: Number.POSITIVE_INFINITY },
      _psychevoToken: "token"
    }, "token")).toBeNull();
    expect(parseMcpAppBridgeMessage({
      jsonrpc: "2.0",
      method: "unknown",
      params: {},
      _psychevoToken: "token"
    }, "token")).toBeNull();
    expect(parseMcpAppBridgeMessage({
      jsonrpc: "2.0",
      id: "mode-1",
      method: "ui/request-display-mode",
      params: { mode: "fullscreen" },
      _psychevoToken: "token"
    }, "token")).toEqual({
      id: "mode-1",
      method: "ui/request-display-mode",
      params: { mode: "fullscreen" }
    });
  });

  it("adds policy before untrusted document head content", () => {
    const document = injectMcpAppPolicy("<html><head><script>run()</script></head></html>", descriptor);
    expect(document.indexOf("Content-Security-Policy")).toBeLessThan(document.indexOf("<script>"));
  });

  it("moves policy ahead of executable content placed before an explicit head", () => {
    const document = injectMcpAppPolicy(
      "<script>runBeforeHead()</script><html><head><title>unsafe order</title></head><body></body></html>",
      descriptor
    );
    expect(document.indexOf("Content-Security-Policy")).toBeLessThan(document.indexOf("<script>"));
  });
});

function responseWithUrl(body: string, contentType: string, url: string): Response {
  const response = new Response(body, { headers: { "content-type": contentType }, status: 200 });
  Object.defineProperty(response, "url", { configurable: true, value: url });
  return response;
}

function dispatchFrameMessage(frame: HTMLIFrameElement, data: unknown, origin: string): void {
  const event = new MessageEvent("message", { data });
  Object.defineProperty(event, "origin", { configurable: true, value: origin });
  Object.defineProperty(event, "source", { configurable: true, value: frame.contentWindow });
  window.dispatchEvent(event);
}
