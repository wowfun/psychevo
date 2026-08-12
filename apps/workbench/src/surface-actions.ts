import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import {
  parseThreadSnapshot,
  reconcileThreadSnapshot,
  gatewayScopeKey,
  scopeForCwd,
  type GatewayClient
} from "@psychevo/client";
import {
  ObservabilityReadResultSchema,
  SettingsReadResultSchema,
  ThreadTraceResultSchema,
  type ContextReadResult,
  type GatewayRequestScope,
  type ObservabilityReadResult,
  type SettingsReadResult,
  type ThreadSnapshot
} from "@psychevo/protocol";
import {
  optionalStringField,
  parseAgentList,
  parseBackendList,
  parseCommandList
} from "./data";
import { normalizeSnapshot } from "./session-utils";
import type { SessionBrowserApplication } from "./session-browser-application";
import type {
  TraceState,
  WorkbenchAgent,
  WorkbenchBackend,
  WorkbenchCommand
} from "./types";
import { shouldApplyReadOnlySnapshot } from "./viewGuard";
import type { WorkspaceApplication } from "./workspace-application";
import type { DebugEventApplication } from "./debug-event-application";
import {
  reconcileThreadSnapshotAfterGatewayBarrier,
  type GatewayEventFeedApplication
} from "./gateway-event-feed";

type ObservabilityRead = {
  client: GatewayClient;
  freshness: number;
  promise: Promise<void>;
  token: symbol;
};

type SurfaceActionsParams = {
  activeScope: GatewayRequestScope | null;
  client: GatewayClient | null;
  currentThreadId: string | null;
  fallbackCwd: string;
  initScope: GatewayRequestScope | null;
  scopeRef: MutableRefObject<GatewayRequestScope | null>;
  selectedThreadIdRef: MutableRefObject<string | null>;
  sessionBrowserApplication: SessionBrowserApplication;
  settings: SettingsReadResult | undefined;
  snapshot: ThreadSnapshot;
  viewEpochRef: MutableRefObject<number>;
  workspaceApplication: WorkspaceApplication;
  gatewayEvents: GatewayEventFeedApplication;
  observabilityFreshness: number;
  observabilityReads: Map<string, ObservabilityRead>;
  snapshotReadFlights: Map<string, { client: GatewayClient; promise: Promise<void> }>;
  setActiveScope: Dispatch<SetStateAction<GatewayRequestScope | null>>;
  setAgents: Dispatch<SetStateAction<WorkbenchAgent[]>>;
  setBackends: Dispatch<SetStateAction<WorkbenchBackend[]>>;
  setCommands: Dispatch<SetStateAction<WorkbenchCommand[]>>;
  setContextUsage: Dispatch<SetStateAction<ContextReadResult | null>>;
  debugEvents: DebugEventApplication;
  setError: Dispatch<SetStateAction<string | null>>;
  setObservability: Dispatch<SetStateAction<ObservabilityReadResult | null>>;
  setRuntimeOptionsError: Dispatch<SetStateAction<string | null>>;
  setSettings: Dispatch<SetStateAction<SettingsReadResult | undefined>>;
  setSnapshot: Dispatch<SetStateAction<ThreadSnapshot>>;
  setTraceState: Dispatch<SetStateAction<TraceState>>;
  onSnapshotAdopted(): void;
};

