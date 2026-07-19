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

  it("uses the canonical Thread Context target for Floating turns", async () => {
    const { createDesktopFloatingRuntime } = await import("./runtime");
    const runtime = createDesktopFloatingRuntime("floating");
    const client = {
      request: vi.fn()
        .mockResolvedValueOnce(threadContext())
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

    expect(client.request).toHaveBeenCalledWith("thread/context/read", {
      threadId: "thread-floating",
      target: null,
      scope: expect.objectContaining({ cwd: "/repo" })
    });
    expect(controls).toMatchObject({
      context: { selectedTargetId: "target:review:native", suggestedTargetId: null },
      controls: {
        targetId: "target:review:native",
        turnOverrides: {},
        expectedContextRevision: "context-1",
        expectedControlRevision: "controls-1"
      },
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

function threadContext() {
  return {
    selectedTargetId: "target:review:native",
    suggestedTargetId: null,
    runtimeProfileRef: "native",
    selectionState: "bound",
    profiles: [],
    binding: { runtimeRef: "native" },
    controls: [],
    stability: "stable",
    capabilities: [],
    compatibleTargets: [{
      targetId: "target:review:native",
      agentRef: "review",
      runtimeProfileRef: "native",
      agentLabel: "review",
      profileLabel: "Psychevo (Native)",
      label: "review · Psychevo (Native)",
      ready: true,
      unavailableReason: null
    }],
    inputCapabilities: [],
    actions: [],
    sendability: { allowed: true, reason: null, recoveryAction: null },
    history: { owner: "psychevo", fidelity: "full", cursor: null, hint: null },
    pendingInteractions: [],
    contextRevision: "context-1",
    controlRevision: "controls-1"
  };
}
