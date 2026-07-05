import { afterEach, describe, expect, it, vi } from "vitest";
import { MemoryHostStorage, browserGatewayEndpoint, capabilityFailure, createBrowserHost, downloadUrl } from "./index";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("browserGatewayEndpoint", () => {
  it("derives cookie-authenticated websocket endpoints from a browser URL", () => {
    const endpoint = browserGatewayEndpoint({
      host: "127.0.0.1:3000",
      origin: "http://127.0.0.1:3000",
      protocol: "http:",
      search: ""
    });

    expect(endpoint.wsUrl).toBe("ws://127.0.0.1:3000/ws");
    expect(downloadUrl(endpoint, "s1", "export")).toBe(
      "http://127.0.0.1:3000/download/session/s1/export"
    );
    expect(downloadUrl(endpoint, "s1", "export", {
      filename: "review.json",
      format: "json",
      include: ["last-provider-request", "last-provider-response"]
    })).toBe(
      "http://127.0.0.1:3000/download/session/s1/export?format=json&include=last-provider-request%2Clast-provider-response&filename=review.json"
    );
  });
});

describe("MemoryHostStorage", () => {
  it("round-trips JSON without browser state", () => {
    const storage = new MemoryHostStorage();
    storage.setJson("prefs", { density: "compact" });
    expect(storage.getJson("prefs", { density: "default" })).toEqual({ density: "compact" });
    storage.remove("prefs");
    expect(storage.getJson("prefs", { density: "default" })).toEqual({ density: "default" });
  });
});

describe("browser floating host", () => {
  it("delegates semantic session downloads to the existing browser URL flow", async () => {
    const open = vi.fn();
    vi.stubGlobal("window", { open });
    const host = createBrowserHost({
      host: "127.0.0.1:3000",
      origin: "http://127.0.0.1:3000",
      protocol: "http:",
      search: ""
    }, new MemoryStorageShim());

    await expect(host.open.downloadSession(host.endpoint, "s1", "export", {
      filename: "review.json",
      format: "json",
      include: ["last-provider-request", "last-provider-response"]
    })).resolves.toEqual({ ok: true, value: undefined });

    expect(open).toHaveBeenCalledWith(
      "http://127.0.0.1:3000/download/session/s1/export?format=json&include=last-provider-request%2Clast-provider-response&filename=review.json",
      "_blank",
      "noopener"
    );
  });

  it("returns typed unsupported results for native-only capture capabilities", async () => {
    const host = createBrowserHost({
      host: "127.0.0.1:3000",
      origin: "http://127.0.0.1:3000",
      protocol: "http:",
      search: ""
    }, new MemoryStorageShim());

    await expect(host.floating.currentSelection()).resolves.toEqual({
      capability: "floating.currentSelection",
      ok: false,
      reason: "unsupported"
    });
    await expect(host.floating.captureRegion({ x: 0, y: 0, width: 20, height: 20 })).resolves.toEqual({
      capability: "floating.captureRegion",
      ok: false,
      reason: "unsupported"
    });
    await expect(host.floating.beginRegionPicker()).resolves.toEqual({
      capability: "floating.beginRegionPicker",
      ok: false,
      reason: "unsupported"
    });
  });

  it("serializes all shared capability failure reasons", () => {
    expect([
      capabilityFailure("demo.unsupported", "unsupported"),
      capabilityFailure("demo.unavailable", "unavailable"),
      capabilityFailure("demo.permissionDenied", "permissionDenied"),
      capabilityFailure("demo.canceled", "canceled"),
      capabilityFailure("demo.failed", "failed", "bounded message")
    ]).toEqual([
      { capability: "demo.unsupported", ok: false, reason: "unsupported" },
      { capability: "demo.unavailable", ok: false, reason: "unavailable" },
      { capability: "demo.permissionDenied", ok: false, reason: "permissionDenied" },
      { capability: "demo.canceled", ok: false, reason: "canceled" },
      { capability: "demo.failed", message: "bounded message", ok: false, reason: "failed" }
    ]);
  });
});

class MemoryStorageShim implements Storage {
  private readonly values = new Map<string, string>();
  length = 0;

  clear(): void {
    this.values.clear();
    this.length = 0;
  }

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  key(index: number): string | null {
    return Array.from(this.values.keys())[index] ?? null;
  }

  removeItem(key: string): void {
    this.values.delete(key);
    this.length = this.values.size;
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
    this.length = this.values.size;
  }
}
