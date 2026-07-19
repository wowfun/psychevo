import { useEffect, useLayoutEffect, useRef, type MutableRefObject } from "react";
import {
  parseThreadSnapshot,
  type GatewayClient,
  type ThreadController
} from "@psychevo/client";
import type { GatewayEndpoint, PsychevoHost } from "@psychevo/host";
import {
  GatewayEventSchema,
  InitializeResultSchema,
  TerminalExitedPayloadSchema,
  TerminalOutputPayloadSchema,
  type GatewayEvent,
  type GatewayRequestScope,
  type InitializeResult,
  type SessionSummary,
  type SettingsReadResult,
  type ThreadSnapshot
} from "@psychevo/protocol";
import { asRecord, commandFeedbackAutoDismissable, optionalStringField } from "./data";
import {
  normalizeSnapshot,
  startupDraftScope
} from "./session-utils";
import { transcriptMayContainWorkspaceFile } from "./search-model";
import type { WorkbenchRuntimeFactory } from "./runtime";
import {
  PINNED_SESSIONS_KEY,
  PREFS_APPEARANCE_VERSION,
  PREFS_KEY,
  readPinnedSessionIdsFromStorage
} from "./storage";
import type {
  Appearance,
  CommandFeedback,
  CommandOverlay,
  DebugEvent,
  MainView,
  RightWorkspaceTab,
  TerminalNotificationEvent,
  TraceState,
  WorkbenchPrefs
} from "./types";
import {
  createHistoryDraftSession,
  type PendingDetachedShell
} from "./viewGuard";
import { parseThreadContext } from "./runtime-context";
import {
  type ComposerSessionCoordinator,
  type DraftOpenToken
} from "./composer-session-coordinator";

const COMMAND_FEEDBACK_AUTO_DISMISS_MS = 3_000;

let terminalEventSeq = 0;

function nextTerminalEventSeq(): number {
  terminalEventSeq += 1;
  return terminalEventSeq;
}

type RefreshSnapshot = (
  runtimeClient?: GatewayClient | null,
  threadId?: string,
  scope?: GatewayRequestScope,
  readOnly?: boolean,
  expectedEpoch?: number | null,
  allowDetachedAdoption?: boolean
) => Promise<void>;

type RefreshWorkspaceSurface = (
  runtimeClient?: GatewayClient | null,
  scope?: GatewayRequestScope,
  threadId?: string | null,
  expectedEpoch?: number | null
) => Promise<void>;

type RefreshWorkspaceFacet = (
  runtimeClient?: GatewayClient | null,
  scope?: GatewayRequestScope,
  expectedEpoch?: number | null
) => Promise<void>;