export function createSurfaceActions(params: SurfaceActionsParams) {
  function defaultScope(): GatewayRequestScope {
    return params.activeScope
      ?? params.initScope
      ?? scopeForCwd(params.settings?.cwd || params.fallbackCwd);
  }

  async function refreshSnapshot(
    nextClient = params.client,
    threadId?: string,
    scope = params.activeScope ?? params.initScope ?? undefined,
    readOnly = false,
    expectedEpoch: number | null | undefined = null,
    allowDetachedAdoption = false
  ) {
    if (!nextClient) {
      return;
    }
    if (threadId && readOnly) {
      const flightKey = `${threadId}:${expectedEpoch ?? params.viewEpochRef.current}`;
      const existing = params.snapshotReadFlights.get(flightKey);
      if (existing?.client === nextClient) return existing.promise;
      const gatewayEventBarrier = params.gatewayEvents.getSnapshot().latestSeq;
      const snapshotFlight = (async (): Promise<ThreadSnapshot | null> => {
        const nextSnapshot = parseThreadSnapshot(await nextClient.request("thread/read", { threadId }));
        if (expectedEpoch != null && expectedEpoch !== params.viewEpochRef.current) {
          return null;
        }
        params.setSnapshot((current) => {
          if (!shouldApplyReadOnlySnapshot(
            current,
            threadId,
            params.viewEpochRef.current,
            expectedEpoch,
            allowDetachedAdoption
          )) {
            return current;
          }
          const next = normalizeSnapshot(reconcileThreadSnapshotAfterGatewayBarrier(
            normalizeSnapshot(current),
            normalizeSnapshot(nextSnapshot),
            params.gatewayEvents.getSnapshot(),
            threadId,
            gatewayEventBarrier
          ));
          params.selectedThreadIdRef.current = next.thread?.id ?? null;
          return next;
        });
        return nextSnapshot;
      })();
      const flight = snapshotFlight.then(() => undefined);
      const registered = { client: nextClient, promise: flight };
      params.snapshotReadFlights.set(flightKey, registered);
      let observedSnapshot: ThreadSnapshot | null = null;
      try {
        observedSnapshot = await snapshotFlight;
      } finally {
        if (params.snapshotReadFlights.get(flightKey) === registered) {
          params.snapshotReadFlights.delete(flightKey);
        }
      }
      if (
        observedSnapshot
        && (params.selectedThreadIdRef.current ?? null)
          === (observedSnapshot.thread?.id ?? threadId)
      ) {
        await refreshObservability(
          nextClient,
          observedSnapshot.scope,
          observedSnapshot.thread?.id ?? threadId,
          expectedEpoch
        );
        params.onSnapshotAdopted();
      }
      return;
    }
    const nextScope = scope ?? defaultScope();
    const requestParams = threadId ? { threadId, scope: nextScope } : { scope: nextScope };
    const nextSnapshot = parseThreadSnapshot(await nextClient.request("thread/resume", requestParams));
    params.setSnapshot((current) => {
      if (expectedEpoch != null && expectedEpoch !== params.viewEpochRef.current) {
        return current;
      }
      const currentSnapshot = normalizeSnapshot(current);
      const incomingSnapshot = normalizeSnapshot(nextSnapshot);
      if (
        !threadId &&
        !allowDetachedAdoption &&
        currentSnapshot.thread === null &&
        incomingSnapshot.thread !== null
      ) {
        return current;
      }
      const next = normalizeSnapshot(reconcileThreadSnapshot(currentSnapshot, incomingSnapshot));
      params.selectedThreadIdRef.current = next.thread?.id ?? null;
      return next;
    });
    if (expectedEpoch != null && expectedEpoch !== params.viewEpochRef.current) {
      return;
    }
    await adoptSnapshotScope(nextClient, nextSnapshot);
    params.onSnapshotAdopted();
  }

  async function refreshRevertedThreadSnapshot(
    nextClient: GatewayClient | null,
    threadId: string | null
  ) {
    if (!nextClient || !threadId) {
      return;
    }
    const nextSnapshot = normalizeSnapshot(parseThreadSnapshot(await nextClient.request("thread/read", { threadId })));
    params.setSnapshot((current) => (
      (current.thread?.id ?? null) === threadId ? (() => {
        params.selectedThreadIdRef.current = nextSnapshot.thread?.id ?? null;
        return nextSnapshot;
      })() : current
    ));
  }

  async function adoptSnapshotScope(nextClient: GatewayClient, nextSnapshot: ThreadSnapshot) {
    void nextClient;
    const scope = nextSnapshot.scope;
    if (!scope?.cwd) {
      return;
    }
    params.scopeRef.current = scope;
    params.setActiveScope((current) => (
      gatewayScopeKey(current) === gatewayScopeKey(scope) ? current : scope
    ));
  }

  async function refreshSettings(
    nextClient = params.client,
    cwd = params.activeScope?.cwd ?? params.initScope?.cwd ?? params.fallbackCwd,
    threadId: string | null = params.currentThreadId ?? null
  ) {
    if (!nextClient || !cwd) {
      return;
    }
    const settingsValue = await nextClient.request("settings/read", { threadId, cwd });
    const nextSettings = SettingsReadResultSchema.parse(settingsValue);
    params.setSettings(nextSettings);
    applyInitialControls(nextSettings);
  }

  async function refreshHistory(
    nextClient = params.client,
    includeArchived = false,
    cwd: string | null = null
  ) {
    return params.sessionBrowserApplication.refreshHistory(nextClient, {
      activeScope: params.activeScope ?? params.initScope,
      currentThreadId: params.currentThreadId,
      cwd,
      includeArchived
    });
  }

  async function refreshAgentCatalog(nextClient = params.client, scope = params.activeScope ?? params.initScope ?? undefined) {
    if (!nextClient || !scope) {
      return;
    }
    const [agentList, backendList] = await Promise.all([
      nextClient.request("agent/list", { scope }),
      nextClient.request("backend/list", { scope })
    ]);
    params.setAgents(parseAgentList(agentList));
    params.setBackends(parseBackendList(backendList));
  }

  async function refreshCommands(
    nextClient = params.client,
    scope = params.activeScope ?? params.initScope ?? undefined,
    threadId: string | null = params.currentThreadId ?? null
  ) {
    if (!nextClient || !scope) {
      return;
    }
    const commandList = await nextClient.request("command/list", { scope, threadId });
    params.setCommands(parseCommandList(commandList));
  }

  async function refreshAgentSurface(nextClient = params.client, scope = params.activeScope ?? params.initScope ?? undefined) {
    await Promise.all([
      refreshAgentCatalog(nextClient, scope),
      refreshCommands(nextClient, scope, params.currentThreadId ?? null)
    ]);
  }

  async function refreshWorkspaceSurface(
    nextClient = params.client,
    scope = params.activeScope ?? params.initScope ?? undefined,
    threadId: string | null = params.currentThreadId ?? null,
    expectedEpoch: number | null = params.viewEpochRef.current
  ) {
    if (!nextClient || !scope) {
      params.workspaceApplication.bind(nextClient, scope ?? null);
      params.setObservability(null);
      params.setContextUsage(null);
      return;
    }
    if (!threadId) {
      params.setObservability(null);
      params.setContextUsage(null);
    }
    await Promise.all([
      params.workspaceApplication.refreshSurface(nextClient, scope),
      threadId
        ? refreshObservability(nextClient, scope, threadId, expectedEpoch)
        : Promise.resolve()
    ]);
  }

  async function refreshWorkspaceFiles(
    nextClient = params.client,
    scope = params.activeScope ?? params.initScope ?? undefined,
    expectedEpoch: number | null = params.viewEpochRef.current
  ) {
    if (!nextClient || !scope) {
      return;
    }
    void expectedEpoch;
    await params.workspaceApplication.refresh("files", nextClient, scope);
  }

  async function refreshWorkspaceDiff(
    nextClient = params.client,
    scope = params.activeScope ?? params.initScope ?? undefined,
    expectedEpoch: number | null = params.viewEpochRef.current
  ) {
    if (!nextClient || !scope) {
      return;
    }
    void expectedEpoch;
    await params.workspaceApplication.refresh("diff", nextClient, scope);
  }

  async function refreshWorkspaceChanges(
    nextClient = params.client,
    scope = params.activeScope ?? params.initScope ?? undefined,
    expectedEpoch: number | null = params.viewEpochRef.current
  ) {
    if (!nextClient || !scope) {
      return;
    }
    void expectedEpoch;
    await params.workspaceApplication.refresh("changes", nextClient, scope);
  }

  async function refreshObservability(
    nextClient = params.client,
    scope = params.activeScope ?? params.initScope ?? undefined,
    threadId: string | null = params.currentThreadId ?? null,
    expectedEpoch: number | null = params.viewEpochRef.current
  ) {
    if (!nextClient || !scope) {
      params.setObservability(null);
      params.setContextUsage(null);
      return;
    }
    const readKey = JSON.stringify([
      threadId ? ["thread", threadId] : ["scope", gatewayScopeKey(scope)],
      expectedEpoch ?? params.viewEpochRef.current
    ]);
    const existing = params.observabilityReads.get(readKey);
    if (
      existing?.client === nextClient
      && existing.freshness === params.observabilityFreshness
    ) {
      return existing.promise;
    }
    const token = Symbol();
    const promise = (async () => {
      try {
        const nextObservability = await nextClient.request("observability/read", { scope, threadId });
        if (
          params.observabilityReads.get(readKey)?.token !== token
          || !shouldApplyAsyncSurfaceResult(scope, expectedEpoch, threadId)
        ) {
          return;
        }
        applyObservability(nextObservability);
      } finally {
        if (params.observabilityReads.get(readKey)?.token === token) {
          params.observabilityReads.delete(readKey);
        }
      }
    })();
    const read: ObservabilityRead = {
      client: nextClient,
      freshness: params.observabilityFreshness,
      promise,
      token
    };
    params.observabilityReads.set(readKey, read);
    return promise;
  }

  function shouldApplyAsyncSurfaceResult(
    scope: GatewayRequestScope,
    expectedEpoch: number | null,
    threadId: string | null
  ): boolean {
    return shouldApplyAsyncWorkspaceResult(scope, expectedEpoch) &&
      (params.selectedThreadIdRef.current ?? null) === threadId;
  }

  function shouldApplyAsyncWorkspaceResult(
    scope: GatewayRequestScope,
    expectedEpoch: number | null
  ): boolean {
    if (expectedEpoch != null && expectedEpoch !== params.viewEpochRef.current) {
      return false;
    }
    const currentScope = params.scopeRef.current ?? params.activeScope ?? params.initScope ?? null;
    return !currentScope?.cwd || currentScope.cwd === scope.cwd;
  }

  function applyObservability(value: unknown) {
    const parsed = ObservabilityReadResultSchema.parse(value);
    params.setObservability(parsed);
    params.setContextUsage(parsed.context);
  }

  function pushDebugEvent(method: string, payload: unknown) {
    params.debugEvents.append(method, payload);
  }

  async function refreshTrace(
    nextClient: GatewayClient | null = params.client,
    threadId: string | null = params.currentThreadId ?? null
  ) {
    if (!nextClient || !threadId) {
      params.setTraceState({ error: null, loading: false, result: null, threadId: null });
      return;
    }
    params.setTraceState((current) => ({
      error: null,
      loading: true,
      result: current.threadId === threadId ? current.result : null,
      threadId
    }));
    try {
      const result = ThreadTraceResultSchema.parse(
        await nextClient.request("thread/trace", { threadId, afterSeq: null, limit: 200 })
      );
      params.setTraceState((current) => (
        current.threadId === threadId
          ? { error: null, loading: false, result, threadId }
          : current
      ));
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      params.setTraceState((current) => (
        current.threadId === threadId
          ? { error: message, loading: false, result: current.result, threadId }
          : current
      ));
    }
  }

  function applyInitialControls(nextSettings: SettingsReadResult) {
    const nextControls = nextSettings.controls;
    if (!nextControls) {
      return;
    }
    params.setRuntimeOptionsError(null);
  }

  async function runAction<T>(action: () => Promise<T>): Promise<T | undefined> {
    try {
      params.setError(null);
      return await action();
    } catch (err) {
      params.setError(err instanceof Error ? err.message : String(err));
      return undefined;
    }
  }

  return {
    adoptSnapshotScope,
    applyInitialControls,
    applyObservability,
    pushDebugEvent,
    refreshAgentCatalog,
    refreshAgentSurface,
    refreshCommands,
    refreshHistory,
    refreshObservability,
    refreshRevertedThreadSnapshot,
    refreshSnapshot,
    refreshSettings,
    refreshTrace,
    refreshWorkspaceChanges,
    refreshWorkspaceDiff,
    refreshWorkspaceFiles,
    refreshWorkspaceSurface,
    runAction
  };
}

export type ReturnTypeOfSurfaceActions = ReturnType<typeof createSurfaceActions>;
