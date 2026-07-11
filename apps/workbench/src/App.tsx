import { useEffect, useMemo, useRef, useState } from "react";
import type { HistoryDraftSession } from "@psychevo/components";
import {
  acceptThreadTurn,
  bindThreadSnapshot,
  GatewayClient,
  latestAssistantTranscriptText,
  prepareThreadTurn,
  scopeForCwd,
  threadTurnStartParams
} from "@psychevo/client";
import type { GatewayEndpoint, PsychevoHost } from "@psychevo/host";
import {
  SettingsReadResultSchema,
  ThreadBrowserResultSchema,
  UsageReadResultSchema,
  type ContextReadResult,
  type GatewayMention,
  type GatewayRequestScope,
  type InitializeResult,
  type ModelOptionView,
  type ObservabilityReadResult,
  type RuntimeContextReadResult,
  type RuntimeControlDescriptorView,
  type RuntimeOptionsResult,
  type SessionSummary,
  type SettingsReadResult,
  type ThreadSnapshot,
  type UsageReadResult,
  type WorkspaceChangesResult,
  type WorkspaceDiffResult,
  type WorkspaceFilesResult
} from "@psychevo/protocol";
import {
  asRecord,
  optionalStringField,
  stringArray
} from "./data";
import { createCommandActions } from "./command-actions";
import { createAppActions } from "./app-actions";
import { useWorkbenchEffects } from "./app-effects";
import { useAutomations } from "./app-automations";
import { EMPTY_SNAPSHOT } from "./app-constants";
import { useGatewayLiveEvents } from "./app-live-events";
import {
  mergeModelCatalogOptionsIntoSettings,
  modelTurnBlockReasonForControls
} from "./app-model-state";
import {
  createSurfaceActions,
  sessionsFromThreadBrowser,
  workspacesFromThreadBrowser
} from "./surface-actions";
import { normalizeSnapshot } from "./session-utils";
import { WorkbenchLayout } from "./workbench-layout";
import {
  rightWorkspaceTabVisibleForSession
} from "./right-workspace";
import {
  agentOptionValue,
  isComposerRunnableAgent,
  projectRuntimeModeOption,
  resolvePeerRuntimeMode,
  runtimeControlAsConfigOption
} from "./runtime-controls";
import {
  agentPairingUnavailableReason,
  parseRuntimeContext,
  registerRuntimeContextChildTabs,
  runtimeControlDependencyMatches,
  runtimeControlSelections,
  runtimeOptionsWithModeFallback
} from "./runtime-context";
import {
  normalizeActivity,
} from "./session-utils";
import {
  readPinnedSessionIds,
  readWorkbenchPrefs
} from "./storage";
import { createRightWorkspaceActions } from "./right-workspace-actions";
import {
  EMPTY_GATEWAY_EVENT_FEED,
  type GatewayThreadEventFeed
} from "./gateway-event-feed";
import {
  browserFallbackCwd,
  createBrowserWorkbenchRuntime,
  type WorkbenchRuntimeFactory
} from "./runtime";
import { startWavRecorder, type VoiceRecorder } from "./voice-capture";
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
  SessionBrowserWorkspaceState,
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
  visibleHistoryDraftSession,
  type PendingDetachedShell
} from "./viewGuard";

function mergeSessionSummaries(current: SessionSummary[], incoming: SessionSummary[]): SessionSummary[] {
  const byId = new Map(current.map((session) => [session.id, session]));
  for (const session of incoming) {
    byId.set(session.id, session);
  }
  return Array.from(byId.values()).sort((left, right) => {
    const rightTime = right.updatedAtMs ?? right.startedAtMs ?? 0;
    const leftTime = left.updatedAtMs ?? left.startedAtMs ?? 0;
    return rightTime - leftTime || left.id.localeCompare(right.id);
  });
}

function mergeBrowserWorkspaces(
  current: SessionBrowserWorkspaceState[],
  incoming: SessionBrowserWorkspaceState[]
): SessionBrowserWorkspaceState[] {
  const byCwd = new Map(current.map((workspace) => [workspace.cwd, workspace]));
  for (const workspace of incoming) {
    byCwd.set(workspace.cwd, workspace);
  }
  return Array.from(byCwd.values());
}