type AppEffectsParams = {
  activeCommandOverlay: CommandOverlay | null;
  activeRightTabKind: RightWorkspaceTab["kind"] | null;
  activeRightTabId: string | null;
  activeScope: GatewayRequestScope | null;
  appearance: Appearance;
  client: GatewayClient | null;
  composerSessionCoordinator: ComposerSessionCoordinator;
  createRuntime: WorkbenchRuntimeFactory;
  commandContextKey: string;
  commandContextKeyRef: MutableRefObject<string | null>;
  commandFeedback: CommandFeedback;
  currentThreadId: string | null;
  debugEnabled: boolean;
  dirtyRightTabs: Record<string, boolean>;
  draftSession: unknown;
  gatewayEventQueueRef: MutableRefObject<GatewayEvent[]>;
  gatewayEventRafRef: MutableRefObject<number | null>;
  host: PsychevoHost | null;
  initScope: GatewayRequestScope | null;
  mainView: MainView;
  mobilePanel: "history" | "transcript" | "status";
  pinnedSessionIds: string[];
  pendingDetachedShellRef: MutableRefObject<PendingDetachedShell | null>;
  firstTurnContextRefreshPendingRef: MutableRefObject<boolean>;
  rightTabs: RightWorkspaceTab[];
  rightWorkspaceOpen: boolean;
  rightWidthPx: number;
  runtimeTargetTransitionRef: MutableRefObject<boolean>;
  settingsSection: string;
  fallbackCwd: string;
  showSessionChrome: boolean;
  skipNextPinnedPersistRef: MutableRefObject<boolean>;
  snapshot: ThreadSnapshot;
  startupStable: boolean;
  threadController: ThreadController;
  scopeRef: MutableRefObject<GatewayRequestScope | null>;
  selectedThreadIdRef: MutableRefObject<string | null>;
  mainViewRef: MutableRefObject<MainView>;
  viewEpochRef: MutableRefObject<number>;
  adoptSnapshotScope(runtimeClient: GatewayClient, nextSnapshot: ThreadSnapshot): Promise<void>;
  applyGatewayEvent(event: GatewayEvent): void;
  patchSessionEvent(event: GatewayEvent): void;
  beginExplicitViewSwitch(): number;
  clearCommandTransientUi(): void;
  pushDebugEvent(method: string, payload: unknown): void;
  refreshAgentSurface(runtimeClient?: GatewayClient | null, scope?: GatewayRequestScope): Promise<void>;
  refreshCommands(runtimeClient?: GatewayClient | null, scope?: GatewayRequestScope, threadId?: string | null): Promise<void>;
  refreshHistory(runtimeClient?: GatewayClient | null, includeArchived?: boolean, cwd?: string | null): Promise<SessionSummary[]>;
  refreshObservability(runtimeClient?: GatewayClient | null, scope?: GatewayRequestScope, threadId?: string | null, expectedEpoch?: number | null): Promise<void>;
  refreshRuntimeContext(): void;
  refreshSettings(runtimeClient?: GatewayClient | null, cwd?: string, threadId?: string | null): Promise<void>;
  refreshSnapshot: RefreshSnapshot;
  refreshTrace(runtimeClient?: GatewayClient | null, threadId?: string | null): Promise<void>;
  refreshWorkspaceChanges: RefreshWorkspaceFacet;
  refreshWorkspaceDiff: RefreshWorkspaceFacet;
  refreshWorkspaceFiles: RefreshWorkspaceFacet;
  refreshWorkspaceSurface: RefreshWorkspaceSurface;
  setActiveRightTabId(value: string | null): void;
  setActiveScope(value: GatewayRequestScope | null): void;
  setClient(value: GatewayClient | null): void;
  setCommandFeedback(value: CommandFeedback): void;
  setDraftSession(value: ReturnType<typeof createHistoryDraftSession> | null): void;
  setEndpoint(value: GatewayEndpoint | null): void;
  setError(value: string | null): void;
  setFallbackCwd(value: string): void;
  setHost(value: PsychevoHost | null): void;
  setHistoryLoading(value: boolean): void;
  setInit(value: InitializeResult | null): void;
  setMobilePanel(value: "history" | "transcript" | "status"): void;
  setPinnedSessionIds(value: string[]): void;
  setRightCollapsed(value: boolean): void;
  setRightTabs(updater: (current: RightWorkspaceTab[]) => RightWorkspaceTab[]): void;
  setRuntimeContext(value: import("@psychevo/protocol").ThreadContextReadResult | null): void;
  setRuntimeContextTargetId(value: string): void;
  setRuntimeOptionsError(value: string | null): void;
  setRuntimeOptionsLoading(value: boolean): void;
  setWorkspaceBranch(value: string | null): void;
  setSelectedTargetId(value: string): void;
  setSnapshot(value: ThreadSnapshot | ((current: ThreadSnapshot) => ThreadSnapshot)): void;
  setStatus(value: string): void;
  setStartupStable(value: boolean): void;
  setTerminalEvents(updater: (current: TerminalNotificationEvent[]) => TerminalNotificationEvent[]): void;
  setTraceState(value: TraceState): void;
  updateMainView(value: MainView): void;
};

