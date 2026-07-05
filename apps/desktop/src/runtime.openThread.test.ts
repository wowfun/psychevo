// @vitest-environment jsdom

import { describe, expect, it, vi } from "vitest";

const bridge = vi.hoisted(() => ({
  desktopFallbackCwd: vi.fn().mockResolvedValue("/repo"),
  desktopGatewayClient: vi.fn(() => ({ client: true })),
  desktopGatewayEndpoint: vi.fn().mockResolvedValue({
    httpBase: "http://127.0.0.1:58080",
    wsUrl: "ws://127.0.0.1:58080/ws"
  }),
  downloadSessionArtifact: vi.fn(),
  floatingBeginRegionPicker: vi.fn(),
  floatingCaptureRegion: vi.fn(),
  floatingCaptureSelection: vi.fn(),
  floatingInitialActivation: vi.fn(),
  listenOpenThreadInWorkbench: vi.fn(),
  openThreadInWorkbench: vi.fn().mockResolvedValue(undefined)
}));

vi.mock("./bridge", () => bridge);

vi.mock("./windowControls", () => ({
  createDesktopFloatingWindowControls: () => ({})
}));

describe("Desktop open-thread runtime wiring", () => {
  it("routes Floating openThreadInWorkbench through the native bridge", async () => {
    const { createDesktopFloatingRuntime } = await import("./runtime");
    const runtime = createDesktopFloatingRuntime("floating");

    await runtime.openThreadInWorkbench?.("thread-floating");

    expect(bridge.openThreadInWorkbench).toHaveBeenCalledWith("thread-floating");
  });

  it("reads shared Workbench controls for Floating turns", async () => {
    const { createDesktopFloatingRuntime } = await import("./runtime");
    const runtime = createDesktopFloatingRuntime("floating");
    const client = {
      request: vi.fn().mockResolvedValue(settingsReadResult())
    };

    const controls = await runtime.turnControls?.({
      client: client as never,
      scope: {
        cwd: "/repo",
        source: {
          kind: "floating",
          lifetime: "process",
          rawId: "activation",
          rawIdentity: null,
          visibleName: "Floating"
        }
      },
      threadId: "thread-floating"
    });

    expect(client.request).toHaveBeenCalledWith("settings/read", {
      cwd: "/repo",
      threadId: "thread-floating"
    });
    expect(controls).toMatchObject({
      agentName: "review",
      mode: "plan",
      model: "deepseek/deepseek-chat",
      permissionMode: "ask",
      reasoningEffort: "medium",
      runtimeOptions: {},
      runtimeRef: "native"
    });
  });

  it("exposes Workbench open-thread requests from the native bridge", async () => {
    const { createDesktopWorkbenchRuntime } = await import("./runtime");
    const handler = vi.fn();
    const unlisten = vi.fn();
    bridge.listenOpenThreadInWorkbench.mockResolvedValue(unlisten);

    const runtime = await createDesktopWorkbenchRuntime("workbench");
    const result = await runtime.onOpenThreadRequest?.(handler);

    expect(bridge.listenOpenThreadInWorkbench).toHaveBeenCalledWith(handler);
    expect(result).toBe(unlisten);
  });
});

function settingsReadResult() {
  return {
    channels: { channels: [] },
    controls: {
      agent: "review",
      mode: "plan",
      modeOptions: ["default", "plan"],
      model: "deepseek/deepseek-chat",
      modelDetails: [],
      modelError: null,
      modelOptions: ["deepseek/deepseek-chat"],
      modelStatus: "resolved",
      permissionMode: "ask",
      permissionModeOptions: ["default", "ask"],
      recentModels: [],
      runtimeRef: "native",
      variant: "medium",
      variantOptions: ["none", "medium"]
    },
    cwd: "/repo",
    memoryResources: {},
    project: {
      branch: null,
      displayPath: "/repo",
      path: "/repo"
    },
    secrets: {}
  };
}
