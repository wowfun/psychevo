import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import {
  ActionReceiptProvider,
  ConfirmActionProvider,
  useConfirmAction,
  type HistoryDraftSession
} from "@psychevo/components";
import {
  GatewayClient,
  latestAssistantTranscriptText,
  parseThreadSnapshot,
  scopeForCwd,
  ThreadSession
} from "@psychevo/client";
import type { GatewayEndpoint, PsychevoHost } from "@psychevo/host";
import {
  SettingsReadResultSchema,
  UsageReadResultSchema,
  type ContextReadResult,
  type GatewayEvent,
  type GatewayMention,
  type GatewayRequestScope,
  type InitializeResult,
  type ModelOptionView,
  type ObservabilityReadResult,
  type ThreadContextReadResult,
  type ThreadControlDescriptorView,
  type ThreadEditableInputPart,
  type SessionSummary,
  type SettingsReadResult,
  type ThreadSnapshot,
  type UsageReadResult,
  type WorkspaceChangesResult,
  type WorkspaceDiffResult,
  type WorkspaceFilesResult
} from "@psychevo/protocol";
import { createCommandActions } from "./command-actions";
import { createAppActions } from "./app-actions";
import { useWorkbenchEffects } from "./app-effects";
import { ComposerSessionCoordinator } from "./composer-session-coordinator";
import { useAutomations } from "./app-automations";
import { EMPTY_SNAPSHOT } from "./app-constants";
import { useGatewayLiveEvents } from "./app-live-events";
import {
  mergeModelCatalogOptionsIntoSettings
} from "./app-model-state";
import { createSurfaceActions } from "./surface-actions";
import {
  normalizeActivity,
  normalizeSnapshot
} from "./session-utils";
import { SessionBrowserApplication } from "./session-browser-application";
import { transcriptMayContainWorkspaceFile } from "./search-model";
import { WorkbenchLayout } from "./workbench-layout";
import {
  rightWorkspaceTabVisibleForSession
} from "./right-workspace-model";
import { runtimeControlAsConfigOption } from "./runtime-controls";
import {
  parseThreadContext,
  shouldRetainFirstTurnDraftContext
} from "./runtime-context";
import {
  readPinnedSessionIds,
  readWorkbenchPrefs
} from "./storage";
import { createRightWorkspaceActions } from "./right-workspace-actions";
import {
  EMPTY_GATEWAY_EVENT_FEED,
  confirmedSteerTurnId,
  type GatewayThreadEventFeed
} from "./gateway-event-feed";
import {
  enabledThreadAction
} from "./thread-application";
import { createWorkbenchIntentOwner } from "./workbench-intents";
import {
  browserFallbackCwd,
  createBrowserWorkbenchRuntime,
  type WorkbenchRuntimeFactory
} from "./runtime";
import { startWavRecorder, type VoiceRecorder } from "./voice-capture";
import { WorkspaceApplication } from "./workspace-application";
import type {
  Appearance,
  BackendDraft,
  CommandFeedback,
  CommandOverlay,
  DebugEvent,
  CapabilityTab,
  MainView,
  PendingAttachment,
  RightWorkspaceTab,
  SettingsSection,
  TerminalNotificationEvent,
  TraceState,
  WorkbenchAgent,
  WorkbenchBackend,
  WorkbenchBackendDoctor,
  WorkbenchChannelDoctor,
  WorkbenchCommand
} from "./types";
import {
  createHistoryDraftSession,
  visibleHistoryDraftSession,
  type PendingDetachedShell
} from "./viewGuard";

declare global {
  interface Window {
    __psychevoJourneyTiming?: Record<string, {
      epochMs: number;
      monotonicMs: number;
    }>;
  }
}

export function App({ runtimeFactory = createBrowserWorkbenchRuntime }: { runtimeFactory?: WorkbenchRuntimeFactory } = {}) {
  return (
    <ConfirmActionProvider>
      <ActionReceiptProvider>
        <WorkbenchApp runtimeFactory={runtimeFactory} />
      </ActionReceiptProvider>
    </ConfirmActionProvider>
  );
}

