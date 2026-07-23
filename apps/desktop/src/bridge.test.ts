import { beforeEach, describe, expect, it, vi } from "vitest";
import { DesktopGatewayTransport, desktopGatewayConnectionId } from "./bridge";

const tauriCore = vi.hoisted(() => ({
  invoke: vi.fn()
}));

const tauriEvent = vi.hoisted(() => ({
  listen: vi.fn()
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriCore.invoke
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: tauriEvent.listen
}));

beforeEach(() => {
  vi.clearAllMocks();
  let generation = 0;
  tauriCore.invoke.mockImplementation(async (command: string) => (
    command === "gateway_connect" ? ++generation : undefined
  ));
  tauriEvent.listen.mockResolvedValue(vi.fn());
});

describe("DesktopGatewayTransport", () => {
  it("creates instance-unique bridge ids for a shared surface label", () => {
    const first = desktopGatewayConnectionId("floating");
    const second = desktopGatewayConnectionId("floating");

    expect(first).toMatch(/^floating:/);
    expect(second).toMatch(/^floating:/);
    expect(first).not.toBe(second);
  });

  it("keeps stale disconnects scoped to their own transport id", async () => {
    const first = new DesktopGatewayTransport("floating");
    const second = new DesktopGatewayTransport("floating");

    await first.connect();
    await second.connect();
    first.close();
    second.send('{"jsonrpc":"2.0","id":"1","method":"initialize"}');

    expect(first.connectionId).not.toBe(second.connectionId);
    expect(tauriCore.invoke).toHaveBeenCalledWith("gateway_disconnect", {
      connectionId: first.connectionId,
      generation: 1
    });
    expect(tauriCore.invoke).toHaveBeenCalledWith("gateway_send", {
      connectionId: second.connectionId,
      generation: 2,
      message: '{"jsonrpc":"2.0","id":"1","method":"initialize"}'
    });
  });

  it("marks the transport disconnected when native send fails", async () => {
    const transport = new DesktopGatewayTransport("floating");
    await transport.connect();
    const disconnected = vi.fn();
    transport.onDisconnect(disconnected);
    tauriCore.invoke.mockImplementation((command: string) => (
      command === "gateway_send"
        ? Promise.reject(new Error("Gateway bridge is not connected"))
        : Promise.resolve(undefined)
    ));

    transport.send("{}");
    await Promise.resolve();
    await Promise.resolve();

    expect(disconnected).toHaveBeenCalledWith("Gateway bridge is not connected");
    expect(() => transport.send("{}")).toThrow("Gateway bridge is not connected");
  });

  it("delivers only messages from the active native generation", async () => {
    const listeners = installTauriListeners();
    const transport = new DesktopGatewayTransport("workbench");
    const received: string[] = [];
    await transport.connect();
    transport.onMessage((message) => received.push(String(message)));
    listeners.get("gateway-message")?.({
      payload: {
        connectionId: transport.connectionId,
        generation: 0,
        message: "stale"
      }
    });
    listeners.get("gateway-message")?.({
      payload: {
        connectionId: transport.connectionId,
        generation: 1,
        message: "current"
      }
    });

    expect(received).toEqual(["current"]);
  });

  it("registers native listeners once while reconnecting by generation", async () => {
    installTauriListeners();
    const transport = new DesktopGatewayTransport("workbench");
    await transport.connect();
    transport.close();
    await transport.connect();
    transport.send("second generation");

    expect(tauriEvent.listen).toHaveBeenCalledTimes(2);
    expect(tauriCore.invoke).toHaveBeenCalledWith("gateway_send", {
      connectionId: transport.connectionId,
      generation: 2,
      message: "second generation"
    });
  });
});

function installTauriListeners(): Map<string, (event: { payload: unknown }) => void> {
  const listeners = new Map<string, (event: { payload: unknown }) => void>();
  tauriEvent.listen.mockImplementation(async (eventName: string, handler: (event: { payload: unknown }) => void) => {
    listeners.set(eventName, handler);
    return vi.fn();
  });
  return listeners;
}
