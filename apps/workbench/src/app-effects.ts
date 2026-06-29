import { useEffect, useLayoutEffect, type MutableRefObject } from "react";
import {
  GatewayClient,
  parseThreadSnapshot,
  scopeForCwd
} from "@psychevo/client";
import { createBrowserHost, type GatewayEndpoint, type PsychevoHost } from "@psychevo/host";
import {
  GatewayEventSchema,
  InitializeResultSchema,
  TerminalExitedPayloadSchema,
  TerminalOutputPayloadSchema,
  type GatewayEvent,
  type GatewayRequestScope,
  type InitializeResult,
  type RuntimeOptionsResult,
  type SessionSummary,
  type SettingsReadResult,
  type ThreadSnapshot
} from "@psychevo/protocol";
import { asRecord, commandFeedbackAutoDismissable, optionalStringField } from "./data";
import {
  agentOptionValue,
  isRuntimeModeOption,
  projectRuntimeModeOption,
  type RuntimeModeProjection
} from "./runtime-controls";
import {
  normalizeSnapshot,
  startupDraftScope
} from "./session-utils";
import {
  PINNED_SESSIONS_KEY,
  PREFS_APPEARANCE_VERSION,
  PREFS_KEY,
  readPinnedSessionIdsFromStorage
} from "./storage";
import type {
  Appearance,
  CommandFeedback,
  DebugEvent,
  MainView,
  RightWorkspaceTab,
  TerminalNotificationEvent,
  TraceState,
  WorkbenchAgent,
  WorkbenchBackend,
  WorkbenchPrefs
} from "./types";
import {
  createHistoryDraftSession,
  type PendingDetachedShell
} from "./viewGuard";

const COMMAND_FEEDBACK_AUTO_DISMISS_MS = 3_000;

let terminalEventSeq = 0;

function nextTerminalEventSeq(): number {
  terminalEventSeq += 1;
  return terminalEventSeq;
}

type RefreshSnapshot = (
  nextClient?: GatewayClient | null,
  threadId?: string,
  scope?: GatewayRequestScope,
  readOnly?: boolean,
  expectedEpoch?: number | null,
  allowDetachedAdoption?: boolean
) => Promise<void>;

type RefreshWorkspaceSurface = (
  nextClient?: GatewayClient | null,
  scope?: GatewayRequestScope,
  threadId?: string | null,
  expectedEpoch?: number | null
) => Promise<void>;

type AppEffectsParams = {
  activeRightTabKind: RightWorkspaceTab["kind"] | null;
  activeRightTabId: string | null;
  activeScope: GatewayRequestScope | null;
  appearance: Appearance;
  client: GatewayClient | null;
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
  rightTabs: RightWorkspaceTab[];
  rightWidthPx: number;
  runnableAgents: WorkbenchAgent[];
  runtimeBackends: WorkbenchBackend[];
  runtimeModeOption: RuntimeOptionsResult["options"][number] | null;
  runtimeModeProjection: RuntimeModeProjection;
  runtimeSessionId: string | null;
  selectedAgentName: string;
  selectedRuntimeRef: string;
  settingsSection: string;
  settingsCwd: string | undefined;
  showSessionChrome: boolean;
  skipNextPinnedPersistRef: MutableRefObject<boolean>;
  snapshot: ThreadSnapshot;
  scopeRef: MutableRefObject<GatewayRequestScope | null>;
  selectedThreadIdRef: MutableRefObject<string | null>;
  mainViewRef: MutableRefObject<MainView>;
  viewEpochRef: MutableRefObject<number>;
  workMode: string;
  adoptSnapshotScope(nextClient: GatewayClient, nextSnapshot: ThreadSnapshot): Promise<void>;
  applyGatewayEvent(event: GatewayEvent): void;
  beginExplicitViewSwitch(): number;
  clearCommandTransientUi(): void;
  pushDebugEvent(method: string, payload: unknown): void;
  refreshAgentSurface(nextClient?: GatewayClient | null, scope?: GatewayRequestScope): Promise<void>;
  refreshHistory(nextClient?: GatewayClient | null, includeArchived?: boolean, cwd?: string | null): Promise<SessionSummary[]>;
  refreshSnapshot: RefreshSnapshot;
  refreshTrace(nextClient?: GatewayClient | null, threadId?: string | null): Promise<void>;
  refreshWorkspaceSurface: RefreshWorkspaceSurface;
  scheduleSnapshotRefreshAfterLiveSettle(nextClient: GatewayClient, threadId: string | null, epoch?: number): void;
  setActiveRightTabId(value: string | null): void;
  setActiveScope(value: GatewayRequestScope | null): void;
  setClient(value: GatewayClient | null): void;
  setCommandFeedback(value: CommandFeedback): void;
  setDraftSession(value: ReturnType<typeof createHistoryDraftSession> | null): void;
  setEndpoint(value: GatewayEndpoint | null): void;
  setError(value: string | null): void;
  setHost(value: PsychevoHost | null): void;
  setInit(value: InitializeResult | null): void;
  setMobilePanel(value: "history" | "transcript" | "status"): void;
  setPinnedSessionIds(value: string[]): void;
  setRightTabs(updater: (current: RightWorkspaceTab[]) => RightWorkspaceTab[]): void;
  setRuntimeOptionsError(value: string | null): void;
  setRuntimeOptionsLoading(value: boolean): void;
  setRuntimeOptionsResult(value: RuntimeOptionsResult | null): void;
  setRuntimeSessionId(value: string | null): void;
  setSelectedAgentName(value: string): void;
  setSelectedRuntimeMode(value: string | ((current: string) => string)): void;
  setSelectedRuntimeRef(value: string): void;
  setSnapshot(value: ThreadSnapshot | ((current: ThreadSnapshot) => ThreadSnapshot)): void;
  setStatus(value: string): void;
  setTerminalEvents(updater: (current: TerminalNotificationEvent[]) => TerminalNotificationEvent[]): void;
  setTraceState(value: TraceState): void;
  setWorkMode(value: string): void;
  updateMainView(value: MainView): void;
};

