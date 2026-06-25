import { describe, expect, it } from "vitest";
import { MemoryHostStorage, browserGatewayEndpoint, downloadUrl } from "./index";

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
