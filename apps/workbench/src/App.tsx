import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent } from "react";
import {
  AlertTriangle,
  GripVertical,
  MessageSquare,
  PanelLeft,
  PanelRight,
  Search
} from "lucide-react";
import {
  Composer,
  HistoryPanel,
  TranscriptPanel,
  type TranscriptAgentSession,
  type HistoryDraftSession
} from "@psychevo/components";
import {
  appendOptimisticPrompt,
  applyLiveTranscriptEvent,
  GatewayClient,
  parseThreadSnapshot,
  reconcileThreadSnapshot,
  scopeForWorkdir
} from "@psychevo/client";
import {
  createBrowserHost,
  downloadUrl,
  type GatewayEndpoint,
  type PsychevoHost
} from "@psychevo/host";
import {
  GatewayEventSchema,
  InitializeResultSchema,
  ObservabilityReadResultSchema,
  SettingsReadResultSchema,
  TerminalExitedPayloadSchema,
  TerminalOutputPayloadSchema,
  ThreadBrowserResultSchema,
  ThreadTraceResultSchema,
  WorkspaceChangeMutationResultSchema,
  WorkspaceChangesResultSchema,
  WorkspaceDiffResultSchema,
  WorkspaceCreateResultSchema,
  WorkspaceFileReadResultSchema,
  WorkspaceFileWriteResultSchema,
  WorkspaceFilesResultSchema,
  type ContextReadResult,
  type GatewayMention,
  type GatewayEvent,
  type GatewayInputPart,
  type GatewayRequestScope,
  type InitializeResult,
  type ObservabilityReadResult,
  type RuntimeOptionsResult,
  type SessionSummary,
  type SettingsReadResult,
  type ThreadSnapshot,
  type ThreadTraceResult,
  type WorkspaceChangesResult,
  type WorkspaceDiffResult,
  type WorkspaceFileReadResult,
  type WorkspaceFileWriteResult,
  type WorkspaceFilesResult
} from "@psychevo/protocol";
import { attachmentFromFile } from "./attachments";
import {
  asOptionalRecord,
  asRecord,
  commandFeedbackAutoDismissable,
  optionalStringField,
  parseAgentList,
  parseBackendDoctor,
  parseBackendList,
  parseCommandList,
  prettyJson,
  stringArray,
  traceEventLabel,
  traceEventSeq,
  traceEventTime
} from "./data";
import { createCommandActions } from "./command-actions";
import { createAppActions } from "./app-actions";
import { useWorkbenchEffects } from "./app-effects";
import {
  createSurfaceActions,
  sessionsFromThreadBrowser,
  workspacesFromThreadBrowser
} from "./surface-actions";
import { WorkbenchLayout } from "./workbench-layout";
import {
  LeftUtilityRail,
  MainSurface,
  PinnedPanel,
  WorkspaceCreateDialog
} from "./app-shell";
import {
  CommandFeedbackView,
  CommandOverlayView
} from "./command-overlay";
import {
  ComposerRequests,
  ComposerStatusLine,
  ComposerSubmitControls
} from "./composer-controls";
import {
  RightWorkspace,
  createRightTabId,
  fileBasename,
  isUnsupportedPreviewFile,
  rightWorkspaceTabVisibleForSession,
  rightWorkspaceDefaultTitle,
  rightWorkspaceTabLabel
} from "./right-workspace";
import {
  EMPTY_BACKEND_DRAFT,
  backendDraftFromBackend,
  parseBackendCommandJson
} from "./settings-panels";
import {
  ComposerRuntimeControls,
  agentOptionValue,
  formatRuntimeModeValues,
  isComposerRunnableAgent,
  isComposerRuntimeBackend,
  isRuntimeModeOption,
  normalizeRequestedRuntimeMode,
  projectRuntimeModeOption,
  resolvePeerRuntimeMode,
  runtimeModeCommandValues,
  runtimeSupportsAgentPersona
} from "./runtime-controls";
import {
  idleActivity,
  multilineList,
  normalizeActivity,
  normalizeSessionSummary,
  normalizeSnapshot,
  startupDraftScope
} from "./session-utils";
import { transcriptSearchText } from "./search";
import {
  DEFAULT_RIGHT_WIDTH_PX,
  PINNED_SESSIONS_KEY,
  PREFS_KEY,
  clampRightWidth,
  readPinnedSessionIds,
  readPinnedSessionIdsFromStorage,
  readWorkbenchPrefs
} from "./storage";
import type {
  Appearance,
  BackendDraft,
  CommandAlternateAction,
  CommandFeedback,
  CommandOverlay,
  CommandTrigger,
  DebugEvent,
  GatewayEventFeed,
  MainView,
  PendingAttachment,
  RightWorkspaceTab,
  RightWorkspaceTabKind,
  SessionBrowserWorkspaceState,
  SettingsSection,
  TerminalNotificationEvent,
  TraceState,
  WorkbenchAgent,
  WorkbenchBackend,
  WorkbenchBackendDoctor,
  WorkbenchCommand,
  WorkbenchDiagnostic,
  WorkbenchPrefs
} from "./types";
import {
  createHistoryDraftSession,
  shouldApplyReadOnlySnapshot,
  shouldAdoptDetachedShellResult,
  visibleHistoryDraftSession,
  type PendingDetachedShell
} from "./viewGuard";

