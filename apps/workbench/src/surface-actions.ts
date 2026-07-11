import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import {
  parseThreadSnapshot,
  reconcileThreadSnapshot,
  scopeForCwd,
  type GatewayClient
} from "@psychevo/client";
import {
  ObservabilityReadResultSchema,
  SettingsReadResultSchema,
  ThreadBrowserResultSchema,
  ThreadListResultSchema,
  ThreadTraceResultSchema,
  WorkspaceChangesResultSchema,
  WorkspaceDiffResultSchema,
  WorkspaceFilesResultSchema,
  type ContextReadResult,
  type GatewayRequestScope,
  type ObservabilityReadResult,
  type RuntimeOptionsResult,
  type SessionSummary,
  type SettingsReadResult,
  type ThreadBrowserResult,
  type ThreadSnapshot,
  type WorkspaceChangesResult,
  type WorkspaceDiffResult,
  type WorkspaceFilesResult
} from "@psychevo/protocol";
import {
  optionalStringField,
  parseAgentList,
  parseBackendList,
  parseCommandList
} from "./data";
import {
  normalizeSessionSummary,
  normalizeSnapshot
} from "./session-utils";
import type {
  DebugEvent,
  TraceState,
  SessionBrowserWorkspaceState,
  WorkbenchAgent,
  WorkbenchBackend,
  WorkbenchCommand
} from "./types";
import { shouldApplyReadOnlySnapshot } from "./viewGuard";