export function useWorkbenchEffects(params: AppEffectsParams) {
  const latestParamsRef = useRef(params);
  latestParamsRef.current = params;

  function refreshVisibleWorkspace(
    current: AppEffectsParams,
    runtimeClient: GatewayClient,
    scope: GatewayRequestScope,
    threadId: string | null,
    epoch = current.viewEpochRef.current
  ): void {
    if (!current.rightWorkspaceOpen) {
      return;
    }
    if (current.activeRightTabKind === "files") {
      void current.refreshWorkspaceFiles(runtimeClient, scope, epoch);
      return;
    }
    if (current.activeRightTabKind === "review") {
      void Promise.all([
        current.refreshWorkspaceDiff(runtimeClient, scope, epoch),
        current.refreshWorkspaceChanges(runtimeClient, scope, epoch)
      ]);
      return;
    }
    if (current.activeRightTabId === null) {
      void current.refreshWorkspaceDiff(runtimeClient, scope, epoch);
      if (threadId) {
        void current.refreshObservability(runtimeClient, scope, threadId, epoch);
      }
    }
  }

  useEffect(() => {
    if (params.debugEnabled) {
      return;
    }
    params.setRightTabs((current) => current.filter((tab) => tab.kind !== "debug"));
    if (params.activeRightTabKind === "debug") {
      params.setActiveRightTabId(null);
    }
  }, [params.activeRightTabKind, params.debugEnabled]);

  useEffect(() => {
    if (!Object.values(params.dirtyRightTabs).some(Boolean)) {
      return;
    }
    const handler = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", handler);
    return () => window.removeEventListener("beforeunload", handler);
  }, [params.dirtyRightTabs]);

  useEffect(() => {
    if (!params.debugEnabled || params.activeRightTabKind !== "debug" || !params.client || !params.currentThreadId) {
      if (!params.currentThreadId) {
        params.setTraceState({ error: null, loading: false, result: null, threadId: null });
      }
      return;
    }
    void params.refreshTrace(params.client, params.currentThreadId);
  }, [params.activeRightTabKind, params.client, params.currentThreadId, params.debugEnabled]);

  useEffect(() => {
    document.documentElement.dataset.pevoAppearance = params.appearance;
    params.host?.storage.setJson<WorkbenchPrefs>(PREFS_KEY, {
      appearance: params.appearance,
      appearanceVersion: PREFS_APPEARANCE_VERSION,
      debug: params.debugEnabled,
      rightWidthPx: params.rightWidthPx
    });
  }, [params.appearance, params.debugEnabled, params.host, params.rightWidthPx]);

  useEffect(() => {
    if (params.host) {
      params.skipNextPinnedPersistRef.current = true;
      params.setPinnedSessionIds(readPinnedSessionIdsFromStorage(params.host.storage));
    }
  }, [params.host]);

  useEffect(() => {
    try {
      if (params.host) {
        if (params.skipNextPinnedPersistRef.current) {
          params.skipNextPinnedPersistRef.current = false;
          return;
        }
        params.host.storage.setJson(PINNED_SESSIONS_KEY, params.pinnedSessionIds);
      } else {
        window.localStorage.setItem(PINNED_SESSIONS_KEY, JSON.stringify(params.pinnedSessionIds));
      }
    } catch {
      // Preference writes should not block session controls.
    }
  }, [params.host, params.pinnedSessionIds]);

  useEffect(() => {
    if (params.currentThreadId && params.draftSession) {
      params.setDraftSession(null);
    }
  }, [params.currentThreadId, params.draftSession]);

  useEffect(() => {
    if (params.activeRightTabId && !params.rightTabs.some((tab) => tab.id === params.activeRightTabId)) {
      params.setActiveRightTabId(params.rightTabs.at(-1)?.id ?? null);
    }
  }, [params.activeRightTabId, params.rightTabs]);

  useEffect(() => {
    if (params.commandContextKeyRef.current === null) {
      params.commandContextKeyRef.current = params.commandContextKey;
      return;
    }
    if (params.commandContextKeyRef.current !== params.commandContextKey) {
      params.commandContextKeyRef.current = params.commandContextKey;
      params.clearCommandTransientUi();
    }
  }, [params.commandContextKey]);

  useEffect(() => {
    if (!params.commandFeedback) {
      return;
    }
    let timer: number | null = null;
    if (commandFeedbackAutoDismissable(params.commandFeedback)) {
      timer = window.setTimeout(() => {
        params.setCommandFeedback(null);
      }, COMMAND_FEEDBACK_AUTO_DISMISS_MS);
    }
    function onPointerDown(event: MouseEvent) {
      const target = event.target;
      if (target instanceof Element && target.closest(".commandFeedback")) {
        return;
      }
      params.setCommandFeedback(null);
    }
    document.addEventListener("mousedown", onPointerDown);
    return () => {
      if (timer !== null) {
        window.clearTimeout(timer);
      }
      document.removeEventListener("mousedown", onPointerDown);
    };
  }, [params.commandFeedback]);

  useLayoutEffect(() => {
    if (!params.showSessionChrome && params.mobilePanel === "status") {
      params.setMobilePanel("transcript");
    }
  }, [params.mobilePanel, params.showSessionChrome]);

  useEffect(() => {
    let alive = true;
    let activeClient: GatewayClient | null = null;
    let openThreadUnlisten: (() => void) | null = null;

    function attachRuntime(runtimeClient: GatewayClient) {
      runtimeClient.subscribe((notification) => {
      const params = latestParamsRef.current;
      params.pushDebugEvent(notification.method, notification.params);
      if (notification.method === "terminal/output") {
        const parsed = TerminalOutputPayloadSchema.safeParse(notification.params);
        if (parsed.success) {
          params.setTerminalEvents((current) => [
            ...current.slice(-240),
            { method: "terminal/output", params: parsed.data, seq: nextTerminalEventSeq() }
          ]);
        }
      }
      if (notification.method === "terminal/exited") {
        const parsed = TerminalExitedPayloadSchema.safeParse(notification.params);
        if (parsed.success) {
          params.setTerminalEvents((current) => [
            ...current.slice(-240),
            { method: "terminal/exited", params: parsed.data, seq: nextTerminalEventSeq() }
          ]);
        }
      }
      if (notification.method === "gateway/event") {
        const parsed = GatewayEventSchema.safeParse(notification.params);
        if (parsed.success) {
          const event = parsed.data;
          params.applyGatewayEvent(event);
          params.patchSessionEvent(event);
          if (
            event.type === "entryCompleted"
            && event.entry.role === "assistant"
            && transcriptMayContainWorkspaceFile([event.entry])
          ) {
            const scope = params.scopeRef.current;
            if (scope) {
              void params.refreshWorkspaceFiles(runtimeClient, scope, params.viewEpochRef.current);
            }
          }
          if (event.type === "turnCompleted" && (event.threadId || event.turn.threadId)) {
            if (event.turn.error?.message) {
              params.setError(event.turn.error.message);
            }
            const threadId = event.threadId ?? event.turn.threadId;
            if (!threadId) {
              return;
            }
            const context = params.threadController.context();
            const refreshFirstTurnContext = params.firstTurnContextRefreshPendingRef.current;
            if (refreshFirstTurnContext) {
              params.firstTurnContextRefreshPendingRef.current = false;
            }
            const refreshAcpContext = context?.binding?.backendKind === "acp"
              || context?.history.owner === "agent";
            if (
              threadId === params.selectedThreadIdRef.current
              && (refreshFirstTurnContext || refreshAcpContext)
            ) {
              params.refreshRuntimeContext();
            }
            const scope = params.scopeRef.current;
            if (scope) {
              const epoch = params.viewEpochRef.current;
              const filesVisible = params.rightWorkspaceOpen && params.activeRightTabKind === "files";
              const transcriptNeedsFiles = transcriptMayContainWorkspaceFile([
                ...params.snapshot.entries,
                ...event.committedEntries
              ]);
              if (filesVisible || transcriptNeedsFiles) {
                void params.refreshWorkspaceFiles(runtimeClient, scope, epoch);
              }
              if (!filesVisible) {
                refreshVisibleWorkspace(params, runtimeClient, scope, threadId, epoch);
              }
            }
          }
          if (["actionRequested", "actionUpdated", "actionResolved", "actionCancelled"].includes(event.type)) {
            const threadId = "action" in event && event.action.threadId
              ? event.action.threadId
              : params.selectedThreadIdRef.current;
            if (threadId) {
              void params.refreshSnapshot(runtimeClient, threadId, undefined, true, params.viewEpochRef.current);
            }
          }
        }
      }
      if (notification.method === "shell/result") {
        const record = asRecord(notification.params);
        const thread = asRecord(record.thread);
        const threadId = optionalStringField(thread.id);
        if (threadId) {
          const eventEpoch = params.viewEpochRef.current;
          const pending = params.pendingDetachedShellRef.current;
          const adoptDetached = pending?.epoch === eventEpoch;
          if (adoptDetached) {
            params.pendingDetachedShellRef.current = null;
          }
          void params.refreshSnapshot(runtimeClient, threadId, undefined, true, eventEpoch, adoptDetached);
          const scope = params.scopeRef.current;
          if (scope) {
            void params.refreshWorkspaceSurface(runtimeClient, scope, threadId);
            void params.refreshAgentSurface(runtimeClient, scope);
          }
        } else {
          void params.refreshSnapshot(runtimeClient);
        }
        void params.refreshHistory(runtimeClient);
      }
      if (notification.method === "shell/error") {
        const record = asRecord(notification.params);
        params.setError(optionalStringField(record.message) ?? "Shell command failed");
        const threadId = optionalStringField(record.threadId);
        if (threadId) {
          void params.refreshSnapshot(runtimeClient, threadId, undefined, true, params.viewEpochRef.current);
        }
        void params.refreshHistory(runtimeClient);
      }
      });
    }

    async function boot() {
      let startupDraftOpenToken: DraftOpenToken | null = null;
      try {
        const runtime = await params.createRuntime();
        if (!alive) {
          runtime.client.close();
          return;
        }
        activeClient = runtime.client;
        params.setHost(runtime.host);
        params.setEndpoint(runtime.endpoint);
        params.setFallbackCwd(runtime.fallbackCwd);
        attachRuntime(runtime.client);
        if (runtime.onOpenThreadRequest) {
          try {
            const maybeUnlisten = await runtime.onOpenThreadRequest((threadId) => {
              const epoch = params.beginExplicitViewSwitch();
              params.updateMainView("transcript");
              params.setMobilePanel("transcript");
              void params.refreshSnapshot(runtime.client, threadId, undefined, false, epoch)
                .then(() => params.refreshHistory(runtime.client));
            });
            if (!alive) {
              maybeUnlisten();
              return;
            }
            openThreadUnlisten = maybeUnlisten;
          } catch (error) {
            params.pushDebugEvent("desktop-open-thread/listen-error", {
              message: error instanceof Error ? error.message : String(error)
            });
          }
        }
        await runtime.client.connect();
        if (!alive) {
          runtime.client.close();
          return;
        }
        params.setStatus("connected");
        params.setClient(runtime.client);
        const startupEpoch = params.viewEpochRef.current;
        const initializeRequest = runtime.client.request("initialize")
          .then((value) => InitializeResultSchema.parse(value));
        const sessionsRequest = params.refreshHistory(runtime.client)
          .then((sessions) => {
            if (alive) {
              params.setHistoryLoading(false);
            }
            return sessions;
          })
          .catch((error) => {
            if (alive) {
              params.setHistoryLoading(false);
              params.pushDebugEvent("thread/browser/startup-error", {
                message: error instanceof Error ? error.message : String(error)
              });
            }
            return [];
          });
        const initialize = await initializeRequest;
        const nextSessions = initialize.scope.cwd.trim()
          ? []
          : await sessionsRequest;
        if (!alive) {
          return;
        }
        params.setInit(initialize);
        if (params.viewEpochRef.current !== startupEpoch) {
          return;
        }
        params.setActiveScope(initialize.scope);
        params.scopeRef.current = initialize.scope;
        const startupScope = startupDraftScope(initialize.scope, nextSessions, runtime.fallbackCwd);
        startupDraftOpenToken = params.composerSessionCoordinator.beginDraftOpen(startupEpoch);
        const draftOpenRequest = runtime.client.request("thread/draft/open", {
          origin: startupScope,
          targetIntent: { kind: "default" }
        });
        const branchRequest = runtime.client.request("workspace/git/branches", {
          scope: startupScope
        }).then((result) => result.current?.trim() || null).catch((error) => {
          params.pushDebugEvent("workspace/git/branches/startup-error", {
            message: error instanceof Error ? error.message : String(error)
          });
          return null;
        });
        params.selectedThreadIdRef.current = null;
        params.setSnapshot(normalizeSnapshot({
          source: initialize.source,
          scope: startupScope,
          thread: null,
          history: params.snapshot.history,
          entries: [],
          activity: { running: false, activeTurnId: null, queuedTurns: 0 },
          pendingActions: []
        }));
        params.setDraftSession(createHistoryDraftSession(startupEpoch, startupScope.cwd));
        params.setRuntimeOptionsLoading(true);
        params.setRuntimeOptionsError(null);
        const [opened, workspaceBranch] = await Promise.all([draftOpenRequest, branchRequest]);
        const nextSnapshot = parseThreadSnapshot(opened.snapshot);
        const nextContext = parseThreadContext(opened.context);
        if (!alive) {
          return;
        }
        if (params.viewEpochRef.current === startupEpoch) {
          const normalized = normalizeSnapshot(nextSnapshot);
          await params.adoptSnapshotScope(runtime.client, nextSnapshot);
          if (!alive || params.viewEpochRef.current !== startupEpoch) {
            return;
          }
          params.selectedThreadIdRef.current = normalized.thread?.id ?? null;
          params.setSnapshot(normalized);
          params.setDraftSession(createHistoryDraftSession(startupEpoch, startupScope.cwd));
          params.threadController.setContext(nextContext);
          params.setRuntimeContext(nextContext);
          params.setWorkspaceBranch(workspaceBranch);
          params.setRuntimeContextTargetId(nextContext.selectedTargetId ?? "");
          params.setSelectedTargetId(
            nextContext.selectedTargetId
            ?? nextContext.suggestedTargetId
            ?? ""
          );
          params.setRuntimeOptionsError(opened.problem?.message ?? null);
          if (params.mainViewRef.current === "transcript") {
            params.updateMainView("transcript");
          }
          params.setStartupStable(true);
          if (opened.problem) {
            params.composerSessionCoordinator.failDraftOpen(startupDraftOpenToken);
          } else {
            params.composerSessionCoordinator.completeDraftOpen(startupDraftOpenToken);
          }
        }
        params.setRuntimeOptionsLoading(false);
      } catch (err) {
        if (startupDraftOpenToken) {
          params.composerSessionCoordinator.failDraftOpen(startupDraftOpenToken);
        }
        if (alive) {
          params.setRuntimeOptionsLoading(false);
          params.setHistoryLoading(false);
          params.setStatus("error");
          params.setError(err instanceof Error ? err.message : String(err));
        }
      }
    }

    void boot();
    return () => {
      alive = false;
      params.gatewayEventQueueRef.current = [];
      if (params.gatewayEventRafRef.current !== null) {
        window.cancelAnimationFrame(params.gatewayEventRafRef.current);
        params.gatewayEventRafRef.current = null;
      }
      openThreadUnlisten?.();
      activeClient?.close();
    };
  }, []);

  const activeScopeSourceKey = params.activeScope
    ? [
        params.activeScope.source.kind,
        params.activeScope.source.rawId ?? "",
        params.activeScope.source.lifetime
      ].join(":")
    : "";

  useEffect(() => {
    if (
      params.startupStable
      && params.client
      && params.activeScope
      && params.mainView === "settings"
      && !params.runtimeTargetTransitionRef.current
    ) {
      void params.refreshSettings(params.client, params.activeScope.cwd, params.currentThreadId ?? null);
    }
  }, [params.startupStable, params.client, params.activeScope?.cwd, params.currentThreadId, params.mainView]);

  useEffect(() => {
    if (
      params.startupStable
      && params.client
      && params.activeScope
      && params.rightWorkspaceOpen
      && !params.runtimeTargetTransitionRef.current
    ) {
      refreshVisibleWorkspace(params, params.client, params.activeScope, params.currentThreadId ?? null);
    }
  }, [
    params.startupStable,
    params.client,
    params.activeScope?.cwd,
    params.currentThreadId,
    params.activeRightTabKind,
    params.rightWorkspaceOpen
  ]);

  useEffect(() => {
    if (params.startupStable && params.client && params.activeScope && params.activeCommandOverlay && !params.runtimeTargetTransitionRef.current) {
      void params.refreshCommands(params.client, params.activeScope, params.currentThreadId ?? null);
    }
  }, [
    params.startupStable,
    params.client,
    params.activeScope?.cwd,
    activeScopeSourceKey,
    params.activeCommandOverlay,
    params.currentThreadId,
  ]);
}