export function useWorkbenchEffects(params: AppEffectsParams) {
  useEffect(() => {
    if (
      params.selectedAgentName &&
      !params.runnableAgents.some((agent) => agentOptionValue(agent) === params.selectedAgentName || agent.name === params.selectedAgentName)
    ) {
      params.setSelectedAgentName("");
    }
  }, [params.runnableAgents, params.selectedAgentName]);

  useEffect(() => {
    if (params.selectedRuntimeRef === "native") {
      params.setRuntimeSessionId(null);
      params.setRuntimeOptionsResult(null);
      params.setRuntimeOptionsLoading(false);
      params.setRuntimeOptionsError(null);
      params.setSelectedRuntimeMode("");
      return;
    }
    if (!params.runtimeBackends.some((backend) => backend.id === params.selectedRuntimeRef)) {
      params.setSelectedRuntimeRef("native");
    }
  }, [params.runtimeBackends, params.selectedRuntimeRef]);

  useEffect(() => {
    if (!params.client || params.selectedRuntimeRef === "native") {
      return;
    }
    const scope = params.activeScope
      ?? params.initScope
      ?? scopeForCwd(params.settingsCwd ?? window.location.pathname);
    let cancelled = false;
    params.setRuntimeOptionsLoading(true);
    params.setRuntimeOptionsError(null);
    void params.client.request("runtime/options", {
      runtimeRef: params.selectedRuntimeRef,
      runtimeSessionId: params.runtimeSessionId,
      scope,
      threadId: params.snapshot.thread?.id ?? null
    }).then((result) => {
      if (cancelled) {
        return;
      }
      params.setRuntimeOptionsResult(result);
      params.setRuntimeSessionId(result.runtimeSessionId ?? null);
      const modeOption = result.options.find(isRuntimeModeOption);
      if (!modeOption) {
        params.setSelectedRuntimeMode("");
        return;
      }
      const projected = projectRuntimeModeOption(modeOption);
      const values = projected.extraValues.map((option) => option.value);
      params.setSelectedRuntimeMode((current) => (
        current && values.includes(current)
          ? current
          : projected.supportsPlan
            ? ""
            : projected.defaultValue
      ));
    }).catch((error) => {
      if (cancelled) {
        return;
      }
      params.setRuntimeOptionsResult(null);
      params.setSelectedRuntimeMode("");
      params.setRuntimeOptionsError(error instanceof Error ? error.message : String(error));
    }).finally(() => {
      if (!cancelled) {
        params.setRuntimeOptionsLoading(false);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [params.activeScope, params.client, params.initScope, params.runtimeSessionId, params.selectedRuntimeRef, params.settingsCwd, params.snapshot.thread?.id]);

  useEffect(() => {
    if (params.selectedRuntimeRef !== "native" && params.runtimeModeOption && !params.runtimeModeProjection.supportsPlan && params.workMode === "plan") {
      params.setWorkMode("default");
    }
  }, [params.runtimeModeOption, params.runtimeModeProjection.supportsPlan, params.selectedRuntimeRef, params.workMode]);

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
    const nextHost = createBrowserHost(window.location, window.localStorage);
    const nextEndpoint = nextHost.endpoint;
    const nextClient = new GatewayClient(nextEndpoint);
    params.setHost(nextHost);
    params.setEndpoint(nextEndpoint);

    nextClient.subscribe((notification) => {
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
          if (event.type === "turnStarted" && event.threadId) {
            void params.refreshHistory(nextClient);
          }
          if (event.type === "activityChanged" || event.type === "titleChanged") {
            void params.refreshHistory(nextClient);
          }
          if (event.type === "turnCompleted" && (event.threadId || event.turn.threadId)) {
            const threadId = event.threadId ?? event.turn.threadId;
            if (!threadId) {
              return;
            }
            const eventEpoch = params.viewEpochRef.current;
            params.scheduleSnapshotRefreshAfterLiveSettle(nextClient, threadId, eventEpoch);
            void params.refreshHistory(nextClient);
            const scope = params.scopeRef.current;
            if (scope) {
              void params.refreshWorkspaceSurface(nextClient, scope, threadId);
            }
            for (const delay of [1_500, 3_000, 7_500, 15_000, 30_000, 60_000, 120_000]) {
              window.setTimeout(() => {
                void params.refreshSnapshot(nextClient, threadId, undefined, true, eventEpoch);
                void params.refreshHistory(nextClient);
              }, delay);
            }
            window.setTimeout(() => {
              void params.refreshSnapshot(nextClient, threadId, undefined, true, eventEpoch);
            }, 750);
          }
          if (["permissionRequested", "permissionResolved", "clarifyRequested", "clarifyResolved"].includes(event.type)) {
            const threadId = "threadId" in event && event.threadId ? event.threadId : null;
            if (threadId) {
              void params.refreshSnapshot(nextClient, threadId, undefined, true, params.viewEpochRef.current);
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
          void params.refreshSnapshot(nextClient, threadId, undefined, true, eventEpoch, adoptDetached);
          const scope = params.scopeRef.current;
          if (scope) {
            void params.refreshWorkspaceSurface(nextClient, scope, threadId);
            void params.refreshAgentSurface(nextClient, scope);
          }
        } else {
          void params.refreshSnapshot(nextClient);
        }
        void params.refreshHistory(nextClient);
      }
      if (notification.method === "shell/error") {
        const record = asRecord(notification.params);
        params.setError(optionalStringField(record.message) ?? "Shell command failed");
        const threadId = optionalStringField(record.threadId);
        if (threadId) {
          void params.refreshSnapshot(nextClient, threadId, undefined, true, params.viewEpochRef.current);
        }
        void params.refreshHistory(nextClient);
      }
      if (notification.method === "turn/result") {
        const record = asRecord(notification.params);
        const thread = asRecord(record.thread);
        const threadId = optionalStringField(thread.id);
        if (threadId) {
          params.scheduleSnapshotRefreshAfterLiveSettle(nextClient, threadId, params.viewEpochRef.current);
          const scope = params.scopeRef.current;
          if (scope) {
            void params.refreshWorkspaceSurface(nextClient, scope, threadId);
          }
        } else {
          void params.refreshSnapshot(nextClient);
        }
        void params.refreshHistory(nextClient);
      }
      if (notification.method === "turn/error") {
        const record = asRecord(notification.params);
        params.setError(optionalStringField(record.message) ?? "Turn failed");
        const threadId = optionalStringField(record.threadId);
        if (threadId) {
          void params.refreshSnapshot(nextClient, threadId, undefined, true, params.viewEpochRef.current);
        } else {
          void params.refreshSnapshot(nextClient);
        }
        void params.refreshHistory(nextClient);
      }
    });

    async function boot() {
      try {
        await nextClient.connect();
        if (!alive) {
          return;
        }
        params.setClient(nextClient);
        params.setStatus("connected");
        const initialize = InitializeResultSchema.parse(await nextClient.request("initialize"));
        params.setInit(initialize);
        params.setActiveScope(initialize.scope);
        params.scopeRef.current = initialize.scope;
        const nextSessions = await params.refreshHistory(nextClient);
        const startupScope = startupDraftScope(initialize.scope, nextSessions);
        const epoch = params.beginExplicitViewSwitch();
        const nextSnapshot = parseThreadSnapshot(await nextClient.request("thread/start", { scope: startupScope }));
        if (!alive) {
          return;
        }
        const normalized = normalizeSnapshot(nextSnapshot);
        params.selectedThreadIdRef.current = normalized.thread?.id ?? null;
        params.setSnapshot(normalized);
        params.setDraftSession(createHistoryDraftSession(epoch, startupScope.cwd));
        if (params.mainViewRef.current === "transcript") {
          params.updateMainView("transcript");
        }
        await params.adoptSnapshotScope(nextClient, nextSnapshot);
      } catch (err) {
        if (alive) {
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
      nextClient.close();
    };
  }, []);

  useEffect(() => {
    if (params.client && params.activeScope) {
      void params.refreshWorkspaceSurface(params.client, params.activeScope, params.currentThreadId ?? null);
    }
  }, [params.client, params.activeScope, params.currentThreadId]);

  useEffect(() => {
    if (params.client) {
      void params.refreshHistory(params.client);
    }
  }, [params.client]);

  useEffect(() => {
    if (params.client && params.mainView === "settings" && params.settingsSection === "archived") {
      void params.refreshHistory(params.client, true);
    }
  }, [params.client, params.mainView, params.settingsSection]);

  useEffect(() => {
    if (params.client && params.activeScope) {
      void params.refreshAgentSurface(params.client, params.activeScope);
    }
  }, [params.client, params.activeScope, params.currentThreadId, params.snapshot.activity.running]);
}
