import type { GatewayClient } from "@psychevo/client";
import type { ObservabilityReadResult } from "@psychevo/protocol";
import { describe, expect, it, vi } from "vitest";

import { GatewayEventFeedApplication } from "./gateway-event-feed";
import { snapshot } from "./liveTranscript.test-support";
import { createSurfaceActions } from "./surface-actions";

describe("surface actions", () => {
  it("coalesces concurrent observability reads for one view", async () => {
    const response = deferred<ObservabilityReadResult>();
    const request = vi.fn(() => response.promise);
    const client = { request } as unknown as GatewayClient;
    const setObservability = vi.fn();
    const setContextUsage = vi.fn();
    const current = snapshot();
    const observabilityReads = new Map();
    const actions = createSurfaceActions({
      activeScope: current.scope,
      client,
      currentThreadId: current.thread?.id ?? null,
      debugEvents: { append: vi.fn() } as never,
      fallbackCwd: current.scope.cwd,
      gatewayEvents: new GatewayEventFeedApplication(),
      initScope: null,
      observabilityFreshness: 0,
      observabilityReads,
      onSnapshotAdopted: vi.fn(),
      scopeRef: { current: current.scope },
      selectedThreadIdRef: { current: current.thread?.id ?? null },
      sessionBrowserApplication: {} as never,
      settings: undefined,
      snapshot: current,
      snapshotReadFlights: new Map(),
      viewEpochRef: { current: 3 },
      workspaceApplication: {} as never,
      setActiveScope: vi.fn(),
      setAgents: vi.fn(),
      setBackends: vi.fn(),
      setCommands: vi.fn(),
      setContextUsage,
      setError: vi.fn(),
      setObservability,
      setRuntimeOptionsError: vi.fn(),
      setSettings: vi.fn(),
      setSnapshot: vi.fn(),
      setTraceState: vi.fn()
    });

    const homeRead = actions.refreshObservability();
    const terminalRead = actions.refreshObservability();
    expect(request).toHaveBeenCalledOnce();
    const terminal = observation(129);
    response.resolve(terminal);
    await Promise.all([homeRead, terminalRead]);

    expect(setObservability).toHaveBeenCalledOnce();
    expect(setObservability).toHaveBeenCalledWith(terminal);
    expect(setContextUsage).toHaveBeenCalledOnce();
    expect(setContextUsage).toHaveBeenCalledWith(terminal.context);
    expect(observabilityReads).toHaveLength(0);
  });

  it("supersedes a pre-terminal observability read at the completion boundary", async () => {
    const beforeTerminal = deferred<ObservabilityReadResult>();
    const afterTerminal = deferred<ObservabilityReadResult>();
    const request = vi.fn()
      .mockImplementationOnce(() => beforeTerminal.promise)
      .mockImplementationOnce(() => afterTerminal.promise);
    const client = { request } as unknown as GatewayClient;
    const setObservability = vi.fn();
    const current = snapshot();
    let freshness = 0;
    const actions = createSurfaceActions({
      activeScope: current.scope,
      client,
      currentThreadId: current.thread?.id ?? null,
      debugEvents: { append: vi.fn() } as never,
      fallbackCwd: current.scope.cwd,
      gatewayEvents: new GatewayEventFeedApplication(),
      initScope: null,
      get observabilityFreshness() {
        return freshness;
      },
      observabilityReads: new Map(),
      onSnapshotAdopted: vi.fn(),
      scopeRef: { current: current.scope },
      selectedThreadIdRef: { current: current.thread?.id ?? null },
      sessionBrowserApplication: {} as never,
      settings: undefined,
      snapshot: current,
      snapshotReadFlights: new Map(),
      viewEpochRef: { current: 3 },
      workspaceApplication: {} as never,
      setActiveScope: vi.fn(),
      setAgents: vi.fn(),
      setBackends: vi.fn(),
      setCommands: vi.fn(),
      setContextUsage: vi.fn(),
      setError: vi.fn(),
      setObservability,
      setRuntimeOptionsError: vi.fn(),
      setSettings: vi.fn(),
      setSnapshot: vi.fn(),
      setTraceState: vi.fn()
    });

    const staleRead = actions.refreshObservability();
    freshness = 1;
    const acceptedScope = {
      ...current.scope,
      source: {
        ...current.scope.source,
        rawId: `${current.scope.source.rawId}:accepted`
      }
    };
    const finalRead = actions.refreshObservability(
      client,
      acceptedScope,
      current.thread?.id ?? null,
      3
    );
    expect(request).toHaveBeenCalledTimes(2);
    beforeTerminal.resolve(observation(4_096));
    await staleRead;
    expect(setObservability).not.toHaveBeenCalled();

    const final = observation(129);
    afterTerminal.resolve(final);
    await finalRead;
    expect(setObservability).toHaveBeenCalledOnce();
    expect(setObservability).toHaveBeenCalledWith(final);
  });

  it("coalesces Workspace Home and another same-freshness observability read", async () => {
    const home = deferred<ObservabilityReadResult>();
    const request = vi.fn(() => home.promise);
    const client = { request } as unknown as GatewayClient;
    const setObservability = vi.fn();
    const current = snapshot();
    const actions = createSurfaceActions({
      activeScope: current.scope,
      client,
      currentThreadId: current.thread?.id ?? null,
      debugEvents: { append: vi.fn() } as never,
      fallbackCwd: current.scope.cwd,
      gatewayEvents: new GatewayEventFeedApplication(),
      initScope: null,
      observabilityFreshness: 0,
      observabilityReads: new Map(),
      onSnapshotAdopted: vi.fn(),
      scopeRef: { current: current.scope },
      selectedThreadIdRef: { current: current.thread?.id ?? null },
      sessionBrowserApplication: {} as never,
      settings: undefined,
      snapshot: current,
      snapshotReadFlights: new Map(),
      viewEpochRef: { current: 3 },
      workspaceApplication: { refreshSurface: vi.fn().mockResolvedValue(undefined) } as never,
      setActiveScope: vi.fn(),
      setAgents: vi.fn(),
      setBackends: vi.fn(),
      setCommands: vi.fn(),
      setContextUsage: vi.fn(),
      setError: vi.fn(),
      setObservability,
      setRuntimeOptionsError: vi.fn(),
      setSettings: vi.fn(),
      setSnapshot: vi.fn(),
      setTraceState: vi.fn()
    });

    const homeRead = actions.refreshWorkspaceSurface();
    const terminalRead = actions.refreshObservability();
    const latest = observation(129);
    expect(request).toHaveBeenCalledOnce();
    home.resolve(latest);
    await Promise.all([homeRead, terminalRead]);

    expect(setObservability).toHaveBeenCalledOnce();
    expect(setObservability).toHaveBeenCalledWith(latest);
  });
});

function observation(usedTokens: number): ObservabilityReadResult {
  return {
    context: {
      advice: [],
      appliesToSessionSeq: 2,
      available: true,
      basis: "latest_provider_turn",
      categories: [],
      contextLimit: 4_096,
      label: `${usedTokens}/4.1k`,
      percent: usedTokens / 40.96,
      status: "reported",
      usedTokens
    },
    usage: {
      accountedProviderCallCount: 1,
      assistantMessageCount: 1,
      available: true,
      billableInputTokens: usedTokens,
      billableOutputTokens: 0,
      cacheReadPercent: null,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
      contextInputTokens: usedTokens,
      costStatus: "unknown",
      effectiveTotalTokens: usedTokens,
      estimatedCostNanodollars: 0,
      estimatedPricingCount: 0,
      freePricingCount: 0,
      includedPricingCount: 0,
      messageCount: 1,
      model: "mock-model",
      provider: "mock",
      reasoningTokens: 0,
      reportedTotalTokens: usedTokens,
      sessionId: "thread-1",
      totalStatus: "reported",
      unaccountedProviderCallCount: 0,
      unknownPricingCount: 1
    }
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}