function WorkbenchApp({ runtimeFactory }: { runtimeFactory: WorkbenchRuntimeFactory }) {
  const confirmAction = useConfirmAction();
  const threadSession = useMemo(() => new ThreadSession({ snapshot: EMPTY_SNAPSHOT }), []);
  const composerSessionCoordinator = useMemo(() => new ComposerSessionCoordinator(), []);
  const sessionBrowserApplication = useMemo(
    () => new SessionBrowserApplication(readPinnedSessionIds()),
    []
  );
  const workspaceApplication = useMemo(() => new WorkspaceApplication(), []);
  const threadSessionViewStore = useMemo(() => ({
    getSnapshot: () => threadSession.getView(),
    subscribe: (listener: () => void) => threadSession.subscribe(listener)
  }), [threadSession]);
  const threadSessionView = useSyncExternalStore(
    threadSessionViewStore.subscribe,
    threadSessionViewStore.getSnapshot,
    threadSessionViewStore.getSnapshot
  );
  const snapshot = threadSessionView.threadSnapshot ?? EMPTY_SNAPSHOT;
  const liveTranscriptEntries = threadSessionView.liveEntries;
  const runtimeContext = useMemo(
    () => threadSessionView.context
      ? parseThreadContext(threadSessionView.context)
      : null,
    [threadSessionView.context]
  );
  const sessionBrowserView = useSyncExternalStore(
    sessionBrowserApplication.subscribe,
    sessionBrowserApplication.getSnapshot,
    sessionBrowserApplication.getSnapshot
  );
  const workspaceView = useSyncExternalStore(
    workspaceApplication.subscribe,
    workspaceApplication.getSnapshot,
    workspaceApplication.getSnapshot
  );
  const {
    archivedSessions,
    loadingOlderCwd,
    pinnedSessionIds,
    sessions,
    workspaces: sessionBrowserWorkspaces
  } = sessionBrowserView;
  const {
    branch: workspaceBranchState,
    changes: workspaceChanges,
    diff: workspaceDiff,
    files: workspaceFiles
  } = workspaceView;
  const workspaceBranch = workspaceBranchState === undefined
    ? undefined
    : workspaceBranchState?.current?.trim() || null;
  const workspaceIsGitRepo = workspaceBranchState?.isGitRepo;
  const [client, setClient] = useState<GatewayClient | null>(null);
  const [startupStable, setStartupStable] = useState(false);
  const [host, setHost] = useState<PsychevoHost | null>(null);
  const [endpoint, setEndpoint] = useState<GatewayEndpoint | null>(null);
  const [init, setInit] = useState<InitializeResult | null>(null);
  const [activeScope, setActiveScope] = useState<GatewayRequestScope | null>(null);
  const [historyLoading, setHistoryLoading] = useState(true);
  const [olderHistoryLoading, setOlderHistoryLoading] = useState(false);
  const [draftSession, setDraftSession] = useState<HistoryDraftSession | null>(() =>
    createHistoryDraftSession(0, browserFallbackCwd())
  );
  const [settings, setSettings] = useState<SettingsReadResult | undefined>();
  const [agents, setAgents] = useState<WorkbenchAgent[]>([]);
  const [backends, setBackends] = useState<WorkbenchBackend[]>([]);
  const [backendDraft, setBackendDraft] = useState<BackendDraft | null>(null);
  const [backendDoctor, setBackendDoctor] = useState<Record<string, WorkbenchBackendDoctor>>({});
  const [channelDoctor, setChannelDoctor] = useState<Record<string, WorkbenchChannelDoctor>>({});
  const [commands, setCommands] = useState<WorkbenchCommand[]>([]);
  const [rightTabs, setRightTabs] = useState<RightWorkspaceTab[]>([]);
  const pinnedMessageKeys = useMemo(() => new Set(
    rightTabs.flatMap((tab) => tab.kind === "pinnedMessage" && tab.pinnedMessage
      ? [tab.pinnedMessage.key]
      : [])
  ), [rightTabs]);
  const [activeRightTabId, setActiveRightTabId] = useState<string | null>(null);
  const [mainView, setMainView] = useState<MainView>("transcript");
  const [capabilitiesTab, setCapabilitiesTab] = useState<CapabilityTab>("skills");
  const [settingsSection, setSettingsSection] = useState<SettingsSection>("appearance");
  const [leftCollapsed, setLeftCollapsed] = useState(false);
  const [rightCollapsed, setRightCollapsed] = useState(true);
  const [commandFeedback, setCommandFeedback] = useState<CommandFeedback>(null);
  const [activeCommandOverlay, setActiveCommandOverlay] = useState<CommandOverlay | null>(null);
  const [selectedTargetId, setSelectedTargetId] = useState<string>("");
  const [runtimeContextTargetId, setRuntimeContextTargetId] = useState<string>("");
  const [runtimeContextRefreshRevision, setRuntimeContextRefreshRevision] = useState(0);
  const [runtimeControlDrafts, setRuntimeControlDrafts] = useState<Record<string, unknown>>({});
  const [runtimeOptionsLoading, setRuntimeOptionsLoading] = useState(false);
  const [runtimeOptionsError, setRuntimeOptionsError] = useState<string | null>(null);
  const [workspaceDialogOpen, setWorkspaceDialogOpen] = useState(false);
  const [contextUsage, setContextUsage] = useState<ContextReadResult | null>(null);
  const [observability, setObservability] = useState<ObservabilityReadResult | null>(null);
  const [usageStats, setUsageStats] = useState<UsageReadResult | null>(null);
  const [usageStatsLoading, setUsageStatsLoading] = useState(false);
  const [usageStatsError, setUsageStatsError] = useState<string | null>(null);
  const [attachments, setAttachments] = useState<PendingAttachment[]>([]);
  const [composerDraftPatch, setComposerDraftPatch] = useState<{
    id: number;
    text: string;
    inputParts?: ThreadEditableInputPart[];
  } | null>(null);
  const [voiceListening, setVoiceListening] = useState(false);
  const [voiceAutoSpeak, setVoiceAutoSpeak] = useState(false);
  const [voiceRealtimeSessionId, setVoiceRealtimeSessionId] = useState<string | null>(null);
  const [debugEvents, setDebugEvents] = useState<DebugEvent[]>([]);
  const [terminalEvents, setTerminalEvents] = useState<TerminalNotificationEvent[]>([]);
  const [latestGatewayEvent, setLatestGatewayEvent] = useState<GatewayThreadEventFeed>(EMPTY_GATEWAY_EVENT_FEED);
  const [traceState, setTraceState] = useState<TraceState>({
    error: null,
    loading: false,
    result: null,
    threadId: null
  });
  const initialPrefs = useMemo(readWorkbenchPrefs, []);
  const [appearance, setAppearance] = useState<Appearance>(initialPrefs.appearance);
  const [debugEnabled, setDebugEnabled] = useState(initialPrefs.debug);
  const [rightWidthPx, setRightWidthPx] = useState(initialPrefs.rightWidthPx);
  const [dirtyRightTabs, setDirtyRightTabs] = useState<Record<string, boolean>>({});
  const [status, setStatus] = useState("connecting");
  const [error, setError] = useState<string | null>(null);
  const [mobilePanel, setMobilePanel] = useState<"history" | "transcript" | "status">("transcript");
  const [fallbackCwd, setFallbackCwd] = useState(browserFallbackCwd);
  const viewEpochRef = useRef(0);
  const mainViewRef = useRef<MainView>("transcript");
  const selectedThreadIdRef = useRef<string | null>(null);
  const scopeRef = useRef<GatewayRequestScope | null>(null);
  const commandContextKeyRef = useRef<string | null>(null);
  const detachedShellTokenRef = useRef(0);
  const pendingDetachedShellRef = useRef<PendingDetachedShell | null>(null);
  const firstTurnContextRefreshPendingRef = useRef(false);
  const skipNextPinnedPersistRef = useRef(false);
  const voiceRecorderRef = useRef<VoiceRecorder | null>(null);
  const voiceAutoSpeakKeyRef = useRef<string | null>(null);
  const pendingTargetSelectionRef = useRef<string | null>(null);
  const runtimeTargetTransitionRef = useRef(false);
  const runtimeMutationSequenceRef = useRef(0);
  const olderHistoryLoadSequenceRef = useRef(0);
  const startupRetryRef = useRef(false);

  useEffect(() => {
    threadSession.attachClient(client);
    return () => threadSession.attachClient(null);
  }, [client, threadSession]);

  useEffect(() => {
    const scope = activeScope ?? init?.scope ?? null;
    sessionBrowserApplication.bind(client, scope);
    workspaceApplication.bind(client, scope);
  }, [
    activeScope,
    client,
    init?.scope,
    sessionBrowserApplication,
    workspaceApplication
  ]);

  useEffect(() => () => threadSession.dispose(), [threadSession]);

  function setRuntimeContext(context: ThreadContextReadResult | null): void {
    threadSession.setContext(context ? parseThreadContext(context) : null);
  }

  function setSnapshot(value: ThreadSnapshot | ((current: ThreadSnapshot) => ThreadSnapshot)) {
    const current = threadSession.getSnapshot() ?? EMPTY_SNAPSHOT;
    const next = typeof value === "function" ? value(current) : value;
    if (next !== current) threadSession.reset(next, threadSession.getContext());
  }

  const activity = normalizeActivity(snapshot.activity);
  const transcriptEntries = Array.isArray(snapshot.entries) ? snapshot.entries : [];
  const workspaceFileLinkDemand = useMemo(
    () => (
      transcriptMayContainWorkspaceFile(transcriptEntries)
      || transcriptMayContainWorkspaceFile(liveTranscriptEntries)
    ),
    [liveTranscriptEntries, transcriptEntries]
  );
  const pendingActions = Array.isArray(snapshot.pendingActions) ? snapshot.pendingActions : [];
  const pendingClarifyActions = pendingActions.filter((action) => action.kind === "clarify");
  const pendingPermissionActions = pendingActions.filter((action) => action.kind === "permission");
  const running = activity.running;
  const disabled = status !== "connected";
  const currentThreadId = snapshot.thread?.id;
  const visibleDraftSession = visibleHistoryDraftSession(draftSession, false);
  const hasSelectedSession = Boolean(currentThreadId || visibleDraftSession);
  const showSessionChrome = mainView === "transcript" && hasSelectedSession;
  const composerShellVisible = Boolean(
    showSessionChrome
      && client
      && (activeScope?.cwd ?? init?.scope.cwd ?? "").trim()
  );
  const commandContextKey = `${activeScope?.cwd ?? ""}:${currentThreadId ?? visibleDraftSession?.id ?? "none"}`;
  const activeWorkbenchCwd = activeScope?.cwd ?? init?.scope.cwd ?? settings?.cwd ?? fallbackCwd;
  const activeRightTab = rightTabs.find((tab) =>
    tab.id === activeRightTabId && rightWorkspaceTabVisibleForSession(tab, currentThreadId ?? null)
  ) ?? null;
  const pinnedSessions = useMemo(
    () => pinnedSessionIds
      .map((id) => sessions.find((session) => session.id === id))
      .filter((session): session is SessionSummary => Boolean(session)),
    [pinnedSessionIds, sessions]
  );
  const runtimeProfiles = runtimeContext?.profiles ?? [];
  const runtimeBinding = runtimeContext?.binding ?? null;
  const compatibleTargets = runtimeContext?.compatibleTargets ?? [];
  const runtimeControls = runtimeContext
    && runtimeContextTargetId === selectedTargetId
    && Boolean(selectedTargetId)
    ? runtimeContext.controls
    : [];
  const runtimeModeControl = runtimeControls.find((control) => control.surfaceRole === "mode") ?? null;
  const runtimeModeOption = useMemo(() => runtimeControlAsConfigOption(runtimeModeControl), [runtimeModeControl]);
  const runtimeContextScope = activeScope
    ?? init?.scope
    ?? scopeForCwd(settings?.cwd ?? fallbackCwd);
  const runtimeContextDraftTargetKey = currentThreadId || runtimeBinding ? "" : selectedTargetId;

  useEffect(() => {
    if (
      runtimeTargetTransitionRef.current
      || composerSessionCoordinator.isDraftOpenPending(viewEpochRef.current)
    ) {
      return;
    }
    if (!client || (!startupStable && !startupRetryRef.current)) {
      setRuntimeContext(null);
      return;
    }
    if (
      runtimeContextRefreshRevision === 0
      && runtimeContextDraftTargetKey
      && runtimeContextTargetId === runtimeContextDraftTargetKey
      && pendingTargetSelectionRef.current === null
    ) {
      return;
    }
    let cancelled = false;
    const requestedTarget = currentThreadId || runtimeBinding
      ? null
      : threadSession.contextReadTarget(pendingTargetSelectionRef.current ?? selectedTargetId);
    setRuntimeOptionsLoading(true);
    setRuntimeOptionsError(null);
    void client.request("thread/context/read", {
      threadId: currentThreadId ?? null,
      target: requestedTarget,
      scope: runtimeContextScope
    }).then((value) => {
      if (cancelled) return;
      const context = parseThreadContext(value);
      if (shouldRetainFirstTurnDraftContext(
        threadSession.getContext(),
        context,
        firstTurnContextRefreshPendingRef.current,
        currentThreadId
      )) {
        return;
      }
      setRuntimeContext(context);
      const pendingTarget = context.compatibleTargets.find((target) => (
        target.targetId === pendingTargetSelectionRef.current
      )) ?? null;
      const retainedTarget = context.compatibleTargets.find((target) => target.targetId === selectedTargetId) ?? null;
      const authoritativeTarget = context.compatibleTargets.find((target) => (
        target.targetId === context.selectedTargetId
      )) ?? null;
      const suggestedTarget = context.compatibleTargets.find((target) => (
        target.targetId === context.suggestedTargetId
      )) ?? null;
      const nextTarget = pendingTarget
        ?? (context.binding ? authoritativeTarget : retainedTarget)
        ?? authoritativeTarget
        ?? suggestedTarget
        ?? context.compatibleTargets[0]
        ?? null;
      setRuntimeContextTargetId(context.selectedTargetId ?? "");
      setSelectedTargetId(nextTarget?.targetId ?? "");
      if (context.binding) {
        pendingTargetSelectionRef.current = null;
        setRuntimeControlDrafts({});
      }
    }).catch((cause) => {
      if (cancelled) return;
      setRuntimeContext(null);
      setRuntimeContextTargetId("");
      setRuntimeOptionsError(cause instanceof Error ? cause.message : String(cause));
    }).finally(() => {
      if (!cancelled) setRuntimeOptionsLoading(false);
    });
    return () => {
      cancelled = true;
    };
  }, [
    client,
    composerSessionCoordinator,
    currentThreadId,
    runtimeContextDraftTargetKey,
    runtimeContextRefreshRevision,
    runtimeContextScope.cwd,
    runtimeContextScope.source.kind,
    runtimeContextScope.source.lifetime,
    runtimeContextScope.source.rawId,
    startupStable
  ]);

  const controls = settings?.controls ?? null;
  const contextSendable = runtimeContextTargetId === selectedTargetId
    && Boolean(selectedTargetId)
    && (runtimeContext?.sendability.allowed ?? false);
  const pendingDraftSendable = !currentThreadId
    && runtimeOptionsLoading
    && !runtimeOptionsError
    && composerSessionCoordinator.isReadinessPending(viewEpochRef.current);
  const turnSendable = pendingDraftSendable
    || (contextSendable && !runtimeOptionsLoading && !runtimeOptionsError);
  const turnBlockReason = runtimeContext?.sendability.reason
    ?? (runtimeOptionsError
      ? `Thread context unavailable: ${runtimeOptionsError}`
      : runtimeOptionsLoading
        ? "Loading Agent and Runtime Profile context."
        : "This Agent and Runtime Profile cannot start a turn.");

  useEffect(() => {
    if (
      status !== "connected"
      || currentThreadId
      || !startupStable
      || runtimeOptionsLoading
      || runtimeOptionsError
      || !contextSendable
    ) {
      return;
    }
    retainJourneyStateMark("psychevo:gui_ready");
    retainJourneyStateMark("psychevo:draft_context_ready");
  }, [contextSendable, currentThreadId, runtimeOptionsError, runtimeOptionsLoading, startupStable, status]);

  function mergeModelCatalogOptions(options: ModelOptionView[]) {
    if (options.length === 0) {
      return;
    }
    setSettings((current) => mergeModelCatalogOptionsIntoSettings(current, options));
  }

  async function refreshWorkbenchControls() {
    if (!client) {
      return;
    }
    await runAction(async () => {
      const nextSettings = SettingsReadResultSchema.parse(await client.request("settings/read", {
        threadId: currentThreadId ?? null,
        cwd: activeWorkbenchCwd
      }));
      setSettings(nextSettings);
      setRuntimeContextRefreshRevision((current) => current + 1);
    });
  }

  function updateMainView(value: MainView) {
    mainViewRef.current = value;
    setMainView(value);
  }
  const sessionUsage = observability?.usage ?? null;
  const surfaceActions = createSurfaceActions({
    activeScope,
    client,
    currentThreadId: currentThreadId ?? null,
    fallbackCwd,
    initScope: init?.scope ?? null,
    sessionBrowserApplication,
    scopeRef,
    selectedThreadIdRef,
    settings,
    snapshot,
    viewEpochRef,
    workspaceApplication,
    setActiveScope,
    setAgents,
    setBackends,
    setCommands,
    setContextUsage,
    setDebugEvents,
    setError,
    setObservability,
    setRuntimeOptionsError,
    setSettings,
    setSnapshot,
    setTraceState,
    onSnapshotAdopted: () => setStartupStable(true)
  });
  const {
    adoptSnapshotScope,
    pushDebugEvent,
    refreshAgentSurface,
    refreshCommands,
    refreshHistory,
    refreshObservability,
    refreshRevertedThreadSnapshot,
    refreshSettings,
    refreshSnapshot,
    refreshTrace,
    refreshWorkspaceChanges,
    refreshWorkspaceDiff,
    refreshWorkspaceFiles,
    refreshWorkspaceSurface,
    runAction
  } = surfaceActions;

  useEffect(() => {
    if (
      !startupStable
      || !client
      || !activeScope
      || !workspaceFileLinkDemand
      || workspaceFiles?.root === activeScope.cwd
    ) {
      return;
    }
    void workspaceApplication.ensure("files", client, activeScope);
  }, [
    startupStable,
    client,
    activeScope,
    workspaceFileLinkDemand,
    workspaceApplication,
    workspaceFiles?.root
  ]);

  async function refreshAgentSurfaceAndRuntimeContext(
    nextClient: GatewayClient | null = client,
    scope: GatewayRequestScope | undefined = activeScope ?? init?.scope ?? undefined
  ) {
    await refreshAgentSurface(nextClient, scope);
    setRuntimeContextRefreshRevision((current) => current + 1);
  }

  const automationModel = useAutomations({
    activeScope,
    activeWorkbenchCwd,
    client,
    fallbackCwd,
    initScope: init?.scope ?? null,
    mainView,
    settingsCwd: settings?.cwd,
    beginExplicitViewSwitch,
    refreshSnapshot,
    runAction,
    setMobilePanel,
    updateMainView
  });
  const {
    applyGatewayEvent,
    gatewayEventQueueRef,
    gatewayEventRafRef
  } = useGatewayLiveEvents({
    selectedThreadIdRef,
    setLatestGatewayEvent,
    threadSession
  });

  const {
    automations,
    automationsError,
    automationsLoading,
    deleteAutomation,
    draftAutomation,
    openAutomationThread,
    pauseAutomation,
    refreshAutomations,
    resumeAutomation,
    runAutomation,
    saveAutomation
  } = automationModel;

  async function loadOlderSessions(cwd: string) {
    await sessionBrowserApplication.loadOlder(client, {
      activeScope: activeScope ?? init?.scope ?? null,
      currentThreadId: currentThreadId ?? null,
      cwd
    });
  }

  async function loadOlderHistory() {
    if (olderHistoryLoading || !snapshot.history.cursor) {
      return;
    }
    const epoch = viewEpochRef.current;
    const sequence = olderHistoryLoadSequenceRef.current + 1;
    olderHistoryLoadSequenceRef.current = sequence;
    setOlderHistoryLoading(true);
    try {
      await threadSession.loadOlder();
    } finally {
      if (
        olderHistoryLoadSequenceRef.current === sequence
        && viewEpochRef.current === epoch
      ) {
        setOlderHistoryLoading(false);
      }
    }
  }

  async function refreshUsageStats(nextClient: GatewayClient | null = client) {
    if (!nextClient) {
      return;
    }
    setUsageStatsLoading(true);
    setUsageStatsError(null);
    try {
      const result = UsageReadResultSchema.parse(
        await nextClient.request("usage/read", { activityDays: 365 })
      );
      setUsageStats(result);
    } catch (error) {
      setUsageStatsError(error instanceof Error ? error.message : String(error));
    } finally {
      setUsageStatsLoading(false);
    }
  }

  useEffect(() => {
    if (!client || mainView !== "settings" || settingsSection !== "usage") {
      return;
    }
    void refreshUsageStats(client);
  }, [client, mainView, settingsSection]);

  useWorkbenchEffects({
    activeCommandOverlay,
    activeRightTabKind: activeRightTab?.kind ?? null,
    activeRightTabId,
    activeScope,
    appearance,
    client,
    composerSessionCoordinator,
    createRuntime: runtimeFactory,
    commandContextKey,
    commandContextKeyRef,
    commandFeedback,
    currentThreadId: currentThreadId ?? null,
    debugEnabled,
    dirtyRightTabs,
    draftSession,
    gatewayEventQueueRef,
    gatewayEventRafRef,
    host,
    initScope: init?.scope ?? null,
    mainView,
    mainViewRef,
    mobilePanel,
    pendingDetachedShellRef,
    firstTurnContextRefreshPendingRef,
    pinnedSessionIds,
    rightTabs,
    rightWorkspaceOpen: showSessionChrome && !rightCollapsed,
    rightWidthPx,
    runtimeTargetTransitionRef,
    scopeRef,
    selectedThreadIdRef,
    settingsSection,
    fallbackCwd,
    showSessionChrome,
    skipNextPinnedPersistRef,
    snapshot,
    startupStable,
    threadSession,
    viewEpochRef,
    workspaceApplication,
    adoptSnapshotScope,
    applyGatewayEvent,
    patchSessionEvent: (event: GatewayEvent) => {
      sessionBrowserApplication.patchGatewayEvent(event);
    },
    beginExplicitViewSwitch,
    clearCommandTransientUi,
    pushDebugEvent,
    refreshAgentSurface: refreshAgentSurfaceAndRuntimeContext,
    refreshCommands,
    refreshHistory,
    refreshObservability,
    refreshRuntimeContext: () => setRuntimeContextRefreshRevision((current) => current + 1),
    refreshSettings,
    refreshSnapshot,
    refreshTrace,
    refreshWorkspaceChanges,
    refreshWorkspaceDiff,
    refreshWorkspaceFiles,
    refreshWorkspaceSurface,
    setActiveRightTabId,
    setActiveScope,
    setClient,
    setCommandFeedback,
    setDraftSession,
    setEndpoint,
    setError,
    setFallbackCwd,
    setHost,
    setHistoryLoading,
    setInit,
    setMobilePanel,
    setPinnedSessionIds: sessionBrowserApplication.setPinnedSessionIds,
    setRightCollapsed,
    setRightTabs,
    setRuntimeContext,
    setRuntimeContextTargetId,
    setRuntimeOptionsError,
    setRuntimeOptionsLoading,
    setSelectedTargetId,
    setSnapshot,
    setStatus,
    setStartupStable,
    setTerminalEvents,
    setTraceState,
    updateMainView
  });

  function beginExplicitViewSwitch(): number {
    composerSessionCoordinator.cancelPending();
    runtimeMutationSequenceRef.current += 1;
    olderHistoryLoadSequenceRef.current += 1;
    setOlderHistoryLoading(false);
    runtimeTargetTransitionRef.current = false;
    pendingTargetSelectionRef.current = null;
    viewEpochRef.current += 1;
    pendingDetachedShellRef.current = null;
    clearCommandTransientUi();
    setDraftSession(null);
    selectedThreadIdRef.current = null;
    setObservability(null);
    setContextUsage(null);
    return viewEpochRef.current;
  }

  function togglePinnedSession(threadId: string) {
    sessionBrowserApplication.togglePinnedSession(threadId);
  }

  function patchComposerDraft(text: string, inputParts?: ThreadEditableInputPart[]) {
    setComposerDraftPatch((current) => ({
      id: (current?.id ?? 0) + 1,
      text,
      ...(inputParts ? { inputParts } : {})
    }));
  }

  function clearCommandTransientUi() {
    setCommandFeedback(null);
    setActiveCommandOverlay(null);
  }

  function switchMainView(value: MainView) {
    if (value === "transcript") {
      clearCommandTransientUi();
    } else {
      setActiveCommandOverlay(null);
    }
    updateMainView(value);
  }

  function openSettingsSection(section: SettingsSection) {
    setActiveCommandOverlay(null);
    setSettingsSection(section);
    updateMainView("settings");
    setMobilePanel("transcript");
  }

  function openCapabilitiesTab(tab: CapabilityTab = "skills") {
    setActiveCommandOverlay(null);
    setCapabilitiesTab(tab);
    updateMainView("capabilities");
    setMobilePanel("transcript");
  }

  function openCommandOverlay(kind: CommandOverlay) {
    setActiveCommandOverlay(kind);
    updateMainView("transcript");
    setMobilePanel("transcript");
  }

  function revealHistoryPanel() {
    setLeftCollapsed(false);
    setActiveCommandOverlay(null);
    setMobilePanel("history");
  }

  const rightWorkspaceActions = createRightWorkspaceActions({
    activeRightTabId,
    client,
    confirmAction,
    currentThreadId: currentThreadId ?? null,
    debugEnabled,
    dirtyRightTabs,
    rightTabs,
    rightWidthPx,
    scope: snapshot.scope,
    runAction,
    setActiveCommandOverlay,
    setActiveRightTabId,
    setDirtyRightTabs,
    setMobilePanel,
    setRightCollapsed,
    setRightTabs,
    setRightWidthPx,
    updateMainView
  });
  const {
    beginRightResize,
    clearRightWorkspaceTabPendingPrompt,
    closeRightWorkspaceTab,
    openAgentSessionTab,
    openReviewTab,
    openRightWorkspaceTab,
    revealRightWorkspace,
    togglePinnedMessage
  } = rightWorkspaceActions;

  const appActions = createAppActions({
    activeScope,
    attachments,
    client,
    composerSessionCoordinator,
    currentThreadId: currentThreadId ?? null,
    detachedShellTokenRef,
    fallbackCwd,
    host,
    initScope: init?.scope ?? null,
    isThreadArchived: (threadId: string) => archivedSessions.some((session) => session.id === threadId),
    pendingDetachedShellRef,
    firstTurnContextRefreshPendingRef,
    runtimeControls,
    runtimeControlDrafts,
    selectedTargetId,
    selectedThreadIdRef,
    settings,
    snapshot,
    viewEpochRef,
    workspaceApplication,
    turnBlockReason,
    adoptSnapshotScope,
    beginExplicitViewSwitch,
    clearCommandTransientUi,
    openReviewTab,
    openRightWorkspaceTab,
    refreshAgentSurface: refreshAgentSurfaceAndRuntimeContext,
    refreshHistory,
    refreshRuntimeContext: () => setRuntimeContextRefreshRevision((current) => current + 1),
    refreshSnapshot,
    refreshWorkspaceSurface,
    setAttachments,
    setBackendDoctor,
    setBackendDraft,
    setChannelDoctor,
    setCommandFeedback,
    setContextUsage,
    setDraftSession,
    setError,
    setMobilePanel,
    setObservability,
    setRightTabs,
    setRuntimeOptionsError,
    setRuntimeOptionsLoading,
    setRuntimeContext,
    setRuntimeContextTargetId,
    setSelectedTargetId,
    setSnapshot,
    setSettings,
    setTraceState,
    patchComposerDraft,
    threadSession,
    updateMainView
  });
  const {
    acceptWorkspaceChange,
    checkoutWorkspaceGitBranch,
    copyText,
    createWorkspace,
    deleteBackend,
    deleteChannel,
    doctorChannel,
    doctorBackend,
    doctorChannels,
    handleAttachment,
    handleAttachmentFiles,
    loadChannelSources,
    loadThreadSearchText,
    openDiffPreview,
    openFilePreview,
    rejectWorkspaceChange,
    readWorkspaceFolders,
    readWorkspaceGitBranches,
    saveBackendDraft,
    saveFileFromEditor,
    pollWechatQrSetup,
    startWechatQrSetup,
    startNewThread,
    startShell,
    submitTurn,
    updateChannel,
    updateBackendDraftFields,
    setChannelEnabled
  } = appActions;

  async function retryComposerStartup() {
    if (!client) {
      return;
    }
    setRuntimeOptionsError(null);
    setError(null);
    startupRetryRef.current = true;
    try {
      await startNewThread(undefined, {
        refreshHistory: false,
        rejectProblem: true
      });
      setStartupStable(true);
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      setRuntimeOptionsError(message);
    } finally {
      startupRetryRef.current = false;
    }
  }

  async function changeRunnableTarget(targetId: string) {
    const target = compatibleTargets.find((candidate) => candidate.targetId === targetId) ?? null;
    if (!target?.ready) {
      setCommandFeedback({
        accepted: false,
        command: "target",
        message: target?.unavailableReason ?? "The selected Agent target is not ready.",
        feedbackAnchor: "composer"
      });
      return;
    }
    const transitionEpoch = viewEpochRef.current;
    const transitionId = runtimeMutationSequenceRef.current + 1;
    runtimeMutationSequenceRef.current = transitionId;
    const ownsTransition = () => runtimeMutationSequenceRef.current === transitionId;
    const canApplyTransition = () => ownsTransition()
      && viewEpochRef.current === transitionEpoch;
    pendingTargetSelectionRef.current = targetId;
    runtimeTargetTransitionRef.current = true;
    setSelectedTargetId(targetId);
    setRuntimeControlDrafts({});
    setRuntimeOptionsLoading(true);
    setRuntimeOptionsError(null);
    if (runtimeBinding) {
      setRuntimeContext(
        runtimeContext
          ? { ...runtimeContext, binding: null, selectionState: "draft" }
          : null
      );
      setRuntimeContextTargetId("");
    }
    const scope = activeScope ?? init?.scope ?? scopeForCwd(settings?.cwd ?? fallbackCwd);
    try {
      if (runtimeBinding) {
        await startNewThread(undefined, {
          refreshHistory: false,
          targetId
        });
        if (ownsTransition()) {
          pendingTargetSelectionRef.current = null;
        }
        return;
      }
      if (!client) {
        return;
      }
      const preparationToken = composerSessionCoordinator.beginDraftPrepare(viewEpochRef.current);
      const result = await client.request("thread/draft/prepare", { scope, targetId });
      if (!canApplyTransition()) {
        return;
      }
      const context = parseThreadContext(result.context);
      setRuntimeContext(context);
      setRuntimeContextTargetId(context.selectedTargetId ?? "");
      pendingTargetSelectionRef.current = null;
      if (result.problem) {
        composerSessionCoordinator.failDraftPrepare(preparationToken);
        setRuntimeOptionsError(result.problem.message);
      } else {
        composerSessionCoordinator.completeDraftPrepare(preparationToken);
      }
    } catch (cause) {
      if (!canApplyTransition()) {
        return;
      }
      composerSessionCoordinator.cancelPending();
      pendingTargetSelectionRef.current = null;
      setRuntimeContextTargetId("");
      setRuntimeContext(null);
      setRuntimeOptionsError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      if (ownsTransition()) {
        runtimeTargetTransitionRef.current = false;
        if (viewEpochRef.current === transitionEpoch) {
          setRuntimeOptionsLoading(false);
        }
      }
    }
  }

  async function changeRuntimeControl(control: ThreadControlDescriptorView, value: unknown) {
    if (
      value !== undefined
      && control.enabled
      && control.mutability === "selectable"
      && client
      && runtimeContext
    ) {
      const mutationEpoch = viewEpochRef.current;
      const mutationId = runtimeMutationSequenceRef.current + 1;
      runtimeMutationSequenceRef.current = mutationId;
      const scope = activeScope ?? init?.scope ?? scopeForCwd(settings?.cwd ?? fallbackCwd);
      setRuntimeOptionsLoading(true);
      try {
        threadSession.setContext(runtimeContext);
        const result = await threadSession.setControl({
          control,
          scope,
          targetId: selectedTargetId,
          threadId: currentThreadId ?? null,
          value
        });
        if (
          runtimeMutationSequenceRef.current !== mutationId
          || viewEpochRef.current !== mutationEpoch
        ) {
          return;
        }
        setRuntimeContextTargetId(result.context.selectedTargetId ?? "");
        setRuntimeControlDrafts((current) => {
          const next = { ...current };
          delete next[control.id];
          return next;
        });
      } finally {
        if (
          runtimeMutationSequenceRef.current === mutationId
          && viewEpochRef.current === mutationEpoch
        ) {
          setRuntimeOptionsLoading(false);
        }
      }
      return;
    }
  }

  async function submitThreadTurn(
    threadId: string,
    text: string,
    mentions: GatewayMention[],
    displayText?: string | null,
    inputOverride?: ThreadEditableInputPart[]
  ): Promise<void> {
    const trimmed = text.trim();
    const input = inputOverride ?? (trimmed ? [{ type: "text" as const, text: trimmed }] : []);
    if (!client || input.length === 0) {
      return;
    }
    if (archivedSessions.some((session) => session.id === threadId)) {
      await client.request("thread/restore", { threadId });
      await Promise.all([refreshHistory(client), refreshHistory(client, true)]);
    }
    const selectedAtStart = snapshot.thread?.id === threadId;
    const targetSnapshot = selectedAtStart
      ? snapshot
      : normalizeSnapshot(parseThreadSnapshot(await client.request("thread/read", { threadId })));
    const targetScope = targetSnapshot.scope;
    const selectedContext = selectedAtStart ? threadSession.getContext() : null;
    const targetContext = selectedContext
      ? selectedContext
      : parseThreadContext(await client.request("thread/context/read", {
        threadId,
        target: null,
        scope: targetScope
      }));
    const targetSession = selectedAtStart
      ? threadSession
      : new ThreadSession({ client, snapshot: targetSnapshot });
    targetSession.setContext(targetContext);
    const binding = targetContext.binding;
    const turnControls = targetSession.turnControls(targetContext.selectedTargetId ?? "", {});
    const admission = targetSession.admitTurn({ controls: turnControls, input, mentions });
    if (!admission.allowed) {
      setCommandFeedback({
        accepted: false,
        command: "turn/start",
        message: admission.reason ?? "The destination Thread cannot accept this turn.",
        feedbackAnchor: "composer"
      });
      throw new Error(admission.reason ?? "The destination Thread cannot accept this turn.");
    }
    clearCommandTransientUi();
    const optimisticText = displayText?.trim()
      || trimmed
      || input.filter((part) => part.type === "image").map(() => "[Image]").join(" ");
    const turnEpoch = viewEpochRef.current;
    const outcome = await targetSession.send({
      controls: turnControls,
      input,
      mentions,
      optimisticText,
      scope: targetScope,
      threadId
    });
    if (outcome.status === "not_sent") {
      if (selectedAtStart && viewEpochRef.current === turnEpoch) {
        setRuntimeContextRefreshRevision((current) => current + 1);
      }
      if (!selectedAtStart) targetSession.dispose();
      throw outcome.error;
    }
    if (outcome.status === "cancelled") {
      if (!selectedAtStart) targetSession.dispose();
      return;
    }
    if (selectedAtStart && viewEpochRef.current !== turnEpoch) {
      await refreshHistory();
      return;
    }
    if (selectedAtStart) {
      selectedThreadIdRef.current = outcome.threadId;
      setRuntimeContextRefreshRevision((current) => current + 1);
    } else {
      targetSession.dispose();
    }
    await refreshHistory();
  }

  function activeVoiceScope(): GatewayRequestScope {
    return activeScope ?? init?.scope ?? scopeForCwd(settings?.cwd || fallbackCwd);
  }

  function setVoiceFeedback(accepted: boolean, command: string, message: string) {
    setCommandFeedback({
      accepted,
      command,
      message,
      feedbackAnchor: "composer"
    });
  }

  function toggleVoiceDictation() {
    void runAction(async () => {
      if (!client) {
        setVoiceFeedback(false, "voice/asr/transcribe", "Gateway is not connected.");
        return;
      }
      const activeRecorder = voiceRecorderRef.current;
      if (activeRecorder) {
        voiceRecorderRef.current = null;
        setVoiceListening(false);
        const recording = await activeRecorder.stop();
        if (recording.durationMs < 150) {
          setVoiceFeedback(false, "voice/asr/transcribe", "No voice input captured.");
          return;
        }
        const result = await client.request("voice/asr/transcribe", {
          scope: activeVoiceScope(),
          audio: {
            data: recording.data,
            format: recording.format,
            mimeType: recording.mimeType
          },
          provider: null,
          model: null,
          language: null
        });
        patchComposerDraft(result.transcript);
        return;
      }
      voiceRecorderRef.current = await startWavRecorder();
      setVoiceListening(true);
    });
  }

  function toggleVoiceAutoSpeak() {
    const next = !voiceAutoSpeak;
    setVoiceAutoSpeak(next);
    setVoiceFeedback(true, "voice/tts/synthesize", next ? "Auto-speak on." : "Auto-speak off.");
  }

  function readAloudText(text: string) {
    void runAction(async () => synthesizeVoiceText(text));
  }

  function toggleVoiceRealtime() {
    void runAction(async () => {
      if (!client) {
        setVoiceFeedback(false, "thread/realtime/start", "Gateway is not connected.");
        return;
      }
      if (voiceRealtimeSessionId) {
        const sessionId = voiceRealtimeSessionId;
        setVoiceRealtimeSessionId(null);
        await client.request("thread/realtime/stop", { sessionId });
        setVoiceFeedback(true, "thread/realtime/stop", "Realtime voice stopped.");
        return;
      }
      const threadId = currentThreadId;
      if (!threadId) {
        setVoiceFeedback(false, "thread/realtime/start", "Open a thread before starting realtime voice.");
        return;
      }
      const result = await client.request("thread/realtime/start", {
        threadId,
        scope: activeVoiceScope(),
        provider: null,
        model: null,
        transport: "webrtc",
        outputModality: "audio",
        voice: null,
        sdpOffer: null
      });
      setVoiceRealtimeSessionId(result.sessionId);
      setVoiceFeedback(true, "thread/realtime/start", "Realtime voice started.");
    });
  }

  async function synthesizeVoiceText(text: string) {
    const trimmed = text.trim();
    if (!client || !trimmed) {
      return;
    }
    const result = await client.request("voice/tts/synthesize", {
      scope: activeVoiceScope(),
      text: trimmed,
      provider: null,
      model: null,
      voice: null,
      format: "wav"
    });
    const AudioCtor = globalThis.Audio;
    if (!AudioCtor) {
      throw new Error("Audio playback is not available in this browser.");
    }
    const audio = new AudioCtor(`data:${result.audio.mimeType};base64,${result.audio.data}`);
    await audio.play();
  }

  useEffect(() => () => {
    voiceRecorderRef.current?.cancel();
    voiceRecorderRef.current = null;
  }, []);

  useEffect(() => {
    if (!voiceAutoSpeak || running || !client) {
      return;
    }
    const text = latestAssistantTranscriptText(liveTranscriptEntries)
      ?? latestAssistantTranscriptText(transcriptEntries);
    if (!text) {
      return;
    }
    const spokenKey = `${currentThreadId ?? "detached"}:${text}`;
    if (voiceAutoSpeakKeyRef.current === spokenKey) {
      return;
    }
    voiceAutoSpeakKeyRef.current = spokenKey;
    void runAction(async () => synthesizeVoiceText(text));
  }, [client, currentThreadId, liveTranscriptEntries, running, transcriptEntries, voiceAutoSpeak]);

  useEffect(() => {
    if (voiceRealtimeSessionId) {
      setVoiceRealtimeSessionId(null);
    }
  }, [currentThreadId]);

  const commandActions = createCommandActions({
    activeScope,
    activity,
    client,
    endpoint,
    fallbackCwd,
    host,
    initScope: init?.scope ?? null,
    pendingDetachedShellRef,
    runtimeModeOption,
    runtimeContext,
    settings,
    snapshot,
    viewEpochRef,
    workspaceApplication,
    workspaceDiff,
    beginExplicitViewSwitch,
    changeRuntimeMode: async (value) => {
      if (runtimeModeControl) {
        await changeRuntimeControl(runtimeModeControl, value);
      }
    },
    clearCommandTransientUi,
    handleAttachment,
    openCapabilitiesTab,
    openCommandOverlay,
    openReviewTab,
    openRightWorkspaceTab,
    patchComposerDraft,
    refreshHistory,
    refreshRevertedThreadSnapshot,
    refreshSnapshot,
    refreshWorkspaceSurface,
    revealHistoryPanel,
    revealRightWorkspace,
    setActiveCommandOverlay,
    setAttachments,
    setCommandFeedback,
    setDraftSession,
    setError,
    setMobilePanel,
    setSnapshot,
    startNewThread: async (cwd) => {
      await startNewThread(cwd);
    },
    submitThreadTurn,
    submitTurn,
    updateMainView
  });
  const { executeCommand, runCommandAlternateAction } = commandActions;

  const contextMatchesTarget = runtimeContextTargetId === selectedTargetId && Boolean(selectedTargetId);
  const inputCapabilities = contextMatchesTarget ? runtimeContext?.inputCapabilities ?? [] : [];
  const agentMentionsEnabled = inputCapabilities.some((capability) => (
    capability.kind === "agentMention" && capability.enabled
  ));
  const steerTurnId = confirmedSteerTurnId(
    latestGatewayEvent,
    snapshot.thread?.id ?? null,
    activity.activeTurnId ?? null
  );
  const steerAvailable = Boolean(steerTurnId)
    && contextMatchesTarget
    && enabledThreadAction(runtimeContext, "steer") !== null;
  const importScope = activeScope ?? init?.scope ?? scopeForCwd(activeWorkbenchCwd);
  const workbenchIntents = createWorkbenchIntentOwner({
    activeScope,
    agentMentionsEnabled,
    archivedSessions,
    beginExplicitViewSwitch,
    clearCommandTransientUi,
    client,
    currentThreadId: currentThreadId ?? null,
    fallbackCwd,
    importScope,
    initScope: init?.scope ?? null,
    patchComposerDraft,
    refreshHistory,
    refreshSnapshot,
    sessions,
    setAttachments,
    setCommandFeedback,
    setDraftSession,
    setMobilePanel,
    setSnapshot,
    settings,
    snapshot,
    startNewThread,
    steerAvailable,
    steerTurnId,
    submitThreadTurn,
    updateMainView,
    viewEpochRef
  });

  return (
    <WorkbenchLayout
      thread={{
        activeCommandOverlay,
        activeScope,
        activeWorkbenchCwd,
        activity,
        attachments,
        changeRunnableTarget,
        changeRuntimeControl,
        clearCommandTransientUi,
        client,
        commandFeedback,
        commands,
        composerDraftPatch,
        composerPresentationReady: startupStable,
        composerShellVisible,
        contextMatchesTarget,
        contextUsage,
        controls,
        currentThreadId,
        disabled,
        error,
        executeCommand,
        fallbackCwd,
        handleAttachment,
        handleAttachmentFiles,
        init,
        latestGatewayEvent,
        liveTranscriptEntries,
        loadOlderHistory,
        onComposerRetry: retryComposerStartup,
        onGatewayRetry: () => void client?.reconnectNow(),
        onReadAloudText: readAloudText,
        onVoiceAutoSpeakToggle: toggleVoiceAutoSpeak,
        onVoiceDictationToggle: toggleVoiceDictation,
        onVoiceRealtimeToggle: toggleVoiceRealtime,
        patchComposerDraft,
        olderHistoryLoading,
        pendingClarifyActions,
        pendingPermissionActions,
        refreshObservability,
        runAction,
        runCommandAlternateAction,
        running,
        runtimeContext,
        runtimeControlDrafts,
        runtimeControls,
        runtimeOptionsError,
        runtimeOptionsLoading,
        runtimeProfiles,
        selectedTargetId,
        sessionUsage,
        setAttachments,
        setCommandFeedback,
        settings,
        snapshot,
        startShell,
        status,
        submitTurn,
        transcriptEntries,
        turnBlockReason,
        turnSendable,
        voiceAutoSpeak,
        voiceListening,
        voiceRealtimeActive: Boolean(voiceRealtimeSessionId),
        workbenchIntents
      }}
      history={{
        archivedSessions,
        createWorkspace,
        endpoint,
        historyLoading,
        host,
        leftCollapsed,
        loadingOlderCwd,
        loadOlderSessions,
        pinnedSessionIds,
        pinnedSessions,
        refreshHistory,
        sessionBrowserWorkspaces,
        sessions,
        setDraftSession,
        setLeftCollapsed,
        setWorkspaceDialogOpen,
        startNewThread,
        switchMainView,
        togglePinnedSession,
        workspaceDialogOpen
      }}
      workspace={{
        acceptWorkspaceChange,
        activeRightTab,
        activeRightTabId,
        beginRightResize,
        checkoutWorkspaceGitBranch,
        clearRightWorkspaceTabPendingPrompt,
        closeRightWorkspaceTab,
        copyText,
        debugEnabled,
        debugEvents,
        openAgentSessionTab,
        openDiffPreview,
        openFilePreview,
        openRightWorkspaceTab,
        pinnedMessageKeys,
        readWorkspaceFolders,
        readWorkspaceGitBranches,
        refreshAgentSurface: refreshAgentSurfaceAndRuntimeContext,
        refreshSnapshot,
        refreshTrace,
        refreshWorkspaceSurface,
        rejectWorkspaceChange,
        revealRightWorkspace,
        rightCollapsed,
        rightTabs,
        rightWidthPx,
        saveFileFromEditor,
        setActiveRightTabId,
        setDirtyRightTabs,
        setError,
        setRightCollapsed,
        setRightTabs,
        setRightWidthPx,
        terminalEvents,
        traceState,
        togglePinnedMessage,
        workspaceBranch,
        workspaceIsGitRepo,
        workspaceChanges,
        workspaceDiff,
        workspaceFiles
      }}
      capabilities={{
        appearance,
        automations,
        automationsError,
        automationsLoading,
        backendDoctor,
        backendDraft,
        backends,
        capabilitiesTab,
        channelDoctor,
        deleteAutomation,
        deleteBackend,
        deleteChannel,
        doctorBackend,
        doctorChannel,
        doctorChannels,
        draftAutomation,
        loadChannelSources,
        loadThreadSearchText,
        mainView,
        mobilePanel,
        onModelAssignmentSaved: refreshWorkbenchControls,
        onModelCatalogLoaded: mergeModelCatalogOptions,
        openAutomationThread,
        openCapabilitiesTab,
        openSettingsSection,
        pauseAutomation,
        pollWechatQrSetup,
        refreshAutomations,
        refreshUsageStats,
        resumeAutomation,
        runAutomation,
        saveAutomation,
        saveBackendDraft,
        setAppearance,
        setBackendDraft,
        setCapabilitiesTab,
        setChannelEnabled,
        setDebugEnabled,
        setMobilePanel,
        setSettingsSection,
        settingsSection,
        showSessionChrome,
        startWechatQrSetup,
        updateBackendDraftFields,
        updateChannel,
        updateMainView,
        usageStats,
        usageStatsError,
        usageStatsLoading
      }}
    />
  );
}

function retainJourneyStateMark(name: string): void {
  if (typeof performance === "undefined") return;
  window.__psychevoJourneyTiming ??= {};
  if (window.__psychevoJourneyTiming[name]) return;
  const monotonicMs = performance.now();
  window.__psychevoJourneyTiming[name] = {
    epochMs: performance.timeOrigin + monotonicMs,
    monotonicMs
  };
  if (
    typeof performance.mark === "function"
    && performance.getEntriesByName(name, "mark").length === 0
  ) {
    performance.mark(name);
  }
}