type SurfaceActionsParams = {
  activeScope: GatewayRequestScope | null;
  client: GatewayClient | null;
  currentThreadId: string | null;
  fallbackCwd: string;
  initScope: GatewayRequestScope | null;
  scopeRef: MutableRefObject<GatewayRequestScope | null>;
  selectedThreadIdRef: MutableRefObject<string | null>;
  pinnedSessionIds: string[];
  settings: SettingsReadResult | undefined;
  snapshot: ThreadSnapshot;
  viewEpochRef: MutableRefObject<number>;
  setActiveScope: Dispatch<SetStateAction<GatewayRequestScope | null>>;
  setAgents: Dispatch<SetStateAction<WorkbenchAgent[]>>;
  setArchivedSessions: Dispatch<SetStateAction<SessionSummary[]>>;
  setBackends: Dispatch<SetStateAction<WorkbenchBackend[]>>;
  setCommands: Dispatch<SetStateAction<WorkbenchCommand[]>>;
  setContextUsage: Dispatch<SetStateAction<ContextReadResult | null>>;
  setDebugEvents: Dispatch<SetStateAction<DebugEvent[]>>;
  setError: Dispatch<SetStateAction<string | null>>;
  setObservability: Dispatch<SetStateAction<ObservabilityReadResult | null>>;
  setPermissionMode: Dispatch<SetStateAction<string>>;
  setRuntimeOptionsError: Dispatch<SetStateAction<string | null>>;
  setRuntimeOptionsResult: Dispatch<SetStateAction<RuntimeOptionsResult | null>>;
  setRuntimeSessionId: Dispatch<SetStateAction<string | null>>;
  setSelectedAgentName: Dispatch<SetStateAction<string>>;
  setSelectedModel: Dispatch<SetStateAction<string | null>>;
  setSelectedRuntimeMode: Dispatch<SetStateAction<string>>;
  setSelectedRuntimeRef: Dispatch<SetStateAction<string>>;
  setSelectedVariant: Dispatch<SetStateAction<string>>;
  setSessions: Dispatch<SetStateAction<SessionSummary[]>>;
  setSessionBrowserWorkspaces: Dispatch<SetStateAction<SessionBrowserWorkspaceState[]>>;
  setSettings: Dispatch<SetStateAction<SettingsReadResult | undefined>>;
  setSnapshot: Dispatch<SetStateAction<ThreadSnapshot>>;
  setTraceState: Dispatch<SetStateAction<TraceState>>;
  setWorkMode: Dispatch<SetStateAction<string>>;
  setWorkspaceChanges: Dispatch<SetStateAction<WorkspaceChangesResult | null>>;
  setWorkspaceDiff: Dispatch<SetStateAction<WorkspaceDiffResult | null>>;
  setWorkspaceFiles: Dispatch<SetStateAction<WorkspaceFilesResult | null>>;
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
      const nextSnapshot = parseThreadSnapshot(await nextClient.request("thread/read", { threadId }));
      if (expectedEpoch != null && expectedEpoch !== params.viewEpochRef.current) {
        return;
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
        const next = normalizeSnapshot(reconcileThreadSnapshot(normalizeSnapshot(current), normalizeSnapshot(nextSnapshot)));
        params.selectedThreadIdRef.current = next.thread?.id ?? null;
        return next;
      });
      if ((params.selectedThreadIdRef.current ?? null) !== (nextSnapshot.thread?.id ?? threadId)) {
        return;
      }
      await refreshObservability(nextClient, nextSnapshot.scope, nextSnapshot.thread?.id ?? threadId, expectedEpoch);
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
    const scope = nextSnapshot.scope;
    if (!scope?.cwd) {
      return;
    }
    const previous = params.scopeRef.current;
    params.scopeRef.current = scope;
    params.setActiveScope(scope);
    const threadId = nextSnapshot.thread?.id ?? null;
    if ((previous?.cwd ?? "") === scope.cwd) {
      const nextSettings = SettingsReadResultSchema.parse(await nextClient.request("settings/read", { threadId, cwd: scope.cwd }));
      params.setSettings(nextSettings);
      applyInitialControls(nextSettings);
      await refreshObservability(nextClient, scope, threadId);
      return;
    }
    const [settingsValue] = await Promise.all([
      nextClient.request("settings/read", { threadId, cwd: scope.cwd }),
      refreshAgentSurface(nextClient, scope),
      refreshWorkspaceSurface(nextClient, scope, threadId)
    ]);
    const nextSettings = SettingsReadResultSchema.parse(settingsValue);
    params.setSettings(nextSettings);
    applyInitialControls(nextSettings);
  }

  async function refreshHistory(nextClient = params.client, includeArchived = false, cwd: string | null = null): Promise<SessionSummary[]> {
    if (!nextClient) {
      return [];
    }
    if (!includeArchived) {
      const result = ThreadBrowserResultSchema.parse(
        await nextClient.request("thread/browser", {
          archived: false,
          cursor: null,
          includeSessionIds: browserIncludeSessionIds(),
          limit: 20,
          recentDays: 7,
          cwd: cwd || null
        })
      );
      const nextSessions = sessionsFromThreadBrowser(result);
      params.setSessions(nextSessions);
      params.setSessionBrowserWorkspaces(workspacesFromThreadBrowser(result));
      return nextSessions;
    }
    const result = ThreadListResultSchema.parse(
      await nextClient.request("thread/list", { archived: includeArchived, limit: 100, cwd: cwd || null })
    );
    const nextSessions = result.sessions.map(normalizeSessionSummary);
    if (includeArchived) {
      params.setArchivedSessions(nextSessions);
    } else {
      params.setSessions(nextSessions);
    }
    return nextSessions;
  }

  function browserIncludeSessionIds(): string[] {
    return Array.from(new Set([
      params.currentThreadId,
      ...params.pinnedSessionIds
    ].filter((id): id is string => Boolean(id))));
  }

  async function refreshAgentSurface(nextClient = params.client, scope = params.activeScope ?? params.initScope ?? undefined) {
    if (!nextClient || !scope) {
      return;
    }
    const [agentList, backendList, commandList] = await Promise.all([
      nextClient.request("agent/list", { scope }),
      nextClient.request("backend/list", { scope }),
      nextClient.request("command/list", { scope, threadId: params.snapshot.thread?.id ?? null })
    ]);
    params.setAgents(parseAgentList(agentList));
    params.setBackends(parseBackendList(backendList));
    params.setCommands(parseCommandList(commandList));
  }

  async function refreshWorkspaceSurface(
    nextClient = params.client,
    scope = params.activeScope ?? params.initScope ?? undefined,
    threadId: string | null = params.currentThreadId ?? null,
    expectedEpoch: number | null = params.viewEpochRef.current
  ) {
    if (!nextClient || !scope) {
      params.setWorkspaceChanges(null);
      params.setObservability(null);
      params.setContextUsage(null);
      return;
    }
    if (!threadId) {
      params.setObservability(null);
      params.setContextUsage(null);
    }
    const [files, diff, changes, nextObservability] = await Promise.all([
      nextClient.request("workspace/files", { scope }),
      nextClient.request("workspace/diff", { scope, path: null }),
      nextClient.request("workspace/changes", { scope }),
      threadId ? nextClient.request("observability/read", { scope, threadId }) : Promise.resolve(null)
    ]);
    if (!shouldApplyAsyncWorkspaceResult(scope, expectedEpoch)) {
      return;
    }
    params.setWorkspaceFiles(WorkspaceFilesResultSchema.parse(files));
    params.setWorkspaceDiff(WorkspaceDiffResultSchema.parse(diff));
    params.setWorkspaceChanges(WorkspaceChangesResultSchema.parse(changes));
    if (nextObservability && shouldApplyAsyncSurfaceResult(scope, expectedEpoch, threadId)) {
      applyObservability(nextObservability);
    }
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
    const nextObservability = await nextClient.request("observability/read", { scope, threadId });
    if (!shouldApplyAsyncSurfaceResult(scope, expectedEpoch, threadId)) {
      return;
    }
    applyObservability(nextObservability);
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
    params.setDebugEvents((current) => [
      {
        id: `${Date.now()}:${method}:${current.length}`,
        at: Date.now(),
        method,
        payload
      },
      ...current
    ].slice(0, 120));
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
    params.setPermissionMode(nextControls.permissionMode || "default");
    params.setWorkMode(nextControls.mode || "default");
    params.setSelectedRuntimeRef(nextControls.runtimeRef || "native");
    params.setRuntimeSessionId(null);
    params.setRuntimeOptionsResult(null);
    params.setRuntimeOptionsError(null);
    params.setSelectedRuntimeMode("");
    params.setSelectedAgentName(nextControls.agent ?? "");
    params.setSelectedModel(nextControls.model ?? null);
    params.setSelectedVariant(nextControls.variant ?? "none");
  }

  async function runAction(action: () => Promise<void>) {
    try {
      params.setError(null);
      await action();
    } catch (err) {
      params.setError(err instanceof Error ? err.message : String(err));
    }
  }

  return {
    adoptSnapshotScope,
    applyInitialControls,
    applyObservability,
    pushDebugEvent,
    refreshAgentSurface,
    refreshHistory,
    refreshObservability,
    refreshRevertedThreadSnapshot,
    refreshSnapshot,
    refreshTrace,
    refreshWorkspaceSurface,
    runAction
  };
}

export function sessionsFromThreadBrowser(result: ThreadBrowserResult): SessionSummary[] {
  const seen = new Set<string>();
  const sessions: SessionSummary[] = [];
  for (const workspace of result.workspaces) {
    for (const session of workspace.sessions) {
      if (seen.has(session.id)) {
        continue;
      }
      seen.add(session.id);
      sessions.push(normalizeSessionSummary(session));
    }
  }
  return sessions;
}

export function workspacesFromThreadBrowser(result: ThreadBrowserResult): SessionBrowserWorkspaceState[] {
  return result.workspaces.map((workspace) => ({
    cwd: workspace.cwd,
    hiddenCount: workspace.hiddenCount,
    nextCursor: workspace.nextCursor
  }));
}