const EMPTY_SNAPSHOT: ThreadSnapshot = {
  source: { kind: "web", rawId: "pending", lifetime: "persistent", rawIdentity: null, visibleName: null },
  scope: scopeForWorkdir(""),
  thread: null,
  entries: [],
  activity: idleActivity(),
  pendingPermissions: [],
  pendingClarifies: []
};

const COMMAND_FEEDBACK_AUTO_DISMISS_MS = 3_000;
const logoUrl = new URL("../../../assets/psychevo-logo.svg", import.meta.url).href;
const LIVE_EVENT_REFRESH_SETTLE_MS = 650;
let terminalEventSeq = 0;

function nextTerminalEventSeq(): number {
  terminalEventSeq += 1;
  return terminalEventSeq;
}

function pacedGatewayEvent(event: GatewayEvent): boolean {
  return event.type === "entryStarted" ||
    event.type === "entryUpdated" ||
    event.type === "entryCompleted" ||
    event.type === "entryDelta" ||
    event.type === "turnCompleted";
}

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
  const byWorkdir = new Map(current.map((workspace) => [workspace.workdir, workspace]));
  for (const workspace of incoming) {
    byWorkdir.set(workspace.workdir, workspace);
  }
  return Array.from(byWorkdir.values());
}

export function App() {
  const [client, setClient] = useState<GatewayClient | null>(null);
  const [host, setHost] = useState<PsychevoHost | null>(null);
  const [endpoint, setEndpoint] = useState<GatewayEndpoint | null>(null);
  const [init, setInit] = useState<InitializeResult | null>(null);
  const [activeScope, setActiveScope] = useState<GatewayRequestScope | null>(null);
  const [snapshot, setSnapshot] = useState<ThreadSnapshot>(EMPTY_SNAPSHOT);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [archivedSessions, setArchivedSessions] = useState<SessionSummary[]>([]);
  const [sessionBrowserWorkspaces, setSessionBrowserWorkspaces] = useState<SessionBrowserWorkspaceState[]>([]);
  const [loadingOlderWorkdir, setLoadingOlderWorkdir] = useState<string | null>(null);
  const [pinnedSessionIds, setPinnedSessionIds] = useState<string[]>(readPinnedSessionIds);
  const [draftSession, setDraftSession] = useState<HistoryDraftSession | null>(null);
  const [settings, setSettings] = useState<SettingsReadResult | undefined>();
  const [agents, setAgents] = useState<WorkbenchAgent[]>([]);
  const [backends, setBackends] = useState<WorkbenchBackend[]>([]);
  const [backendDraft, setBackendDraft] = useState<BackendDraft | null>(null);
  const [backendDoctor, setBackendDoctor] = useState<Record<string, WorkbenchBackendDoctor>>({});
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
  const [attachments, setAttachments] = useState<PendingAttachment[]>([]);
  const [composerDraftPatch, setComposerDraftPatch] = useState<{ id: number; text: string } | null>(null);
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
  const viewEpochRef = useRef(0);
  const mainViewRef = useRef<MainView>("transcript");
  const selectedThreadIdRef = useRef<string | null>(null);
  const scopeRef = useRef<GatewayRequestScope | null>(null);
  const commandContextKeyRef = useRef<string | null>(null);
  const detachedShellTokenRef = useRef(0);
  const pendingDetachedShellRef = useRef<PendingDetachedShell | null>(null);
  const skipNextPinnedPersistRef = useRef(false);
  const gatewayEventQueueRef = useRef<GatewayEvent[]>([]);
  const gatewayEventRafRef = useRef<number | null>(null);
  const gatewayEventSeqRef = useRef(0);

  const activity = normalizeActivity(snapshot.activity);
  const transcriptEntries = Array.isArray(snapshot.entries) ? snapshot.entries : [];
  const pendingClarifies = Array.isArray(snapshot.pendingClarifies) ? snapshot.pendingClarifies : [];
  const pendingPermissions = Array.isArray(snapshot.pendingPermissions) ? snapshot.pendingPermissions : [];
  const running = activity.running;
  const disabled = status !== "connected";
  const currentThreadId = snapshot.thread?.id;
  const visibleDraftSession = visibleHistoryDraftSession(draftSession, false);
  const hasSelectedSession = Boolean(currentThreadId || visibleDraftSession);
  const showSessionChrome = mainView === "transcript" && hasSelectedSession;
  const commandContextKey = `${activeScope?.workdir ?? ""}:${currentThreadId ?? visibleDraftSession?.id ?? "none"}`;
  const activeWorkbenchWorkdir = activeScope?.workdir ?? init?.scope.workdir ?? settings?.workdir ?? window.location.pathname;
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

  function scheduleGatewayEventFlush() {
    if (gatewayEventRafRef.current !== null) {
      return;
    }
    gatewayEventRafRef.current = window.requestAnimationFrame(() => {
      gatewayEventRafRef.current = null;
      const event = gatewayEventQueueRef.current.shift();
      if (event) {
        setSnapshot((current) => {
          const next = normalizeSnapshot(applyLiveTranscriptEvent(current, event));
          selectedThreadIdRef.current = next.thread?.id ?? null;
          return next;
        });
      }
      if (gatewayEventQueueRef.current.length > 0) {
        scheduleGatewayEventFlush();
      }
    });
  }

  function publishGatewayEvent(event: GatewayEvent) {
    gatewayEventSeqRef.current += 1;
    setLatestGatewayEvent({
      event,
      seq: gatewayEventSeqRef.current
    });
  }

  function applyGatewayEvent(event: GatewayEvent) {
    publishGatewayEvent(event);
    if (!pacedGatewayEvent(event)) {
      setSnapshot((current) => {
        const next = normalizeSnapshot(applyLiveTranscriptEvent(current, event));
        selectedThreadIdRef.current = next.thread?.id ?? null;
        return next;
      });
      return;
    }
    gatewayEventQueueRef.current.push(event);
    scheduleGatewayEventFlush();
  }

  function scheduleSnapshotRefreshAfterLiveSettle(
    nextClient: GatewayClient,
    threadId: string | null,
    epoch = viewEpochRef.current
  ) {
    window.setTimeout(() => {
      if (threadId) {
        void refreshSnapshot(nextClient, threadId, undefined, true, epoch);
      } else {
        void refreshSnapshot(nextClient);
      }
    }, LIVE_EVENT_REFRESH_SETTLE_MS);
  }
  const controls = settings?.controls ?? null;

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

  async function loadOlderSessions(workdir: string) {
    if (!client) {
      return;
    }
    const cursor = sessionBrowserWorkspaces.find((workspace) => workspace.workdir === workdir)?.nextCursor;
    if (!cursor || loadingOlderWorkdir) {
      return;
    }
    setLoadingOlderWorkdir(workdir);
    try {
      const result = ThreadBrowserResultSchema.parse(
        await client.request("thread/browser", {
          archived: false,
          cursor,
          includeSessionIds: [currentThreadId ?? null, ...pinnedSessionIds].filter((id): id is string => Boolean(id)),
          limit: 20,
          recentDays: 7,
          workdir
        })
      );
      setSessions((current) => mergeSessionSummaries(current, sessionsFromThreadBrowser(result)));
      setSessionBrowserWorkspaces((current) => mergeBrowserWorkspaces(current, workspacesFromThreadBrowser(result)));
    } finally {
      setLoadingOlderWorkdir(null);
    }
  }

  useWorkbenchEffects({
    activeRightTabKind: activeRightTab?.kind ?? null,
    activeRightTabId,
    activeScope,
    appearance,
    client,
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
    settingsWorkdir: settings?.workdir,
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

  function revealRightWorkspace(tabId: string | null = activeRightTabId) {
    setActiveCommandOverlay(null);
    setRightCollapsed(false);
    setActiveRightTabId(tabId);
    setMobilePanel("status");
  }

  function openRightWorkspaceTab(kind: RightWorkspaceTabKind, patch: Partial<RightWorkspaceTab> = {}, forceNew = false) {
    if (kind === "debug" && !debugEnabled) {
      return;
    }
    const reusable = kind === "review" || kind === "files" || kind === "debug";
    const threadReusable = kind === "agentSession" && patch.threadId;
    const existingThreadTab = threadReusable
      ? rightTabs.find((tab) => tab.kind === kind && tab.threadId === patch.threadId)
      : null;
    const nextId = existingThreadTab?.id
      ?? (reusable && !forceNew ? rightTabs.find((tab) => tab.kind === kind)?.id ?? createRightTabId(kind) : createRightTabId(kind));
    const nextTab: RightWorkspaceTab = {
      id: nextId,
      kind,
      title: patch.title ?? rightWorkspaceDefaultTitle(kind),
      threadId: patch.threadId ?? null,
      parentThreadId: patch.parentThreadId ?? null,
      pendingPrompt: patch.pendingPrompt ?? null,
      path: patch.path ?? null,
      diff: patch.diff ?? null,
      file: patch.file ?? null,
      message: patch.message ?? null
    };
    setRightTabs((current) => {
      const existing = current.find((tab) => tab.id === nextId);
      if (!existing) {
        return [...current, nextTab];
      }
      return current.map((tab) => (
        tab.id === nextId
          ? { ...tab, ...nextTab, id: tab.id, kind: tab.kind }
          : tab
      ));
    });
    revealRightWorkspace(nextId);
  }

  function clearRightWorkspaceTabPendingPrompt(tabId: string) {
    setRightTabs((current) => current.map((tab) => (
      tab.id === tabId ? { ...tab, pendingPrompt: null } : tab
    )));
  }

  function closeRightWorkspaceTab(tabId: string) {
    if (dirtyRightTabs[tabId] && !window.confirm("Discard unsaved file edits?")) {
      return;
    }
    const closingTab = rightTabs.find((tab) => tab.id === tabId) ?? null;
    if (closingTab?.kind === "sideConversation" && closingTab.threadId) {
      const threadId = closingTab.threadId;
      void runAction(async () => {
        await client?.request("turn/interrupt", { threadId });
        await client?.request("thread/delete", { threadId });
      });
    }
    setRightTabs((current) => current.filter((tab) => tab.id !== tabId));
    setDirtyRightTabs((current) => {
      const next = { ...current };
      delete next[tabId];
      return next;
    });
    setActiveRightTabId((current) => {
      if (current !== tabId) {
        return current;
      }
      const remaining = rightTabs.filter((tab) => tab.id !== tabId);
      return remaining.at(-1)?.id ?? null;
    });
  }

  function openReviewTab(diff: WorkspaceDiffResult, path?: string | null) {
    const selectedPath = diff.selectedPath ?? path ?? null;
    openRightWorkspaceTab("review", {
      diff,
      path: selectedPath,
      title: selectedPath ? fileBasename(selectedPath) : "Review"
    });
  }

  function openAgentSessionTab(session: TranscriptAgentSession) {
    openRightWorkspaceTab("agentSession", {
      parentThreadId: session.parentSessionId ?? currentThreadId ?? null,
      threadId: session.childSessionId,
      title: session.taskName ?? session.agentName ?? session.title ?? "Agent"
    });
  }

  function beginRightResize(event: ReactPointerEvent<HTMLButtonElement>) {
    if (window.matchMedia("(max-width: 780px)").matches) {
      return;
    }
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = rightWidthPx;
    const pointerId = event.pointerId;
    event.currentTarget.setPointerCapture(pointerId);
    function onPointerMove(moveEvent: PointerEvent) {
      const nextWidth = clampRightWidth(startWidth + startX - moveEvent.clientX);
      setRightWidthPx(nextWidth);
    }
    function onPointerUp() {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
    }
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp, { once: true });
  }

  const {
    acceptWorkspaceChange,
    changeAgentSelection,
    copyTranscriptText,
    createWorkspace,
    deleteArchivedSession,
    deleteBackend,
    doctorBackend,
    handleAttachment,
    loadThreadSearchText,
    openDiffPreview,
    openFilePreview,
    rejectWorkspaceChange,
    restoreArchivedSession,
    saveBackendDraft,
    saveFileFromEditor,
    startNewThread,
    startShell,
    submitTurn,
    updateBackendDraftFields
  } = createAppActions({
    activeScope,
    attachments,
    client,
    currentThreadId: currentThreadId ?? null,
    detachedShellTokenRef,
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
    await client.request("turn/start", {
      agentName: runtimeAcceptsAgentPersona ? selectedAgentName || null : null,
      input: [{ type: "text", text: trimmed }],
      mentions: submittedMentions,
      mode: selectedRuntimeRef === "native" ? workMode : null,
      model: selectedModel,
      permissionMode,
      reasoningEffort: selectedVariant === "none" ? null : selectedVariant,
      runtimeOptions,
      runtimeRef: selectedRuntimeRef,
      runtimeSessionId,
      scope: activeScope ?? init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname),
      threadId,
      text: null
    });
    await refreshHistory();
  }

  const { executeCommand, runCommandAlternateAction } = createCommandActions({
    activeScope,
    activity,
    client,
    endpoint,
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

  return <WorkbenchLayout {...{
    acceptWorkspaceChange, activeCommandOverlay, activeRightTab, activeRightTabId, activeScope, activeWorkbenchWorkdir,
    activity, appearance, archivedSessions, attachments, backendDoctor, backendDraft, backends, beginExplicitViewSwitch,
    beginRightResize, changeAgentSelection, clearCommandTransientUi, client, closeRightWorkspaceTab, commandFeedback,
    commands, composerDraftPatch, contextUsage, controls, copyTranscriptText, createWorkspace, currentThreadId,
    debugEnabled, debugEvents, deleteArchivedSession, deleteBackend, disabled, doctorBackend, endpoint, error,
    executeCommand, extraRuntimeModeValues, handleAttachment, host, init, latestGatewayEvent, leftCollapsed, loadThreadSearchText,
    loadingOlderWorkdir, loadOlderSessions, mainView, mobilePanel, openDiffPreview, openAgentSessionTab, openFilePreview, openRightWorkspaceTab, openSettingsSection,
    pendingClarifies, pendingPermissions, permissionMode, pinnedSessionIds, pinnedSessions, planModeAvailable,
    refreshAgentSurface, refreshHistory, refreshSnapshot, refreshTrace, refreshWorkspaceSurface, rejectWorkspaceChange,
    restoreArchivedSession, revealRightWorkspace, rightCollapsed, rightTabs, rightWidthPx, runnableAgents, runAction,
    runCommandAlternateAction, running, runtimeAcceptsAgentPersona, runtimeBackends, runtimeModeOption,
    runtimeModeUnavailable, runtimeOptionsError, saveBackendDraft, saveFileFromEditor, selectedAgentName, selectedModel,
    selectedRuntimeMode, selectedRuntimeRef, selectedVariant, sessionBrowserWorkspaces, sessionUsage, sessions, setActiveRightTabId, setAppearance,
    setAttachments, setBackendDraft, setDebugEnabled, setDirtyRightTabs, setDraftSession, setLeftCollapsed, setMainView,
    setMobilePanel, setCommandFeedback, setPermissionMode, setRightCollapsed, setRightTabs, setRightWidthPx, setRuntimeOptionsError,
    setRuntimeOptionsResult, setRuntimeSessionId, setSelectedModel, setSelectedRuntimeMode, setSelectedRuntimeRef,
    setSelectedVariant, setSettingsSection, setSnapshot, setWorkMode, setWorkspaceDialogOpen, settings, settingsSection,
    clearRightWorkspaceTabPendingPrompt, showSessionChrome, snapshot, startNewThread, startShell, status, submitTurn, submitThreadTurn, switchMainView, terminalEvents,
    togglePinnedSession, traceState, transcriptEntries, updateBackendDraftFields, updateMainView, viewEpochRef, workMode,
    workspaceChanges, workspaceDialogOpen, workspaceDiff, workspaceFiles
  }} />;
}
