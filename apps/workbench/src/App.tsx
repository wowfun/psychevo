import { useEffect, useMemo, useRef, useState } from "react";
import type { HistoryDraftSession } from "@psychevo/components";
import {
  GatewayClient,
  latestAssistantTranscriptText,
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
import { WorkbenchLayout } from "./workbench-layout";
import {
  rightWorkspaceTabVisibleForSession
} from "./right-workspace";
import {
  isComposerRunnableAgent,
  isComposerRuntimeBackend,
  isRuntimeModeOption,
  projectRuntimeModeOption,
  resolvePeerRuntimeMode,
  runtimeSupportsAgentPersona
} from "./runtime-controls";
import {
  normalizeActivity,
} from "./session-utils";
import {
  readPinnedSessionIds,
  readWorkbenchPrefs
} from "./storage";
import { createRightWorkspaceActions } from "./right-workspace-actions";
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
  GatewayEventFeed,
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
  const [settingsSection, setSettingsSection] = useState<SettingsSection>("appearance");
  const [leftCollapsed, setLeftCollapsed] = useState(false);
  const [rightCollapsed, setRightCollapsed] = useState(true);
  const [commandFeedback, setCommandFeedback] = useState<CommandFeedback>(null);
  const [activeCommandOverlay, setActiveCommandOverlay] = useState<CommandOverlay | null>(null);
  const [selectedAgentName, setSelectedAgentName] = useState<string>("");
  const [selectedRuntimeRef, setSelectedRuntimeRef] = useState<string>("native");
  const [runtimeSessionId, setRuntimeSessionId] = useState<string | null>(null);
  const [runtimeOptionsResult, setRuntimeOptionsResult] = useState<RuntimeOptionsResult | null>(null);
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
  const [latestGatewayEvent, setLatestGatewayEvent] = useState<GatewayEventFeed | null>(null);
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
  const runtimeBackends = useMemo(
    () => backends.filter(isComposerRuntimeBackend),
    [backends]
  );
  const selectedRuntimeBackend = selectedRuntimeRef === "native"
    ? null
    : runtimeBackends.find((backend) => backend.id === selectedRuntimeRef) ?? null;
  const runtimeModeOption = useMemo(
    () => runtimeOptionsResult?.options.find(isRuntimeModeOption) ?? null,
    [runtimeOptionsResult]
  );
  const runtimeModeValues = runtimeModeOption?.values ?? [];
  const runtimeModeProjection = useMemo(
    () => projectRuntimeModeOption(runtimeModeOption),
    [runtimeModeOption]
  );
  const extraRuntimeModeValues = runtimeModeProjection.extraValues;
  const selectedPeerRuntimeMode = selectedRuntimeRef === "native"
    ? ""
    : resolvePeerRuntimeMode(runtimeModeProjection, workMode, selectedRuntimeMode);
  const planModeAvailable = selectedRuntimeRef === "native" || runtimeModeProjection.supportsPlan;
  const runtimeModeUnavailable = selectedRuntimeRef !== "native"
    && !runtimeOptionsLoading
    && !runtimeOptionsError
    && runtimeModeOption
    && runtimeModeValues.length === 0;
  const runtimeAcceptsAgentPersona = runtimeSupportsAgentPersona(selectedRuntimeRef);

  const controls = settings?.controls ?? null;
  const modelReady = Boolean(selectedModel?.trim());
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
    runtimeBackends,
    runtimeModeOption,
    runtimeModeProjection,
    runtimeSessionId,
    scopeRef,
    selectedAgentName,
    selectedRuntimeRef,
    selectedThreadIdRef,
    settingsSection,
    settingsCwd: settings?.cwd,
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
    setRightTabs,
    setRuntimeOptionsError,
    setRuntimeOptionsLoading,
    setRuntimeOptionsResult,
    setRuntimeSessionId,
    setSelectedAgentName,
    setSelectedRuntimeMode,
    setSelectedRuntimeRef,
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
    changeAgentSelection,
    copyText,
    createWorkspace,
    deleteArchivedSession,
    deleteBackend,
    deleteChannel,
    doctorChannel,
    doctorBackend,
    doctorChannels,
    handleAttachment,
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

  async function submitThreadTurn(threadId: string, text: string, mentions: GatewayMention[]) {
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
    const submittedMentions = runtimeAcceptsAgentPersona
      ? mentions
      : mentions.filter((mention) => mention.target.kind !== "agent");
    if (selectedRuntimeRef !== "native" && runtimeOptionsError) {
      setCommandFeedback({
        accepted: false,
        command: selectedRuntimeRef,
        message: `Unable to load ${selectedRuntimeRef} runtime options: ${runtimeOptionsError}`,
        feedbackAnchor: "composer"
      });
      return;
    }
    const runtimeOptions = selectedRuntimeRef !== "native" && selectedPeerRuntimeMode
      ? { mode: selectedPeerRuntimeMode }
      : {};
    clearCommandTransientUi();
    await client.request("turn/start", threadTurnStartParams({
      controls: {
        agentName: runtimeAcceptsAgentPersona ? selectedAgentName || null : null,
        mode: selectedRuntimeRef === "native" ? workMode : null,
        model: selectedModel,
        permissionMode,
        reasoningEffort: selectedVariant === "none" ? null : selectedVariant,
        runtimeOptions,
        runtimeRef: selectedRuntimeRef,
        runtimeSessionId
      },
      input: [{ type: "text", text: trimmed }],
      mentions: submittedMentions,
      scope: activeScope ?? init?.scope ?? scopeForCwd(settings?.cwd || fallbackCwd),
      threadId,
      text: null
    }));
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
        setVoiceFeedback(true, "voice/asr/transcribe", "Dictation inserted.");
        return;
      }
      voiceRecorderRef.current = await startWavRecorder();
      setVoiceListening(true);
      setVoiceFeedback(true, "voice/asr/transcribe", "Listening.");
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
    activity, appearance, archivedSessions, attachments, backendDoctor, backendDraft, backends, beginExplicitViewSwitch,
    beginRightResize, changeAgentSelection, clearCommandTransientUi, client, closeRightWorkspaceTab, commandFeedback,
    channelDoctor, commands, composerDraftPatch, contextUsage, controls, copyText, createWorkspace, currentThreadId,
    debugEnabled, debugEvents, deleteArchivedSession, deleteBackend, deleteChannel, disabled, doctorBackend, doctorChannel, doctorChannels, endpoint, error,
    executeCommand, extraRuntimeModeValues, handleAttachment, host, init, latestGatewayEvent, leftCollapsed, loadChannelSources, loadThreadSearchText,
    loadingOlderCwd, loadOlderSessions, mainView, mobilePanel, openDiffPreview, openAgentSessionTab, openFilePreview, openRightWorkspaceTab, openSettingsSection,
    openAutomationThread,
    onModelAssignmentSaved: refreshWorkbenchControls, onModelCatalogLoaded: mergeModelCatalogOptions,
    pendingClarifyActions, pendingPermissionActions, permissionMode, pinnedSessionIds, pinnedSessions, planModeAvailable, pollWechatQrSetup,
    refreshAgentSurface, refreshHistory, refreshSnapshot, refreshTrace, refreshWorkspaceSurface, rejectWorkspaceChange,
    restoreArchivedSession, revealRightWorkspace, rightCollapsed, rightTabs, rightWidthPx, runnableAgents, runAction,
    deleteAutomation, draftAutomation, pauseAutomation, refreshAutomations, resumeAutomation, runAutomation, saveAutomation,
    runCommandAlternateAction, running, runtimeAcceptsAgentPersona, runtimeBackends, runtimeModeOption,
    runtimeModeUnavailable, runtimeOptionsError, saveBackendDraft, saveFileFromEditor, selectedAgentName, selectedModel,
    selectedRuntimeMode, selectedRuntimeRef, selectedVariant, modelReady, modelTurnBlockReason, sessionBrowserWorkspaces, sessionUsage, sessions, setActiveRightTabId, setAppearance,
    setAttachments, setBackendDraft, setChannelEnabled, setDebugEnabled, setDirtyRightTabs, setDraftSession, setLeftCollapsed, setMainView,
    setMobilePanel, setCommandFeedback, setPermissionMode, setRightCollapsed, setRightTabs, setRightWidthPx, setRuntimeOptionsError,
    setRuntimeOptionsResult, setRuntimeSessionId, setSelectedModel: changeComposerModel, setSelectedModelSelection: changeComposerModelSelection, setSelectedRuntimeMode, setSelectedRuntimeRef,
    setSelectedVariant: changeComposerVariant, setSettingsSection, setSnapshot, setWorkMode, setWorkspaceDialogOpen, fallbackCwd, settings, settingsSection,
    usageStats, usageStatsError, usageStatsLoading, refreshUsageStats,
    clearRightWorkspaceTabPendingPrompt, showSessionChrome, snapshot, startNewThread, startShell, startWechatQrSetup, status, submitTurn, submitThreadTurn, switchMainView, terminalEvents,
    togglePinnedSession, traceState, transcriptEntries, updateBackendDraftFields, updateChannel, updateMainView, viewEpochRef, workMode,
    voiceAutoSpeak, voiceListening, voiceRealtimeActive: Boolean(voiceRealtimeSessionId),
    onReadAloudText: readAloudText, onVoiceAutoSpeakToggle: toggleVoiceAutoSpeak,
    onVoiceDictationToggle: toggleVoiceDictation, onVoiceRealtimeToggle: toggleVoiceRealtime,
    workspaceChanges, workspaceDialogOpen, workspaceDiff, workspaceFiles
  }} />;
}
