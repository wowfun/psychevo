import { useEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import type { Terminal as XTermTerminal, ITheme } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import {
  AlertTriangle,
  Archive,
  ArrowLeft,
  Bot,
  Bug,
  Check,
  ChevronDown,
  ChevronRight,
  Edit3,
  FileText,
  FolderPlus,
  FolderTree,
  GitBranch,
  GitPullRequest,
  GripVertical,
  Home,
  MessageSquare,
  Moon,
  PanelLeft,
  PanelRight,
  Pin,
  PlugZap,
  Plus,
  RefreshCw,
  RotateCcw,
  Save,
  Search,
  Settings,
  Shield,
  Sun,
  TerminalSquare,
  Trash2,
  Wrench,
  X
} from "lucide-react";
import {
  Composer,
  DismissibleDetails,
  HistoryPanel,
  MarkdownText,
  TranscriptPanel,
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
  type HostStorage,
  type PsychevoHost
} from "@psychevo/host";
import {
  GatewayEventSchema,
  InitializeResultSchema,
  ObservabilityReadResultSchema,
  SettingsReadResultSchema,
  TerminalExitedPayloadSchema,
  TerminalOutputPayloadSchema,
  ThreadListResultSchema,
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
  type PendingClarify,
  type PendingPermission,
  type PermissionDecision,
  type SessionSummary,
  type SettingsReadResult,
  type SessionUsageSummaryView,
  type TerminalExitedPayload,
  type TerminalOutputPayload,
  type ThreadSnapshot,
  type ThreadTraceResult,
  type WorkspaceChangesResult,
  type WorkspaceDiffResult,
  type WorkspaceFileEntry,
  type WorkspaceFileReadResult,
  type WorkspaceFileWriteResult,
  type WorkspaceFilesResult
} from "@psychevo/protocol";
import { highlightToHtml, languageForPath } from "./highlight";
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
const CONTEXT_CATEGORY_ORDER = [
  "base_policy",
  "developer_prompt",
  "project_context",
  "history",
  "turn_context",
  "current_prompt",
  "system_tools"
];

type ContextUsageCategory = ContextReadResult["categories"][number];

type WorkbenchAgent = {
  name: string;
  description: string;
  source: string;
  generated: boolean;
  path?: string | null;
  entrypoints: string[];
  backend?: { ref?: string } | null;
};

type WorkbenchBackend = {
  id: string;
  kind: string;
  enabled: boolean;
  label: string;
  description?: string | null;
  command?: string | null;
  args: string[];
  cwd: string;
  entrypoints: string[];
  clientCapabilities: string[];
  mcpServers: string[];
  envKeys: string[];
  sourceTargets: BackendConfigTarget[];
  diagnostics: WorkbenchDiagnostic[];
};

type BackendConfigTarget = "project" | "profile";

type WorkbenchDiagnostic = {
  kind: string;
  message: string;
};

type WorkbenchBackendDoctor = {
  id: string;
  ok: boolean;
  checks: Array<{ name: string; ok: boolean; message: string; path: string | null }>;
};

type BackendDraft = {
  id: string;
  enabled: boolean;
  label: string;
  description: string;
  commandJsonText: string;
  cwd: string;
  entrypoints: string[];
  clientCapabilities: string[];
  mcpServersText: string;
};

type BackendCommandJson = {
  command: string;
  args: string[];
  env: Record<string, string>;
};

type WorkbenchCommand = {
  name: string;
  slash: string;
  usage: string;
  summary: string;
  aliases: string[];
  argumentKind: string;
  source: string;
  presentationKind: string;
  destination: string | null;
  feedbackAnchor: string | null;
  alternateAction: CommandAlternateAction | null;
};

type RightWorkspaceTabKind = "review" | "terminal" | "files" | "debug";
type RightWorkspaceTab = {
  id: string;
  kind: RightWorkspaceTabKind;
  title: string;
  path?: string | null;
  diff?: WorkspaceDiffResult | null;
  file?: WorkspaceFileReadResult | null;
  message?: string | null;
};
type MainView = "transcript" | "search" | "settings";
type SettingsSection = "appearance" | "debug" | "agents" | "archived";
type Appearance = "dark" | "light";
const BACKEND_ENTRYPOINTS = ["peer", "subagent"] as const;
const BACKEND_CLIENT_CAPABILITIES = ["fs.read", "fs.write", "terminal"] as const;
const BACKEND_COMMAND_JSON_PLACEHOLDER = `{
  "command": "opencode",
  "args": ["acp"],
  "env": {}
}`;
const EMPTY_BACKEND_DRAFT: BackendDraft = {
  id: "",
  enabled: true,
  label: "",
  description: "",
  commandJsonText: "",
  cwd: "",
  entrypoints: ["peer", "subagent"],
  clientCapabilities: ["fs.read", "fs.write", "terminal"],
  mcpServersText: ""
};
const SETTINGS_SECTIONS: Array<{ id: SettingsSection; label: string; description: string }> = [
  { id: "appearance", label: "Appearance", description: "Theme" },
  { id: "debug", label: "Debug", description: "Developer diagnostics" },
  { id: "agents", label: "Agents", description: "Profile ACP backends" },
  { id: "archived", label: "Archived sessions", description: "Restore or delete" }
];
type CommandOverlay = "commands";
type CommandTrigger = "composer" | "commandsPanel" | "commandOverlay";

type CommandAlternateAction = {
  type: string;
  target: string;
  label: string;
};

type CommandFeedback = {
  accepted: boolean;
  command: string;
  message: string;
  feedbackAnchor?: string | null;
  alternateAction?: CommandAlternateAction | null;
} | null;

type DebugEvent = {
  id: string;
  at: number;
  method: string;
  payload: unknown;
};

type TraceState = {
  error: string | null;
  loading: boolean;
  result: ThreadTraceResult | null;
  threadId: string | null;
};

type PendingAttachment = {
  id: string;
  input: GatewayInputPart;
  kind: "file" | "image" | "text";
  name: string;
  size: number;
  sizeLabel: string;
};

type SearchResult = {
  excerpt: string;
  id: string;
  kind: "message" | "session";
  subtitle: string;
  title: string;
};

type WorkbenchPrefs = {
  appearance: Appearance;
  debug: boolean;
  rightWidthPx: number;
};

type TerminalNotificationEvent =
  | { method: "terminal/output"; params: TerminalOutputPayload; seq: number }
  | { method: "terminal/exited"; params: TerminalExitedPayload; seq: number };

type WorkspaceFileTreeItem = {
  badge?: string | null;
  disabled?: boolean;
  kind: "directory" | "file";
  name: string;
  path: string;
  depth: number;
  status?: string | null;
};

type ParsedDiffLineKind = "add" | "delete" | "context" | "meta";

type ParsedDiffLine = {
  kind: ParsedDiffLineKind;
  marker: string;
  newNumber: number | null;
  oldNumber: number | null;
  text: string;
};

type ParsedDiffHunk = {
  header: string;
  lines: ParsedDiffLine[];
};

type ParsedDiffFile = {
  headers: string[];
  hunks: ParsedDiffHunk[];
  path: string;
};

const logoUrl = new URL("../../../assets/psychevo-logo.svg", import.meta.url).href;
const PREFS_KEY = "psychevo.workbench.v0.prefs";
const PINNED_SESSIONS_KEY = "psychevo.workbench.v0.pinnedSessions";
const MAX_TEXT_ATTACHMENT_BYTES = 256 * 1024;
const MAX_IMAGE_ATTACHMENT_BYTES = 6 * 1024 * 1024;
const DEFAULT_RIGHT_WIDTH_PX = 520;
const MIN_RIGHT_WIDTH_PX = 300;
const MAX_RIGHT_WIDTH_PX = 1200;
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

export function App() {
  const [client, setClient] = useState<GatewayClient | null>(null);
  const [host, setHost] = useState<PsychevoHost | null>(null);
  const [endpoint, setEndpoint] = useState<GatewayEndpoint | null>(null);
  const [init, setInit] = useState<InitializeResult | null>(null);
  const [activeScope, setActiveScope] = useState<GatewayRequestScope | null>(null);
  const [snapshot, setSnapshot] = useState<ThreadSnapshot>(EMPTY_SNAPSHOT);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [archivedSessions, setArchivedSessions] = useState<SessionSummary[]>([]);
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
  const activeRightTab = rightTabs.find((tab) => tab.id === activeRightTabId) ?? null;
  const pinnedSessions = useMemo(
    () => pinnedSessionIds
      .map((id) => sessions.find((session) => session.id === id))
      .filter((session): session is SessionSummary => Boolean(session)),
    [pinnedSessionIds, sessions]
  );
  const runnableAgents = useMemo(
    () => agents.filter((agent) => agent.name),
    [agents]
  );

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

  function applyGatewayEvent(event: GatewayEvent) {
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

  useEffect(() => {
    if (
      selectedAgentName &&
      !runnableAgents.some((agent) => agentOptionValue(agent) === selectedAgentName || agent.name === selectedAgentName)
    ) {
      setSelectedAgentName("");
    }
  }, [runnableAgents, selectedAgentName]);

  useEffect(() => {
    if (debugEnabled) {
      return;
    }
    setRightTabs((current) => current.filter((tab) => tab.kind !== "debug"));
    if (activeRightTab?.kind === "debug") {
      setActiveRightTabId(null);
    }
  }, [activeRightTab?.kind, debugEnabled]);

  useEffect(() => {
    if (!Object.values(dirtyRightTabs).some(Boolean)) {
      return;
    }
    const handler = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", handler);
    return () => window.removeEventListener("beforeunload", handler);
  }, [dirtyRightTabs]);

  useEffect(() => {
    if (!debugEnabled || activeRightTab?.kind !== "debug" || !client || !currentThreadId) {
      if (!currentThreadId) {
        setTraceState({ error: null, loading: false, result: null, threadId: null });
      }
      return;
    }
    void refreshTrace(client, currentThreadId);
  }, [activeRightTab?.kind, client, currentThreadId, debugEnabled]);

  useEffect(() => {
    document.documentElement.dataset.pevoAppearance = appearance;
    host?.storage.setJson<WorkbenchPrefs>(PREFS_KEY, { appearance, debug: debugEnabled, rightWidthPx });
  }, [appearance, debugEnabled, host, rightWidthPx]);

  useEffect(() => {
    if (host) {
      skipNextPinnedPersistRef.current = true;
      setPinnedSessionIds(readPinnedSessionIdsFromStorage(host.storage));
    }
  }, [host]);

  useEffect(() => {
    try {
      if (host) {
        if (skipNextPinnedPersistRef.current) {
          skipNextPinnedPersistRef.current = false;
          return;
        }
        host.storage.setJson(PINNED_SESSIONS_KEY, pinnedSessionIds);
      } else {
        window.localStorage.setItem(PINNED_SESSIONS_KEY, JSON.stringify(pinnedSessionIds));
      }
    } catch {
      // Preference writes should not block session controls.
    }
  }, [host, pinnedSessionIds]);

  useEffect(() => {
    if (currentThreadId && draftSession) {
      setDraftSession(null);
    }
  }, [currentThreadId, draftSession]);

  useEffect(() => {
    if (activeRightTabId && !rightTabs.some((tab) => tab.id === activeRightTabId)) {
      setActiveRightTabId(rightTabs.at(-1)?.id ?? null);
    }
  }, [activeRightTabId, rightTabs]);

  useEffect(() => {
    if (commandContextKeyRef.current === null) {
      commandContextKeyRef.current = commandContextKey;
      return;
    }
    if (commandContextKeyRef.current !== commandContextKey) {
      commandContextKeyRef.current = commandContextKey;
      clearCommandTransientUi();
    }
  }, [commandContextKey]);

  useEffect(() => {
    if (!commandFeedback) {
      return;
    }
    let timer: number | null = null;
    if (commandFeedbackAutoDismissable(commandFeedback)) {
      timer = window.setTimeout(() => {
        setCommandFeedback(null);
      }, COMMAND_FEEDBACK_AUTO_DISMISS_MS);
    }
    function onPointerDown(event: MouseEvent) {
      const target = event.target;
      if (target instanceof Element && target.closest(".commandFeedback")) {
        return;
      }
      setCommandFeedback(null);
    }
    document.addEventListener("mousedown", onPointerDown);
    return () => {
      if (timer !== null) {
        window.clearTimeout(timer);
      }
      document.removeEventListener("mousedown", onPointerDown);
    };
  }, [commandFeedback]);

  useEffect(() => {
    if (!showSessionChrome && mobilePanel === "status") {
      setMobilePanel("transcript");
    }
  }, [mobilePanel, showSessionChrome]);

  useEffect(() => {
    let alive = true;
    const nextHost = createBrowserHost(window.location, window.localStorage);
    const nextEndpoint = nextHost.endpoint;
    const nextClient = new GatewayClient(nextEndpoint);
    setHost(nextHost);
    setEndpoint(nextEndpoint);

    nextClient.subscribe((notification) => {
      pushDebugEvent(notification.method, notification.params);
      if (notification.method === "terminal/output") {
        const parsed = TerminalOutputPayloadSchema.safeParse(notification.params);
        if (parsed.success) {
          setTerminalEvents((current) => [
            ...current.slice(-240),
            { method: "terminal/output", params: parsed.data, seq: nextTerminalEventSeq() }
          ]);
        }
      }
      if (notification.method === "terminal/exited") {
        const parsed = TerminalExitedPayloadSchema.safeParse(notification.params);
        if (parsed.success) {
          setTerminalEvents((current) => [
            ...current.slice(-240),
            { method: "terminal/exited", params: parsed.data, seq: nextTerminalEventSeq() }
          ]);
        }
      }
      if (notification.method === "gateway/event") {
        const parsed = GatewayEventSchema.safeParse(notification.params);
        if (parsed.success) {
          const event = parsed.data;
          applyGatewayEvent(event);
          if (event.type === "turnStarted" && event.threadId) {
            void refreshHistory(nextClient);
          }
          if (event.type === "turnCompleted" && event.threadId) {
            const threadId = event.threadId;
            const eventEpoch = viewEpochRef.current;
            scheduleSnapshotRefreshAfterLiveSettle(nextClient, threadId, eventEpoch);
            void refreshHistory(nextClient);
            const scope = scopeRef.current;
            if (scope) {
              void refreshWorkspaceSurface(nextClient, scope, threadId);
            }
            for (const delay of [1_500, 3_000, 7_500, 15_000, 30_000, 60_000, 120_000]) {
              window.setTimeout(() => {
                void refreshSnapshot(nextClient, threadId, undefined, true, eventEpoch);
                void refreshHistory(nextClient);
              }, delay);
            }
            window.setTimeout(() => {
              void refreshSnapshot(nextClient, threadId, undefined, true, eventEpoch);
            }, 750);
          }
          if (["permissionRequested", "permissionResolved", "clarifyRequested", "clarifyResolved"].includes(event.type)) {
            void refreshSnapshot(nextClient);
          }
        }
      }
      if (notification.method === "shell/result") {
        const record = asRecord(notification.params);
        const thread = asRecord(record.thread);
        const threadId = optionalStringField(thread.id);
        if (threadId) {
          const pending = pendingDetachedShellRef.current;
          const eventEpoch = viewEpochRef.current;
          const adoptDetached = pending?.epoch === eventEpoch;
          if (adoptDetached) {
            pendingDetachedShellRef.current = null;
          }
          void refreshSnapshot(nextClient, threadId, undefined, true, eventEpoch, adoptDetached);
          const scope = scopeRef.current;
          if (scope) {
            void refreshWorkspaceSurface(nextClient, scope, threadId);
            void refreshAgentSurface(nextClient, scope);
          }
        } else {
          void refreshSnapshot(nextClient);
        }
        void refreshHistory(nextClient);
      }
      if (notification.method === "shell/error") {
        const record = asRecord(notification.params);
        setError(optionalStringField(record.message) ?? "Shell command failed");
        const threadId = optionalStringField(record.threadId);
        if (threadId) {
          void refreshSnapshot(nextClient, threadId, undefined, true, viewEpochRef.current);
        }
        void refreshHistory(nextClient);
      }
      if (notification.method === "turn/result") {
        const record = asRecord(notification.params);
        const thread = asRecord(record.thread);
        const threadId = optionalStringField(thread.id);
        if (threadId) {
          scheduleSnapshotRefreshAfterLiveSettle(nextClient, threadId, viewEpochRef.current);
          const scope = scopeRef.current;
          if (scope) {
            void refreshWorkspaceSurface(nextClient, scope, threadId);
          }
        } else {
          void refreshSnapshot(nextClient);
        }
        void refreshHistory(nextClient);
      }
      if (notification.method === "turn/error") {
        void refreshSnapshot(nextClient);
        void refreshHistory(nextClient);
      }
    });

    async function boot() {
      try {
        await nextClient.connect();
        if (!alive) {
          return;
        }
        setClient(nextClient);
        setStatus("connected");
        const initialize = InitializeResultSchema.parse(await nextClient.request("initialize"));
        setInit(initialize);
        setActiveScope(initialize.scope);
        scopeRef.current = initialize.scope;
        const nextSessions = await refreshHistory(nextClient);
        const startupScope = startupDraftScope(initialize.scope, nextSessions);
        const epoch = beginExplicitViewSwitch();
        const nextSnapshot = parseThreadSnapshot(await nextClient.request("thread/start", { scope: startupScope }));
        if (!alive) {
          return;
        }
        const normalized = normalizeSnapshot(nextSnapshot);
        selectedThreadIdRef.current = normalized.thread?.id ?? null;
        setSnapshot(normalized);
        setDraftSession(createHistoryDraftSession(epoch, startupScope.workdir));
        if (mainViewRef.current === "transcript") {
          updateMainView("transcript");
        }
        await adoptSnapshotScope(nextClient, nextSnapshot);
      } catch (err) {
        if (alive) {
          setStatus("error");
          setError(err instanceof Error ? err.message : String(err));
        }
      }
    }

    void boot();
    return () => {
      alive = false;
      gatewayEventQueueRef.current = [];
      if (gatewayEventRafRef.current !== null) {
        window.cancelAnimationFrame(gatewayEventRafRef.current);
        gatewayEventRafRef.current = null;
      }
      nextClient.close();
    };
  }, []);

  useEffect(() => {
    if (client && activeScope) {
      void refreshWorkspaceSurface(client, activeScope, currentThreadId ?? null);
    }
  }, [client, activeScope, currentThreadId]);

  useEffect(() => {
    if (client) {
      void refreshHistory(client);
    }
  }, [client]);

  useEffect(() => {
    if (client && mainView === "settings" && settingsSection === "archived") {
      void refreshHistory(client, true);
    }
  }, [client, mainView, settingsSection]);

  useEffect(() => {
    if (client && activeScope) {
      void refreshAgentSurface(client, activeScope);
    }
  }, [client, activeScope, currentThreadId, running]);

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

  async function refreshSnapshot(
    nextClient = client,
    threadId?: string,
    scope = activeScope ?? init?.scope,
    readOnly = false,
    expectedEpoch: number | null | undefined = null,
    allowDetachedAdoption = false
  ) {
    if (!nextClient) {
      return;
    }
    if (threadId && readOnly) {
      const nextSnapshot = parseThreadSnapshot(await nextClient.request("thread/read", { threadId }));
      if (expectedEpoch != null && expectedEpoch !== viewEpochRef.current) {
        return;
      }
      setSnapshot((current) => {
        if (!shouldApplyReadOnlySnapshot(
          current,
          threadId,
          viewEpochRef.current,
          expectedEpoch,
          allowDetachedAdoption
        )) {
          return current;
        }
        const next = normalizeSnapshot(reconcileThreadSnapshot(normalizeSnapshot(current), normalizeSnapshot(nextSnapshot)));
        selectedThreadIdRef.current = next.thread?.id ?? null;
        return next;
      });
      if ((selectedThreadIdRef.current ?? null) !== (nextSnapshot.thread?.id ?? threadId)) {
        return;
      }
      await refreshObservability(nextClient, nextSnapshot.scope, nextSnapshot.thread?.id ?? threadId, expectedEpoch);
      return;
    }
    const nextScope = scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    const params = threadId ? { threadId, scope: nextScope } : { scope: nextScope };
    const nextSnapshot = parseThreadSnapshot(await nextClient.request("thread/resume", params));
    setSnapshot((current) => {
      if (expectedEpoch != null && expectedEpoch !== viewEpochRef.current) {
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
      selectedThreadIdRef.current = next.thread?.id ?? null;
      return next;
    });
    if (expectedEpoch != null && expectedEpoch !== viewEpochRef.current) {
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
    setSnapshot((current) => (
      (current.thread?.id ?? null) === threadId ? (() => {
        selectedThreadIdRef.current = nextSnapshot.thread?.id ?? null;
        return nextSnapshot;
      })() : current
    ));
  }

  function patchComposerDraft(text: string) {
    setComposerDraftPatch((current) => ({
      id: (current?.id ?? 0) + 1,
      text
    }));
  }

  async function adoptSnapshotScope(nextClient: GatewayClient, nextSnapshot: ThreadSnapshot) {
    const scope = nextSnapshot.scope;
    if (!scope?.workdir) {
      return;
    }
    const previous = scopeRef.current;
    scopeRef.current = scope;
    setActiveScope(scope);
    const threadId = nextSnapshot.thread?.id ?? null;
    if (previous?.workdir === scope.workdir) {
      const nextSettings = SettingsReadResultSchema.parse(await nextClient.request("settings/read", { threadId, workdir: scope.workdir }));
      setSettings(nextSettings);
      applyInitialControls(nextSettings);
      await refreshObservability(nextClient, scope, threadId);
      return;
    }
    const [settingsValue] = await Promise.all([
      nextClient.request("settings/read", { threadId, workdir: scope.workdir }),
      refreshAgentSurface(nextClient, scope),
      refreshWorkspaceSurface(nextClient, scope, threadId)
    ]);
    const nextSettings = SettingsReadResultSchema.parse(settingsValue);
    setSettings(nextSettings);
    applyInitialControls(nextSettings);
  }

  async function refreshHistory(nextClient = client, includeArchived = false, workdir: string | null = null): Promise<SessionSummary[]> {
    if (!nextClient) {
      return [];
    }
    const result = ThreadListResultSchema.parse(
      await nextClient.request("thread/list", { archived: includeArchived, limit: 100, workdir: workdir ?? null })
    );
    const nextSessions = result.sessions.map(normalizeSessionSummary);
    if (includeArchived) {
      setArchivedSessions(nextSessions);
    } else {
      setSessions(nextSessions);
    }
    return nextSessions;
  }

  async function refreshAgentSurface(nextClient = client, scope = activeScope ?? init?.scope) {
    if (!nextClient || !scope) {
      return;
    }
    const [agentList, backendList, commandList] = await Promise.all([
      nextClient.request("agent/list", { scope }),
      nextClient.request("backend/list", { scope }),
      nextClient.request("command/list", { scope, threadId: snapshot.thread?.id ?? null })
    ]);
    setAgents(parseAgentList(agentList));
    setBackends(parseBackendList(backendList));
    setCommands(parseCommandList(commandList));
  }

  async function refreshWorkspaceSurface(
    nextClient = client,
    scope = activeScope ?? init?.scope,
    threadId: string | null = currentThreadId ?? null,
    expectedEpoch: number | null = viewEpochRef.current
  ) {
    if (!nextClient || !scope) {
      setWorkspaceChanges(null);
      setObservability(null);
      setContextUsage(null);
      return;
    }
    const [files, diff, changes, nextObservability] = await Promise.all([
      nextClient.request("workspace/files", { scope }),
      nextClient.request("workspace/diff", { scope, path: null }),
      nextClient.request("workspace/changes", { scope }),
      nextClient.request("observability/read", { scope, threadId })
    ]);
    if (!shouldApplyAsyncSurfaceResult(scope, expectedEpoch, threadId)) {
      return;
    }
    setWorkspaceFiles(WorkspaceFilesResultSchema.parse(files));
    setWorkspaceDiff(WorkspaceDiffResultSchema.parse(diff));
    setWorkspaceChanges(WorkspaceChangesResultSchema.parse(changes));
    applyObservability(nextObservability);
  }

  async function refreshObservability(
    nextClient = client,
    scope = activeScope ?? init?.scope,
    threadId: string | null = currentThreadId ?? null,
    expectedEpoch: number | null = viewEpochRef.current
  ) {
    if (!nextClient || !scope) {
      setObservability(null);
      setContextUsage(null);
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
    if (expectedEpoch != null && expectedEpoch !== viewEpochRef.current) {
      return false;
    }
    if ((selectedThreadIdRef.current ?? null) !== threadId) {
      return false;
    }
    const currentScope = scopeRef.current ?? activeScope ?? init?.scope ?? null;
    return !currentScope?.workdir || currentScope.workdir === scope.workdir;
  }

  function applyObservability(value: unknown) {
    const parsed = ObservabilityReadResultSchema.parse(value);
    setObservability(parsed);
    setContextUsage(parsed.context);
  }

  function pushDebugEvent(method: string, payload: unknown) {
    setDebugEvents((current) => [
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
    nextClient: GatewayClient | null = client,
    threadId: string | null = currentThreadId ?? null
  ) {
    if (!nextClient || !threadId) {
      setTraceState({ error: null, loading: false, result: null, threadId: null });
      return;
    }
    setTraceState((current) => ({
      error: null,
      loading: true,
      result: current.threadId === threadId ? current.result : null,
      threadId
    }));
    try {
      const result = ThreadTraceResultSchema.parse(
        await nextClient.request("thread/trace", { threadId, afterSeq: null, limit: 200 })
      );
      setTraceState((current) => (
        current.threadId === threadId
          ? { error: null, loading: false, result, threadId }
          : current
      ));
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setTraceState((current) => (
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
    setPermissionMode(nextControls.permissionMode || "default");
    setWorkMode(nextControls.mode || "default");
    setSelectedAgentName(nextControls.agent ?? "");
    setSelectedModel(nextControls.model ?? null);
    setSelectedVariant(nextControls.variant ?? "none");
  }

  async function runAction(action: () => Promise<void>) {
    try {
      setError(null);
      await action();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
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
    const nextId = reusable && !forceNew ? rightTabs.find((tab) => tab.kind === kind)?.id ?? createRightTabId(kind) : createRightTabId(kind);
    const nextTab: RightWorkspaceTab = {
      id: nextId,
      kind,
      title: patch.title ?? rightWorkspaceDefaultTitle(kind),
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

  function closeRightWorkspaceTab(tabId: string) {
    if (dirtyRightTabs[tabId] && !window.confirm("Discard unsaved file edits?")) {
      return;
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

  function revealCommandsPanel(trigger: CommandTrigger = "commandsPanel") {
    openCommandOverlay("commands");
  }

  function revealTranscriptPanel() {
    clearCommandTransientUi();
    updateMainView("transcript");
    setMobilePanel("transcript");
  }

  function revealHostPanel(panel: string, trigger: CommandTrigger = "commandsPanel") {
    switch (panel) {
      case "history":
      case "sessions":
        revealHistoryPanel();
        return;
      case "commands":
      case "help":
        revealCommandsPanel(trigger);
        return;
      case "preview":
        openRightWorkspaceTab("review", { diff: workspaceDiff, title: "Review" });
        return;
      case "files":
        openRightWorkspaceTab("files");
        return;
      case "debug":
        openRightWorkspaceTab("debug");
        return;
      case "status":
      default:
        revealRightWorkspace(null);
    }
  }

  function routeCommandFeedback(feedback: CommandFeedback, trigger: CommandTrigger) {
    const anchor = feedback?.feedbackAnchor;
    if (trigger === "commandsPanel" || anchor === "commandsPanel") {
      revealCommandsPanel(trigger);
      return;
    }
    if (anchor === "status") {
      revealRightWorkspace(null);
    }
  }

  async function runCommandAlternateAction(action: CommandAlternateAction | null | undefined) {
    if (!action) {
      return;
    }
    if (action.type === "openPanel") {
      switch (action.target) {
        case "history":
        case "sessions":
        case "commands":
        case "status":
        case "preview":
          revealHostPanel(action.target);
          return;
        default:
          return;
      }
    }
    if (action.type === "openComposerControl") {
      if (action.target === "attachments") {
        await handleAttachment();
        return;
      }
      revealRightWorkspace(null);
    }
  }

  async function executeCommand(command: string, trigger: CommandTrigger = "composer") {
    const scope = activeScope ?? init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    const result = await client?.request("command/execute", {
      command,
      scope,
      threadId: snapshot.thread?.id ?? null
    });
    if (!result) {
      return;
    }
    const record = asRecord(result);
    const action = asRecord(record.action);
    const downloadThreadId = action.type === "downloadSession"
      ? optionalStringField(action.threadId) ?? snapshot.thread?.id ?? null
      : null;
    const feedback = commandFeedbackFromResult(command, record, trigger, {
      downloadAvailable: action.type !== "downloadSession" || Boolean(endpoint && downloadThreadId)
    });
    if (action.type === "passThroughPrompt") {
      await runHostAction(record.action, trigger);
      return;
    }
    if (record.accepted !== true && record.known === false) {
      await submitTurn(command, []);
      return;
    }
    if (record.accepted !== true) {
      setCommandFeedback(feedback ?? {
        accepted: false,
        command,
        message: `Unsupported command: ${command}`,
        feedbackAnchor: trigger
      });
      routeCommandFeedback(feedback, trigger);
      return;
    }
    setCommandFeedback(feedback);
    if (feedback) {
      routeCommandFeedback(feedback, trigger);
    }
    await runHostAction(record.action, trigger);
  }

  async function startNewThread(workdir?: string) {
    if (!client) {
      return;
    }
    const epoch = beginExplicitViewSwitch();
    const scope = workdir
      ? scopeForWorkdir(workdir)
      : activeScope ?? init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    const nextSnapshot = parseThreadSnapshot(await client.request("thread/start", { scope }));
    if (viewEpochRef.current === epoch) {
      const normalized = normalizeSnapshot(nextSnapshot);
      selectedThreadIdRef.current = normalized.thread?.id ?? null;
      setSnapshot(normalized);
      setDraftSession(createHistoryDraftSession(epoch, scope.workdir));
      await adoptSnapshotScope(client, nextSnapshot);
    }
    await refreshHistory(client);
    setMobilePanel("transcript");
  }

  async function createWorkspace(name: string) {
    if (!client) {
      return;
    }
    const created = WorkspaceCreateResultSchema.parse(await client.request("workspace/create", { name }));
    const epoch = beginExplicitViewSwitch();
    const nextSnapshot = parseThreadSnapshot(await client.request("thread/start", { scope: created.scope }));
    if (viewEpochRef.current === epoch) {
      const normalized = normalizeSnapshot(nextSnapshot);
      selectedThreadIdRef.current = normalized.thread?.id ?? null;
      setSnapshot(normalized);
      setDraftSession(createHistoryDraftSession(epoch, created.workdir));
      await adoptSnapshotScope(client, nextSnapshot);
    }
    await refreshHistory(client);
    updateMainView("transcript");
    setMobilePanel("transcript");
  }

  async function runHostAction(action: unknown, trigger: CommandTrigger = "commandsPanel") {
    const record = asRecord(action);
    switch (record.type) {
      case "threadStart": {
        await startNewThread();
        break;
      }
      case "threadArchive":
        if (snapshot.thread?.id) {
          setDraftSession(null);
          await client?.request("thread/archive", { threadId: snapshot.thread.id });
          await refreshHistory();
        }
        break;
      case "threadDelete":
        if (snapshot.thread?.id) {
          setDraftSession(null);
          await client?.request("thread/delete", { threadId: snapshot.thread.id });
          await refreshHistory();
        }
        break;
      case "turnInterrupt":
        await client?.request("turn/interrupt", { threadId: snapshot.thread?.id ?? null });
        await refreshSnapshot();
        break;
      case "sessionUndo": {
        const threadId = optionalStringField(record.threadId) ?? snapshot.thread?.id ?? null;
        await refreshRevertedThreadSnapshot(client, threadId);
        await refreshHistory();
        await refreshWorkspaceSurface(client, activeScope ?? init?.scope, threadId);
        setAttachments([]);
        patchComposerDraft(stringField(record.prompt));
        updateMainView("transcript");
        setMobilePanel("transcript");
        break;
      }
      case "sessionRedo": {
        const threadId = optionalStringField(record.threadId) ?? snapshot.thread?.id ?? null;
        await refreshRevertedThreadSnapshot(client, threadId);
        await refreshHistory();
        await refreshWorkspaceSurface(client, activeScope ?? init?.scope, threadId);
        setAttachments([]);
        patchComposerDraft("");
        updateMainView("transcript");
        setMobilePanel("transcript");
        break;
      }
      case "queuePrompt": {
        const text = stringField(record.text).trim();
        const displayText = optionalStringField(record.displayText);
        if (text) {
          await submitTurn(text, [], displayText);
        }
        break;
      }
      case "passThroughPrompt":
      case "submitPrompt": {
        const text = stringField(record.text).trim();
        const displayText = optionalStringField(record.displayText);
        if (text) {
          await submitTurn(text, [], displayText);
        }
        break;
      }
      case "steerPrompt": {
        const text = stringField(record.text).trim();
        if (text && activity.activeTurnId) {
          setSnapshot((current) => appendOptimisticPrompt(current, text));
          await client?.request("turn/steer", {
            expectedTurnId: activity.activeTurnId,
            threadId: snapshot.thread?.id ?? null,
            text
          });
          await refreshHistory();
        } else if (text) {
          setCommandFeedback({
            accepted: false,
            command: "/steer",
            message: "/steer is only available while a turn is running.",
            feedbackAnchor: "composer"
          });
          setMobilePanel("transcript");
        }
        break;
      }
      case "downloadSession":
        {
          const threadId = optionalStringField(record.threadId) ?? snapshot.thread?.id ?? null;
          if (endpoint && threadId) {
            const kind = stringField(record.kind) === "share" ? "share" : "export";
            void host?.open.openDownload(downloadUrl(endpoint, threadId, kind));
          }
        }
        break;
      case "workspaceDiff": {
        const diff = WorkspaceDiffResultSchema.parse(record.diff);
        setActiveCommandOverlay(null);
        setWorkspaceDiff(diff);
        openReviewTab(diff, diff.selectedPath);
        break;
      }
      case "showPanel":
        revealHostPanel(stringField(record.panel), trigger);
        break;
      default:
        if (record.type) {
          setError(`Unsupported host action: ${String(record.type)}`);
        }
    }
  }

  async function submitTurn(text: string, mentions: GatewayMention[], displayText?: string | null) {
    const scope = activeScope ?? init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    const nextInput: GatewayInputPart[] = [
      ...(text.trim() ? [{ type: "text" as const, text }] : []),
      ...attachments.map((attachment) => attachment.input)
    ];
    const optimisticText = displayText?.trim()
      || text.trim()
      || attachments.map((attachment) => `[Attachment: ${attachment.name}]`).join(" ");
    pendingDetachedShellRef.current = null;
    clearCommandTransientUi();
    setSnapshot((current) => appendOptimisticPrompt(current, optimisticText));
    await client?.request("turn/start", {
      agentName: selectedAgentName || null,
      input: nextInput,
      mentions,
      mode: workMode,
      model: selectedModel,
      permissionMode,
      reasoningEffort: selectedVariant === "none" ? null : selectedVariant,
      scope,
      threadId: snapshot.thread?.id ?? null,
      text: null
    });
    setAttachments([]);
    await refreshHistory();
  }

  async function changeAgentSelection(value: string) {
    setSelectedAgentName(value);
    if (!client || !currentThreadId) {
      return;
    }
    const scope = activeScope ?? init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    const nextSettings = SettingsReadResultSchema.parse(await client.request("settings/update", {
      agent: value || null,
      threadId: currentThreadId,
      scope
    }));
    setSettings(nextSettings);
    setSelectedAgentName(nextSettings.controls?.agent ?? value);
  }

  async function startShell(command: string) {
    const scope = activeScope ?? init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    clearCommandTransientUi();
    const pendingShell = snapshot.thread?.id
      ? null
      : {
          epoch: viewEpochRef.current,
          token: detachedShellTokenRef.current + 1
        };
    if (pendingShell) {
      detachedShellTokenRef.current = pendingShell.token;
      pendingDetachedShellRef.current = pendingShell;
    }
    const result = await client?.request("shell/start", {
      command,
      scope,
      threadId: snapshot.thread?.id ?? null
    });
    const record = asRecord(result);
    if (record.accepted !== true) {
      if (pendingDetachedShellRef.current?.token === pendingShell?.token) {
        pendingDetachedShellRef.current = null;
      }
      setCommandFeedback({
        accepted: false,
        command: `!${command}`,
        message: optionalStringField(record.message) ?? "Shell command was not accepted.",
        feedbackAnchor: "composer"
      });
      setMobilePanel("transcript");
      return;
    }
    const threadId = optionalStringField(record.threadId);
    if (threadId) {
      const adoptDetached = shouldAdoptDetachedShellResult(
        snapshot,
        threadId,
        viewEpochRef.current,
        pendingDetachedShellRef.current
      );
      if (adoptDetached || snapshot.thread?.id) {
        if (pendingDetachedShellRef.current?.token === pendingShell?.token) {
          pendingDetachedShellRef.current = null;
        }
        await refreshSnapshot(client, threadId, undefined, true, viewEpochRef.current, adoptDetached);
      }
    }
    await refreshHistory();
  }

  async function openFilePreview(path: string) {
    if (isUnsupportedPreviewFile(path)) {
      openRightWorkspaceTab("files", {
        path,
        title: fileBasename(path),
        file: null,
        message: "Preview is not available for this file type."
      });
      return;
    }
    const scope = activeScope ?? init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    const result = WorkspaceFileReadResultSchema.parse(await client?.request("workspace/file/read", { scope, path }));
    if (result.binary || result.content === null) {
      openRightWorkspaceTab("files", {
        path: result.path,
        title: fileBasename(result.path),
        file: result,
        message: result.unreadable ?? "Preview is not available for this file."
      });
      return;
    }
    openRightWorkspaceTab("files", {
      path: result.path,
      title: fileBasename(result.path),
      file: result,
      message: result.truncated ? "Preview truncated." : null
    });
  }

  async function saveFileFromEditor(
    path: string,
    content: string,
    expectedRevision: string | null,
    force: boolean
  ): Promise<WorkspaceFileWriteResult> {
    const scope = activeScope ?? init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    const result = WorkspaceFileWriteResultSchema.parse(await client?.request("workspace/file/write", {
      scope,
      path,
      content,
      expectedRevision,
      force
    }));
    const read = WorkspaceFileReadResultSchema.parse(await client?.request("workspace/file/read", { scope, path: result.path }));
    setRightTabs((current) => current.map((tab) => (
      tab.kind === "files" && tab.path === result.path
        ? { ...tab, file: read, message: null, title: fileBasename(result.path) }
        : tab
    )));
    await refreshWorkspaceSurface(client, scope, currentThreadId ?? null);
    return result;
  }

  async function acceptWorkspaceChange(turnId: string, path: string) {
    const scope = activeScope ?? init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    const result = WorkspaceChangeMutationResultSchema.parse(await client?.request("workspace/change/accept", {
      scope,
      turnId,
      path
    }));
    setWorkspaceChanges(result.changes);
  }

  async function rejectWorkspaceChange(turnId: string, path: string) {
    const scope = activeScope ?? init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    const result = WorkspaceChangeMutationResultSchema.parse(await client?.request("workspace/change/reject", {
      scope,
      turnId,
      path
    }));
    setWorkspaceChanges(result.changes);
    await refreshWorkspaceSurface(client, scope, currentThreadId ?? null);
  }

  async function openDiffPreview(path?: string | null) {
    const scope = activeScope ?? init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    const result = WorkspaceDiffResultSchema.parse(await client?.request("workspace/diff", { scope, path: path ?? null }));
    setWorkspaceDiff((current) => path ? current : result);
    openReviewTab(result, path ?? null);
  }

  async function loadThreadSearchText(threadId: string): Promise<string> {
    if (!client) {
      return "";
    }
    const snapshot = parseThreadSnapshot(await client.request("thread/read", { threadId }));
    return transcriptSearchText(snapshot.entries);
  }

  async function copyTranscriptText(text: string) {
    const result = await host?.clipboard.writeText(text);
    if (!result || !result.ok) {
      const message = "Clipboard copy is not supported by this host.";
      setError(message);
      throw new Error(message);
    }
    setError(null);
  }

  async function handleAttachment() {
    const result = await host?.files.pickFile();
    if (!result || !result.ok) {
      setError("Attachments are not supported by this host yet.");
      return;
    }
    const attachment = await attachmentFromFile(result.value);
    setAttachments((current) => [...current, attachment]);
    setError(null);
  }

  async function writeBackendDraft(draft: BackendDraft) {
    if (!client) {
      throw new Error("Gateway client is unavailable.");
    }
    const commandConfig = parseBackendCommandJson(draft.commandJsonText);
    if (commandConfig.error) {
      throw new Error(commandConfig.error);
    }
    const scope = activeScope ?? init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    await client.request("backend/write", {
      scope,
      id: draft.id.trim(),
      target: "profile",
      enabled: draft.enabled,
      label: draft.label.trim() || null,
      description: draft.description.trim() || null,
      command: commandConfig.command.trim() || null,
      args: commandConfig.args,
      env: commandConfig.env,
      cwd: draft.cwd.trim() || "invocation",
      entrypoints: draft.entrypoints,
      clientCapabilities: draft.clientCapabilities,
      mcpServers: multilineList(draft.mcpServersText)
    });
    await refreshAgentSurface(client, scope);
  }

  async function saveBackendDraft(draft: BackendDraft) {
    await writeBackendDraft(draft);
    setBackendDraft(null);
  }

  async function updateBackendDraftFields(backend: WorkbenchBackend, patch: Partial<BackendDraft>) {
    await writeBackendDraft({ ...backendDraftFromBackend(backend), ...patch });
  }

  async function deleteBackend(backend: WorkbenchBackend) {
    if (!client) {
      throw new Error("Gateway client is unavailable.");
    }
    const scope = activeScope ?? init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    await client.request("backend/delete", { scope, id: backend.id, target: "profile" });
    await refreshAgentSurface(client, scope);
  }

  async function doctorBackend(backend: WorkbenchBackend) {
    if (!client) {
      throw new Error("Gateway client is unavailable.");
    }
    const scope = activeScope ?? init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    const result = await client.request("backend/doctor", { scope, id: backend.id });
    setBackendDoctor((current) => ({
      ...current,
      [backend.id]: parseBackendDoctor(result)
    }));
  }

  async function restoreArchivedSession(threadId: string) {
    if (!client) {
      return;
    }
    await client.request("thread/restore", { threadId });
    await refreshHistory(client);
    await refreshHistory(client, true);
  }

  async function deleteArchivedSession(threadId: string) {
    if (!client) {
      return;
    }
    await client.request("thread/delete", { threadId });
    await refreshHistory(client);
    await refreshHistory(client, true);
  }

  return (
    <main className="appShell" data-main-view={mainView}>
      {error && (
        <div className="errorBand" role="alert">
          <AlertTriangle size={17} aria-hidden />
          <span>{error}</span>
        </div>
      )}
      {workspaceDialogOpen && (
        <WorkspaceCreateDialog
          disabled={disabled}
          onCancel={() => setWorkspaceDialogOpen(false)}
          onCreate={(name) => void runAction(async () => {
            await createWorkspace(name);
            setWorkspaceDialogOpen(false);
          })}
        />
      )}
      <nav className="mobileTabs" aria-label="Workbench panels">
        <button className={mobilePanel === "history" ? "is-selected" : ""} onClick={() => setMobilePanel("history")} type="button">
          <PanelLeft size={17} />
          History
        </button>
        <button className={mobilePanel === "transcript" ? "is-selected" : ""} onClick={() => setMobilePanel("transcript")} type="button">
          <MessageSquare size={17} />
          Transcript
        </button>
        {showSessionChrome && (
          <button className={mobilePanel === "status" ? "is-selected" : ""} onClick={() => setMobilePanel("status")} type="button">
            <PanelRight size={17} />
            {activeRightTab ? rightWorkspaceTabLabel(activeRightTab.kind) : "Status"}
          </button>
        )}
      </nav>

      <div
        className={`workbench ${leftCollapsed ? "is-leftCollapsed" : ""} ${rightCollapsed || !showSessionChrome ? "is-rightCollapsed" : ""}`}
        style={{ "--right-column-width": `${rightWidthPx}px` } as CSSProperties}
      >
        <aside className={`historyColumn ${leftCollapsed ? "is-collapsed" : ""} ${mobilePanel === "history" ? "is-mobileSelected" : ""}`}>
          <div className="leftChrome">
            <div className="leftBrandRow">
              <div className="brandMark">
                <span className="brandGlyph"><img alt="Psychevo" src={logoUrl} /></span>
                <div>
                  <h1>Psychevo</h1>
                </div>
              </div>
              <button
                aria-label={leftCollapsed ? "Expand left sidebar" : "Collapse left sidebar"}
                className={`sidebarToggle ${leftCollapsed ? "is-logoToggle" : ""}`}
                onClick={() => setLeftCollapsed((value) => !value)}
                title={leftCollapsed ? "Expand left sidebar" : "Collapse left sidebar"}
                type="button"
              >
                {leftCollapsed ? <img alt="" aria-hidden className="sidebarToggleLogo" src={logoUrl} /> : <PanelLeft size={16} />}
              </button>
            </div>
            <div className="leftActions" aria-label="Session actions">
              <button aria-label="New Session" onClick={() => void runAction(async () => startNewThread())} type="button">
                <MessageSquare size={16} /> <span>New Session</span>
              </button>
              <button aria-label="Search" className={mainView === "search" ? "is-selected" : ""} onClick={() => switchMainView("search")} type="button">
                <Search size={16} /> <span>Search</span>
              </button>
            </div>
            {!leftCollapsed && (
              <>
                <PinnedPanel
                  currentThreadId={currentThreadId}
                  disabled={disabled}
                  sessions={pinnedSessions}
                  onResume={(threadId) => void runAction(async () => {
                    const epoch = beginExplicitViewSwitch();
                    await refreshSnapshot(client, threadId, undefined, false, epoch);
                    updateMainView("transcript");
                    setMobilePanel("transcript");
                  })}
                  onUnpin={togglePinnedSession}
                />
                <HistoryPanel
                  archived={false}
                  currentThreadId={currentThreadId}
                  disabled={disabled}
                  draftSession={null}
                  pinnedSessionIds={pinnedSessionIds}
                  sessions={sessions}
                  onArchive={(threadId) => void runAction(async () => {
                    setDraftSession(null);
                    await client?.request("thread/archive", { threadId });
                    await refreshHistory();
                    await refreshHistory(client, true);
                  })}
                  onDelete={(threadId) => void runAction(async () => {
                    setDraftSession(null);
                    await client?.request("thread/delete", { threadId });
                    await refreshHistory();
                    await refreshHistory(client, true);
                  })}
                  onExport={(threadId) => {
                    if (endpoint) {
                      void host?.open.openDownload(downloadUrl(endpoint, threadId, "export"));
                    }
                  }}
                  onNew={() => void runAction(async () => {
                    await startNewThread();
                  })}
                  onCreateWorkspace={() => setWorkspaceDialogOpen(true)}
                  onNewInWorkdir={(workdir) => void runAction(async () => {
                    await startNewThread(workdir);
                  })}
                  onTogglePinned={togglePinnedSession}
                  onRename={(threadId, title) => void runAction(async () => {
                    await client?.request("thread/rename", { threadId, title });
                    await refreshHistory();
                  })}
                  onRestore={(threadId) => void runAction(async () => {
                    setDraftSession(null);
                    await client?.request("thread/restore", { threadId });
                    await refreshHistory();
                    await refreshHistory(client, true);
                  })}
                  onResumeDraft={() => {
                    switchMainView("transcript");
                    setMobilePanel("transcript");
                  }}
                  onResume={(threadId) => void runAction(async () => {
                    const epoch = beginExplicitViewSwitch();
                    await refreshSnapshot(client, threadId, undefined, false, epoch);
                    updateMainView("transcript");
                    setMobilePanel("transcript");
                  })}
                  onShare={(threadId) => {
                    if (endpoint) {
                      void host?.open.openDownload(downloadUrl(endpoint, threadId, "share"));
                    }
                  }}
                />
              </>
            )}
            <LeftUtilityRail
              value={mainView}
              onChange={(value) => {
                if (value === "settings") {
                  openSettingsSection(settingsSection);
                } else {
                  switchMainView(value);
                }
              }}
            />
          </div>
        </aside>

        <section className={`conversationColumn ${mobilePanel === "transcript" ? "is-mobileSelected" : ""}`}>
          <div className="conversationChrome">
            {showSessionChrome && (
              <button
                aria-label={rightCollapsed ? "Show right inspector" : "Collapse right inspector"}
                className="rightInspectorToggle"
                onClick={() => setRightCollapsed((value) => !value)}
                title={rightCollapsed ? "Show right inspector" : "Collapse right inspector"}
                type="button"
              >
                <PanelRight size={16} />
              </button>
            )}
          </div>
          <div className="centerWorkspace">
            <MainSurface
              appearance={appearance}
              archivedSessions={archivedSessions}
              backendDraft={backendDraft}
              backendDoctor={backendDoctor}
              backends={backends}
              debugEnabled={debugEnabled}
              disabled={disabled}
              mainView={mainView}
              sessions={sessions}
              settingsSection={settingsSection}
              workdir={activeWorkbenchWorkdir}
              loadThreadSearchText={loadThreadSearchText}
              onAppearanceChange={setAppearance}
              onDeleteArchivedSession={(threadId) => void runAction(async () => deleteArchivedSession(threadId))}
              onRestoreArchivedSession={(threadId) => void runAction(async () => restoreArchivedSession(threadId))}
              onDebugChange={setDebugEnabled}
              onCancelBackendEdit={() => setBackendDraft(null)}
              onChangeBackendDraft={setBackendDraft}
              onDeleteBackend={(backend) => void runAction(async () => deleteBackend(backend))}
              onDoctorBackend={(backend) => void runAction(async () => doctorBackend(backend))}
              onEditBackend={(backend) => setBackendDraft(backendDraftFromBackend(backend))}
              onSetBackendEnabled={(backend, enabled) => void runAction(async () => updateBackendDraftFields(backend, { enabled }))}
              onSetBackendEntrypoints={(backend, entrypoints) => void runAction(async () => updateBackendDraftFields(backend, { entrypoints }))}
              onMainViewChange={switchMainView}
              onNewBackend={() => {
                setSettingsSection("agents");
                setBackendDraft({ ...EMPTY_BACKEND_DRAFT });
              }}
              onOpenSession={(threadId) => void runAction(async () => {
                const epoch = beginExplicitViewSwitch();
                await refreshSnapshot(client, threadId, undefined, false, epoch);
                updateMainView("transcript");
                setMobilePanel("transcript");
              })}
              onSettingsSectionChange={setSettingsSection}
              onSaveBackendDraft={(draft) => void runAction(async () => saveBackendDraft(draft))}
              transcript={<TranscriptPanel activity={activity} entries={transcriptEntries} onCopyText={copyTranscriptText} />}
            />
            {showSessionChrome && activeCommandOverlay && (
              <CommandOverlay
                commands={commands}
                feedback={commandFeedback}
                onAlternateAction={(action) => void runAction(async () => runCommandAlternateAction(action))}
                onClose={clearCommandTransientUi}
                onExecute={(slash) => void runAction(async () => executeCommand(slash, "commandOverlay"))}
              />
            )}
          </div>
          {showSessionChrome && <div className="composerDock">
            {(commandFeedback?.feedbackAnchor === "composer" || commandFeedback?.feedbackAnchor === "status") && (
              <CommandFeedbackView
                className="composerCommandFeedback"
                feedback={commandFeedback}
                onAlternateAction={(action) => void runAction(async () => runCommandAlternateAction(action))}
              />
            )}
            <Composer
              attachments={attachments}
              completionProvider={async (text, cursor) => {
                const scope = activeScope ?? init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
                return await client?.request("completion/list", {
                  cursor,
                  scope,
                  text,
                  threadId: snapshot.thread?.id ?? null
                }) ?? { items: [], replacement: null };
              }}
              disabled={disabled}
              draftPatch={composerDraftPatch ?? undefined}
              leftControls={(
                <AgentRunSelector
                  agents={runnableAgents}
                  disabled={disabled}
                  value={selectedAgentName}
                  onChange={(value) => void runAction(async () => changeAgentSelection(value))}
                />
              )}
              mode={workMode}
              rightControls={(
                <ComposerSubmitControls
                  context={contextUsage}
                  controls={controls}
                  usage={sessionUsage}
                  model={selectedModel}
                  variant={selectedVariant}
                  onModelChange={setSelectedModel}
                  onVariantChange={setSelectedVariant}
                />
              )}
              requestPanel={(pendingClarifies.length > 0 || pendingPermissions.length > 0) ? (
                <ComposerRequests
                  clarifies={pendingClarifies}
                  permissions={pendingPermissions}
                  onClarify={(requestId, answer) => void runAction(async () => {
                    await client?.request("clarify/respond", { requestId, threadId: snapshot.thread?.id ?? null, answers: [[answer]] });
                    await refreshSnapshot();
                  })}
                  onPermission={(requestId, decision) => void runAction(async () => {
                    await client?.request("permission/respond", { requestId, threadId: snapshot.thread?.id ?? null, decision });
                    await refreshSnapshot();
                  })}
                />
              ) : null}
              running={running}
              onAttach={() => void runAction(async () => handleAttachment())}
              onCommand={(command) => void runAction(async () => executeCommand(command, "composer"))}
              onInterrupt={() => void runAction(async () => {
                await client?.request("turn/interrupt", { threadId: snapshot.thread?.id ?? null });
                await refreshSnapshot();
              })}
              onModeChange={setWorkMode}
              onRemoveAttachment={(id) => setAttachments((current) => current.filter((attachment) => attachment.id !== id))}
              onShell={(command) => void runAction(async () => startShell(command))}
              onSteer={(text) => void runAction(async () => {
                if (!activity.activeTurnId) {
                  return;
                }
                clearCommandTransientUi();
                setSnapshot((current) => appendOptimisticPrompt(current, text));
                await client?.request("turn/steer", {
                  expectedTurnId: activity.activeTurnId,
                  threadId: snapshot.thread?.id ?? null,
                  text
                });
                await refreshHistory();
              })}
              onSubmit={(text, mentions) => void runAction(async () => submitTurn(text, mentions))}
            />
            <ComposerStatusLine
              branch={settings?.project?.branch ?? null}
              controls={controls}
              path={settings?.project?.displayPath ?? settings?.workdir ?? ""}
              permissionMode={permissionMode}
              profile={init?.profile ?? null}
              onBranchClick={() => {
                void runAction(async () => openDiffPreview(null));
              }}
              onPathClick={() => {
                openRightWorkspaceTab("files");
              }}
              onPermissionModeChange={setPermissionMode}
            />
          </div>}
        </section>

        {showSessionChrome && !rightCollapsed && (
          <aside className={`statusColumn ${mobilePanel === "status" ? "is-mobileSelected" : ""}`}>
            <button
              aria-label="Resize right workspace"
              className="rightResizeHandle"
              onDoubleClick={() => setRightWidthPx(DEFAULT_RIGHT_WIDTH_PX)}
              onPointerDown={(event) => beginRightResize(event)}
              title="Resize right workspace"
              type="button"
            >
              <GripVertical size={15} />
            </button>
            <RightWorkspace
              activeTabId={activeRightTabId}
              activity={activity}
              appearance={appearance}
              client={client}
              context={contextUsage}
              debugEnabled={debugEnabled}
              debugEvents={debugEvents}
              files={workspaceFiles?.entries ?? []}
              root={workspaceFiles?.root ?? settings?.workdir ?? ""}
              scope={activeScope ?? init?.scope ?? null}
              sessionId={snapshot.thread?.id ?? null}
              status={status}
              usage={sessionUsage}
              tabs={rightTabs}
              terminalEvents={terminalEvents}
              trace={traceState}
              truncated={workspaceFiles?.truncated ?? false}
              workdir={settings?.project?.displayPath ?? settings?.workdir ?? ""}
              workspaceChanges={workspaceChanges}
              workspaceDiff={workspaceDiff}
              onActivate={setActiveRightTabId}
              onAcceptChange={(turnId, path) => void runAction(async () => acceptWorkspaceChange(turnId, path))}
              onChangedFile={(path) => void runAction(async () => openDiffPreview(path))}
              onClose={closeRightWorkspaceTab}
              onDirtyTabChange={(tabId, dirty) => {
                setDirtyRightTabs((current) => current[tabId] === dirty ? current : { ...current, [tabId]: dirty });
              }}
              onOpenFile={(path) => void runAction(async () => openFilePreview(path))}
              onOpenKind={(kind) => openRightWorkspaceTab(kind, {}, true)}
              onRejectChange={(turnId, path) => void runAction(async () => rejectWorkspaceChange(turnId, path))}
              onRefresh={() => void runAction(async () => {
                await refreshSnapshot();
                await refreshHistory();
                await refreshAgentSurface();
                await refreshWorkspaceSurface();
              })}
              onRefreshTrace={() => void refreshTrace()}
              onSaveFile={(path, content, expectedRevision, force) => saveFileFromEditor(path, content, expectedRevision, force)}
              onShowHome={() => revealRightWorkspace(null)}
            />
          </aside>
        )}
      </div>
    </main>
  );
}

function LeftUtilityRail({
  value,
  onChange
}: {
  value: MainView;
  onChange(value: MainView): void;
}) {
  const items: Array<{ icon: ReactNode; label: string; value: MainView }> = [
    { icon: <Settings size={16} />, label: "Settings", value: "settings" }
  ];
  return (
    <nav className="leftUtilityRail" aria-label="Workbench utilities">
      {items.map((item) => (
        <button
          className={value === item.value ? "is-selected" : ""}
          key={item.value}
          onClick={() => onChange(item.value)}
          title={item.label}
          type="button"
        >
          {item.icon}
          <span>{item.label}</span>
        </button>
      ))}
    </nav>
  );
}

function WorkspaceCreateDialog({
  disabled,
  onCancel,
  onCreate
}: {
  disabled: boolean;
  onCancel(): void;
  onCreate(name: string): void;
}) {
  const [name, setName] = useState("");
  const trimmed = name.trim();

  return (
    <div
      className="modalBackdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onCancel();
        }
      }}
      role="presentation"
    >
      <form
        aria-label="New workspace"
        className="workspaceDialog"
        onSubmit={(event) => {
          event.preventDefault();
          if (trimmed && !disabled) {
            onCreate(trimmed);
          }
        }}
      >
        <header>
          <div className="workspaceDialogTitle">
            <FolderPlus size={18} aria-hidden />
            <h2>New Workspace</h2>
          </div>
          <button aria-label="Close" onClick={onCancel} title="Close" type="button">
            <X size={15} />
          </button>
        </header>
        <label>
          <span>Name</span>
          <input
            autoFocus
            disabled={disabled}
            onChange={(event) => setName(event.target.value)}
            placeholder="general notes"
            value={name}
          />
        </label>
        <footer>
          <button disabled={disabled} onClick={onCancel} type="button">
            Cancel
          </button>
          <button disabled={disabled || !trimmed} type="submit">
            Create
          </button>
        </footer>
      </form>
    </div>
  );
}

function PinnedPanel({
  currentThreadId,
  disabled,
  sessions,
  onResume,
  onUnpin
}: {
  currentThreadId: string | undefined;
  disabled: boolean;
  sessions: SessionSummary[];
  onResume(threadId: string): void;
  onUnpin(threadId: string): void;
}) {
  return (
    <section className="leftPinnedPanel" aria-label="Pinned sessions">
      <header>
        <Pin size={16} />
        <span>Pinned</span>
      </header>
      {sessions.length === 0 ? (
        <p>No pinned sessions</p>
      ) : (
        <div className="pinnedSessionList">
          {sessions.map((session) => (
            <div className={`pinnedSessionRow ${session.id === currentThreadId ? "is-active" : ""}`} key={session.id}>
              <button disabled={disabled} onClick={() => onResume(session.id)} type="button">
                <span>{session.displayTitle?.trim() || session.title?.trim() || shortSessionId(session.id)}</span>
                <small>{session.project?.label ?? "workspace"}</small>
              </button>
              <button aria-label="Unpin session" disabled={disabled} onClick={() => onUnpin(session.id)} title="Unpin" type="button">
                <X size={13} />
              </button>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

function MainSurface({
  appearance,
  archivedSessions,
  backendDraft,
  backendDoctor,
  backends,
  debugEnabled,
  disabled,
  loadThreadSearchText,
  mainView,
  onAppearanceChange,
  onCancelBackendEdit,
  onChangeBackendDraft,
  onDebugChange,
  onDeleteArchivedSession,
  onDeleteBackend,
  onDoctorBackend,
  onEditBackend,
  onMainViewChange,
  onNewBackend,
  onOpenSession,
  onRestoreArchivedSession,
  onSaveBackendDraft,
  onSetBackendEnabled,
  onSetBackendEntrypoints,
  onSettingsSectionChange,
  settingsSection,
  sessions,
  transcript,
  workdir
}: {
  appearance: Appearance;
  archivedSessions: SessionSummary[];
  backendDraft: BackendDraft | null;
  backendDoctor: Record<string, WorkbenchBackendDoctor>;
  backends: WorkbenchBackend[];
  debugEnabled: boolean;
  disabled: boolean;
  loadThreadSearchText(threadId: string): Promise<string>;
  mainView: MainView;
  onAppearanceChange(value: Appearance): void;
  onCancelBackendEdit(): void;
  onChangeBackendDraft(draft: BackendDraft): void;
  onDebugChange(value: boolean): void;
  onDeleteArchivedSession(threadId: string): void;
  onDeleteBackend(backend: WorkbenchBackend): void;
  onDoctorBackend(backend: WorkbenchBackend): void;
  onEditBackend(backend: WorkbenchBackend): void;
  onMainViewChange(value: MainView): void;
  onNewBackend(): void;
  onOpenSession(threadId: string): void;
  onRestoreArchivedSession(threadId: string): void;
  onSaveBackendDraft(draft: BackendDraft): void;
  onSetBackendEnabled(backend: WorkbenchBackend, enabled: boolean): void;
  onSetBackendEntrypoints(backend: WorkbenchBackend, entrypoints: string[]): void;
  onSettingsSectionChange(value: SettingsSection): void;
  settingsSection: SettingsSection;
  sessions: SessionSummary[];
  transcript: ReactNode;
  workdir: string;
}) {
  if (mainView === "transcript") {
    return <>{transcript}</>;
  }
  if (mainView === "settings") {
    return (
      <SettingsPage
        appearance={appearance}
        archivedSessions={archivedSessions}
        backendDraft={backendDraft}
        backendDoctor={backendDoctor}
        backends={backends}
        debugEnabled={debugEnabled}
        disabled={disabled}
        section={settingsSection}
        onAppearanceChange={onAppearanceChange}
        onCancelBackendEdit={onCancelBackendEdit}
        onChangeBackendDraft={onChangeBackendDraft}
        onDebugChange={onDebugChange}
        onDeleteArchivedSession={onDeleteArchivedSession}
        onDeleteBackend={onDeleteBackend}
        onDoctorBackend={onDoctorBackend}
        onEditBackend={onEditBackend}
        onNewBackend={onNewBackend}
        onOpenTranscript={() => onMainViewChange("transcript")}
        onRestoreArchivedSession={onRestoreArchivedSession}
        onSaveBackendDraft={onSaveBackendDraft}
        onSectionChange={onSettingsSectionChange}
        onSetBackendEnabled={onSetBackendEnabled}
        onSetBackendEntrypoints={onSetBackendEntrypoints}
        workdir={workdir}
      />
    );
  }
  if (mainView === "search") {
    return (
      <SearchPage
        loadThreadSearchText={loadThreadSearchText}
        sessions={sessions}
        onOpenSession={onOpenSession}
        onOpenTranscript={() => onMainViewChange("transcript")}
      />
    );
  }
  return <>{transcript}</>;
}

function SettingsPage({
  appearance,
  archivedSessions,
  backendDraft,
  backendDoctor,
  backends,
  debugEnabled,
  disabled,
  section,
  onAppearanceChange,
  onCancelBackendEdit,
  onChangeBackendDraft,
  onDebugChange,
  onDeleteArchivedSession,
  onDeleteBackend,
  onDoctorBackend,
  onEditBackend,
  onNewBackend,
  onOpenTranscript,
  onRestoreArchivedSession,
  onSaveBackendDraft,
  onSectionChange,
  onSetBackendEnabled,
  onSetBackendEntrypoints,
  workdir
}: {
  appearance: Appearance;
  archivedSessions: SessionSummary[];
  backendDraft: BackendDraft | null;
  backendDoctor: Record<string, WorkbenchBackendDoctor>;
  backends: WorkbenchBackend[];
  debugEnabled: boolean;
  disabled: boolean;
  section: SettingsSection;
  onAppearanceChange(value: Appearance): void;
  onCancelBackendEdit(): void;
  onChangeBackendDraft(draft: BackendDraft): void;
  onDebugChange(value: boolean): void;
  onDeleteArchivedSession(threadId: string): void;
  onDeleteBackend(backend: WorkbenchBackend): void;
  onDoctorBackend(backend: WorkbenchBackend): void;
  onEditBackend(backend: WorkbenchBackend): void;
  onNewBackend(): void;
  onOpenTranscript(): void;
  onRestoreArchivedSession(threadId: string): void;
  onSaveBackendDraft(draft: BackendDraft): void;
  onSectionChange(value: SettingsSection): void;
  onSetBackendEnabled(backend: WorkbenchBackend, enabled: boolean): void;
  onSetBackendEntrypoints(backend: WorkbenchBackend, entrypoints: string[]): void;
  workdir: string;
}) {
  const [query, setQuery] = useState("");
  const active = SETTINGS_SECTIONS.find((item) => item.id === section) ?? SETTINGS_SECTIONS[0]!;
  const normalizedQuery = query.trim().toLowerCase();
  const primarySections = SETTINGS_SECTIONS.filter((item) => item.id !== "archived");
  const archivedSection = SETTINGS_SECTIONS.find((item) => item.id === "archived")!;
  const sectionMatches = (item: (typeof SETTINGS_SECTIONS)[number]) => (
    !normalizedQuery
    || item.label.toLowerCase().includes(normalizedQuery)
    || item.description.toLowerCase().includes(normalizedQuery)
  );
  const visiblePrimarySections = primarySections.filter(sectionMatches);
  const showArchivedSection = sectionMatches(archivedSection);
  return (
    <section className="centerPage settingsPage" aria-label="Settings">
      <div className="settingsShell">
        <aside className="settingsNav" aria-label="Settings sections">
          <div className="settingsNavTop">
            <button className="settingsBackButton" onClick={onOpenTranscript} type="button">
              <ArrowLeft size={15} />
              <span>Back to app</span>
            </button>
            <label className="settingsSearch">
              <Search size={14} aria-hidden />
              <input
                aria-label="Search settings"
                onChange={(event) => setQuery(event.currentTarget.value)}
                placeholder="Search settings"
                type="search"
                value={query}
              />
            </label>
          </div>
          <div className="settingsNavGroups">
            {visiblePrimarySections.map((item) => (
              <button
                aria-current={item.id === section ? "page" : undefined}
                className={item.id === section ? "is-selected" : ""}
                key={item.id}
                onClick={() => onSectionChange(item.id)}
                type="button"
              >
                {settingsSectionIcon(item.id, 15)}
                <span>{item.label}</span>
              </button>
            ))}
            {visiblePrimarySections.length === 0 && !showArchivedSection && (
              <p className="settingsNavEmpty">No settings found</p>
            )}
          </div>
          <div className="settingsNavFooter">
            {showArchivedSection && (
              <button
                aria-current={section === archivedSection.id ? "page" : undefined}
                className={section === archivedSection.id ? "is-selected" : ""}
                onClick={() => onSectionChange(archivedSection.id)}
                type="button"
              >
                {settingsSectionIcon(archivedSection.id, 15)}
                <span>{archivedSection.label}</span>
              </button>
            )}
          </div>
        </aside>
        <div className="settingsContent">
          <header className="settingsSectionHeader">
            <span>{settingsSectionIcon(active.id, 17)}</span>
            <div>
              <h3>{active.label}</h3>
            </div>
          </header>
          <SettingsSectionPanel
            appearance={appearance}
            archivedSessions={archivedSessions}
            backendDraft={backendDraft}
            backendDoctor={backendDoctor}
            backends={backends}
            debugEnabled={debugEnabled}
            disabled={disabled}
            section={section}
            onAppearanceChange={onAppearanceChange}
            onCancelBackendEdit={onCancelBackendEdit}
            onChangeBackendDraft={onChangeBackendDraft}
            onDebugChange={onDebugChange}
            onDeleteArchivedSession={onDeleteArchivedSession}
            onDeleteBackend={onDeleteBackend}
            onDoctorBackend={onDoctorBackend}
            onEditBackend={onEditBackend}
            onNewBackend={onNewBackend}
            onRestoreArchivedSession={onRestoreArchivedSession}
            onSaveBackendDraft={onSaveBackendDraft}
            onSetBackendEnabled={onSetBackendEnabled}
            onSetBackendEntrypoints={onSetBackendEntrypoints}
            workdir={workdir}
          />
        </div>
      </div>
    </section>
  );
}

function SettingsSectionPanel({
  appearance,
  archivedSessions,
  backendDraft,
  backendDoctor,
  backends,
  debugEnabled,
  disabled,
  section,
  onAppearanceChange,
  onCancelBackendEdit,
  onChangeBackendDraft,
  onDebugChange,
  onDeleteArchivedSession,
  onDeleteBackend,
  onDoctorBackend,
  onEditBackend,
  onNewBackend,
  onRestoreArchivedSession,
  onSaveBackendDraft,
  onSetBackendEnabled,
  onSetBackendEntrypoints,
  workdir
}: {
  appearance: Appearance;
  archivedSessions: SessionSummary[];
  backendDraft: BackendDraft | null;
  backendDoctor: Record<string, WorkbenchBackendDoctor>;
  backends: WorkbenchBackend[];
  debugEnabled: boolean;
  disabled: boolean;
  section: SettingsSection;
  onAppearanceChange(value: Appearance): void;
  onCancelBackendEdit(): void;
  onChangeBackendDraft(draft: BackendDraft): void;
  onDebugChange(value: boolean): void;
  onDeleteArchivedSession(threadId: string): void;
  onDeleteBackend(backend: WorkbenchBackend): void;
  onDoctorBackend(backend: WorkbenchBackend): void;
  onEditBackend(backend: WorkbenchBackend): void;
  onNewBackend(): void;
  onRestoreArchivedSession(threadId: string): void;
  onSaveBackendDraft(draft: BackendDraft): void;
  onSetBackendEnabled(backend: WorkbenchBackend, enabled: boolean): void;
  onSetBackendEntrypoints(backend: WorkbenchBackend, entrypoints: string[]): void;
  workdir: string;
}) {
  switch (section) {
    case "appearance":
      return (
        <div className="settingsRows">
          <SettingsOptionRow title="Theme" description="Dark or light Workbench appearance.">
            <div className="segmentedControl">
              <button className={appearance === "dark" ? "is-selected" : ""} onClick={() => onAppearanceChange("dark")} type="button">
                <Moon size={15} /> Dark
              </button>
              <button className={appearance === "light" ? "is-selected" : ""} onClick={() => onAppearanceChange("light")} type="button">
                <Sun size={15} /> Light
              </button>
            </div>
          </SettingsOptionRow>
        </div>
      );
    case "archived":
      return (
        <ArchivedSessionsPanel
          disabled={disabled}
          sessions={archivedSessions}
          onDelete={onDeleteArchivedSession}
          onRestore={onRestoreArchivedSession}
        />
      );
    case "debug":
      return (
        <div className="settingsRows">
          <SettingsOptionRow title="Show debug tab" description="Recent Gateway notifications in the right inspector.">
            <label className="switchControl">
              <input checked={debugEnabled} onChange={(event) => onDebugChange(event.target.checked)} type="checkbox" />
              <span>{debugEnabled ? "On" : "Off"}</span>
            </label>
          </SettingsOptionRow>
        </div>
      );
    case "agents":
      return (
        <AgentsConfigPanel
          backendDraft={backendDraft}
          backendDoctor={backendDoctor}
          backends={backends}
          disabled={disabled}
          onCancelBackendEdit={onCancelBackendEdit}
          onChangeBackendDraft={onChangeBackendDraft}
          onDeleteBackend={onDeleteBackend}
          onDoctorBackend={onDoctorBackend}
          onEditBackend={onEditBackend}
          onNewBackend={onNewBackend}
          onSaveBackendDraft={onSaveBackendDraft}
          onSetBackendEnabled={onSetBackendEnabled}
          onSetBackendEntrypoints={onSetBackendEntrypoints}
          workdir={workdir}
        />
      );
  }
}

function SettingsOptionRow({
  children,
  description,
  title
}: {
  children: ReactNode;
  description: string;
  title: string;
}) {
  return (
    <div className="settingsRow">
      <div>
        <strong>{title}</strong>
        <span>{description}</span>
      </div>
      {children}
    </div>
  );
}

function ArchivedSessionsPanel({
  disabled,
  sessions,
  onDelete,
  onRestore
}: {
  disabled: boolean;
  sessions: SessionSummary[];
  onDelete(threadId: string): void;
  onRestore(threadId: string): void;
}) {
  return (
    <section className="archivedSessionsPanel" aria-label="Archived sessions">
      {sessions.length === 0 ? (
        <p>No archived sessions.</p>
      ) : (
        <div className="archivedSessionList">
          {sessions.map((session) => {
            const title = session.displayTitle?.trim() || session.title?.trim() || shortSessionId(session.id);
            const workspace = session.project?.label || session.project?.displayPath || session.workdir || "workspace";
            return (
              <div className="archivedSessionRow" key={session.id}>
                <div>
                  <strong>{title}</strong>
                  <span>{workspace}</span>
                </div>
                <div className="archivedSessionActions">
                  <button aria-label={`Restore ${title}`} disabled={disabled} onClick={() => onRestore(session.id)} title="Restore" type="button">
                    <RotateCcw size={13} />
                  </button>
                  <button aria-label={`Delete ${title}`} disabled={disabled} onClick={() => onDelete(session.id)} title="Delete" type="button">
                    <Trash2 size={13} />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}

function settingsSectionIcon(section: SettingsSection, size: number): ReactNode {
  switch (section) {
    case "appearance":
      return <Sun size={size} />;
    case "archived":
      return <Archive size={size} />;
    case "debug":
      return <Bug size={size} />;
    case "agents":
      return <Bot size={size} />;
  }
}

function SearchPage({
  loadThreadSearchText,
  sessions,
  onOpenSession,
  onOpenTranscript
}: {
  loadThreadSearchText(threadId: string): Promise<string>;
  sessions: SessionSummary[];
  onOpenSession(threadId: string): void;
  onOpenTranscript(): void;
}) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [searching, setSearching] = useState(false);

  useEffect(() => {
    const needle = normalizeSearchText(query);
    if (!needle) {
      setResults([]);
      setSearching(false);
      return;
    }
    let cancelled = false;
    setSearching(true);
    const timer = window.setTimeout(() => {
      void (async () => {
        const next: SearchResult[] = [];
        const seen = new Set<string>();
        for (const session of sessions) {
          const title = session.displayTitle?.trim() || session.title?.trim() || shortSessionId(session.id);
          const workspace = session.project?.label ?? "";
          const summaryHaystack = normalizeSearchText(`${session.id} ${title} ${session.preview ?? ""} ${workspace} ${session.workdir}`);
          if (summaryHaystack.includes(needle)) {
            next.push({
              excerpt: session.id,
              id: session.id,
              kind: "session",
              subtitle: `${workspace || "workspace"} · ${session.visibleEntryCount ?? session.messageCount ?? 0} entries`,
              title
            });
            seen.add(`${session.id}:session`);
          }
        }
        for (const session of sessions.filter((item) => (item.messageCount ?? 0) > 0)) {
          const text = await loadThreadSearchText(session.id);
          const normalized = normalizeSearchText(text);
          if (cancelled) {
            return;
          }
          if (normalized.includes(needle)) {
            const key = `${session.id}:message`;
            if (!seen.has(key)) {
              next.push({
                excerpt: searchExcerpt(text, query),
                id: session.id,
                kind: "message",
                subtitle: session.displayTitle?.trim() || session.title?.trim() || shortSessionId(session.id),
                title: "Message match"
              });
              seen.add(key);
            }
          }
        }
        if (!cancelled) {
          setResults(next);
          setSearching(false);
        }
      })();
    }, 180);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [loadThreadSearchText, query, sessions]);

  return (
    <section className="centerPage searchPage" aria-label="Search">
      <header>
        <Search size={18} />
        <div>
          <h2>Search</h2>
          <p>Search session ids, session names, and message text.</p>
        </div>
      </header>
      <input autoFocus placeholder="Search current workspace" value={query} onChange={(event) => setQuery(event.target.value)} />
      {query.trim() && results.length > 0 ? (
        <div className="searchResults">
          {results.map((result) => (
            <button key={`${result.id}:${result.kind}`} onClick={() => onOpenSession(result.id)} type="button">
              <strong>{result.title}</strong>
              <span>{result.subtitle}</span>
              <small>{result.excerpt}</small>
            </button>
          ))}
        </div>
      ) : (
        <div className="emptyLedger">
          <span>{query.trim() ? (searching ? "Searching sessions..." : "No matches in this workspace.") : "Type to search local session material."}</span>
          <button onClick={onOpenTranscript} type="button">Back to transcript</button>
        </div>
      )}
    </section>
  );
}

function RightWorkspace({
  activeTabId,
  activity,
  appearance,
  client,
  context,
  debugEnabled,
  debugEvents,
  files,
  root,
  scope,
  sessionId,
  status,
  usage,
  tabs,
  terminalEvents,
  trace,
  truncated,
  workdir,
  workspaceChanges,
  workspaceDiff,
  onActivate,
  onAcceptChange,
  onChangedFile,
  onClose,
  onDirtyTabChange,
  onOpenFile,
  onOpenKind,
  onRejectChange,
  onRefresh,
  onRefreshTrace,
  onSaveFile,
  onShowHome
}: {
  activeTabId: string | null;
  activity: ReturnType<typeof normalizeActivity>;
  appearance: Appearance;
  client: GatewayClient | null;
  context: ContextReadResult | null;
  debugEnabled: boolean;
  debugEvents: DebugEvent[];
  files: WorkspaceFileEntry[];
  root: string;
  scope: GatewayRequestScope | null;
  sessionId: string | null;
  status: string;
  usage: SessionUsageSummaryView | null;
  tabs: RightWorkspaceTab[];
  terminalEvents: TerminalNotificationEvent[];
  trace: TraceState;
  truncated: boolean;
  workdir: string;
  workspaceChanges: WorkspaceChangesResult | null;
  workspaceDiff: WorkspaceDiffResult | null;
  onActivate(tabId: string): void;
  onAcceptChange(turnId: string, path: string): void;
  onChangedFile(path: string): void;
  onClose(tabId: string): void;
  onDirtyTabChange(tabId: string, dirty: boolean): void;
  onOpenFile(path: string): void;
  onOpenKind(kind: RightWorkspaceTabKind): void;
  onRejectChange(turnId: string, path: string): void;
  onRefresh(): void;
  onRefreshTrace(): void;
  onSaveFile(path: string, content: string, expectedRevision: string | null, force: boolean): Promise<WorkspaceFileWriteResult>;
  onShowHome(): void;
}) {
  const activeTab = tabs.find((tab) => tab.id === activeTabId) ?? null;
  return (
    <section className="rightWorkspace" aria-label="Right workspace">
      {tabs.length > 0 && (
        <RightWorkspaceTabs
          activeTabId={activeTabId}
          tabs={tabs}
          onActivate={onActivate}
          onClose={onClose}
          onOpenKind={onOpenKind}
          onShowHome={onShowHome}
        />
      )}
      <div className="rightTabPanels">
        <div className="rightTabPanel" hidden={activeTab !== null}>
          <RightWorkspaceHome
            context={context}
            files={workspaceDiff?.files ?? []}
            sessionId={sessionId}
            usage={usage}
            onChangedFile={onChangedFile}
            onOpenKind={onOpenKind}
            onRefresh={onRefresh}
          />
        </div>
        {tabs.map((tab) => (
          <div className="rightTabPanel" hidden={tab.id !== activeTab?.id} key={tab.id}>
            {tab.kind === "review" && (
              <ReviewPanel
                activity={activity}
                changedFiles={workspaceDiff?.files ?? []}
                changes={workspaceChanges}
                context={context}
                diff={tab.diff ?? workspaceDiff}
                root={root || workdir}
                sessionId={sessionId}
                status={status}
                workdir={workdir}
                onAcceptChange={onAcceptChange}
                onChangedFile={onChangedFile}
                onRejectChange={onRejectChange}
                onRefresh={onRefresh}
              />
            )}
            {tab.kind === "files" && (
              <FilesPanel
                files={files}
                preview={tab.file ?? null}
                previewMessage={tab.message ?? null}
                root={root}
                selectedPath={tab.path ?? null}
                tabId={tab.id}
                truncated={truncated}
                onCompare={onChangedFile}
                onDirtyChange={onDirtyTabChange}
                onOpen={onOpenFile}
                onSave={onSaveFile}
              />
            )}
            {tab.kind === "terminal" && (
              <TerminalPanel
                appearance={appearance}
                client={client}
                scope={scope}
                terminalEvents={terminalEvents}
                workdir={workdir}
              />
            )}
            {tab.kind === "debug" && debugEnabled && (
              <DebugPanel
                events={debugEvents}
                trace={trace}
                onRefreshTrace={onRefreshTrace}
              />
            )}
          </div>
        ))}
      </div>
    </section>
  );
}

function RightWorkspaceTabs({
  activeTabId,
  tabs,
  onActivate,
  onClose,
  onOpenKind,
  onShowHome
}: {
  activeTabId: string | null;
  tabs: RightWorkspaceTab[];
  onActivate(tabId: string): void;
  onClose(tabId: string): void;
  onOpenKind(kind: RightWorkspaceTabKind): void;
  onShowHome(): void;
}) {
  const menuItems: Array<{ icon: ReactNode; kind: RightWorkspaceTabKind; label: string }> = [
    { icon: <GitPullRequest size={14} />, kind: "review", label: "Review" },
    { icon: <TerminalSquare size={14} />, kind: "terminal", label: "Terminal" },
    { icon: <FolderTree size={14} />, kind: "files", label: "Files" }
  ];
  return (
    <div className="rightWorkspaceTabs" aria-label="Right workspace tabs">
      <button
        aria-label="Workspace home"
        className={activeTabId === null ? "is-selected" : ""}
        onClick={onShowHome}
        title="Workspace home"
        type="button"
      >
        <Home size={14} />
      </button>
      {tabs.map((tab) => (
        <div className={`rightWorkspaceTab ${tab.id === activeTabId ? "is-selected" : ""}`} key={tab.id}>
          <button onClick={() => onActivate(tab.id)} title={tab.title} type="button">
            {rightWorkspaceTabIcon(tab.kind)}
            <span>{tab.title}</span>
          </button>
          <button aria-label={`Close ${tab.title}`} onClick={() => onClose(tab.id)} title="Close" type="button">
            <X size={12} />
          </button>
        </div>
      ))}
      <DismissibleDetails
        className="rightAddMenu"
        summary={<Plus size={15} />}
        summaryProps={{ "aria-label": "Open right workspace tab", title: "Open tab" }}
      >
        {({ close }) => (
          <div role="menu" aria-label="Open right workspace tab">
            {menuItems.map((item) => (
              <button
                key={item.kind}
                onClick={() => {
                  close();
                  onOpenKind(item.kind);
                }}
                role="menuitem"
                type="button"
              >
                {item.icon}
                <span>{item.label}</span>
              </button>
            ))}
          </div>
        )}
      </DismissibleDetails>
    </div>
  );
}

function RightWorkspaceHome({
  context,
  files,
  sessionId,
  usage,
  onChangedFile,
  onOpenKind,
  onRefresh
}: {
  context: ContextReadResult | null;
  files: WorkspaceDiffResult["files"];
  sessionId: string | null;
  usage: SessionUsageSummaryView | null;
  onChangedFile(path: string): void;
  onOpenKind(kind: RightWorkspaceTabKind): void;
  onRefresh(): void;
}) {
  return (
    <section className="rightWorkspaceHome" aria-label="Workspace status">
      <header>
        <div>
          <h2>Status</h2>
          <p className="rightWorkspaceSessionId" title={sessionId ?? undefined}>{sessionId ?? "draft"}</p>
        </div>
        <button aria-label="Refresh workspace" onClick={onRefresh} title="Refresh" type="button">
          <RefreshCw size={15} />
        </button>
      </header>
      <SessionObservability context={context} usage={usage} showCategories />
      <nav className="rightHomeNav" aria-label="Open workspace tab">
        <button onClick={() => onOpenKind("review")} type="button">
          <GitPullRequest size={16} />
          <span>Review</span>
        </button>
        <button onClick={() => onOpenKind("terminal")} type="button">
          <TerminalSquare size={16} />
          <span>Terminal</span>
        </button>
        <button onClick={() => onOpenKind("files")} type="button">
          <FolderTree size={16} />
          <span>Files</span>
        </button>
      </nav>
      <div className="rightChangedFiles">
        <div className="rightSectionLabel">
          <span>Changed files</span>
          <b>{files.length}</b>
        </div>
        {files.slice(0, 8).map((file) => (
          <button key={`${file.status}:${file.path}`} onClick={() => onChangedFile(file.path)} type="button">
            <span>{file.path}</span>
            <small>{file.status}</small>
          </button>
        ))}
        {files.length === 0 && <p>No changed files.</p>}
      </div>
    </section>
  );
}

function SessionObservability({
  context,
  usage,
  showCategories = false
}: {
  context: ContextReadResult | null;
  usage: SessionUsageSummaryView | null;
  showCategories?: boolean;
}) {
  const contextPercent = normalizedPercent(context?.percent);
  const orderedCategories = orderedContextCategories(context?.categories ?? []);
  const rawContextPercent = context?.percent;
  const contextPercentAvailable = typeof rawContextPercent === "number" && Number.isFinite(rawContextPercent);
  return (
    <section className="sessionObservability" aria-label="Session observability">
      <div className="sessionObservabilitySummary">
        <strong>{context?.label ?? "No active context"}</strong>
        <small>{context?.status ?? "unavailable"}</small>
      </div>
      {showCategories && orderedCategories.length > 0 && (
        <PromptTokenStack
          categories={orderedCategories}
          contextPercent={contextPercent}
          contextPercentAvailable={contextPercentAvailable}
        />
      )}
      {usage?.available ? (
        <SessionUsageGrid usage={usage} />
      ) : (
        <p className="sessionObservabilityEmpty">No session usage yet.</p>
      )}
      {showCategories && orderedCategories.length > 0 && (
        <details className="promptTokensDisclosure">
          <summary>
            <span>Prompt tokens</span>
            <ChevronRight size={13} aria-hidden="true" />
          </summary>
          <div className="promptTokenCategoryList">
            {orderedCategories.map((category) => (
              <PromptTokenCategory category={category} key={category.id} />
            ))}
          </div>
        </details>
      )}
    </section>
  );
}

function PromptTokenStack({
  categories,
  contextPercent,
  contextPercentAvailable
}: {
  categories: ContextUsageCategory[];
  contextPercent: number;
  contextPercentAvailable: boolean;
}) {
  const totalTokens = categories.reduce((total, category) => total + Math.max(0, category.tokens), 0);
  const rawSegments = categories.map((category) => {
    const categoryPercent = typeof category.percent === "number" && Number.isFinite(category.percent)
      ? Math.max(0, category.percent)
      : totalTokens > 0
        ? (Math.max(0, category.tokens) / totalTokens) * (contextPercentAvailable ? contextPercent : 100)
        : 0;
    return { category, percent: categoryPercent };
  }).filter((segment) => segment.percent > 0 || segment.category.tokens > 0);
  const rawPercentTotal = rawSegments.reduce((total, segment) => total + segment.percent, 0);
  const scale = rawPercentTotal > 100 ? 100 / rawPercentTotal : 1;
  const displayPercentTotal = Math.min(100, rawPercentTotal * scale);
  const freePercent = contextPercentAvailable ? Math.max(0, 100 - displayPercentTotal) : 0;
  if (rawSegments.length === 0) {
    return <div className="promptTokenStack is-empty" aria-label="No prompt token categories" />;
  }
  return (
    <div
      className="promptTokenStack"
      aria-label={`Prompt token categories use ${formatPercentPrecise(displayPercentTotal)} of the context window`}
    >
      {rawSegments.map(({ category, percent }) => {
        const title = `${contextCategoryLabel(category)}: ${formatTokenEstimate(category.tokens, category.estimated)} (${formatPercentPrecise(category.percent ?? percent)})`;
        return (
          <span
            aria-label={title}
            className="promptTokenSegment"
            key={category.id}
            style={{
              "--prompt-token-color": promptTokenCategoryColor(category.id),
              "--prompt-token-width": `${Math.max(0, percent * scale)}%`
            } as CSSProperties}
            title={title}
          />
        );
      })}
      {freePercent > 0 && (
        <span
          aria-hidden="true"
          className="promptTokenFreeSpace"
          style={{ "--prompt-token-width": `${freePercent}%` } as CSSProperties}
        />
      )}
    </div>
  );
}

function SessionUsageGrid({
  compact = false,
  usage
}: {
  compact?: boolean;
  usage: SessionUsageSummaryView;
}) {
  const metrics = [
    { label: "Session tokens", value: formatCompactNumber(usage.reportedTotalTokens) },
    { label: "Cache read", value: formatPercent(usage.cacheReadPercent) },
    { label: "Cost", value: formatNanodollars(usage.estimatedCostNanodollars) },
    { label: "Reasoning", value: formatCompactNumber(usage.reasoningTokens) },
    { label: "Input", value: formatCompactNumber(usage.billableInputTokens) },
    { label: "Output", value: formatCompactNumber(usage.billableOutputTokens) },
    { label: "Cache write", value: formatCompactNumber(usage.cacheWriteTokens) }
  ];
  const visibleMetrics = compact ? metrics.slice(0, 4) : metrics;
  return (
    <div className={compact ? "composerContextUsageGrid" : "sessionUsageGrid"}>
      {visibleMetrics.map((metric) => (
        <div key={metric.label} title={metric.value}>
          <span>{metric.label}</span>
          <strong>{metric.value}</strong>
        </div>
      ))}
    </div>
  );
}

function PromptTokenCategory({ category }: { category: ContextUsageCategory }) {
  const rows = contextCategoryDetailRows(category);
  return (
    <div className="promptTokenCategory">
      <div className="promptTokenCategorySummary">
        <span>{contextCategoryLabel(category)}</span>
        <strong>{formatTokenEstimate(category.tokens, category.estimated)}</strong>
        <small>{formatPercentPrecise(category.percent)}</small>
      </div>
      {rows.length > 0 && (
        <dl className="promptTokenCategoryDetails">
          {rows.map((row) => (
            <div key={`${row.label}:${row.value}`}>
              <dt>{row.label}</dt>
              <dd>{row.value}</dd>
            </div>
          ))}
        </dl>
      )}
    </div>
  );
}

function ReviewPanel({
  activity,
  changedFiles,
  changes,
  context,
  diff,
  root,
  sessionId,
  status,
  workdir,
  onAcceptChange,
  onChangedFile,
  onRejectChange,
  onRefresh
}: {
  activity: ReturnType<typeof normalizeActivity>;
  changedFiles: WorkspaceDiffResult["files"];
  changes: WorkspaceChangesResult | null;
  context: ContextReadResult | null;
  diff: WorkspaceDiffResult | null;
  root: string;
  sessionId: string | null;
  status: string;
  workdir: string;
  onAcceptChange(turnId: string, path: string): void;
  onChangedFile(path: string): void;
  onRejectChange(turnId: string, path: string): void;
  onRefresh(): void;
}) {
  const [filesOpen, setFilesOpen] = useState(false);
  const contextPercent = typeof context?.percent === "number" ? Math.round(context.percent) : 0;
  const changedTreeItems = useMemo(() => changedFileTreeItems(changedFiles), [changedFiles]);
  const selectedPath = diff?.selectedPath ?? diff?.files[0]?.path ?? null;
  return (
    <section className={`reviewPanel ${filesOpen ? "has-fileTree" : ""}`} aria-label="Review">
      <header>
        <GitPullRequest size={17} />
        <div>
          <h2>Review</h2>
          <p>{workdir || "workspace"}</p>
        </div>
        <div className="rightPanelActions">
          <button
            aria-label={filesOpen ? "Hide changed files" : "Show changed files"}
            aria-pressed={filesOpen}
            className={`reviewFilesToggle ${filesOpen ? "is-pressed" : ""}`}
            onClick={() => setFilesOpen((value) => !value)}
            title="Files"
            type="button"
          >
            <FolderTree size={14} />
            <span>Files</span>
          </button>
          <button aria-label="Refresh Review" onClick={onRefresh} title="Refresh" type="button">
            <RefreshCw size={15} />
          </button>
        </div>
      </header>
      <div className="reviewStatusRows">
        <span>{status}</span>
        <span>{sessionId ? shortSessionId(sessionId) : "draft"}</span>
        <span>{activity.running ? "running" : "idle"}</span>
        <span>{context?.available ? `${contextPercent}% context` : "no context"}</span>
      </div>
      <ReviewChanges
        changes={changes}
        onAcceptChange={onAcceptChange}
        onChangedFile={onChangedFile}
        onRejectChange={onRejectChange}
      />
      <div className="reviewSplit">
        <div className="reviewDiffPane">
          <DiffPreview diff={diff} root={root} />
        </div>
        {filesOpen && (
          <aside className="reviewFilesPane" aria-label="Changed files">
            <WorkspaceFileTree
              emptyLabel="No changed files."
              filterLabel="Filter changed files"
              filterPlaceholder="Filter files..."
              items={changedTreeItems}
              selectedPath={selectedPath}
              onOpen={onChangedFile}
            />
          </aside>
        )}
      </div>
    </section>
  );
}

function ReviewChanges({
  changes,
  onAcceptChange,
  onChangedFile,
  onRejectChange
}: {
  changes: WorkspaceChangesResult | null;
  onAcceptChange(turnId: string, path: string): void;
  onChangedFile(path: string): void;
  onRejectChange(turnId: string, path: string): void;
}) {
  const groups = changes?.groups ?? [];
  if (groups.length === 0) {
    return (
      <div className="reviewChanges is-empty">
        <p>No turn changes.</p>
      </div>
    );
  }
  return (
    <div className="reviewChanges" aria-label="Turn changes">
      {groups.map((group) => (
        <section className="reviewChangeGroup" key={group.turnId}>
          <header>
            <span>{shortSessionId(group.turnId)}</span>
            <b>{group.files.length}</b>
          </header>
          {group.files.map((file) => (
            <div className={`reviewChangeFile is-${file.reviewStatus}`} key={`${group.turnId}:${file.path}`}>
              <button className="reviewChangePath" onClick={() => onChangedFile(file.path)} title={file.path} type="button">
                <span>{file.path}</span>
                <small>{file.status}</small>
              </button>
              <span className="reviewChangeState">{file.reviewStatus}</span>
              <button
                aria-label={`Accept ${file.path}`}
                disabled={file.reviewStatus === "accepted"}
                onClick={() => onAcceptChange(group.turnId, file.path)}
                title="Accept"
                type="button"
              >
                <Check size={13} />
              </button>
              <button
                aria-label={`Reject ${file.path}`}
                disabled={!file.canReject || file.reviewStatus === "rejected"}
                onClick={() => onRejectChange(group.turnId, file.path)}
                title={file.message ?? "Reject"}
                type="button"
              >
                <RotateCcw size={13} />
              </button>
              {file.message && <em title={file.message}>{file.message}</em>}
            </div>
          ))}
        </section>
      ))}
    </div>
  );
}

function DiffPreview({ diff, root }: { diff: WorkspaceDiffResult | null; root: string }) {
  const diffText = useMemo(() => {
    if (!diff) {
      return "";
    }
    if (diff.unifiedDiff.trim()) {
      return diff.unifiedDiff;
    }
    return diff.files
      .map((file) => file.placeholder)
      .filter((value): value is string => Boolean(value?.trim()))
      .join("\n\n");
  }, [diff]);
  const files = useMemo(() => parseUnifiedDiff(diffText), [diffText]);
  const statusByPath = useMemo(() => {
    const map = new Map<string, string>();
    for (const file of diff?.files ?? []) {
      map.set(file.path, file.status);
    }
    return map;
  }, [diff?.files]);

  if (!diff || !diffText.trim()) {
    return (
      <div className="diffPreview is-empty">
        <p>No diff content.</p>
      </div>
    );
  }

  return (
    <div className="diffPreview" aria-label="Diff preview">
      {diff.truncation.truncated && (
        <div className="diffNotice">
          Diff truncated after {diff.truncation.maxLines} lines.
        </div>
      )}
      {files.map((file, fileIndex) => {
        const status = statusByPath.get(file.path) ?? null;
        const statusToken = diffStatusToken(status);
        const stats = diffLineStats(file);
        return (
          <article className="diffFile" key={`${file.path}:${fileIndex}`}>
            <header title={absoluteWorkspacePath(root, file.path)}>
              <span className={`diffFileStatus ${statusToken.className}`} title={statusToken.title}>
                {statusToken.label}
              </span>
              <span className="diffFilePath">{normalizedWorkspacePath(file.path)}</span>
              <span className="diffFileStats" aria-label={`${stats.additions} additions, ${stats.deletions} deletions`}>
                <span className="diffAddStat">+{stats.additions}</span>
                <span className="diffDeleteStat">-{stats.deletions}</span>
              </span>
            </header>
            {file.hunks.length === 0 ? (
              <p className="diffEmptyHunk">No line diff available.</p>
            ) : (
              file.hunks.map((hunk, hunkIndex) => (
                <section className="diffHunk" key={`${hunk.header}:${hunkIndex}`}>
                  <div className="diffHunkHeader">{hunk.header}</div>
                  <div className="diffLines">
                    {hunk.lines.map((line, lineIndex) => (
                      <div className={`diffLine is-${line.kind}`} key={`${line.oldNumber}:${line.newNumber}:${lineIndex}`}>
                        <span className="diffLineNumber">{line.oldNumber ?? ""}</span>
                        <span className="diffLineNumber">{line.newNumber ?? ""}</span>
                        <span className="diffLineMarker">{line.marker}</span>
                        <code>{line.text || " "}</code>
                      </div>
                    ))}
                  </div>
                </section>
              ))
            )}
          </article>
        );
      })}
    </div>
  );
}

function diffLineStats(file: ParsedDiffFile): { additions: number; deletions: number } {
  let additions = 0;
  let deletions = 0;
  for (const hunk of file.hunks) {
    for (const line of hunk.lines) {
      if (line.kind === "add") {
        additions += 1;
      }
      if (line.kind === "delete") {
        deletions += 1;
      }
    }
  }
  return { additions, deletions };
}

function diffStatusToken(status: string | null): { className: string; label: string; title: string } {
  switch (status) {
    case "added":
      return { className: "is-added", label: "A+", title: "Added" };
    case "deleted":
      return { className: "is-deleted", label: "D-", title: "Deleted" };
    case "renamed":
      return { className: "is-renamed", label: "R↷", title: "Renamed" };
    case "untracked":
      return { className: "is-added", label: "U+", title: "Untracked" };
    case "modified":
    default:
      return { className: "is-modified", label: "M↓", title: status ?? "Modified" };
  }
}

function WorkspaceFileTree({
  emptyLabel,
  filterLabel,
  filterPlaceholder,
  items,
  selectedPath,
  onOpen
}: {
  emptyLabel: string;
  filterLabel: string;
  filterPlaceholder: string;
  items: WorkspaceFileTreeItem[];
  selectedPath: string | null;
  onOpen(path: string): void;
}) {
  const [collapsedDirs, setCollapsedDirs] = useState<Set<string>>(() => new Set());
  const [filter, setFilter] = useState("");
  const directoryPaths = useMemo(
    () => new Set(items.filter((item) => item.kind === "directory").map((item) => item.path)),
    [items]
  );
  const visibleItems = useMemo(
    () => visibleWorkspaceTreeItems(items, collapsedDirs, filter),
    [collapsedDirs, filter, items]
  );

  useEffect(() => {
    setCollapsedDirs((current) => {
      const next = new Set([...current].filter((path) => directoryPaths.has(path)));
      return next.size === current.size ? current : next;
    });
  }, [directoryPaths]);

  function toggleDirectory(path: string) {
    setCollapsedDirs((current) => {
      const next = new Set(current);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }

  return (
    <div className="workspaceFileTree">
      <label className="workspaceFileTreeFilter">
        <Search size={14} aria-hidden />
        <input
          aria-label={filterLabel}
          onChange={(event) => setFilter(event.currentTarget.value)}
          placeholder={filterPlaceholder}
          type="search"
          value={filter}
        />
      </label>
      <div className="fileTree" role="tree">
        {visibleItems.map((item) => {
          const directory = item.kind === "directory";
          const collapsed = directory && collapsedDirs.has(item.path);
          const selected = !directory && selectedPath === item.path;
          const badge = item.badge ?? item.status ?? null;
          return (
            <button
              aria-expanded={directory ? !collapsed : undefined}
              aria-selected={selected || undefined}
              className={[
                directory ? "is-directory" : "is-file",
                selected ? "is-selected" : "",
                item.status ? `is-${item.status}` : ""
              ].filter(Boolean).join(" ")}
              disabled={item.disabled}
              key={`${item.kind}:${item.path}`}
              onClick={() => directory ? toggleDirectory(item.path) : onOpen(item.path)}
              role="treeitem"
              style={{ "--depth": item.depth } as CSSProperties}
              title={item.path}
              type="button"
            >
              <span className="fileTreeDisclosure" aria-hidden>
                {directory ? (collapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />) : null}
              </span>
              {directory ? <FolderTree size={14} /> : <FileText size={14} />}
              <span>{item.name}</span>
              {badge && <small>{badge}</small>}
            </button>
          );
        })}
        {visibleItems.length === 0 && <p>{emptyLabel}</p>}
      </div>
    </div>
  );
}

function FilesPanel({
  files,
  preview,
  previewMessage,
  root,
  selectedPath,
  tabId,
  truncated,
  onCompare,
  onDirtyChange,
  onOpen,
  onSave
}: {
  files: WorkspaceFileEntry[];
  preview: WorkspaceFileReadResult | null;
  previewMessage: string | null;
  root: string;
  selectedPath: string | null;
  tabId: string;
  truncated: boolean;
  onCompare(path: string): void;
  onDirtyChange(tabId: string, dirty: boolean): void;
  onOpen(path: string): void;
  onSave(path: string, content: string, expectedRevision: string | null, force: boolean): Promise<WorkspaceFileWriteResult>;
}) {
  const treeItems = useMemo(() => workspaceFileTreeItems(files), [files]);
  const previewPath = preview?.path ?? selectedPath ?? "";
  const previewLabel = previewPath ? absoluteWorkspacePath(root, previewPath) : "Preview";
  const previewContent = typeof preview?.content === "string" ? preview.content : null;
  const editable = Boolean(previewContent !== null && preview && preview.editable !== false && !preview.binary && !preview.truncated);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [baseRevision, setBaseRevision] = useState<string | null>(null);
  const [wrap, setWrap] = useState(true);
  const [findText, setFindText] = useState("");
  const [goLine, setGoLine] = useState("");
  const [cursor, setCursor] = useState({ line: 1, column: 1 });
  const [saving, setSaving] = useState(false);
  const [editorError, setEditorError] = useState<string | null>(null);
  const [conflict, setConflict] = useState<string | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const lineNumbersRef = useRef<HTMLPreElement | null>(null);
  const dirty = editing && draft !== (previewContent ?? "");
  const lineCount = Math.max(1, draft.split("\n").length);

  useEffect(() => {
    setEditing(false);
    setDraft(previewContent ?? "");
    setBaseRevision(preview?.revision ?? null);
    setEditorError(null);
    setConflict(null);
    updateCursorFromText(previewContent ?? "", 0, setCursor);
  }, [preview?.path, preview?.revision, previewContent]);

  useEffect(() => {
    onDirtyChange(tabId, dirty);
  }, [dirty, onDirtyChange, tabId]);

  function confirmDiscard(): boolean {
    return !dirty || window.confirm("Discard unsaved file edits?");
  }

  function openTreePath(path: string) {
    if (!confirmDiscard()) {
      return;
    }
    setEditing(false);
    onOpen(path);
  }

  function exitEditMode() {
    if (!confirmDiscard()) {
      return;
    }
    setEditing(false);
    setDraft(previewContent ?? "");
    setEditorError(null);
    setConflict(null);
  }

  async function saveDraft(force = false) {
    if (!previewPath || saving) {
      return;
    }
    setSaving(true);
    setEditorError(null);
    setConflict(null);
    try {
      const result = await onSave(previewPath, draft, baseRevision, force);
      setBaseRevision(result.revision);
      setDraft(draft);
      setEditing(true);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (message.includes("changed on disk")) {
        setConflict(message);
      } else {
        setEditorError(message);
      }
    } finally {
      setSaving(false);
    }
  }

  function handleTextareaKeyDown(event: ReactKeyboardEvent<HTMLTextAreaElement>) {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
      event.preventDefault();
      void saveDraft(false);
      return;
    }
    if (event.key === "Tab") {
      event.preventDefault();
      const input = event.currentTarget;
      const start = input.selectionStart;
      const end = input.selectionEnd;
      const next = `${draft.slice(0, start)}  ${draft.slice(end)}`;
      setDraft(next);
      requestAnimationFrame(() => {
        input.selectionStart = start + 2;
        input.selectionEnd = start + 2;
        updateCursorFromText(next, start + 2, setCursor);
      });
    }
  }

  function findInDraft() {
    if (!findText || !textareaRef.current) {
      return;
    }
    const start = textareaRef.current.selectionEnd;
    let index = draft.indexOf(findText, start);
    if (index < 0) {
      index = draft.indexOf(findText);
    }
    if (index >= 0) {
      textareaRef.current.focus();
      textareaRef.current.selectionStart = index;
      textareaRef.current.selectionEnd = index + findText.length;
      updateCursorFromText(draft, index, setCursor);
    }
  }

  function jumpToLine() {
    const line = Math.max(1, Number.parseInt(goLine, 10) || 1);
    const lines = draft.split("\n");
    let index = 0;
    for (let i = 0; i < Math.min(line - 1, lines.length - 1); i += 1) {
      index += (lines[i] ?? "").length + 1;
    }
    textareaRef.current?.focus();
    if (textareaRef.current) {
      textareaRef.current.selectionStart = index;
      textareaRef.current.selectionEnd = index;
    }
    updateCursorFromText(draft, index, setCursor);
  }

  return (
    <section className="filesPanel" aria-label="Workspace files">
      <header>
        <FolderTree size={17} />
        <div>
          <h2>Files</h2>
        </div>
      </header>
      <div className="filesSplit">
        <div className="filePreview">
          <div className="rightSectionLabel filePreviewPath">
            <span>{previewLabel}</span>
            {preview?.truncated && <b>truncated</b>}
            {dirty && <b>unsaved</b>}
            {previewContent !== null && !editing && (
              <button
                aria-label={`Edit ${previewPath}`}
                disabled={!editable}
                onClick={() => {
                  setDraft(previewContent);
                  setBaseRevision(preview?.revision ?? null);
                  setEditing(true);
                  requestAnimationFrame(() => textareaRef.current?.focus());
                }}
                title={editable ? "Edit" : preview?.editableReason ?? "This file cannot be edited"}
                type="button"
              >
                <Edit3 size={13} />
              </button>
            )}
          </div>
          {previewContent !== null ? (
            editing ? (
              <div className="fileEditor">
                <div className="fileEditorToolbar">
                  <label>
                    <Search size={13} aria-hidden />
                    <input
                      aria-label="Find in file"
                      onChange={(event) => setFindText(event.currentTarget.value)}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") {
                          event.preventDefault();
                          findInDraft();
                        }
                      }}
                      placeholder="Find"
                      type="search"
                      value={findText}
                    />
                  </label>
                  <button aria-label="Find next" onClick={findInDraft} title="Find next" type="button">
                    <Search size={13} />
                  </button>
                  <label>
                    <span>:</span>
                    <input
                      aria-label="Go to line"
                      inputMode="numeric"
                      onChange={(event) => setGoLine(event.currentTarget.value)}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") {
                          event.preventDefault();
                          jumpToLine();
                        }
                      }}
                      placeholder="Line"
                      value={goLine}
                    />
                  </label>
                  <button aria-label="Go to line" onClick={jumpToLine} title="Go to line" type="button">
                    <ChevronRight size={13} />
                  </button>
                  <button
                    aria-label="Toggle word wrap"
                    aria-pressed={wrap}
                    onClick={() => setWrap((value) => !value)}
                    title="Toggle word wrap"
                    type="button"
                  >
                    <FileText size={13} />
                  </button>
                  <span>{cursor.line}:{cursor.column}</span>
                  <button aria-label="Save file" disabled={!dirty || saving} onClick={() => void saveDraft(false)} title="Save" type="button">
                    <Save size={13} />
                  </button>
                  <button aria-label="Exit edit mode" onClick={exitEditMode} title="Exit edit mode" type="button">
                    <X size={13} />
                  </button>
                </div>
                <div className={`fileEditorBody ${wrap ? "is-wrapped" : "is-unwrapped"}`}>
                  <pre className="fileEditorLines" aria-hidden ref={lineNumbersRef}>
                    {Array.from({ length: lineCount }, (_, index) => index + 1).join("\n")}
                  </pre>
                  <textarea
                    ref={textareaRef}
                    aria-label={`Edit ${previewPath}`}
                    onChange={(event) => {
                      setDraft(event.currentTarget.value);
                      updateCursorFromText(event.currentTarget.value, event.currentTarget.selectionStart, setCursor);
                    }}
                    onClick={(event) => updateCursorFromText(draft, event.currentTarget.selectionStart, setCursor)}
                    onKeyDown={handleTextareaKeyDown}
                    onKeyUp={(event) => updateCursorFromText(draft, event.currentTarget.selectionStart, setCursor)}
                    onScroll={(event) => {
                      if (lineNumbersRef.current) {
                        lineNumbersRef.current.scrollTop = event.currentTarget.scrollTop;
                      }
                    }}
                    spellCheck={false}
                    value={draft}
                    wrap={wrap ? "soft" : "off"}
                  />
                </div>
                {(editorError || conflict) && (
                  <div className="fileEditorNotice">
                    <AlertTriangle size={14} />
                    <span>{conflict ?? editorError}</span>
                    {conflict && (
                      <>
                        <button onClick={() => onCompare(previewPath)} type="button">Compare</button>
                        <button onClick={() => openTreePath(previewPath)} type="button">Reload</button>
                        <button onClick={() => void saveDraft(true)} type="button">Force</button>
                      </>
                    )}
                  </div>
                )}
              </div>
            ) : isMarkdownFile(previewPath) ? (
              <div className="fileMarkdownPreview">
                <MarkdownText text={previewContent} />
              </div>
            ) : (
              <HighlightedCodePreview content={previewContent} path={previewPath} />
            )
          ) : (
            <p>{previewMessage ?? "Select a text file to preview."}</p>
          )}
        </div>
        <aside className="filesTreePane" aria-label="Workspace file tree">
          <WorkspaceFileTree
            emptyLabel="No workspace files."
            filterLabel="Filter workspace files"
            filterPlaceholder="Filter files..."
            items={treeItems}
            selectedPath={selectedPath}
            onOpen={openTreePath}
          />
          {truncated && <footer>File tree truncated.</footer>}
        </aside>
      </div>
    </section>
  );
}

function updateCursorFromText(
  text: string,
  position: number,
  setCursor: (cursor: { line: number; column: number }) => void
) {
  const bounded = Math.max(0, Math.min(position, text.length));
  const before = text.slice(0, bounded);
  const lines = before.split("\n");
  setCursor({
    line: lines.length,
    column: (lines.at(-1)?.length ?? 0) + 1
  });
}

function HighlightedCodePreview({ content, path }: { content: string; path: string }) {
  const language = useMemo(() => languageForPath(path), [path]);
  const html = useMemo(() => highlightToHtml(content, language), [content, language]);
  return (
    <pre className="rightCodePreview hljs" data-lang={language || undefined}>
      <code dangerouslySetInnerHTML={{ __html: html }} />
    </pre>
  );
}

function TerminalPanel({
  appearance,
  client,
  scope,
  terminalEvents,
  workdir
}: {
  appearance: Appearance;
  client: GatewayClient | null;
  scope: GatewayRequestScope | null;
  terminalEvents: TerminalNotificationEvent[];
  workdir: string;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const terminalRef = useRef<XTermTerminal | null>(null);
  const fitRef = useRef<{ fit(): void } | null>(null);
  const terminalIdRef = useRef<string | null>(null);
  const lastEventSeqRef = useRef(0);
  const [terminalId, setTerminalId] = useState<string | null>(null);
  const [state, setState] = useState<"starting" | "running" | "exited" | "error">("starting");
  const [message, setMessage] = useState("Starting terminal...");

  useEffect(() => {
    if (!client || !scope || !containerRef.current) {
      setState("error");
      setMessage("Terminal is unavailable until the gateway is connected.");
      return;
    }
    let cancelled = false;
    let dataDisposable: { dispose(): void } | null = null;
    let resizeObserver: ResizeObserver | null = null;
    void Promise.all([
      import("@xterm/xterm"),
      import("@xterm/addon-fit")
    ]).then(([xterm, fitModule]) => {
      if (cancelled || !containerRef.current) {
        return;
      }
      const terminal = new xterm.Terminal({
        allowProposedApi: false,
        convertEol: true,
        cursorBlink: true,
        fontFamily: '"SFMono-Regular", "Cascadia Code", "Roboto Mono", monospace',
        fontSize: 12,
        scrollback: 4000,
        theme: terminalTheme(appearance)
      });
      const fit = new fitModule.FitAddon();
      terminal.loadAddon(fit);
      terminal.open(containerRef.current);
      fit.fit();
      terminalRef.current = terminal;
      fitRef.current = fit;
      dataDisposable = terminal.onData((data) => {
        const id = terminalIdRef.current;
        if (!id) {
          return;
        }
        void client.request("terminal/write", {
          terminalId: id,
          dataBase64: bytesToBase64(new TextEncoder().encode(data))
        }).catch(() => {
          setState("error");
          setMessage("Terminal write failed.");
        });
      });
      resizeObserver = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => {
        fit.fit();
        const id = terminalIdRef.current;
        if (id) {
          void client.request("terminal/resize", {
            terminalId: id,
            cols: terminal.cols,
            rows: terminal.rows
          }).catch(() => {});
        }
      });
      resizeObserver?.observe(containerRef.current);
      void client.request("terminal/start", {
        scope,
        cwd: null,
        cols: terminal.cols || 80,
        rows: terminal.rows || 24
      }).then((result) => {
        if (cancelled) {
          void client.request("terminal/terminate", { terminalId: result.terminalId }).catch(() => {});
          return;
        }
        terminalIdRef.current = result.terminalId;
        setTerminalId(result.terminalId);
        setState("running");
        setMessage(result.cwd);
        terminal.focus();
      }).catch((error) => {
        setState("error");
        setMessage(error instanceof Error ? error.message : String(error));
      });
    }).catch((error) => {
      setState("error");
      setMessage(error instanceof Error ? error.message : String(error));
    });
    return () => {
      cancelled = true;
      resizeObserver?.disconnect();
      dataDisposable?.dispose();
      terminalRef.current?.dispose();
      terminalRef.current = null;
      fitRef.current = null;
      const id = terminalIdRef.current;
      terminalIdRef.current = null;
      if (id) {
        void client.request("terminal/terminate", { terminalId: id }).catch(() => {});
      }
    };
  }, [client, scope?.workdir]);

  useEffect(() => {
    const terminal = terminalRef.current;
    if (terminal) {
      terminal.options.theme = terminalTheme(appearance);
    }
  }, [appearance]);

  useEffect(() => {
    const terminal = terminalRef.current;
    const id = terminalIdRef.current;
    if (!terminal || !id) {
      return;
    }
    for (const event of terminalEvents) {
      if (event.seq <= lastEventSeqRef.current) {
        continue;
      }
      if (event.params.terminalId !== id) {
        continue;
      }
      if (event.method === "terminal/output") {
        terminal.write(base64ToBytes(event.params.dataBase64));
      } else {
        setState("exited");
        setMessage(event.params.reason || "exited");
      }
      lastEventSeqRef.current = event.seq;
    }
  }, [terminalEvents, terminalId]);

  return (
    <section className="terminalPanel" aria-label="Terminal">
      <div className="terminalViewport" ref={containerRef}>
        {state !== "running" && <div className={`terminalOverlay is-${state}`}>{message}</div>}
      </div>
    </section>
  );
}

function hasCollapsedDirectoryAncestor(path: string, collapsedDirs: Set<string>): boolean {
  for (const directory of collapsedDirs) {
    if (path !== directory && path.startsWith(`${directory}/`)) {
      return true;
    }
  }
  return false;
}

function workspaceFileTreeItems(files: WorkspaceFileEntry[]): WorkspaceFileTreeItem[] {
  return files.map((file) => ({
    disabled: file.kind === "file" && isUnsupportedPreviewFile(file.path),
    kind: file.kind,
    name: file.name,
    path: file.path,
    depth: file.depth
  }));
}

function changedFileTreeItems(files: WorkspaceDiffResult["files"]): WorkspaceFileTreeItem[] {
  const items = new Map<string, WorkspaceFileTreeItem>();
  for (const file of files) {
    for (const directory of ancestorDirectoryPaths(file.path)) {
      items.set(`directory:${directory}`, {
        kind: "directory",
        name: fileBasename(directory),
        path: directory,
        depth: workspacePathDepth(directory)
      });
    }
    items.set(`file:${file.path}`, {
      badge: file.status,
      kind: "file",
      name: fileBasename(file.path),
      path: file.path,
      depth: workspacePathDepth(file.path),
      status: file.status
    });
  }
  return [...items.values()].sort(compareTreeItems);
}

function visibleWorkspaceTreeItems(
  items: WorkspaceFileTreeItem[],
  collapsedDirs: Set<string>,
  filter: string
): WorkspaceFileTreeItem[] {
  const normalizedFilter = filter.trim().toLowerCase();
  if (!normalizedFilter) {
    return items.filter((item) => !hasCollapsedDirectoryAncestor(item.path, collapsedDirs));
  }
  const matchingPaths = new Set<string>();
  const visibleAncestorPaths = new Set<string>();
  const matchingDirectoryPaths = new Set<string>();
  for (const item of items) {
    if (!treeItemMatches(item, normalizedFilter)) {
      continue;
    }
    matchingPaths.add(item.path);
    if (item.kind === "directory") {
      matchingDirectoryPaths.add(item.path);
    }
    for (const ancestor of ancestorDirectoryPaths(item.path)) {
      visibleAncestorPaths.add(ancestor);
    }
  }
  return items.filter((item) => {
    if (matchingPaths.has(item.path) || visibleAncestorPaths.has(item.path)) {
      return true;
    }
    for (const directory of matchingDirectoryPaths) {
      if (item.path !== directory && item.path.startsWith(`${directory}/`)) {
        return true;
      }
    }
    return false;
  });
}

function treeItemMatches(item: WorkspaceFileTreeItem, normalizedFilter: string): boolean {
  return item.path.toLowerCase().includes(normalizedFilter) || item.name.toLowerCase().includes(normalizedFilter);
}

function ancestorDirectoryPaths(path: string): string[] {
  const segments = normalizedWorkspacePath(path).split("/").filter(Boolean);
  const directories: string[] = [];
  for (let index = 1; index < segments.length; index += 1) {
    directories.push(segments.slice(0, index).join("/"));
  }
  return directories;
}

function compareTreeItems(left: WorkspaceFileTreeItem, right: WorkspaceFileTreeItem): number {
  const leftSegments = left.path.split("/");
  const rightSegments = right.path.split("/");
  const length = Math.min(leftSegments.length, rightSegments.length);
  for (let index = 0; index < length; index += 1) {
    const leftSegment = leftSegments[index] ?? "";
    const rightSegment = rightSegments[index] ?? "";
    if (leftSegment !== rightSegment) {
      return leftSegment.localeCompare(rightSegment);
    }
  }
  if (leftSegments.length !== rightSegments.length) {
    return leftSegments.length - rightSegments.length;
  }
  if (left.kind !== right.kind) {
    return left.kind === "directory" ? -1 : 1;
  }
  return left.path.localeCompare(right.path);
}

function workspacePathDepth(path: string): number {
  return Math.max(0, normalizedWorkspacePath(path).split("/").filter(Boolean).length - 1);
}

function normalizedWorkspacePath(path: string): string {
  return path.replace(/\\/g, "/").replace(/^\/+/, "").replace(/\/+$/, "");
}

function absoluteWorkspacePath(root: string, path: string): string {
  const trimmedPath = path.trim();
  if (!trimmedPath) {
    return root || "";
  }
  if (/^(?:[a-zA-Z]:[\\/]|\/)/.test(trimmedPath)) {
    return trimmedPath;
  }
  const trimmedRoot = root.trim().replace(/[\\/]+$/, "");
  if (!trimmedRoot) {
    return trimmedPath;
  }
  return `${trimmedRoot}/${normalizedWorkspacePath(trimmedPath)}`;
}

function isMarkdownFile(path: string): boolean {
  const extension = path.split(/[\\/]/).pop()?.split(".").pop()?.toLowerCase();
  return extension === "md" || extension === "markdown";
}

function parseUnifiedDiff(text: string): ParsedDiffFile[] {
  const trimmed = text.replace(/\r\n/g, "\n").replace(/\n$/, "");
  if (!trimmed.trim()) {
    return [];
  }
  const lines = trimmed.split("\n");
  const files: ParsedDiffFile[] = [];
  let currentFile: ParsedDiffFile | null = null;
  let currentHunk: ParsedDiffHunk | null = null;
  let oldLineNumber = 0;
  let newLineNumber = 0;

  function ensureFile(path = "Diff"): ParsedDiffFile {
    if (currentFile) {
      return currentFile;
    }
    currentFile = { headers: [], hunks: [], path };
    files.push(currentFile);
    return currentFile;
  }

  for (const line of lines) {
    if (line.startsWith("diff --git ")) {
      currentFile = { headers: [line], hunks: [], path: diffPathFromGitHeader(line) };
      currentHunk = null;
      files.push(currentFile);
      continue;
    }
    const file = ensureFile();
    if (line.startsWith("--- ") || line.startsWith("+++ ")) {
      file.headers.push(line);
      if (line.startsWith("+++ ")) {
        const path = cleanDiffPath(line.slice(4).trim());
        if (path && path !== "/dev/null") {
          file.path = path;
        }
      }
      continue;
    }
    if (line.startsWith("@@ ")) {
      const range = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(line);
      oldLineNumber = range?.[1] ? Number(range[1]) : 0;
      newLineNumber = range?.[2] ? Number(range[2]) : 0;
      currentHunk = { header: line, lines: [] };
      file.hunks.push(currentHunk);
      continue;
    }
    if (!currentHunk) {
      file.headers.push(line);
      continue;
    }
    if (line.startsWith("+")) {
      currentHunk.lines.push({
        kind: "add",
        marker: "+",
        newNumber: newLineNumber,
        oldNumber: null,
        text: line.slice(1)
      });
      newLineNumber += 1;
      continue;
    }
    if (line.startsWith("-")) {
      currentHunk.lines.push({
        kind: "delete",
        marker: "-",
        newNumber: null,
        oldNumber: oldLineNumber,
        text: line.slice(1)
      });
      oldLineNumber += 1;
      continue;
    }
    if (line.startsWith(" ")) {
      currentHunk.lines.push({
        kind: "context",
        marker: "",
        newNumber: newLineNumber,
        oldNumber: oldLineNumber,
        text: line.slice(1)
      });
      oldLineNumber += 1;
      newLineNumber += 1;
      continue;
    }
    currentHunk.lines.push({
      kind: "meta",
      marker: "",
      newNumber: null,
      oldNumber: null,
      text: line
    });
  }

  return files;
}

function diffPathFromGitHeader(line: string): string {
  const [, left, right] = /^diff --git\s+(.+?)\s+(.+)$/.exec(line) ?? [];
  return cleanDiffPath(right ?? left ?? "Diff") || "Diff";
}

function cleanDiffPath(path: string): string {
  const unquoted = path.replace(/^"|"$/g, "");
  if (unquoted === "/dev/null") {
    return unquoted;
  }
  return unquoted.replace(/^[ab]\//, "");
}

function isUnsupportedPreviewFile(path: string): boolean {
  const extension = path.split(/[\\/]/).pop()?.split(".").pop()?.toLowerCase();
  return Boolean(extension && UNSUPPORTED_PREVIEW_EXTENSIONS.has(extension));
}

const UNSUPPORTED_PREVIEW_EXTENSIONS = new Set([
  "7z",
  "avif",
  "bin",
  "bmp",
  "bz2",
  "dylib",
  "exe",
  "gif",
  "gz",
  "ico",
  "jpeg",
  "jpg",
  "mov",
  "mp3",
  "mp4",
  "o",
  "parquet",
  "pdf",
  "png",
  "rar",
  "so",
  "tar",
  "tgz",
  "wasm",
  "webp",
  "xz",
  "zip",
  "zst"
]);

function DebugPanel({
  events,
  onRefreshTrace,
  trace
}: {
  events: DebugEvent[];
  onRefreshTrace(): void;
  trace: TraceState;
}) {
  const traceEvents = trace.result?.events ?? [];
  const traceWarnings = trace.result?.warnings ?? [];
  return (
    <section className="debugPanel" aria-label="Debug event stream">
      <header>
        <Bug size={17} />
        <div>
          <h2>Debug</h2>
          <p>{traceEvents.length} trace events · {events.length} recent notifications</p>
        </div>
        <button aria-label="Refresh Trace" onClick={onRefreshTrace} type="button">
          <RefreshCw size={15} />
        </button>
      </header>
      <div className="debugSection">
        <div className="debugSectionHeader">
          <strong>Trace</strong>
          <span>{trace.loading ? "loading" : trace.result?.available ? "persisted" : "unavailable"}</span>
        </div>
        {trace.error && <p className="debugNotice">{trace.error}</p>}
        {traceWarnings.map((warning) => (
          <p className="debugNotice" key={warning}>{warning}</p>
        ))}
        <div className="debugList">
          {traceEvents.map((event, index) => (
            <details key={`${trace.threadId ?? "trace"}:${traceEventSeq(event) ?? index}`}>
              <summary>
                <code>{traceEventLabel(event)}</code>
                <span>{traceEventTime(event)}</span>
              </summary>
              <pre>{prettyJson(event)}</pre>
            </details>
          ))}
          {traceEvents.length === 0 && <p>No persisted trace events.</p>}
        </div>
      </div>
      <div className="debugSection">
        <div className="debugSectionHeader">
          <strong>Notifications</strong>
          <span>{events.length} recent</span>
        </div>
      <div className="debugList">
        {events.map((event) => (
          <details key={event.id}>
            <summary>
              <code>{event.method}</code>
              <span>{new Date(event.at).toLocaleTimeString()}</span>
            </summary>
            <pre>{prettyJson(event.payload)}</pre>
          </details>
        ))}
        {events.length === 0 && <p>No events yet.</p>}
      </div>
      </div>
    </section>
  );
}

function ComposerRequests({
  clarifies,
  permissions,
  onClarify,
  onPermission
}: {
  clarifies: PendingClarify[];
  permissions: PendingPermission[];
  onClarify(requestId: string, answer: string): void;
  onPermission(requestId: string, decision: PermissionDecision): void;
}) {
  if (permissions.length === 0 && clarifies.length === 0) {
    return null;
  }
  return (
    <div className="composerRequests" aria-label="Pending requests">
      {permissions.map((permission) => (
        <div className="composerRequest" key={permission.requestId}>
          <strong>{permission.toolName}</strong>
          <p>{permission.reason}</p>
          <div>
            <button onClick={() => onPermission(permission.requestId, "allowOnce")} type="button">Once</button>
            <button onClick={() => onPermission(permission.requestId, "allowSession")} type="button">Session</button>
            <button onClick={() => onPermission(permission.requestId, "deny")} type="button">Deny</button>
          </div>
        </div>
      ))}
      {clarifies.map((clarify) => (
        <ClarifyComposerRequest key={clarify.requestId} request={clarify} onSubmit={onClarify} />
      ))}
    </div>
  );
}

function ClarifyComposerRequest({
  request,
  onSubmit
}: {
  request: PendingClarify;
  onSubmit(requestId: string, answer: string): void;
}) {
  const [answer, setAnswer] = useState("");
  return (
    <form
      className="composerRequest"
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit(request.requestId, answer);
        setAnswer("");
      }}
    >
      <strong>Clarify</strong>
      <pre>{JSON.stringify(request.raw, null, 2)}</pre>
      <div>
        <input value={answer} onChange={(event) => setAnswer(event.target.value)} />
        <button type="submit">Submit</button>
      </div>
    </form>
  );
}

function ComposerSubmitControls({
  context,
  controls,
  usage,
  model,
  variant,
  onModelChange,
  onVariantChange
}: {
  context: ContextReadResult | null;
  controls: SettingsReadResult["controls"];
  usage: SessionUsageSummaryView | null;
  model: string | null;
  variant: string;
  onModelChange(value: string | null): void;
  onVariantChange(value: string): void;
}) {
  const contextPercent = normalizedPercent(context?.percent);
  const [contextOpen, setContextOpen] = useState(false);
  const contextPopoverRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!contextOpen) {
      return;
    }
    function onPointerDown(event: MouseEvent) {
      if (contextPopoverRef.current?.contains(event.target as Node)) {
        return;
      }
      setContextOpen(false);
    }
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setContextOpen(false);
      }
    }
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [contextOpen]);

  return (
    <div className="composerSubmitControls" aria-label="Composer submit controls">
      <StatusSelect
        label="Model"
        value={model ?? ""}
        values={["", ...(controls?.modelOptions ?? [])]}
        renderDisplayValue={compactModelLabel}
        onChange={(value) => onModelChange(value || null)}
      />
      <StatusSelect label="Variant" value={variant} values={controls?.variantOptions ?? ["none"]} onChange={onVariantChange} />
      <div className="composerStatusContext" ref={contextPopoverRef}>
        <button
          aria-label="Context usage"
          aria-expanded={contextOpen}
          className="contextStatusButton"
          onClick={() => {
            setContextOpen((value) => !value);
          }}
          title={context?.label ?? "No active context"}
          type="button"
        >
          <span style={{ "--pevo-context-percent": `${contextPercent}%` } as CSSProperties} />
        </button>
        {contextOpen && (
          <div className="composerContextPopover" role="dialog" aria-label="Context usage">
            <div className="composerContextSummary">
              <span style={{ "--pevo-context-percent": `${contextPercent}%` } as CSSProperties}>
                {context?.available ? `${Math.round(contextPercent)}%` : "0%"}
              </span>
              <div>
                <strong>{context?.label ?? "No active context"}</strong>
                <small>{context?.status ?? "unavailable"}</small>
              </div>
            </div>
            {usage?.available && (
              <SessionUsageGrid compact usage={usage} />
            )}
            {!context?.available && (
              <p>No session context is active.</p>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function ComposerStatusLine({
  branch,
  controls,
  path,
  permissionMode,
  profile,
  onBranchClick,
  onPathClick,
  onPermissionModeChange
}: {
  branch: string | null;
  controls: SettingsReadResult["controls"];
  path: string;
  permissionMode: string;
  profile: InitializeResult["profile"] | null;
  onBranchClick(): void;
  onPathClick(): void;
  onPermissionModeChange(value: string): void;
}) {
  const profileLabel = profile && !profile.default ? profile.name : null;
  return (
    <div className="composerStatusLine" aria-label="Composer status">
      <StatusSelect label="Permission mode" value={permissionMode} values={controls?.permissionModeOptions ?? ["default"]} onChange={onPermissionModeChange} />
      {profileLabel ? (
        <span className="profileStatusPill" title={profile?.home ?? profileLabel}>
          <Pin size={12} />
          <span>{profileLabel}</span>
        </span>
      ) : null}
      <button className="pathStatusButton" onClick={onPathClick} title={path} type="button">{path || "workspace"}</button>
      <button className="branchStatusButton" onClick={onBranchClick} type="button">
        <GitBranch size={13} />
        <span>{branch || "no-branch"}</span>
      </button>
    </div>
  );
}

function StatusSelect({
  label,
  renderDisplayValue,
  value,
  values,
  onChange
}: {
  label: string;
  renderDisplayValue?(value: string): string;
  value: string;
  values: string[];
  onChange(value: string): void;
}) {
  const displayValue = renderDisplayValue?.(value);
  const valueWidth = displayValue ? `${Math.max(5, displayValue.length + 1)}ch` : undefined;
  return (
    <label
      className={`statusSelect ${displayValue ? "has-displayValue" : ""}`}
      data-status={label.toLowerCase().replace(/\s+/g, "-")}
      style={valueWidth ? { "--pevo-status-select-value-width": valueWidth } as CSSProperties : undefined}
      title={label}
    >
      {displayValue && <span aria-hidden="true" className="statusSelectDisplay">{displayValue}</span>}
      <select aria-label={label} title={value || label} value={value} onChange={(event) => onChange(event.target.value)}>
        {values.map((option) => (
          <option key={option || "default"} value={option}>{defaultStatusSelectValue(label, option)}</option>
        ))}
      </select>
    </label>
  );
}

function defaultStatusSelectValue(label: string, value: string): string {
  if (label === "Permission mode" && value === "default") {
    return "Default Permission";
  }
  return value || label.toLowerCase();
}

function compactModelLabel(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) {
    return "model";
  }
  const slash = trimmed.lastIndexOf("/");
  const label = slash >= 0 ? trimmed.slice(slash + 1).trim() : trimmed;
  return label || trimmed;
}

function normalizedPercent(value: number | null | undefined): number {
  return typeof value === "number" && Number.isFinite(value)
    ? Math.max(0, Math.min(100, value))
    : 0;
}

function orderedContextCategories(categories: ContextUsageCategory[]): ContextUsageCategory[] {
  const order = new Map(CONTEXT_CATEGORY_ORDER.map((id, index) => [id, index]));
  return [...categories].sort((left, right) => {
    const leftOrder = order.get(left.id) ?? 99;
    const rightOrder = order.get(right.id) ?? 99;
    return leftOrder - rightOrder || left.label.localeCompare(right.label);
  });
}

function contextCategoryLabel(category: ContextUsageCategory): string {
  return category.label || category.id;
}

function promptTokenCategoryColor(categoryId: string): string {
  switch (categoryId) {
    case "base_policy":
      return "var(--pevo-context-base-policy)";
    case "developer_prompt":
      return "var(--pevo-context-developer-prompt)";
    case "project_context":
      return "var(--pevo-context-project-context)";
    case "history":
      return "var(--pevo-context-history)";
    case "turn_context":
      return "var(--pevo-context-turn-context)";
    case "current_prompt":
      return "var(--pevo-context-current-prompt)";
    case "system_tools":
      return "var(--pevo-context-system-tools)";
    default:
      return "var(--pevo-accent)";
  }
}

function formatTokenEstimate(value: number, estimated = false): string {
  return `${estimated ? "~" : ""}${formatCompactNumber(value)}`;
}

function formatCompactNumber(value: number): string {
  if (!Number.isFinite(value)) {
    return "0";
  }
  if (value >= 1_000_000) {
    return `${(value / 1_000_000).toFixed(1)}M`;
  }
  if (value >= 1_000) {
    return `${(value / 1_000).toFixed(1)}k`;
  }
  return `${Math.max(0, Math.round(value))}`;
}

function formatPercent(value: number | null | undefined): string {
  return typeof value === "number" && Number.isFinite(value) ? `${Math.round(value)}%` : "none";
}

function formatPercentPrecise(value: number | null | undefined): string {
  return typeof value === "number" && Number.isFinite(value) ? `${value.toFixed(1)}%` : "none";
}

function formatNanodollars(value: number): string {
  if (!Number.isFinite(value) || value <= 0) {
    return "$0.000000";
  }
  return `$${(value / 1_000_000_000).toFixed(6)}`;
}

function contextCategoryDetailRows(category: ContextUsageCategory): Array<{ label: string; value: string }> {
  const rows: Array<{ label: string; value: string }> = [];
  const details = asOptionalRecord(category.details);
  if (!details) {
    return rows;
  }
  if (category.id === "developer_prompt") {
    const entries = Array.isArray(details.skill_entries) ? details.skill_entries : [];
    const skillRows = entries
      .map(asOptionalRecord)
      .filter((entry): entry is Record<string, unknown> => Boolean(entry))
      .map((entry) => ({
        name: optionalStringField(entry.name) ?? "skill",
        tokens: numericDetail(entry.tokens)
      }))
      .filter((entry) => entry.tokens > 0)
      .sort((left, right) => right.tokens - left.tokens || left.name.localeCompare(right.name));
    for (const entry of skillRows) {
      rows.push({ label: entry.name, value: formatTokenEstimate(entry.tokens, true) });
    }
    const skillCount = numericDetail(details.skill_count);
    if (skillRows.length === 0 && skillCount > 0) {
      rows.push({ label: "Skills", value: `${skillCount}` });
    }
  }
  if (category.id === "history") {
    const roles = asOptionalRecord(details.roles);
    for (const [role, value] of Object.entries(roles ?? {}).sort(([left], [right]) => left.localeCompare(right))) {
      const record = asOptionalRecord(value);
      if (!record) {
        continue;
      }
      const count = numericDetail(record.count);
      const tokens = numericDetail(record.tokens);
      rows.push({
        label: role,
        value: `${count} ${count === 1 ? "msg" : "msgs"}, ${formatTokenEstimate(tokens, true)}`
      });
    }
  }
  if (category.id === "project_context") {
    const count = numericDetail(details.count);
    if (count > 0) {
      rows.push({ label: "project_context", value: `${count} ${count === 1 ? "msg" : "msgs"}` });
    }
  }
  if (category.id === "turn_context") {
    const count = numericDetail(details.selected_skill_context_count);
    const tokens = numericDetail(details.selected_skill_context_tokens);
    if (count > 0 || tokens > 0) {
      rows.push({
        label: "selected_skill_context",
        value: `${count} ${count === 1 ? "msg" : "msgs"}, ${formatTokenEstimate(tokens, true)}`
      });
    }
  }
  if (category.id === "system_tools") {
    const count = numericDetail(details.tool_count);
    if (count > 0) {
      rows.push({ label: "tools", value: `${count}` });
    }
  }
  return rows;
}

function numericDetail(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? Math.max(0, value) : 0;
}

function startupDraftScope(launchScope: GatewayRequestScope, sessions: SessionSummary[]): GatewayRequestScope {
  if (launchScope.workdir?.trim()) {
    return launchScope;
  }
  const recentWorkdir = sessions.find((session) => session.workdir?.trim())?.workdir;
  return scopeForWorkdir(recentWorkdir?.trim() || window.location.pathname);
}

function AgentRunSelector({
  agents,
  disabled,
  value,
  onChange
}: {
  agents: WorkbenchAgent[];
  disabled: boolean;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <label className="agentRunSelector" title="Agent">
      <select
        aria-label="Agent"
        disabled={disabled}
        value={value}
        onChange={(event) => onChange(event.target.value)}
      >
        <option value="">Default Agent</option>
        {agents.map((agent) => (
          <option key={`${agent.source}:${agent.path ?? agent.name}`} value={agentOptionValue(agent)}>
            {agent.backend?.ref ? `${agent.name} (ACP)` : agent.name}
          </option>
        ))}
      </select>
    </label>
  );
}

function agentOptionValue(agent: WorkbenchAgent): string {
  return agent.source === "explicit" ? agent.path?.trim() || agent.name : agent.name;
}

function CommandOverlay({
  commands,
  feedback,
  onAlternateAction,
  onClose,
  onExecute
}: {
  commands: WorkbenchCommand[];
  feedback: CommandFeedback;
  onAlternateAction(action: CommandAlternateAction): void;
  onClose(): void;
  onExecute: (slash: string) => void;
}) {
  return (
    <section className="commandOverlay" aria-label="Commands overlay">
      <header>
        <div className="centerPageTitle">
          <TerminalSquare size={18} />
          <div>
            <h2>Commands</h2>
            <p>Slash command catalog</p>
          </div>
        </div>
        <button
          aria-label="Close Commands"
          className="centerPageBack"
          data-tooltip="Back to transcript"
          onClick={onClose}
          title="Back to transcript"
          type="button"
        >
          <X size={15} />
        </button>
      </header>
      <div className="commandOverlayBody">
        <CommandsPanel
          commands={commands}
          feedback={feedback}
          onAlternateAction={onAlternateAction}
          onExecute={onExecute}
        />
      </div>
    </section>
  );
}

function AgentsConfigPanel({
  backendDraft,
  backendDoctor,
  backends,
  disabled,
  onCancelBackendEdit,
  onChangeBackendDraft,
  onDeleteBackend,
  onDoctorBackend,
  onEditBackend,
  onNewBackend,
  onSaveBackendDraft,
  onSetBackendEnabled,
  onSetBackendEntrypoints,
  workdir
}: {
  backendDraft: BackendDraft | null;
  backendDoctor: Record<string, WorkbenchBackendDoctor>;
  backends: WorkbenchBackend[];
  disabled: boolean;
  onCancelBackendEdit(): void;
  onChangeBackendDraft(draft: BackendDraft): void;
  onDeleteBackend(backend: WorkbenchBackend): void;
  onDoctorBackend(backend: WorkbenchBackend): void;
  onEditBackend(backend: WorkbenchBackend): void;
  onNewBackend(): void;
  onSaveBackendDraft(draft: BackendDraft): void;
  onSetBackendEnabled(backend: WorkbenchBackend, enabled: boolean): void;
  onSetBackendEntrypoints(backend: WorkbenchBackend, entrypoints: string[]): void;
  workdir: string;
}) {
  const profileBackends = backends.filter((backend) => backend.sourceTargets.includes("profile"));
  return (
    <section className="agentSurfacePanel agentsConfigPanel" aria-label="Agents">
      <header className="agentSurfaceHeaderWithAction">
        <span><PlugZap size={15} /> Profile ACP Backends <b>{profileBackends.length}</b></span>
        {!backendDraft && (
          <button aria-label="Add ACP backend" disabled={disabled} onClick={onNewBackend} title="Add ACP backend" type="button">
            <Plus size={14} />
          </button>
        )}
      </header>
      {backendDraft && (
        <BackendEditorForm
          draft={backendDraft}
          disabled={disabled}
          invocationWorkdir={workdir}
          onCancel={onCancelBackendEdit}
          onChange={onChangeBackendDraft}
          onSave={onSaveBackendDraft}
        />
      )}
      {(!backendDraft || profileBackends.length > 0) && <div className="agentSurfaceList">
        {profileBackends.map((backend) => {
          const doctor = backendDoctor[backend.id] ?? null;
          return (
            <div className="agentSurfaceRow agentBackendRow" key={backend.id}>
	              <div>
	                <strong>{backend.label || backend.id}</strong>
	                <span>{backend.command ? [backend.command, ...backend.args].join(" ") : backend.description || backend.kind}</span>
	                {backend.diagnostics.length > 0 && (
	                  <small className="agentSurfaceWarning">{backend.diagnostics.map((diagnostic) => diagnostic.message).join(" · ")}</small>
	                )}
                {doctor && (
                  <small className={doctor.ok ? "agentSurfaceOk" : "agentSurfaceWarning"}>
                    {doctor.checks.map((check) => `${check.name}: ${check.ok ? "ok" : check.message}`).join(" · ")}
                  </small>
                )}
              </div>
              <div className="agentBackendSide">
                <div className="agentBackendControls">
                  <label className="backendSwitch">
                    <input
                      aria-label={`${backend.enabled ? "Disable" : "Enable"} ${backend.id}`}
                      checked={backend.enabled}
                      disabled={disabled}
                      onChange={(event) => onSetBackendEnabled(backend, event.currentTarget.checked)}
                      role="switch"
                      type="checkbox"
                    />
                    <span className="backendSwitchTrack" aria-hidden />
                    <span>{backend.enabled ? "Enabled" : "Disabled"}</span>
                  </label>
                  <BackendEntrypointControls
                    backend={backend}
                    disabled={disabled}
                    onChange={(entrypoints) => onSetBackendEntrypoints(backend, entrypoints)}
                  />
                </div>
                <div className="agentBackendActions">
                  <button aria-label={`Edit ${backend.id}`} disabled={disabled} onClick={() => onEditBackend(backend)} title="Edit Profile backend" type="button">
                    <Edit3 size={13} />
                  </button>
                  <button aria-label={`Doctor ${backend.id}`} disabled={disabled} onClick={() => onDoctorBackend(backend)} title="Doctor" type="button">
                    <Wrench size={13} />
                  </button>
                  <button
                    aria-label={`Delete ${backend.id} from Profile`}
                    disabled={disabled}
                    onClick={() => onDeleteBackend(backend)}
                    title="Delete Profile backend"
                    type="button"
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
              </div>
            </div>
          );
        })}
        {profileBackends.length === 0 && <p>No Profile ACP backends configured.</p>}
      </div>}
    </section>
  );
}

function BackendEntrypointControls({
  backend,
  disabled,
  onChange
}: {
  backend: WorkbenchBackend;
  disabled: boolean;
  onChange(entrypoints: string[]): void;
}) {
  const selected = backend.entrypoints.length > 0 ? backend.entrypoints : ["peer", "subagent"];
  return (
    <div className="backendEntrypointControls" aria-label={`${backend.id} entrypoints`}>
      {BACKEND_ENTRYPOINTS.map((entrypoint) => {
        const checked = selected.includes(entrypoint);
        const isLastSelected = checked && selected.length === 1;
        return (
          <label key={entrypoint}>
            <input
              aria-label={`${backend.id} ${entrypoint} entrypoint`}
              checked={checked}
              disabled={disabled || isLastSelected}
              onChange={(event) => {
                const next = event.currentTarget.checked
                  ? [...selected, entrypoint]
                  : selected.filter((item) => item !== entrypoint);
                onChange(BACKEND_ENTRYPOINTS.filter((item) => next.includes(item)));
              }}
              type="checkbox"
            />
            <span>{entrypoint}</span>
          </label>
        );
      })}
    </div>
  );
}

function BackendEditorForm({
  draft,
  disabled,
  invocationWorkdir,
  onCancel,
  onChange,
  onSave
}: {
  draft: BackendDraft;
  disabled: boolean;
  invocationWorkdir: string;
  onCancel(): void;
  onChange(draft: BackendDraft): void;
  onSave(draft: BackendDraft): void;
}) {
  const commandConfig = parseBackendCommandJson(draft.commandJsonText);
  const commandJsonError = draft.commandJsonText.trim() ? commandConfig.error : null;
  const canSave = Boolean(draft.id.trim() && commandConfig.command.trim() && !commandConfig.error);
  const resolvedCwd = resolvedBackendCwd(draft.cwd, invocationWorkdir);
  function patch(patch: Partial<BackendDraft>) {
    onChange({ ...draft, ...patch });
  }
  function toggleClientCapability(value: string) {
    const current = draft.clientCapabilities;
    patch({
      clientCapabilities: current.includes(value)
        ? current.filter((item) => item !== value)
        : [...current, value]
    });
  }
  return (
    <form
      aria-label="Profile ACP backend"
      className="backendEditor"
      onSubmit={(event) => {
        event.preventDefault();
        if (canSave && !disabled) {
          onSave(draft);
        }
      }}
    >
      <header>
        <div className="workspaceDialogTitle">
          <PlugZap size={18} aria-hidden />
          <h4>Backend details</h4>
        </div>
        <button aria-label="Close backend editor" onClick={onCancel} title="Close" type="button">
          <X size={15} />
        </button>
      </header>
        <label>
          <span>ID</span>
          <input aria-label="ID" disabled={disabled} onChange={(event) => patch({ id: event.currentTarget.value })} value={draft.id} />
        </label>
        <label>
          <span>Label <em>Optional</em></span>
          <input aria-label="Label" disabled={disabled} onChange={(event) => patch({ label: event.currentTarget.value })} value={draft.label} />
        </label>
        <label>
          <span>Description <em>Optional</em></span>
          <input aria-label="Description" disabled={disabled} onChange={(event) => patch({ description: event.currentTarget.value })} value={draft.description} />
        </label>
        <label>
          <span>Command JSON</span>
          <textarea
            aria-describedby={commandJsonError ? "backend-command-json-error" : undefined}
            aria-invalid={commandJsonError ? true : undefined}
            aria-label="Command JSON"
            className="backendJsonInput"
            disabled={disabled}
            onChange={(event) => patch({ commandJsonText: event.currentTarget.value })}
            placeholder={BACKEND_COMMAND_JSON_PLACEHOLDER}
            spellCheck={false}
            value={draft.commandJsonText}
          />
          {commandJsonError && <small className="backendFieldError" id="backend-command-json-error">{commandJsonError}</small>}
        </label>
        <label>
          <span>CWD</span>
          <input
            aria-describedby="backend-cwd-resolution"
            aria-label="CWD"
            disabled={disabled}
            onChange={(event) => patch({ cwd: event.currentTarget.value })}
            placeholder="Invocation workdir"
            value={draft.cwd}
          />
          <small className="backendFieldHint" id="backend-cwd-resolution">Resolves to {resolvedCwd}</small>
        </label>
        <fieldset className="backendDialogChecks">
          <legend>Client Capabilities</legend>
          {BACKEND_CLIENT_CAPABILITIES.map((capability) => (
            <label key={capability}>
              <input
                checked={draft.clientCapabilities.includes(capability)}
                disabled={disabled}
                onChange={() => toggleClientCapability(capability)}
                type="checkbox"
              />
              <span>{capability}</span>
            </label>
          ))}
        </fieldset>
        <label>
          <span>MCP Servers</span>
          <textarea aria-label="MCP Servers" disabled={disabled} onChange={(event) => patch({ mcpServersText: event.currentTarget.value })} value={draft.mcpServersText} />
        </label>
        <footer>
          <button disabled={disabled} onClick={onCancel} type="button">
            <X size={14} />
            Cancel
          </button>
          <button disabled={disabled || !canSave} type="submit">
            <Save size={14} />
            Save
          </button>
        </footer>
    </form>
  );
}

function backendDraftFromBackend(backend: WorkbenchBackend): BackendDraft {
  return {
    id: backend.id,
    enabled: backend.enabled,
    label: backend.label && backend.label !== backend.id ? backend.label : "",
    description: backend.description ?? "",
    commandJsonText: backend.command ? formatBackendCommandJson({
      command: backend.command,
      args: backend.args,
      env: {}
    }) : "",
    cwd: backend.cwd && backend.cwd !== "invocation" ? backend.cwd : "",
    entrypoints: backend.entrypoints.length > 0 ? backend.entrypoints : ["peer", "subagent"],
    clientCapabilities: backend.clientCapabilities.length > 0
      ? backend.clientCapabilities
      : ["fs.read", "fs.write", "terminal"],
    mcpServersText: backend.mcpServers.join("\n")
  };
}

function formatBackendCommandJson(config: BackendCommandJson): string {
  return prettyJson({
    command: config.command,
    args: config.args,
    env: config.env
  });
}

function parseBackendCommandJson(value: string): BackendCommandJson & { error: string | null } {
  const trimmed = value.trim();
  if (!trimmed) {
    return { command: "", args: [], env: {}, error: null };
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch {
    return { command: "", args: [], env: {}, error: "Command JSON must be valid JSON." };
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    return { command: "", args: [], env: {}, error: "Command JSON must be an object." };
  }
  const record = parsed as Record<string, unknown>;
  if (typeof record.command !== "string") {
    return { command: "", args: [], env: {}, error: "Command JSON must include a string command." };
  }
  const args = record.args === undefined ? [] : record.args;
  if (!Array.isArray(args) || !args.every((item) => typeof item === "string")) {
    return { command: "", args: [], env: {}, error: "Command JSON args must be an array of strings." };
  }
  const envValue = record.env === undefined ? {} : record.env;
  if (!envValue || typeof envValue !== "object" || Array.isArray(envValue)) {
    return { command: "", args: [], env: {}, error: "Command JSON env must be an object." };
  }
  const env: Record<string, string> = {};
  for (const [key, envItem] of Object.entries(envValue as Record<string, unknown>)) {
    if (typeof envItem !== "string") {
      return { command: "", args: [], env: {}, error: "Command JSON env values must be strings." };
    }
    if (key.trim()) {
      env[key.trim()] = envItem;
    }
  }
  return {
    command: record.command,
    args,
    env,
    error: null
  };
}

function resolvedBackendCwd(cwd: string, invocationWorkdir: string): string {
  const value = cwd.trim();
  const root = invocationWorkdir.trim() || ".";
  if (!value || value === "invocation") {
    return root;
  }
  if (isAbsoluteDisplayPath(value)) {
    return value;
  }
  return `${root.replace(/\/+$/, "")}/${value.replace(/^\.\//, "")}`;
}

function isAbsoluteDisplayPath(value: string): boolean {
  return value.startsWith("/") || /^[A-Za-z]:[\\/]/.test(value) || value.startsWith("\\\\");
}

function CommandsPanel({
  commands,
  feedback,
  onAlternateAction,
  onExecute
}: {
  commands: WorkbenchCommand[];
  feedback: CommandFeedback;
  onAlternateAction(action: CommandAlternateAction): void;
  onExecute: (slash: string) => void;
}) {
  const groups = commandPresentationGroups(commands);
  return (
    <section className="agentSurfacePanel commandSurfacePanel" aria-label="Commands">
      <header>
        <span><TerminalSquare size={15} /> Commands</span>
        <b>{commands.length}</b>
      </header>
      {feedback && (
        <CommandFeedbackView feedback={feedback} onAlternateAction={onAlternateAction} />
      )}
      <div className="commandSurfaceList">
        {groups.map((group) => (
          <div className="commandSurfaceGroup" key={group.kind}>
            <h3>{commandPresentationLabel(group.kind)}</h3>
            {group.commands.map((command) => {
              const details = [
                commandDestinationLabel(command.destination),
                command.aliases.length > 0 ? command.aliases.map((alias) => `/${alias}`).join(" ") : null
              ].filter(Boolean).join(" · ");
              return (
                <button
                  className="commandSurfaceRow"
                  key={`${command.source}:${command.name}`}
                  onClick={() => onExecute(command.slash)}
                  title={command.usage || command.summary}
                  type="button"
                >
                  <code>{command.slash}</code>
                  <span>{command.summary}</span>
                  {details && <small>{details}</small>}
                </button>
              );
            })}
          </div>
        ))}
        {commands.length === 0 && <p>No commands available.</p>}
      </div>
    </section>
  );
}

function CommandFeedbackView({
  className = "",
  feedback,
  onAlternateAction
}: {
  className?: string;
  feedback: NonNullable<CommandFeedback>;
  onAlternateAction(action: CommandAlternateAction): void;
}) {
  const alternateAction = feedback.alternateAction;
  return (
    <div className={`commandFeedback ${feedback.accepted ? "is-ok" : "is-error"} ${className}`.trim()}>
      <div>
        <strong>{feedback.command}</strong>
        <span>{feedback.message}</span>
      </div>
      {alternateAction && (
        <button
          className="commandFeedbackAction"
          onClick={() => onAlternateAction(alternateAction)}
          type="button"
        >
          {alternateAction.label}
        </button>
      )}
    </div>
  );
}

function readWorkbenchPrefs(): WorkbenchPrefs {
  try {
    const raw = window.localStorage.getItem(PREFS_KEY);
    const value = raw ? JSON.parse(raw) as Partial<WorkbenchPrefs> : {};
    return {
      appearance: value.appearance === "light" ? "light" : "dark",
      debug: value.debug === true,
      rightWidthPx: clampRightWidth(value.rightWidthPx)
    };
  } catch {
    return { appearance: "dark", debug: false, rightWidthPx: DEFAULT_RIGHT_WIDTH_PX };
  }
}

function readPinnedSessionIds(): string[] {
  try {
    const raw = window.localStorage.getItem(PINNED_SESSIONS_KEY);
    return normalizePinnedSessionIds(raw ? JSON.parse(raw) : []);
  } catch {
    return [];
  }
}

function readPinnedSessionIdsFromStorage(storage: HostStorage): string[] {
  return normalizePinnedSessionIds(storage.getJson(PINNED_SESSIONS_KEY, []));
}

function normalizePinnedSessionIds(value: unknown): string[] {
  return Array.isArray(value)
    ? Array.from(new Set(value.filter((item): item is string => typeof item === "string" && item.trim() !== "")))
    : [];
}

function createRightTabId(kind: RightWorkspaceTabKind): string {
  return `${kind}:${Date.now()}:${Math.random().toString(16).slice(2)}`;
}

function rightWorkspaceDefaultTitle(kind: RightWorkspaceTabKind): string {
  return rightWorkspaceTabLabel(kind);
}

function rightWorkspaceTabLabel(kind: RightWorkspaceTabKind): string {
  switch (kind) {
    case "files":
      return "Files";
    case "terminal":
      return "Terminal";
    case "debug":
      return "Debug";
    case "review":
    default:
      return "Review";
  }
}

function rightWorkspaceTabIcon(kind: RightWorkspaceTabKind): ReactNode {
  switch (kind) {
    case "files":
      return <FolderTree size={14} />;
    case "terminal":
      return <TerminalSquare size={14} />;
    case "debug":
      return <Bug size={14} />;
    case "review":
    default:
      return <GitPullRequest size={14} />;
  }
}

function clampRightWidth(value: unknown): number {
  const numeric = typeof value === "number" ? value : DEFAULT_RIGHT_WIDTH_PX;
  return Math.max(MIN_RIGHT_WIDTH_PX, Math.min(MAX_RIGHT_WIDTH_PX, Math.round(numeric)));
}

function fileBasename(path: string): string {
  const normalized = path.replace(/\\/g, "/").replace(/\/+$/, "");
  return normalized.split("/").pop() || normalized || "workspace";
}

function terminalTheme(appearance: Appearance): ITheme {
  if (appearance === "light") {
    return {
      background: "transparent",
      foreground: "#2d261f",
      cursor: "#2d261f",
      selectionBackground: "#eadfce"
    };
  }
  return {
    background: "transparent",
    foreground: "#f3efe7",
    cursor: "#f3efe7",
    selectionBackground: "#3f372d"
  };
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let index = 0; index < bytes.length; index += 1) {
    binary += String.fromCharCode(bytes[index] ?? 0);
  }
  return window.btoa(binary);
}

function base64ToBytes(value: string): Uint8Array {
  const binary = window.atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

async function attachmentFromFile(file: File): Promise<PendingAttachment> {
  const id = `${Date.now()}:${file.name}:${file.size}:${Math.random().toString(16).slice(2)}`;
  const sizeLabel = formatBytes(file.size);
  if (file.type.startsWith("image/")) {
    if (file.size > MAX_IMAGE_ATTACHMENT_BYTES) {
      throw new Error(`Image attachment is too large: ${file.name} (${sizeLabel})`);
    }
    return {
      id,
      input: { type: "image", input: { kind: "url", url: await fileToDataUrl(file) } },
      kind: "image",
      name: file.name || "image",
      size: file.size,
      sizeLabel
    };
  }

  if (isTextLikeFile(file)) {
    const truncated = file.size > MAX_TEXT_ATTACHMENT_BYTES;
    const text = await file.slice(0, MAX_TEXT_ATTACHMENT_BYTES).text();
    return {
      id,
      input: {
        type: "context",
        label: `Attachment: ${file.name || "file"}`,
        text: [
          `Attached text file: ${file.name || "file"}`,
          `MIME: ${file.type || "unknown"}`,
          `Size: ${sizeLabel}`,
          truncated ? `Content is truncated to ${formatBytes(MAX_TEXT_ATTACHMENT_BYTES)}.` : "",
          "",
          text
        ].filter(Boolean).join("\n"),
        visibleToModel: true
      },
      kind: "text",
      name: file.name || "file",
      size: file.size,
      sizeLabel
    };
  }

  return {
    id,
    input: {
      type: "context",
      label: `Attachment: ${file.name || "file"}`,
      text: [
        `Attached file: ${file.name || "file"}`,
        `MIME: ${file.type || "unknown"}`,
        `Size: ${sizeLabel}`,
        "Binary content is selected in Workbench but is not embedded as model text."
      ].join("\n"),
      visibleToModel: true
    },
    kind: "file",
    name: file.name || "file",
    size: file.size,
    sizeLabel
  };
}

function fileToDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.addEventListener("load", () => resolve(String(reader.result ?? "")), { once: true });
    reader.addEventListener("error", () => reject(reader.error ?? new Error("failed to read file")), { once: true });
    reader.readAsDataURL(file);
  });
}

function isTextLikeFile(file: File): boolean {
  if (file.type.startsWith("text/")) {
    return true;
  }
  const name = file.name.toLowerCase();
  return [
    ".css",
    ".csv",
    ".html",
    ".js",
    ".json",
    ".jsx",
    ".md",
    ".py",
    ".rs",
    ".toml",
    ".ts",
    ".tsx",
    ".txt",
    ".xml",
    ".yaml",
    ".yml"
  ].some((extension) => name.endsWith(extension));
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  const kib = bytes / 1024;
  if (kib < 1024) {
    return `${Math.round(kib * 10) / 10} KiB`;
  }
  const mib = kib / 1024;
  return `${Math.round(mib * 10) / 10} MiB`;
}

function transcriptSearchText(entries: ThreadSnapshot["entries"]): string {
  return entries
    .flatMap((entry) => [
      entry.role,
      ...entry.blocks.flatMap((block) => {
        const record = asRecord(block);
        return [
          stringField(record.title),
          stringField(record.body),
          stringField(record.preview),
          stringField(record.detail)
        ];
      })
    ])
    .filter(Boolean)
    .join("\n");
}

function normalizeSearchText(value: string): string {
  return value.trim().toLowerCase();
}

function searchExcerpt(text: string, query: string): string {
  const normalized = text.replace(/\s+/g, " ").trim();
  if (!normalized) {
    return "Message text matched.";
  }
  const index = normalized.toLowerCase().indexOf(query.trim().toLowerCase());
  if (index < 0) {
    return normalized.slice(0, 160);
  }
  const start = Math.max(0, index - 56);
  const end = Math.min(normalized.length, index + query.trim().length + 96);
  const prefix = start > 0 ? "..." : "";
  const suffix = end < normalized.length ? "..." : "";
  return `${prefix}${normalized.slice(start, end)}${suffix}`;
}

function shortSessionId(id: string): string {
  return id.length > 12 ? `${id.slice(0, 8)}...${id.slice(-4)}` : id;
}

function parseAgentList(value: unknown): WorkbenchAgent[] {
  const agents = asRecord(value).agents;
  return Array.isArray(agents)
    ? agents.map((agent) => {
        const item = asRecord(agent);
        return {
          name: stringField(item.name),
          description: stringField(item.description),
          source: stringField(item.source),
          generated: item.generated === true,
          path: optionalStringField(item.path),
          entrypoints: stringArray(item.entrypoints),
          backend: asOptionalRecord(item.backend) as { ref?: string } | null
        };
      }).filter((agent) => agent.name)
    : [];
}

function parseBackendList(value: unknown): WorkbenchBackend[] {
  const backends = asRecord(value).backends;
  return Array.isArray(backends)
    ? backends.map((backend) => {
        const item = asRecord(backend);
        return {
          id: stringField(item.id),
          kind: stringField(item.kind),
          enabled: item.enabled !== false,
          label: stringField(item.label),
          description: optionalStringField(item.description),
          command: optionalStringField(item.command),
          args: stringArray(item.args),
          cwd: optionalStringField(item.cwd) ?? "invocation",
          entrypoints: stringArray(item.entrypoints),
          clientCapabilities: stringArray(item.clientCapabilities),
          mcpServers: stringArray(item.mcpServers),
          envKeys: stringArray(item.envKeys),
          sourceTargets: parseBackendTargets(item.sourceTargets),
          diagnostics: parseDiagnostics(item.diagnostics)
        };
      }).filter((backend) => backend.id)
    : [];
}

function parseBackendDoctor(value: unknown): WorkbenchBackendDoctor {
  const item = asRecord(value);
  const checks = Array.isArray(item.checks) ? item.checks : [];
  return {
    id: stringField(item.id),
    ok: item.ok === true,
    checks: checks.map((check) => {
      const record = asRecord(check);
      return {
        name: stringField(record.name),
        ok: record.ok === true,
        message: stringField(record.message),
        path: optionalStringField(record.path)
      };
    }).filter((check) => check.name)
  };
}

function parseBackendTargets(value: unknown): BackendConfigTarget[] {
  return stringArray(value)
    .filter((item): item is BackendConfigTarget => item === "project" || item === "profile");
}

function parseDiagnostics(value: unknown): WorkbenchDiagnostic[] {
  return Array.isArray(value)
    ? value.map((diagnostic) => {
        const item = asRecord(diagnostic);
        return {
          kind: stringField(item.kind),
          message: stringField(item.message)
        };
      }).filter((diagnostic) => diagnostic.message)
    : [];
}

function parseCommandList(value: unknown): WorkbenchCommand[] {
  const commands = asRecord(value).commands;
  return Array.isArray(commands)
    ? commands.map((command) => {
        const item = asRecord(command);
        return {
          name: stringField(item.name),
          slash: stringField(item.slash),
          usage: stringField(item.usage),
          summary: stringField(item.summary),
          aliases: stringArray(item.aliases),
          argumentKind: stringField(item.argumentKind),
          source: stringField(item.source),
          presentationKind: optionalStringField(item.presentationKind) ?? "control",
          destination: optionalStringField(item.destination),
          feedbackAnchor: optionalStringField(item.feedbackAnchor),
          alternateAction: parseCommandAlternateAction(item.alternateAction)
        };
      }).filter((command) => command.name)
    : [];
}

function multilineList(value: string): string[] {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

function parseCommandAlternateAction(value: unknown): CommandAlternateAction | null {
  const action = asOptionalRecord(value);
  if (!action) {
    return null;
  }
  const type = stringField(action.type);
  const target = stringField(action.target);
  const label = stringField(action.label);
  return type && target && label ? { type, target, label } : null;
}

function commandFeedbackFromResult(
  command: string,
  record: Record<string, unknown>,
  trigger: CommandTrigger,
  options: { downloadAvailable?: boolean } = {}
): CommandFeedback {
  const action = asRecord(record.action);
  const message = optionalStringField(record.message) ?? commandActionFeedbackMessage(action, options);
  if (!message) {
    return null;
  }
  const downloadFailed = action.type === "downloadSession" && options.downloadAvailable === false;
  return {
    accepted: record.accepted === true && !downloadFailed,
    command: optionalStringField(record.command) ?? command,
    message,
    feedbackAnchor: resolveCommandFeedbackAnchor(optionalStringField(record.feedbackAnchor), trigger),
    alternateAction: parseCommandAlternateAction(record.alternateAction)
  };
}

function commandFeedbackAutoDismissable(feedback: NonNullable<CommandFeedback>): boolean {
  return feedback.accepted && !feedback.alternateAction;
}

function resolveCommandFeedbackAnchor(anchor: string | null, trigger: CommandTrigger): string {
  if (
    (trigger === "commandsPanel" || trigger === "commandOverlay")
    && (!anchor || anchor === "trigger" || anchor === "commandsPanel")
  ) {
    return "commandsPanel";
  }
  if (!anchor || anchor === "trigger") {
    return trigger;
  }
  if (trigger === "composer" && anchor === "commandsPanel") {
    return "composer";
  }
  return anchor;
}

function commandActionFeedbackMessage(
  action: Record<string, unknown>,
  options: { downloadAvailable?: boolean } = {}
): string | null {
  if (action.type === "downloadSession") {
    if (options.downloadAvailable === false) {
      return stringField(action.kind) === "share"
        ? "Share is not available for this session."
        : "Export is not available for this session.";
    }
    return stringField(action.kind) === "share" ? "Share artifact opened." : "Export download opened.";
  }
  if (action.type === "showPanel") {
    return `Opened ${hostPanelLabel(stringField(action.panel))}.`;
  }
  return null;
}

function hostPanelLabel(panel: string): string {
  switch (panel) {
    case "history":
    case "sessions":
      return "History";
    case "agents":
      return "Agents";
    case "commands":
    case "help":
      return "Commands";
    case "preview":
      return "Preview";
    case "files":
      return "Files";
    case "debug":
      return "Debug";
    case "status":
    default:
      return "Status";
  }
}

const COMMAND_PRESENTATION_ORDER = ["navigate", "inspect", "control", "submit", "export", "extension"];

function commandPresentationGroups(commands: WorkbenchCommand[]): Array<{ kind: string; commands: WorkbenchCommand[] }> {
  const order = new Map(COMMAND_PRESENTATION_ORDER.map((kind, index) => [kind, index]));
  const grouped = new Map<string, WorkbenchCommand[]>();
  for (const command of commands) {
    const kind = command.presentationKind || "control";
    grouped.set(kind, [...(grouped.get(kind) ?? []), command]);
  }
  return [...grouped.entries()]
    .sort(([left], [right]) => (order.get(left) ?? 99) - (order.get(right) ?? 99) || left.localeCompare(right))
    .map(([kind, commands]) => ({ kind, commands }));
}

function commandPresentationLabel(kind: string): string {
  switch (kind) {
    case "navigate":
      return "Navigate";
    case "inspect":
      return "Inspect";
    case "control":
      return "Control";
    case "submit":
      return "Submit";
    case "export":
      return "Export";
    case "extension":
      return "Extensions";
    default:
      return kind ? `${kind.slice(0, 1).toUpperCase()}${kind.slice(1)}` : "Commands";
  }
}

function commandDestinationLabel(destination: string | null): string | null {
  switch (destination) {
    case "commands":
      return "Commands";
    case "history":
      return "History";
    case "agents":
      return "Agents";
    case "status":
      return "Status";
    case "preview":
      return "Preview";
    case "composer":
      return "Composer";
    case "download":
      return "Download";
    default:
      return null;
  }
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function asOptionalRecord(value: unknown): Record<string, unknown> | null {
  const record = asRecord(value);
  return Object.keys(record).length > 0 ? record : null;
}

function stringField(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function optionalStringField(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

function traceEventSeq(value: unknown): number | null {
  const seq = asRecord(value).seq;
  return typeof seq === "number" && Number.isFinite(seq) ? seq : null;
}

function traceEventLabel(value: unknown): string {
  const record = asRecord(value);
  const seq = traceEventSeq(value);
  const kind = optionalStringField(record.kind) ?? optionalStringField(record.type) ?? "event";
  return seq === null ? kind : `#${seq} ${kind}`;
}

function traceEventTime(value: unknown): string {
  const timestamp = asRecord(value).timestamp_ms;
  if (typeof timestamp !== "number" || !Number.isFinite(timestamp)) {
    return "";
  }
  return new Date(timestamp).toLocaleTimeString();
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
}

function prettyJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function idleActivity(): ThreadSnapshot["activity"] {
  return { running: false, activeTurnId: null, queuedTurns: 0 };
}

function normalizeActivity(activity: Partial<ThreadSnapshot["activity"]> | null | undefined): ThreadSnapshot["activity"] {
  return {
    running: activity?.running === true,
    activeTurnId: typeof activity?.activeTurnId === "string" ? activity.activeTurnId : null,
    queuedTurns: Number.isFinite(activity?.queuedTurns) ? Number(activity?.queuedTurns) : 0
  };
}

function normalizeSnapshot(snapshot: ThreadSnapshot): ThreadSnapshot {
  return {
    ...snapshot,
    entries: Array.isArray(snapshot.entries) ? snapshot.entries : [],
    activity: normalizeActivity(snapshot.activity),
    pendingPermissions: Array.isArray(snapshot.pendingPermissions) ? snapshot.pendingPermissions : [],
    pendingClarifies: Array.isArray(snapshot.pendingClarifies) ? snapshot.pendingClarifies : []
  };
}

function normalizeSessionSummary(session: SessionSummary): SessionSummary {
  return {
    ...session,
    activity: normalizeActivity(session.activity)
  };
}