export function App({ runtimeFactory = createBrowserWorkbenchRuntime }: { runtimeFactory?: WorkbenchRuntimeFactory } = {}) {
  const [client, setClient] = useState<GatewayClient | null>(null);
  const [host, setHost] = useState<PsychevoHost | null>(null);
  const [endpoint, setEndpoint] = useState<GatewayEndpoint | null>(null);
  const [init, setInit] = useState<InitializeResult | null>(null);
  const [activeScope, setActiveScope] = useState<GatewayRequestScope | null>(null);
  const [snapshot, setSnapshot] = useState<ThreadSnapshot>(EMPTY_SNAPSHOT);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [archivedSessions, setArchivedSessions] = useState<SessionSummary[]>([]);
  const [sessionBrowserWorkspaces, setSessionBrowserWorkspaces] = useState<SessionBrowserWorkspaceState[]>([]);
  const [loadingOlderCwd, setLoadingOlderCwd] = useState<string | null>(null);
  const [pinnedSessionIds, setPinnedSessionIds] = useState<string[]>(readPinnedSessionIds);
  const [draftSession, setDraftSession] = useState<HistoryDraftSession | null>(null);
  const [settings, setSettings] = useState<SettingsReadResult | undefined>();
  const [agents, setAgents] = useState<WorkbenchAgent[]>([]);
  const [backends, setBackends] = useState<WorkbenchBackend[]>([]);
  const [backendDraft, setBackendDraft] = useState<BackendDraft | null>(null);
  const [backendDoctor, setBackendDoctor] = useState<Record<string, WorkbenchBackendDoctor>>({});
  const [channelDoctor, setChannelDoctor] = useState<Record<string, WorkbenchChannelDoctor>>({});
  const [commands, setCommands] = useState<WorkbenchCommand[]>([]);
  const [rightTabs, setRightTabs] = useState<RightWorkspaceTab[]>([]);
  const [activeRightTabId, setActiveRightTabId] = useState<string | null>(null);
  const [mainView, setMainView] = useState<MainView>("transcript");
  const [capabilitiesTab, setCapabilitiesTab] = useState<CapabilityTab>("skills");
  const [settingsSection, setSettingsSection] = useState<SettingsSection>("appearance");
  const [leftCollapsed, setLeftCollapsed] = useState(false);
  const [rightCollapsed, setRightCollapsed] = useState(true);
  const [commandFeedback, setCommandFeedback] = useState<CommandFeedback>(null);
  const [activeCommandOverlay, setActiveCommandOverlay] = useState<CommandOverlay | null>(null);
  const [selectedAgentName, setSelectedAgentName] = useState<string>("");
  const [selectedRuntimeRef, setSelectedRuntimeRef] = useState<string>("native");
  const [runtimeContext, setRuntimeContext] = useState<RuntimeContextReadResult | null>(null);
  const [runtimeControlValues, setRuntimeControlValues] = useState<Record<string, unknown>>({});
  const [runtimeSessionId, setRuntimeSessionId] = useState<string | null>(null);
  const [, setRuntimeOptionsResult] = useState<RuntimeOptionsResult | null>(null);
  const [runtimeOptionsLoading, setRuntimeOptionsLoading] = useState(false);
  const [runtimeOptionsError, setRuntimeOptionsError] = useState<string | null>(null);
  const [selectedRuntimeMode, setSelectedRuntimeMode] = useState<string>("");
  const [permissionMode, setPermissionMode] = useState("default");
  const [workMode, setWorkMode] = useState("default");
  const [selectedModel, setSelectedModel] = useState<string | null>(null);
  const [selectedVariant, setSelectedVariant] = useState<string>("none");
  const [workspaceFiles, setWorkspaceFiles] = useState<WorkspaceFilesResult | null>(null);
  const [workspaceDialogOpen, setWorkspaceDialogOpen] = useState(false);
  const [workspaceDiff, setWorkspaceDiff] = useState<WorkspaceDiffResult | null>(null);
  const [workspaceChanges, setWorkspaceChanges] = useState<WorkspaceChangesResult | null>(null);
  const [contextUsage, setContextUsage] = useState<ContextReadResult | null>(null);
  const [observability, setObservability] = useState<ObservabilityReadResult | null>(null);
  const [usageStats, setUsageStats] = useState<UsageReadResult | null>(null);
  const [usageStatsLoading, setUsageStatsLoading] = useState(false);
  const [usageStatsError, setUsageStatsError] = useState<string | null>(null);
  const [attachments, setAttachments] = useState<PendingAttachment[]>([]);
  const [composerDraftPatch, setComposerDraftPatch] = useState<{ id: number; text: string } | null>(null);
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
  const skipNextPinnedPersistRef = useRef(false);
  const voiceRecorderRef = useRef<VoiceRecorder | null>(null);
  const voiceAutoSpeakKeyRef = useRef<string | null>(null);
  const pendingRuntimeSelectionRef = useRef<string | null>(null);

  const activity = normalizeActivity(snapshot.activity);
  const transcriptEntries = Array.isArray(snapshot.entries) ? snapshot.entries : [];
  const pendingActions = Array.isArray(snapshot.pendingActions) ? snapshot.pendingActions : [];
  const pendingClarifyActions = pendingActions.filter((action) => action.kind === "clarify");
  const pendingPermissionActions = pendingActions.filter((action) => action.kind === "permission");
  const running = activity.running;
  const disabled = status !== "connected";
  const currentThreadId = snapshot.thread?.id;
  const visibleDraftSession = visibleHistoryDraftSession(draftSession, false);
  const hasSelectedSession = Boolean(currentThreadId || visibleDraftSession);
  const showSessionChrome = mainView === "transcript" && hasSelectedSession;
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
  const runnableAgents = useMemo(
    () => agents.filter(isComposerRunnableAgent),
    [agents]
  );
  const runtimeProfiles = runtimeContext?.profiles ?? [];
  const runtimeBinding = runtimeContext?.binding ?? null;
  const selectedRuntimeProfile = runtimeProfiles.find((profile) => profile.id === selectedRuntimeRef) ?? null;
  const nativeRuntimeSelected = selectedRuntimeProfile?.runtime === "native"
    || (!selectedRuntimeProfile && selectedRuntimeRef === "native");
  const selectedAgent = runnableAgents.find((agent) => agentOptionValue(agent) === selectedAgentName) ?? null;
  const runtimeControls = runtimeContext?.runtimeRef === selectedRuntimeRef ? runtimeContext.controls : [];
  const runtimeModeControl = runtimeControls.find((control) => control.id === "mode") ?? null;
  const runtimeModeOption = useMemo(() => runtimeControlAsConfigOption(runtimeModeControl), [runtimeModeControl]);
  const runtimeModeProjection = useMemo(
    () => projectRuntimeModeOption(runtimeModeOption),
    [runtimeModeOption]
  );
  const selectedPeerRuntimeMode = runtimeBinding || nativeRuntimeSelected
    ? ""
    : resolvePeerRuntimeMode(runtimeModeProjection, workMode, selectedRuntimeMode);
  const planModeAvailable = !runtimeBinding && (selectedRuntimeProfile?.runtime === "native" || runtimeModeProjection.supportsPlan);
  const runtimeAcceptsAgentPersona = nativeRuntimeSelected;
  const agentPairingError = agentPairingUnavailableReason(selectedAgent, selectedRuntimeProfile);

  useEffect(() => {
    if (!client) {
      setRuntimeContext(null);
      return;
    }
    const scope = activeScope
      ?? init?.scope
      ?? scopeForCwd(settings?.cwd ?? fallbackCwd);
    let cancelled = false;
    setRuntimeOptionsLoading(true);
    setRuntimeOptionsError(null);
    void client.request("runtime/context/read", {
      threadId: currentThreadId ?? null,
      runtimeRef: runtimeBinding ? null : selectedRuntimeRef,
      scope
    }).then((value) => {
      if (cancelled) return;
      const context = parseRuntimeContext(value);
      setRuntimeContext(context);
      setRightTabs((current) => registerRuntimeContextChildTabs(current, context));
      const pendingRuntimeRef = pendingRuntimeSelectionRef.current;
      const nextRuntimeRef = context.binding?.runtimeRef
        ?? (pendingRuntimeRef && context.profiles.some((profile) => profile.id === pendingRuntimeRef)
          ? pendingRuntimeRef
          : context.runtimeRef);
      if (context.binding) pendingRuntimeSelectionRef.current = null;
      setSelectedRuntimeRef(nextRuntimeRef);
      setRuntimeSessionId(context.activeSession?.sessionHandle ?? context.binding?.sessionHandle ?? null);
      const nextControlValues = Object.fromEntries(
        context.controls
          .filter((control) => control.currentValue != null)
          .map((control) => [control.id, control.currentValue])
      );
      setRuntimeControlValues(nextControlValues);
      const modeOption = runtimeControlAsConfigOption(context.controls.find((control) => control.id === "mode") ?? null);
      setRuntimeOptionsResult({
        runtimeRef: context.runtimeRef,
        runtimeSessionId: context.activeSession?.sessionHandle ?? context.binding?.sessionHandle ?? null,
        options: modeOption ? [modeOption] : []
      });
      const projectedMode = projectRuntimeModeOption(modeOption);
      const currentMode = typeof context.controls.find((control) => control.id === "mode")?.currentValue === "string"
        ? String(context.controls.find((control) => control.id === "mode")?.currentValue)
        : "";
      if (currentMode === "plan" && projectedMode.supportsPlan) {
        setWorkMode("plan");
        setSelectedRuntimeMode("");
      } else {
        setSelectedRuntimeMode(
          currentMode && projectedMode.extraValues.some((choice) => choice.value === currentMode)
            ? currentMode
            : projectedMode.supportsPlan
              ? ""
              : projectedMode.defaultValue
        );
      }
    }).catch((cause) => {
      if (cancelled) return;
      setRuntimeOptionsError(cause instanceof Error ? cause.message : String(cause));
    }).finally(() => {
      if (!cancelled) setRuntimeOptionsLoading(false);
    });
    return () => {
      cancelled = true;
    };
  }, [activeScope, backends, client, currentThreadId, fallbackCwd, init?.scope, runtimeBinding?.runtimeRef, selectedRuntimeRef, settings?.cwd]);

  const controls = settings?.controls ?? null;
  const modelReady = !nativeRuntimeSelected || Boolean(selectedModel?.trim());
  const modelTurnBlockReason = modelTurnBlockReasonForControls(controls);

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
      const nextControls = nextSettings.controls;
      if (!nextControls) {
        return;
      }
      setSelectedModel(nextControls.model ?? null);
      setSelectedVariant(nextControls.variant ?? "none");
    });
  }

  function updateMainView(value: MainView) {
    mainViewRef.current = value;
    setMainView(value);
  }
  const sessionUsage = observability?.usage ?? null;
  const {
    adoptSnapshotScope,
    pushDebugEvent,
    refreshAgentSurface,
    refreshHistory,
    refreshRevertedThreadSnapshot,
    refreshSnapshot,
    refreshTrace,
    refreshWorkspaceSurface,
    runAction
  } = createSurfaceActions({
    activeScope,
    client,
    currentThreadId: currentThreadId ?? null,
    fallbackCwd,
    initScope: init?.scope ?? null,
    pinnedSessionIds,
    scopeRef,
    selectedThreadIdRef,
    settings,
    snapshot,
    viewEpochRef,
    setActiveScope,
    setAgents,
    setArchivedSessions,
    setBackends,
    setCommands,
    setContextUsage,
    setDebugEvents,
    setError,
    setObservability,
    setPermissionMode,
    setRuntimeOptionsError,
    setRuntimeOptionsResult,
    setRuntimeSessionId,
    setSelectedAgentName,
    setSelectedModel,
    setSelectedRuntimeMode,
    setSelectedRuntimeRef,
    setSelectedVariant,
    setSessions,
    setSessionBrowserWorkspaces,
    setSettings,
    setSnapshot,
    setTraceState,
    setWorkMode,
    setWorkspaceChanges,
    setWorkspaceDiff,
    setWorkspaceFiles
  });

  const {
    applyGatewayEvent,
    gatewayEventQueueRef,
    gatewayEventRafRef,
    scheduleSnapshotRefreshAfterLiveSettle
  } = useGatewayLiveEvents({
    refreshSnapshot,
    selectedThreadIdRef,
    setLatestGatewayEvent,
    setSnapshot,
    viewEpochRef
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
  } = useAutomations({
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

  async function loadOlderSessions(cwd: string) {
    if (!client) {
      return;
    }
    const cursor = sessionBrowserWorkspaces.find((workspace) => workspace.cwd === cwd)?.nextCursor;
    if (!cursor || loadingOlderCwd) {
      return;
    }
    setLoadingOlderCwd(cwd);
    try {
      const result = ThreadBrowserResultSchema.parse(
        await client.request("thread/browser", {
          archived: false,
          cursor,
          includeSessionIds: [currentThreadId ?? null, ...pinnedSessionIds].filter((id): id is string => Boolean(id)),
          limit: 20,
          recentDays: 7,
          cwd
        })
      );
      setSessions((current) => mergeSessionSummaries(current, sessionsFromThreadBrowser(result)));
      setSessionBrowserWorkspaces((current) => mergeBrowserWorkspaces(current, workspacesFromThreadBrowser(result)));
    } finally {
      setLoadingOlderCwd(null);
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
    activeRightTabKind: activeRightTab?.kind ?? null,
    activeRightTabId,
    activeScope,
    appearance,
    client,
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
    pinnedSessionIds,
    rightTabs,
    rightWidthPx,
    runnableAgents,
    runtimeModeOption,
    runtimeModeProjection,
    scopeRef,
    selectedAgentName,
    selectedRuntimeRef,
    selectedThreadIdRef,
    settingsSection,
    fallbackCwd,
    showSessionChrome,
    skipNextPinnedPersistRef,
    snapshot,
    viewEpochRef,
    workMode,
    adoptSnapshotScope,
    applyGatewayEvent,
    beginExplicitViewSwitch,
    clearCommandTransientUi,
    pushDebugEvent,
    refreshAgentSurface,
    refreshHistory,
    refreshSnapshot,
    refreshTrace,
    refreshWorkspaceSurface,
    scheduleSnapshotRefreshAfterLiveSettle,
    setActiveRightTabId,
    setActiveScope,
    setClient,
    setCommandFeedback,
    setDraftSession,
    setEndpoint,
    setError,
    setFallbackCwd,
    setHost,
    setInit,
    setMobilePanel,
    setPinnedSessionIds,
    setRightCollapsed,
    setRightTabs,
    setSelectedAgentName,
    setSnapshot,
    setStatus,
    setTerminalEvents,
    setTraceState,
    setWorkMode,
    updateMainView
  });

  function beginExplicitViewSwitch(): number {
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
    setPinnedSessionIds((current) => (
      current.includes(threadId)
        ? current.filter((id) => id !== threadId)
        : [threadId, ...current]
    ));
  }

  function patchComposerDraft(text: string) {
    setComposerDraftPatch((current) => ({
      id: (current?.id ?? 0) + 1,
      text
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

  const {
    beginRightResize,
    clearRightWorkspaceTabPendingPrompt,
    closeRightWorkspaceTab,
    openAgentSessionTab,
    openReviewTab,
    openRightWorkspaceTab,
    revealRightWorkspace
  } = createRightWorkspaceActions({
    activeRightTabId,
    client,
    currentThreadId: currentThreadId ?? null,
    debugEnabled,
    dirtyRightTabs,
    rightTabs,
    rightWidthPx,
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
    acceptWorkspaceChange,
    changeAgentSelection: changeAgentSelectionInPlace,
    copyText,
    createWorkspace,
    deleteArchivedSession,
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
    restoreArchivedSession,
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
  } = createAppActions({
    activeScope,
    attachments,
    client,
    currentThreadId: currentThreadId ?? null,
    detachedShellTokenRef,
    fallbackCwd,
    host,
    initScope: init?.scope ?? null,
    pendingDetachedShellRef,
    permissionMode,
    runtimeAcceptsAgentPersona,
    agentPairingError,
    runtimeControls,
    runtimeControlValues,
    runtimeOptionsError,
    runtimeSessionId,
    selectedAgentName,
    selectedModel,
    selectedPeerRuntimeMode,
    selectedRuntimeRef,
    selectedThreadIdRef,
    selectedVariant,
    settings,
    snapshot,
    viewEpochRef,
    workMode,
    modelReady,
    modelTurnBlockReason,
    adoptSnapshotScope,
    beginExplicitViewSwitch,
    clearCommandTransientUi,
    openReviewTab,
    openRightWorkspaceTab,
    refreshAgentSurface,
    refreshHistory,
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
    setRuntimeOptionsResult,
    setRuntimeSessionId,
    setSelectedAgentName,
    setSelectedRuntimeMode,
    setSelectedRuntimeRef,
    setSnapshot,
    setSettings,
    setTraceState,
    setWorkspaceChanges,
    setWorkspaceDiff,
    setWorkMode,
    updateMainView
  });

  async function changeAgentSelection(value: string) {
    if (value === selectedAgentName) return;
    if (runtimeBinding?.backendKind === "runtime") {
      const runtimeRef = runtimeBinding.runtimeRef;
      pendingRuntimeSelectionRef.current = runtimeRef;
      await startNewThread();
      setSelectedRuntimeRef(runtimeRef);
      setRuntimeSessionId(null);
      setRuntimeControlValues({});
      setRuntimeOptionsResult(null);
      setRuntimeOptionsError(null);
      setSelectedRuntimeMode("");
      setWorkMode("default");
      setSelectedAgentName(value);
      setRuntimeContext((current) => current ? {
        ...current,
        runtimeRef,
        selectionState: "default",
        binding: null,
        controls: [],
        activeSession: null
      } : current);
      return;
    }
    await changeAgentSelectionInPlace(value);
  }

  async function changeRuntimeProfile(runtimeRef: string) {
    if (runtimeRef === selectedRuntimeRef && !runtimeBinding) return;
    pendingRuntimeSelectionRef.current = runtimeRef;
    if (runtimeBinding) {
      await startNewThread();
    }
    setSelectedRuntimeRef(runtimeRef);
    setRuntimeSessionId(null);
    setRuntimeControlValues({});
    setRuntimeOptionsResult(null);
    setRuntimeOptionsError(null);
    setSelectedRuntimeMode("");
    setWorkMode("default");
    setRuntimeContext((current) => current ? {
      ...current,
      runtimeRef,
      selectionState: "default",
      binding: null,
      controls: [],
      activeSession: null
    } : current);
  }

  async function changeRuntimeControl(control: RuntimeControlDescriptorView, value: unknown) {
    if (value !== undefined && runtimeBinding && control.state === "selectable" && client) {
      const scope = activeScope ?? init?.scope ?? scopeForCwd(settings?.cwd ?? fallbackCwd);
      const result = await client.request("runtime/control/set", {
        runtimeRef: selectedRuntimeRef,
        controlId: control.id,
        value,
        expectedCapabilityRevision: control.capabilityRevision,
        expectedBindingRevision: runtimeBinding.bindingRevision,
        scope
      });
      setRuntimeContext((current) => current ? {
        ...current,
        controls: current.controls.map((item) => item.id === result.control.id ? result.control : item),
        binding: current.binding ? { ...current.binding, bindingRevision: result.bindingRevision } : null
      } : current);
    }
    setRuntimeControlValues((current) => {
      const next = { ...current };
      if (value !== undefined) next[control.id] = value;
      else delete next[control.id];
      for (const candidate of runtimeControls) {
        if (
          candidate.dependsOn?.controlId === control.id
          && !runtimeControlDependencyMatches(candidate, runtimeControls, next)
        ) {
          delete next[candidate.id];
        }
      }
      return next;
    });
    if (control.id === "mode" && typeof value === "string") {
      if (value === "plan") {
        setWorkMode("plan");
        setSelectedRuntimeMode("");
      } else {
        setWorkMode("default");
        setSelectedRuntimeMode(value);
      }
    } else if (control.id === "mode" && value === undefined) {
      setWorkMode("default");
      setSelectedRuntimeMode("");
    }
  }

  async function submitThreadTurn(
    threadId: string,
    text: string,
    mentions: GatewayMention[],
    displayText?: string | null
  ) {
    const trimmed = text.trim();
    if (!client || !trimmed) {
      return;
    }
    if (!modelReady) {
      setCommandFeedback({
        accepted: false,
        command: "model",
        message: modelTurnBlockReason,
        feedbackAnchor: "composer"
      });
      return;
    }
    if (agentPairingError) {
      setCommandFeedback({
        accepted: false,
        command: "agent",
        message: agentPairingError,
        feedbackAnchor: "composer"
      });
      return;
    }
    const submittedMentions = runtimeAcceptsAgentPersona
      ? mentions
      : mentions.filter((mention) => mention.target.kind !== "agent");
    if (!nativeRuntimeSelected && runtimeOptionsError) {
      setCommandFeedback({
        accepted: false,
        command: selectedRuntimeRef,
        message: `Unable to load ${selectedRuntimeRef} runtime options: ${runtimeOptionsError}`,
        feedbackAnchor: "composer"
      });
      return;
    }
    const runtimeOptions = nativeRuntimeSelected
      ? {}
      : runtimeOptionsWithModeFallback(
        runtimeControlSelections(runtimeControls, runtimeControlValues),
        selectedPeerRuntimeMode
      );
    clearCommandTransientUi();
    const optimisticText = displayText?.trim() || trimmed;
    const prepared = prepareThreadTurn(snapshot, optimisticText, threadId);
    setSnapshot(prepared.snapshot);
    const result = await client.request("turn/start", threadTurnStartParams({
      controls: {
        agentName: selectedAgentName || null,
        mode: nativeRuntimeSelected ? workMode : null,
        model: nativeRuntimeSelected ? selectedModel : null,
        permissionMode: nativeRuntimeSelected ? permissionMode : null,
        reasoningEffort: nativeRuntimeSelected && selectedVariant !== "none"
          ? selectedVariant
          : null,
        runtimeOptions,
        runtimeRef: selectedRuntimeRef,
        runtimeSessionId
      },
      input: [{ type: "text", text: trimmed }],
      mentions: submittedMentions,
      scope: activeScope ?? init?.scope ?? scopeForCwd(settings?.cwd || fallbackCwd),
      threadId: prepared.requestedThreadId,
      text: null
    }));
    const accepted = acceptThreadTurn(prepared.snapshot, result, prepared.requestedThreadId);
    selectedThreadIdRef.current = accepted.threadId;
    setSnapshot((current) => {
      const currentThreadId = current.thread?.id ?? null;
      if (currentThreadId && currentThreadId !== accepted.threadId) {
        return current;
      }
      return normalizeSnapshot(bindThreadSnapshot(current, accepted.threadId));
    });
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
    const text = latestAssistantTranscriptText(transcriptEntries);
    if (!text) {
      return;
    }
    const spokenKey = `${currentThreadId ?? "detached"}:${text}`;
    if (voiceAutoSpeakKeyRef.current === spokenKey) {
      return;
    }
    voiceAutoSpeakKeyRef.current = spokenKey;
    void runAction(async () => synthesizeVoiceText(text));
  }, [client, currentThreadId, running, transcriptEntries, voiceAutoSpeak]);

  useEffect(() => {
    if (voiceRealtimeSessionId) {
      setVoiceRealtimeSessionId(null);
    }
  }, [currentThreadId]);

  const { executeCommand, runCommandAlternateAction } = createCommandActions({
    activeScope,
    activity,
    client,
    endpoint,
    fallbackCwd,
    host,
    initScope: init?.scope ?? null,
    pendingDetachedShellRef,
    runtimeModeOption,
    runtimeOptionsError,
    runtimeSessionId,
    selectedRuntimeMode,
    selectedRuntimeRef,
    settings,
    snapshot,
    viewEpochRef,
    workMode,
    workspaceDiff,
    beginExplicitViewSwitch,
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
    setRuntimeOptionsError,
    setRuntimeOptionsResult,
    setRuntimeSessionId,
    setSelectedRuntimeMode,
    setSnapshot,
    setWorkMode,
    setWorkspaceDiff,
    startNewThread,
    submitThreadTurn,
    submitTurn,
    updateMainView
  });

  function persistComposerModelState(nextModel: string | null, nextVariant: string) {
    const model = nextModel?.trim();
    if (!client || !model) {
      return;
    }
    void runAction(async () => {
      const result = asRecord(await client.request("model/state/set", {
        cwd: activeWorkbenchCwd,
        threadId: currentThreadId ?? null,
        model,
        reasoningEffort: nextVariant === "none" ? null : nextVariant
      }));
      const resultModel = optionalStringField(result.model) ?? model;
      const resultVariant = optionalStringField(result.reasoningEffort) ?? "none";
      const recentModels = stringArray(result.recentModels);
      setSettings((current) => {
        if (!current?.controls) {
          return current;
        }
        return {
          ...current,
          controls: {
            ...current.controls,
            model: resultModel,
            variant: resultVariant,
            recentModels: recentModels.length > 0 ? recentModels : current.controls.recentModels
          }
        };
      });
    });
  }

  function changeComposerModelSelection(nextModel: string | null, nextVariant: string) {
    setSelectedModel(nextModel);
    setSelectedVariant(nextVariant);
    persistComposerModelState(nextModel, nextVariant);
  }

  function changeComposerModel(nextModel: string | null) {
    setSelectedModel(nextModel);
    persistComposerModelState(nextModel, selectedVariant);
  }

  function changeComposerVariant(nextVariant: string) {
    setSelectedVariant(nextVariant);
    persistComposerModelState(selectedModel, nextVariant);
  }

  return <WorkbenchLayout {...{
    acceptWorkspaceChange, activeCommandOverlay, activeRightTab, activeRightTabId, activeScope, activeWorkbenchCwd,
    automations, automationsError, automationsLoading,
    activity, appearance, archivedSessions, attachments, backendDoctor, backendDraft, backends, beginExplicitViewSwitch, capabilitiesTab,
    beginRightResize, changeAgentSelection, changeRuntimeControl, changeRuntimeProfile, clearCommandTransientUi, client, closeRightWorkspaceTab, commandFeedback,
    channelDoctor, commands, composerDraftPatch, contextUsage, controls, copyText, createWorkspace, currentThreadId,
    debugEnabled, debugEvents, deleteArchivedSession, deleteBackend, deleteChannel, disabled, doctorBackend, doctorChannel, doctorChannels, endpoint, error,
    executeCommand, handleAttachment, handleAttachmentFiles, host, init, latestGatewayEvent, leftCollapsed, loadChannelSources, loadThreadSearchText,
    loadingOlderCwd, loadOlderSessions, mainView, mobilePanel, openCapabilitiesTab, openDiffPreview, openAgentSessionTab, openFilePreview, openRightWorkspaceTab, openSettingsSection,
    openAutomationThread,
    onModelAssignmentSaved: refreshWorkbenchControls, onModelCatalogLoaded: mergeModelCatalogOptions,
    pendingClarifyActions, pendingPermissionActions, permissionMode, pinnedSessionIds, pinnedSessions, planModeAvailable, pollWechatQrSetup,
    refreshAgentSurface, refreshHistory, refreshSnapshot, refreshTrace, refreshWorkspaceSurface, rejectWorkspaceChange,
    restoreArchivedSession, revealRightWorkspace, rightCollapsed, rightTabs, rightWidthPx, runnableAgents, runAction,
    deleteAutomation, draftAutomation, pauseAutomation, refreshAutomations, resumeAutomation, runAutomation, saveAutomation,
    runCommandAlternateAction, running, runtimeAcceptsAgentPersona, runtimeContext, runtimeControlValues, runtimeOptionsError, runtimeOptionsLoading, runtimeProfiles,
    saveBackendDraft, saveFileFromEditor, selectedAgentName, selectedModel,
    selectedRuntimeRef, selectedVariant, nativeRuntimeSelected, modelReady, modelTurnBlockReason, sessionBrowserWorkspaces, sessionUsage, sessions, setActiveRightTabId, setAppearance,
    setAttachments, setBackendDraft, setCapabilitiesTab, setChannelEnabled, setDebugEnabled, setDirtyRightTabs, setDraftSession, setLeftCollapsed, setMainView,
    setMobilePanel, setCommandFeedback, setPermissionMode, setRightCollapsed, setRightTabs, setRightWidthPx,
    setSelectedModel: changeComposerModel, setSelectedModelSelection: changeComposerModelSelection,
    setSelectedRuntimeMode, setSelectedVariant: changeComposerVariant, setSettingsSection, setSnapshot, setWorkMode, setWorkspaceDialogOpen, fallbackCwd, settings, settingsSection,
    usageStats, usageStatsError, usageStatsLoading, refreshUsageStats,
    clearRightWorkspaceTabPendingPrompt, showSessionChrome, snapshot, startNewThread, startShell, startWechatQrSetup, status, submitTurn, submitThreadTurn, switchMainView, terminalEvents,
    togglePinnedSession, traceState, transcriptEntries, updateBackendDraftFields, updateChannel, updateMainView, viewEpochRef, workMode,
    voiceAutoSpeak, voiceListening, voiceRealtimeActive: Boolean(voiceRealtimeSessionId),
    onReadAloudText: readAloudText, onVoiceAutoSpeakToggle: toggleVoiceAutoSpeak,
    onVoiceDictationToggle: toggleVoiceDictation, onVoiceRealtimeToggle: toggleVoiceRealtime,
    workspaceChanges, workspaceDialogOpen, workspaceDiff, workspaceFiles
  }} />;
}
