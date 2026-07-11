import { useEffect, useLayoutEffect, type MutableRefObject } from "react";
import {
  applyTurnResultToThreadSnapshot,
  parseThreadSnapshot,
  type GatewayClient
} from "@psychevo/client";
import type { GatewayEndpoint, PsychevoHost } from "@psychevo/host";
import {
  GatewayEventSchema,
  InitializeResultSchema,
  TerminalExitedPayloadSchema,
  TerminalOutputPayloadSchema,
  TurnResultNotificationSchema,
  type GatewayEvent,
  type GatewayRequestScope,
  type InitializeResult,
  type RuntimeConfigOptionView,
  type SessionSummary,
  type SettingsReadResult,
  type ThreadSnapshot
} from "@psychevo/protocol";
import { asRecord, commandFeedbackAutoDismissable, optionalStringField } from "./data";
import { agentOptionValue, type RuntimeModeProjection } from "./runtime-controls";
import {
  normalizeSnapshot,
  startupDraftScope
} from "./session-utils";
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
  DebugEvent,
  MainView,
  RightWorkspaceTab,
  TerminalNotificationEvent,
  TraceState,
  WorkbenchAgent,
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

type AppEffectsParams = {
  activeRightTabKind: RightWorkspaceTab["kind"] | null;
  activeRightTabId: string | null;
  activeScope: GatewayRequestScope | null;
  appearance: Appearance;
  client: GatewayClient | null;
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
  rightTabs: RightWorkspaceTab[];
  rightWidthPx: number;
  runnableAgents: WorkbenchAgent[];
  runtimeModeOption: RuntimeConfigOptionView | null;
  runtimeModeProjection: RuntimeModeProjection;
  selectedAgentName: string;
  selectedRuntimeRef: string;
  settingsSection: string;
  fallbackCwd: string;
  showSessionChrome: boolean;
  skipNextPinnedPersistRef: MutableRefObject<boolean>;
  snapshot: ThreadSnapshot;
  scopeRef: MutableRefObject<GatewayRequestScope | null>;
  selectedThreadIdRef: MutableRefObject<string | null>;
  mainViewRef: MutableRefObject<MainView>;
  viewEpochRef: MutableRefObject<number>;
  workMode: string;
  adoptSnapshotScope(runtimeClient: GatewayClient, nextSnapshot: ThreadSnapshot): Promise<void>;
  applyGatewayEvent(event: GatewayEvent): void;
  beginExplicitViewSwitch(): number;
  clearCommandTransientUi(): void;
  pushDebugEvent(method: string, payload: unknown): void;
  refreshAgentSurface(runtimeClient?: GatewayClient | null, scope?: GatewayRequestScope): Promise<void>;
  refreshHistory(runtimeClient?: GatewayClient | null, includeArchived?: boolean, cwd?: string | null): Promise<SessionSummary[]>;
  refreshSnapshot: RefreshSnapshot;
  refreshTrace(runtimeClient?: GatewayClient | null, threadId?: string | null): Promise<void>;
  refreshWorkspaceSurface: RefreshWorkspaceSurface;
  scheduleSnapshotRefreshAfterLiveSettle(runtimeClient: GatewayClient, threadId: string | null, epoch?: number): void;
  setActiveRightTabId(value: string | null): void;
  setActiveScope(value: GatewayRequestScope | null): void;
  setClient(value: GatewayClient | null): void;
  setCommandFeedback(value: CommandFeedback): void;
  setDraftSession(value: ReturnType<typeof createHistoryDraftSession> | null): void;
  setEndpoint(value: GatewayEndpoint | null): void;
  setError(value: string | null): void;
  setFallbackCwd(value: string): void;
  setHost(value: PsychevoHost | null): void;
  setInit(value: InitializeResult | null): void;
  setMobilePanel(value: "history" | "transcript" | "status"): void;
  setPinnedSessionIds(value: string[]): void;
  setRightCollapsed(value: boolean): void;
  setRightTabs(updater: (current: RightWorkspaceTab[]) => RightWorkspaceTab[]): void;
  setSelectedAgentName(value: string): void;
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
    let activeClient: GatewayClient | null = null;
    let openThreadUnlisten: (() => void) | null = null;

    function attachRuntime(runtimeClient: GatewayClient) {
      runtimeClient.subscribe((notification) => {
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
            void params.refreshHistory(runtimeClient);
          }
          if (event.type === "activityChanged" || event.type === "titleChanged") {
            void params.refreshHistory(runtimeClient);
          }
          if (event.type === "runtimeChildChanged" && event.threadId) {
            params.setRightTabs((current) => registerRuntimeChildTab(current, event));
            params.setRightCollapsed(false);
            params.setMobilePanel("status");
          }
          if (event.type === "turnCompleted" && (event.threadId || event.turn.threadId)) {
            const threadId = event.threadId ?? event.turn.threadId;
            if (!threadId) {
              return;
            }
            const eventEpoch = params.viewEpochRef.current;
            params.scheduleSnapshotRefreshAfterLiveSettle(runtimeClient, threadId, eventEpoch);
            void params.refreshHistory(runtimeClient);
            const scope = params.scopeRef.current;
            if (scope) {
              void params.refreshWorkspaceSurface(runtimeClient, scope, threadId);
            }
            for (const delay of [1_500, 3_000, 7_500, 15_000, 30_000, 60_000, 120_000]) {
              window.setTimeout(() => {
                void params.refreshSnapshot(runtimeClient, threadId, undefined, true, eventEpoch);
                void params.refreshHistory(runtimeClient);
              }, delay);
            }
            window.setTimeout(() => {
              void params.refreshSnapshot(runtimeClient, threadId, undefined, true, eventEpoch);
            }, 750);
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
      if (notification.method === "turn/result") {
        const parsed = TurnResultNotificationSchema.safeParse(notification.params);
        const record = asRecord(notification.params);
        const thread = asRecord(record.thread);
        const threadId = parsed.success
          ? parsed.data.thread.id
          : optionalStringField(thread.id);
        if (parsed.success) {
          params.setSnapshot((current) => {
            const next = normalizeSnapshot(applyTurnResultToThreadSnapshot(current, parsed.data));
            params.selectedThreadIdRef.current = next.thread?.id ?? null;
            return next;
          });
        }
        if (threadId) {
          params.scheduleSnapshotRefreshAfterLiveSettle(runtimeClient, threadId, params.viewEpochRef.current);
          const scope = params.scopeRef.current;
          if (scope) {
            void params.refreshWorkspaceSurface(runtimeClient, scope, threadId);
          }
        } else {
          void params.refreshSnapshot(runtimeClient);
        }
        void params.refreshHistory(runtimeClient);
      }
      if (notification.method === "turn/error") {
        const record = asRecord(notification.params);
        params.setError(optionalStringField(record.message) ?? "Turn failed");
        const threadId = optionalStringField(record.threadId);
        if (threadId) {
          void params.refreshSnapshot(runtimeClient, threadId, undefined, true, params.viewEpochRef.current);
        } else {
          void params.refreshSnapshot(runtimeClient);
        }
        void params.refreshHistory(runtimeClient);
      }
      });
    }

    async function boot() {
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
        params.setClient(runtime.client);
        params.setStatus("connected");
        const initialize = InitializeResultSchema.parse(await runtime.client.request("initialize"));
        params.setInit(initialize);
        params.setActiveScope(initialize.scope);
        params.scopeRef.current = initialize.scope;
        const nextSessions = await params.refreshHistory(runtime.client);
        const startupScope = startupDraftScope(initialize.scope, nextSessions, runtime.fallbackCwd);
        const epoch = params.beginExplicitViewSwitch();
        const nextSnapshot = parseThreadSnapshot(await runtime.client.request("thread/start", { scope: startupScope }));
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
        await params.adoptSnapshotScope(runtime.client, nextSnapshot);
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
      openThreadUnlisten?.();
      activeClient?.close();
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

function registerRuntimeChildTab(
  current: RightWorkspaceTab[],
  event: Extract<GatewayEvent, { type: "runtimeChildChanged" }>
): RightWorkspaceTab[] {
  if (!event.threadId) {
    return current;
  }
  const existing = current.find((tab) => (
    tab.kind === "agentSession" && tab.threadId === event.threadId
  ));
  const next: RightWorkspaceTab = {
    id: existing?.id ?? `runtime-child:${encodeURIComponent(event.threadId)}`,
    kind: "agentSession",
    title: `${runtimeRefLabel(event.runtimeRef)} child`,
    threadId: event.threadId,
    parentThreadId: event.parentThreadId,
    runtimeRef: event.runtimeRef,
    runtimeStatus: event.status,
    runtimeReadOnly: event.readOnly,
    historyFidelity: existing?.historyFidelity ?? null,
    pendingPrompt: null,
    path: null,
    diff: null,
    file: null,
    preview: null,
    message: null
  };
  if (!existing) {
    return [...current, next];
  }
  return current.map((tab) => tab.id === existing.id ? { ...tab, ...next } : tab);
}

function runtimeRefLabel(runtimeRef: string): string {
  if (runtimeRef === "opencode") return "OpenCode";
  if (runtimeRef === "codex") return "Codex";
  if (runtimeRef === "native") return "Native";
  return runtimeRef;
}
