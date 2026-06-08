import { useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import {
  AlertTriangle,
  Archive,
  Bot,
  Box,
  Bug,
  Cable,
  ChevronDown,
  ChevronRight,
  FileText,
  FolderTree,
  GitBranch,
  MessageSquare,
  Moon,
  PanelLeft,
  PanelRight,
  Pin,
  PlugZap,
  Search,
  Settings,
  Shield,
  Sparkles,
  Sun,
  TerminalSquare,
  Wrench,
  X
} from "lucide-react";
import {
  Composer,
  HistoryPanel,
  StatusPanel,
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
  ContextReadResultSchema,
  InitializeResultSchema,
  SettingsReadResultSchema,
  ThreadListResultSchema,
  WorkspaceDiffResultSchema,
  WorkspaceFileReadResultSchema,
  WorkspaceFilesResultSchema,
  type ContextReadResult,
  type GatewayMention,
  type GatewayInputPart,
  type GatewayRequestScope,
  type InitializeResult,
  type PendingClarify,
  type PendingPermission,
  type PermissionDecision,
  type SessionSummary,
  type SettingsReadResult,
  type ThreadSnapshot,
  type WorkspaceDiffResult,
  type WorkspaceFileEntry,
  type WorkspaceFileReadResult,
  type WorkspaceFilesResult
} from "@psychevo/protocol";
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
  entrypoints: string[];
};

type WorkbenchCommand = {
  name: string;
  slash: string;
  usage: string;
  summary: string;
  aliases: string[];
  argumentKind: string;
  source: string;
};

type RightTab = "status" | "files" | "debug";
type MainView = "transcript" | "search" | "artifacts" | "agents" | "skills" | "tools" | "mcp" | "settings";
type Appearance = "dark" | "light";

type CommandFeedback = {
  accepted: boolean;
  command: string;
  message: string;
} | null;

type PreviewState =
  | { body: string; kind: "diff"; path?: string | null; title: string }
  | { body: string; kind: "file"; path: string; title: string; truncated: boolean; binary: boolean }
  | null;

type DebugEvent = {
  id: string;
  at: number;
  method: string;
  payload: unknown;
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
};

const logoUrl = new URL("../../../assets/psychevo-logo.svg", import.meta.url).href;
const PREFS_KEY = "psychevo.workbench.v0.prefs";
const PINNED_SESSIONS_KEY = "psychevo.workbench.v0.pinnedSessions";
const MAX_TEXT_ATTACHMENT_BYTES = 256 * 1024;
const MAX_IMAGE_ATTACHMENT_BYTES = 6 * 1024 * 1024;

export function App() {
  const [client, setClient] = useState<GatewayClient | null>(null);
  const [host, setHost] = useState<PsychevoHost | null>(null);
  const [endpoint, setEndpoint] = useState<GatewayEndpoint | null>(null);
  const [init, setInit] = useState<InitializeResult | null>(null);
  const [activeScope, setActiveScope] = useState<GatewayRequestScope | null>(null);
  const [snapshot, setSnapshot] = useState<ThreadSnapshot>(EMPTY_SNAPSHOT);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [pinnedSessionIds, setPinnedSessionIds] = useState<string[]>(readPinnedSessionIds);
  const [draftSession, setDraftSession] = useState<HistoryDraftSession | null>(null);
  const [settings, setSettings] = useState<SettingsReadResult | undefined>();
  const [agents, setAgents] = useState<WorkbenchAgent[]>([]);
  const [backends, setBackends] = useState<WorkbenchBackend[]>([]);
  const [commands, setCommands] = useState<WorkbenchCommand[]>([]);
  const [rightTab, setRightTab] = useState<RightTab>("status");
  const [mainView, setMainView] = useState<MainView>("transcript");
  const [leftCollapsed, setLeftCollapsed] = useState(false);
  const [rightCollapsed, setRightCollapsed] = useState(true);
  const [commandFeedback, setCommandFeedback] = useState<CommandFeedback>(null);
  const [selectedAgentName, setSelectedAgentName] = useState<string>("");
  const [permissionMode, setPermissionMode] = useState("default");
  const [workMode, setWorkMode] = useState("default");
  const [selectedModel, setSelectedModel] = useState<string | null>(null);
  const [selectedVariant, setSelectedVariant] = useState<string>("none");
  const [workspaceFiles, setWorkspaceFiles] = useState<WorkspaceFilesResult | null>(null);
  const [workspaceDiff, setWorkspaceDiff] = useState<WorkspaceDiffResult | null>(null);
  const [contextUsage, setContextUsage] = useState<ContextReadResult | null>(null);
  const [preview, setPreview] = useState<PreviewState>(null);
  const [attachments, setAttachments] = useState<PendingAttachment[]>([]);
  const [debugEvents, setDebugEvents] = useState<DebugEvent[]>([]);
  const initialPrefs = useMemo(readWorkbenchPrefs, []);
  const [appearance, setAppearance] = useState<Appearance>(initialPrefs.appearance);
  const [debugEnabled, setDebugEnabled] = useState(initialPrefs.debug);
  const [archived, setArchived] = useState(false);
  const [status, setStatus] = useState("connecting");
  const [error, setError] = useState<string | null>(null);
  const [mobilePanel, setMobilePanel] = useState<"history" | "transcript" | "status">("transcript");
  const viewEpochRef = useRef(0);
  const scopeRef = useRef<GatewayRequestScope | null>(null);
  const detachedShellTokenRef = useRef(0);
  const pendingDetachedShellRef = useRef<PendingDetachedShell | null>(null);
  const skipNextPinnedPersistRef = useRef(false);

  const activity = normalizeActivity(snapshot.activity);
  const transcriptEntries = Array.isArray(snapshot.entries) ? snapshot.entries : [];
  const pendingClarifies = Array.isArray(snapshot.pendingClarifies) ? snapshot.pendingClarifies : [];
  const pendingPermissions = Array.isArray(snapshot.pendingPermissions) ? snapshot.pendingPermissions : [];
  const running = activity.running;
  const disabled = status !== "connected";
  const currentThreadId = snapshot.thread?.id;
  const visibleDraftSession = visibleHistoryDraftSession(draftSession, archived);
  const hasSelectedSession = Boolean(currentThreadId || visibleDraftSession);
  const showSessionChrome = mainView === "transcript" && hasSelectedSession;
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
  const controls = settings?.controls ?? null;

  useEffect(() => {
    if (
      selectedAgentName &&
      !runnableAgents.some((agent) => agentOptionValue(agent) === selectedAgentName || agent.name === selectedAgentName)
    ) {
      setSelectedAgentName("");
    }
  }, [runnableAgents, selectedAgentName]);

  useEffect(() => {
    if (!debugEnabled && rightTab === "debug") {
      setRightTab("status");
    }
  }, [debugEnabled, rightTab]);

  useEffect(() => {
    document.documentElement.dataset.pevoAppearance = appearance;
    host?.storage.setJson<WorkbenchPrefs>(PREFS_KEY, { appearance, debug: debugEnabled });
  }, [appearance, debugEnabled, host]);

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
      if (notification.method === "gateway/event") {
        const parsed = GatewayEventSchema.safeParse(notification.params);
        if (parsed.success) {
          const event = parsed.data;
          setSnapshot((current) => applyLiveTranscriptEvent(current, event));
          if (event.type === "turnStarted" && event.threadId) {
            void refreshHistory(nextClient, archived);
          }
          if (event.type === "turnCompleted" && event.threadId) {
            const threadId = event.threadId;
            const eventEpoch = viewEpochRef.current;
            void refreshSnapshot(nextClient, threadId, undefined, true, eventEpoch);
            void refreshHistory(nextClient, archived);
            const scope = scopeRef.current;
            if (scope) {
              void refreshWorkspaceSurface(nextClient, scope, threadId);
            }
            for (const delay of [1_500, 3_000, 7_500, 15_000, 30_000, 60_000, 120_000]) {
              window.setTimeout(() => {
                void refreshSnapshot(nextClient, threadId, undefined, true, eventEpoch);
                void refreshHistory(nextClient, archived);
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
          }
        } else {
          void refreshSnapshot(nextClient);
        }
        void refreshHistory(nextClient, archived);
      }
      if (notification.method === "shell/error") {
        const record = asRecord(notification.params);
        setError(optionalStringField(record.message) ?? "Shell command failed");
        const threadId = optionalStringField(record.threadId);
        if (threadId) {
          void refreshSnapshot(nextClient, threadId, undefined, true, viewEpochRef.current);
        }
        void refreshHistory(nextClient, archived);
      }
      if (notification.method === "turn/result") {
        const record = asRecord(notification.params);
        const thread = asRecord(record.thread);
        const threadId = optionalStringField(thread.id);
        if (threadId) {
          void refreshSnapshot(nextClient, threadId, undefined, true, viewEpochRef.current);
          const scope = scopeRef.current;
          if (scope) {
            void refreshWorkspaceSurface(nextClient, scope, threadId);
          }
        } else {
          void refreshSnapshot(nextClient);
        }
        void refreshHistory(nextClient, archived);
      }
      if (notification.method === "turn/error") {
        void refreshSnapshot(nextClient);
        void refreshHistory(nextClient, archived);
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
        await refreshSnapshot(nextClient, undefined, initialize.scope, false, null, true);
        await refreshHistory(nextClient, archived);
        await refreshAgentSurface(nextClient, initialize.scope);
        await refreshWorkspaceSurface(nextClient, initialize.scope, null);
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
      void refreshHistory(client, archived);
    }
  }, [archived, client]);

  useEffect(() => {
    if (client && activeScope) {
      void refreshAgentSurface(client, activeScope);
    }
  }, [client, activeScope, currentThreadId, running]);

  function beginExplicitViewSwitch(): number {
    viewEpochRef.current += 1;
    pendingDetachedShellRef.current = null;
    setDraftSession(null);
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
        return normalizeSnapshot(reconcileThreadSnapshot(normalizeSnapshot(current), normalizeSnapshot(nextSnapshot)));
      });
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
      return normalizeSnapshot(reconcileThreadSnapshot(currentSnapshot, incomingSnapshot));
    });
    await adoptSnapshotScope(nextClient, nextSnapshot);
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

  async function refreshHistory(nextClient = client, includeArchived = archived, workdir: string | null = null) {
    if (!nextClient) {
      return;
    }
    const result = ThreadListResultSchema.parse(
      await nextClient.request("thread/list", { archived: includeArchived, limit: 100, workdir: workdir ?? null })
    );
    setSessions(result.sessions.map(normalizeSessionSummary));
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
    threadId: string | null = currentThreadId ?? null
  ) {
    if (!nextClient || !scope) {
      return;
    }
    const [files, diff, context] = await Promise.all([
      nextClient.request("workspace/files", { scope }),
      nextClient.request("workspace/diff", { scope, path: null }),
      nextClient.request("context/read", { scope, threadId })
    ]);
    setWorkspaceFiles(WorkspaceFilesResultSchema.parse(files));
    setWorkspaceDiff(WorkspaceDiffResultSchema.parse(diff));
    setContextUsage(ContextReadResultSchema.parse(context));
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

  async function executeCommand(command: string) {
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
    if (asRecord(record.action).type === "passThroughPrompt") {
      await runHostAction(record.action);
      return;
    }
    if (record.accepted !== true && !isKnownCommand(command, commands)) {
      await submitTurn(command, []);
      return;
    }
    if (record.accepted !== true) {
      setCommandFeedback({
        accepted: false,
        command,
        message: optionalStringField(record.message) ?? `Unsupported command: ${command}`
      });
      setMainView("tools");
      setMobilePanel("transcript");
      return;
    }
    setCommandFeedback(optionalStringField(record.message) ? {
      accepted: true,
      command,
      message: optionalStringField(record.message) ?? ""
    } : null);
    await runHostAction(record.action);
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
      setSnapshot(normalizeSnapshot(nextSnapshot));
      setDraftSession(createHistoryDraftSession(epoch, scope.workdir));
      setArchived(false);
      await adoptSnapshotScope(client, nextSnapshot);
    }
    await refreshHistory(client, false);
    setMobilePanel("transcript");
  }

  async function runHostAction(action: unknown) {
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
      case "queuePrompt": {
        const text = stringField(record.text).trim();
        if (text) {
          await submitTurn(text, []);
        }
        break;
      }
      case "passThroughPrompt":
      case "submitPrompt": {
        const text = stringField(record.text).trim();
        if (text) {
          await submitTurn(text, []);
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
            message: "/steer is only available while a turn is running."
          });
          setMainView("tools");
          setMobilePanel("transcript");
        }
        break;
      }
      case "downloadSession":
        if (endpoint && snapshot.thread?.id) {
          const kind = stringField(record.kind) === "share" ? "share" : "export";
          void host?.open.openDownload(downloadUrl(endpoint, snapshot.thread.id, kind));
        }
        break;
      case "workspaceDiff": {
        const diff = WorkspaceDiffResultSchema.parse(record.diff);
        setPreview(diffPreviewState(diff));
        setMainView("transcript");
        setMobilePanel("transcript");
        break;
      }
      case "showPanel":
        switch (stringField(record.panel)) {
          case "history":
          case "sessions":
            setMobilePanel("history");
            break;
          case "agents":
            setMainView("agents");
            setMobilePanel("transcript");
            break;
          case "commands":
          case "help":
            setMainView("tools");
            setMobilePanel("transcript");
            break;
          case "status":
          default:
            setRightTab("status");
            setMobilePanel("status");
            break;
        }
        break;
      default:
        if (record.type) {
          setError(`Unsupported host action: ${String(record.type)}`);
        }
    }
  }

  async function submitTurn(text: string, mentions: GatewayMention[]) {
    const scope = activeScope ?? init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    const nextInput: GatewayInputPart[] = [
      ...(text.trim() ? [{ type: "text" as const, text }] : []),
      ...attachments.map((attachment) => attachment.input)
    ];
    const optimisticText = text.trim() || attachments.map((attachment) => `[Attachment: ${attachment.name}]`).join(" ");
    pendingDetachedShellRef.current = null;
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
        message: optionalStringField(record.message) ?? "Shell command was not accepted."
      });
      setMainView("tools");
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
      setPreview(null);
      return;
    }
    const scope = activeScope ?? init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    const result = WorkspaceFileReadResultSchema.parse(await client?.request("workspace/file/read", { scope, path }));
    if (result.binary || result.content === null) {
      setPreview(null);
      return;
    }
    setPreview(filePreviewState(result));
    setMainView("transcript");
    setMobilePanel("transcript");
  }

  async function openDiffPreview(path?: string | null) {
    const scope = activeScope ?? init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    const result = WorkspaceDiffResultSchema.parse(await client?.request("workspace/diff", { scope, path: path ?? null }));
    setWorkspaceDiff((current) => path ? current : result);
    setPreview(diffPreviewState(result));
    setMainView("transcript");
    setMobilePanel("transcript");
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

  return (
    <main className="appShell" data-main-view={mainView}>
      {error && (
        <div className="errorBand" role="alert">
          <AlertTriangle size={17} aria-hidden />
          <span>{error}</span>
        </div>
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
            {rightTabLabel(rightTab)}
          </button>
        )}
      </nav>

      <div className={`workbench ${leftCollapsed ? "is-leftCollapsed" : ""} ${rightCollapsed || !showSessionChrome ? "is-rightCollapsed" : ""}`}>
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
              <button aria-label="Search" className={mainView === "search" ? "is-selected" : ""} onClick={() => setMainView("search")} type="button">
                <Search size={16} /> <span>Search</span>
              </button>
              <button aria-label="Artifacts" className={mainView === "artifacts" ? "is-selected" : ""} onClick={() => setMainView("artifacts")} type="button">
                <Archive size={16} /> <span>Artifacts</span>
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
                    setMainView("transcript");
                    setMobilePanel("transcript");
                  })}
                  onUnpin={togglePinnedSession}
                />
                <HistoryPanel
                  archived={archived}
                  currentThreadId={currentThreadId}
                  disabled={disabled}
                  draftSession={visibleDraftSession}
                  pinnedSessionIds={pinnedSessionIds}
                  sessions={sessions}
                  onArchive={(threadId) => void runAction(async () => {
                    setDraftSession(null);
                    await client?.request("thread/archive", { threadId });
                    await refreshHistory();
                  })}
                  onDelete={(threadId) => void runAction(async () => {
                    setDraftSession(null);
                    await client?.request("thread/delete", { threadId });
                    await refreshHistory();
                  })}
                  onExport={(threadId) => {
                    if (endpoint) {
                      void host?.open.openDownload(downloadUrl(endpoint, threadId, "export"));
                    }
                  }}
                  onNew={() => void runAction(async () => {
                    await startNewThread();
                  })}
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
                  })}
                  onResumeDraft={() => {
                    setMainView("transcript");
                    setMobilePanel("transcript");
                  }}
                  onResume={(threadId) => void runAction(async () => {
                    const epoch = beginExplicitViewSwitch();
                    await refreshSnapshot(client, threadId, undefined, false, epoch);
                    setMainView("transcript");
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
            <LeftUtilityRail value={mainView} onChange={setMainView} />
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
          <div className={`centerWorkspace ${showSessionChrome && preview ? "has-preview" : ""}`}>
            <MainSurface
              agents={agents}
              appearance={appearance}
              archived={archived}
              backends={backends}
              commands={commands}
              debugEnabled={debugEnabled}
              feedback={commandFeedback}
              mainView={mainView}
              sessions={sessions}
              loadThreadSearchText={loadThreadSearchText}
              onAppearanceChange={setAppearance}
              onArchivedChange={setArchived}
              onCommand={(slash) => void runAction(async () => executeCommand(slash))}
              onDebugChange={setDebugEnabled}
              onMainViewChange={setMainView}
              onOpenSession={(threadId) => void runAction(async () => {
                const epoch = beginExplicitViewSwitch();
                await refreshSnapshot(client, threadId, undefined, false, epoch);
                setMainView("transcript");
                setMobilePanel("transcript");
              })}
              settings={settings}
              transcript={<TranscriptPanel activity={activity} entries={transcriptEntries} onCopyText={copyTranscriptText} />}
            />
            {showSessionChrome && preview && <PreviewPane preview={preview} onClose={() => setPreview(null)} />}
          </div>
          {showSessionChrome && <div className="composerDock">
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
                  model={selectedModel}
                  variant={selectedVariant}
                  onContextClick={() => setRightTab("status")}
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
              onCommand={(command) => void runAction(async () => executeCommand(command))}
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
              onBranchClick={() => {
                setRightTab("status");
                setMobilePanel("status");
              }}
              onPathClick={() => {
                setRightTab("files");
                setMobilePanel("status");
              }}
              onPermissionModeChange={setPermissionMode}
            />
          </div>}
        </section>

        {showSessionChrome && !rightCollapsed && (
          <aside className={`statusColumn ${mobilePanel === "status" ? "is-mobileSelected" : ""}`}>
            <RightInspectorTabs debugEnabled={debugEnabled} value={rightTab} onChange={setRightTab} />
            {rightTab === "status" && (
              <StatusPanel
                activity={activity}
                changedFiles={workspaceDiff?.files}
                context={contextUsage ?? undefined}
                sessionId={snapshot.thread?.id ?? null}
                status={status}
                onChangedFile={(path) => void runAction(async () => openDiffPreview(path))}
                onRefresh={() => void runAction(async () => {
                  await refreshSnapshot();
                  await refreshHistory();
                  await refreshAgentSurface();
                  await refreshWorkspaceSurface();
                })}
              />
            )}
            {rightTab === "files" && (
              <FilesPanel
                files={workspaceFiles?.entries ?? []}
                root={workspaceFiles?.root ?? settings?.workdir ?? ""}
                truncated={workspaceFiles?.truncated ?? false}
                onOpen={(path) => void runAction(async () => openFilePreview(path))}
              />
            )}
            {rightTab === "debug" && debugEnabled && <DebugPanel events={debugEvents} />}
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
                <small>{session.project?.label ?? "project"}</small>
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
  agents,
  appearance,
  archived,
  backends,
  commands,
  debugEnabled,
  feedback,
  loadThreadSearchText,
  mainView,
  onAppearanceChange,
  onArchivedChange,
  onCommand,
  onDebugChange,
  onMainViewChange,
  onOpenSession,
  settings,
  sessions,
  transcript
}: {
  agents: WorkbenchAgent[];
  appearance: Appearance;
  archived: boolean;
  backends: WorkbenchBackend[];
  commands: WorkbenchCommand[];
  debugEnabled: boolean;
  feedback: CommandFeedback;
  loadThreadSearchText(threadId: string): Promise<string>;
  mainView: MainView;
  onAppearanceChange(value: Appearance): void;
  onArchivedChange(value: boolean): void;
  onCommand(slash: string): void;
  onDebugChange(value: boolean): void;
  onMainViewChange(value: MainView): void;
  onOpenSession(threadId: string): void;
  settings: SettingsReadResult | undefined;
  sessions: SessionSummary[];
  transcript: ReactNode;
}) {
  if (mainView === "transcript") {
    return <>{transcript}</>;
  }
  if (mainView === "settings") {
    return (
      <SettingsPage
        appearance={appearance}
        archived={archived}
        debugEnabled={debugEnabled}
        settings={settings}
        onAppearanceChange={onAppearanceChange}
        onArchivedChange={onArchivedChange}
        onDebugChange={onDebugChange}
        onOpenTranscript={() => onMainViewChange("transcript")}
      />
    );
  }
  if (mainView === "agents") {
    return <AgentsPanel agents={agents} backends={backends} />;
  }
  if (mainView === "tools") {
    return <CommandsPanel commands={commands} feedback={feedback} onExecute={onCommand} />;
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
  if (mainView === "artifacts") {
    return <PlaceholderPage icon={<Archive size={18} />} title="Artifacts" body="No artifacts in this session." />;
  }
  if (mainView === "skills") {
    return <PlaceholderPage icon={<Sparkles size={18} />} title="Skills" body="Skill browsing is available through completion and command surfaces in this slice." />;
  }
  return <PlaceholderPage icon={<Cable size={18} />} title="MCP" body="MCP status will appear here when servers are configured." />;
}

function SettingsPage({
  appearance,
  archived,
  debugEnabled,
  settings,
  onAppearanceChange,
  onArchivedChange,
  onDebugChange,
  onOpenTranscript
}: {
  appearance: Appearance;
  archived: boolean;
  debugEnabled: boolean;
  settings: SettingsReadResult | undefined;
  onAppearanceChange(value: Appearance): void;
  onArchivedChange(value: boolean): void;
  onDebugChange(value: boolean): void;
  onOpenTranscript(): void;
}) {
  return (
    <section className="centerPage settingsPage" aria-label="Settings">
      <header>
        <div className="centerPageTitle">
          <Settings size={18} />
          <div>
            <h2>Settings</h2>
            <p>{settings?.project?.displayPath ?? settings?.workdir ?? "local workbench"}</p>
          </div>
        </div>
        <button
          aria-label="Back to transcript"
          className="centerPageBack"
          data-tooltip="Back to transcript"
          onClick={onOpenTranscript}
          title="Back to transcript"
          type="button"
        >
          <X size={15} />
        </button>
      </header>
      <div className="settingsRows">
        <div className="settingsRow">
          <div>
            <strong>Appearance</strong>
            <span>Switch the shared Workbench surface between dark and light.</span>
          </div>
          <div className="segmentedControl">
            <button className={appearance === "dark" ? "is-selected" : ""} onClick={() => onAppearanceChange("dark")} type="button">
              <Moon size={15} /> Dark
            </button>
            <button className={appearance === "light" ? "is-selected" : ""} onClick={() => onAppearanceChange("light")} type="button">
              <Sun size={15} /> Light
            </button>
          </div>
        </div>
        <div className="settingsRow">
          <div>
            <strong>Session history</strong>
            <span>Choose whether the Sessions list shows active or archived sessions.</span>
          </div>
          <div className="segmentedControl">
            <button className={!archived ? "is-selected" : ""} onClick={() => onArchivedChange(false)} type="button">
              <MessageSquare size={15} /> Active
            </button>
            <button className={archived ? "is-selected" : ""} onClick={() => onArchivedChange(true)} type="button">
              <Archive size={15} /> Archived
            </button>
          </div>
        </div>
        <div className="settingsRow">
          <div>
            <strong>Debug</strong>
            <span>Show a right-side Debug tab with recent Gateway notifications.</span>
          </div>
          <label className="switchControl">
            <input checked={debugEnabled} onChange={(event) => onDebugChange(event.target.checked)} type="checkbox" />
            <span>{debugEnabled ? "On" : "Off"}</span>
          </label>
        </div>
      </div>
    </section>
  );
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
          const project = session.project?.label ?? "";
          const summaryHaystack = normalizeSearchText(`${session.id} ${title} ${session.preview ?? ""} ${project} ${session.workdir}`);
          if (summaryHaystack.includes(needle)) {
            next.push({
              excerpt: session.id,
              id: session.id,
              kind: "session",
              subtitle: `${project || "project"} · ${session.visibleEntryCount ?? session.messageCount ?? 0} entries`,
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
      <input autoFocus placeholder="Search current project" value={query} onChange={(event) => setQuery(event.target.value)} />
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
          <span>{query.trim() ? (searching ? "Searching sessions..." : "No matches in this project.") : "Type to search local session material."}</span>
          <button onClick={onOpenTranscript} type="button">Back to transcript</button>
        </div>
      )}
    </section>
  );
}

function PlaceholderPage({ body, icon, title }: { body: string; icon: ReactNode; title: string }) {
  return (
    <section className="centerPage" aria-label={title}>
      <header>
        {icon}
        <div>
          <h2>{title}</h2>
          <p>{body}</p>
        </div>
      </header>
    </section>
  );
}

function PreviewPane({ preview, onClose }: { preview: NonNullable<PreviewState>; onClose(): void }) {
  return (
    <aside className={`previewPane is-${preview.kind}`} aria-label="Inline preview">
      <header>
        <div>
          <span>{preview.kind}</span>
          <h2>{preview.title}</h2>
        </div>
        <button aria-label="Close preview" onClick={onClose} type="button">
          <X size={16} />
        </button>
      </header>
      <pre>{preview.body || (preview.kind === "diff" ? "No diff content" : "No preview content")}</pre>
      {preview.kind === "file" && preview.truncated && <p>Preview truncated.</p>}
      {preview.kind === "file" && preview.binary && <p>Binary file preview is not available.</p>}
    </aside>
  );
}

function RightInspectorTabs({
  debugEnabled,
  value,
  onChange
}: {
  debugEnabled: boolean;
  value: RightTab;
  onChange(value: RightTab): void;
}) {
  const tabs: Array<{ icon: ReactNode; label: string; value: RightTab }> = [
    { icon: <Shield size={15} />, label: "Status", value: "status" },
    { icon: <FolderTree size={15} />, label: "Files", value: "files" },
    ...(debugEnabled ? [{ icon: <Bug size={15} />, label: "Debug", value: "debug" as const }] : [])
  ];
  return (
    <nav className={`rightTabs ${debugEnabled ? "has-debug" : ""}`} aria-label="Inspector tabs">
      {tabs.map((tab) => (
        <button className={value === tab.value ? "is-selected" : ""} key={tab.value} onClick={() => onChange(tab.value)} type="button">
          {tab.icon}
          <span>{tab.label}</span>
        </button>
      ))}
    </nav>
  );
}

function FilesPanel({
  files,
  root,
  truncated,
  onOpen
}: {
  files: WorkspaceFileEntry[];
  root: string;
  truncated: boolean;
  onOpen(path: string): void;
}) {
  const [collapsedDirs, setCollapsedDirs] = useState<Set<string>>(() => new Set());
  const directoryPaths = useMemo(
    () => new Set(files.filter((file) => file.kind === "directory").map((file) => file.path)),
    [files]
  );
  const visibleFiles = useMemo(
    () => files.filter((file) => !hasCollapsedDirectoryAncestor(file.path, collapsedDirs)),
    [collapsedDirs, files]
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
    <section className="filesPanel" aria-label="Project files">
      <header>
        <FolderTree size={17} />
        <div>
          <h2>Files</h2>
          <p>{root}</p>
        </div>
      </header>
      <div className="fileTree">
        {visibleFiles.map((file) => {
          const directory = file.kind === "directory";
          const collapsed = directory && collapsedDirs.has(file.path);
          const previewable = directory || !isUnsupportedPreviewFile(file.path);
          return (
            <button
              aria-expanded={directory ? !collapsed : undefined}
              className={directory ? "is-directory" : "is-file"}
              disabled={!previewable}
              key={`${file.kind}:${file.path}`}
              onClick={() => directory ? toggleDirectory(file.path) : onOpen(file.path)}
              style={{ "--depth": file.depth } as CSSProperties}
              type="button"
            >
              <span className="fileTreeDisclosure" aria-hidden>
                {directory ? (collapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />) : null}
              </span>
              {directory ? <FolderTree size={14} /> : <FileText size={14} />}
              <span>{file.name}</span>
            </button>
          );
        })}
        {visibleFiles.length === 0 && <p>No project files.</p>}
      </div>
      {truncated && <footer>File tree truncated.</footer>}
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

function DebugPanel({ events }: { events: DebugEvent[] }) {
  return (
    <section className="debugPanel" aria-label="Debug event stream">
      <header>
        <Bug size={17} />
        <div>
          <h2>Debug</h2>
          <p>{events.length} recent notifications</p>
        </div>
      </header>
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
  model,
  variant,
  onContextClick,
  onModelChange,
  onVariantChange
}: {
  context: ContextReadResult | null;
  controls: SettingsReadResult["controls"];
  model: string | null;
  variant: string;
  onContextClick(): void;
  onModelChange(value: string | null): void;
  onVariantChange(value: string): void;
}) {
  const contextPercent = typeof context?.percent === "number" ? Math.max(0, Math.min(100, context.percent)) : 0;
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
        renderValue={compactModelLabel}
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
            onContextClick();
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
            {context?.categories?.length ? (
              <div className="composerContextBars">
                {context.categories.slice(0, 5).map((category) => (
                  <div className="composerContextBar" key={category.id}>
                    <span>{category.label}</span>
                    <meter max={100} min={0} value={category.percent ?? 0} />
                  </div>
                ))}
              </div>
            ) : (
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
  onBranchClick,
  onPathClick,
  onPermissionModeChange
}: {
  branch: string | null;
  controls: SettingsReadResult["controls"];
  path: string;
  permissionMode: string;
  onBranchClick(): void;
  onPathClick(): void;
  onPermissionModeChange(value: string): void;
}) {
  return (
    <div className="composerStatusLine" aria-label="Composer status">
      <StatusSelect label="Permission mode" value={permissionMode} values={controls?.permissionModeOptions ?? ["default"]} onChange={onPermissionModeChange} />
      <button className="pathStatusButton" onClick={onPathClick} title={path} type="button">{path || "project"}</button>
      <button className="branchStatusButton" onClick={onBranchClick} type="button">
        <GitBranch size={13} />
        <span>{branch || "no-branch"}</span>
      </button>
    </div>
  );
}

function StatusSelect({
  label,
  renderValue,
  value,
  values,
  onChange
}: {
  label: string;
  renderValue?(value: string): string;
  value: string;
  values: string[];
  onChange(value: string): void;
}) {
  return (
    <label className="statusSelect" data-status={label.toLowerCase().replace(/\s+/g, "-")} title={label}>
      <select aria-label={label} title={value || label} value={value} onChange={(event) => onChange(event.target.value)}>
        {values.map((option) => (
          <option key={option || "default"} value={option}>{renderValue?.(option) ?? defaultStatusSelectValue(label, option)}</option>
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
            {agent.name}
          </option>
        ))}
      </select>
    </label>
  );
}

function agentOptionValue(agent: WorkbenchAgent): string {
  return agent.source === "explicit" ? agent.path?.trim() || agent.name : agent.name;
}

function AgentSurfacePanel({
  agents,
  backends,
  commands
}: {
  agents: WorkbenchAgent[];
  backends: WorkbenchBackend[];
  commands: WorkbenchCommand[];
}) {
  return (
    <section className="agentSurfacePanel" aria-label="Agents and commands">
      <header>
        <span>Agents</span>
        <b>{agents.length}</b>
      </header>
      <div className="agentSurfaceList">
        {agents.slice(0, 5).map((agent) => (
          <div className="agentSurfaceRow" key={`${agent.source}:${agent.name}`}>
            <div>
              <strong>{agent.name}</strong>
              <span>{agent.description}</span>
            </div>
            <small>{agent.entrypoints.join("/") || agent.source}</small>
          </div>
        ))}
        {agents.length === 0 && <p>No agents configured.</p>}
      </div>

      <header>
        <span><PlugZap size={15} /> Backends</span>
        <b>{backends.length}</b>
      </header>
      <div className="agentSurfaceList">
        {backends.slice(0, 4).map((backend) => (
          <div className="agentSurfaceRow" key={backend.id}>
            <div>
              <strong>{backend.label || backend.id}</strong>
              <span>{backend.command || backend.description || backend.kind}</span>
            </div>
            <small>{backend.enabled ? "enabled" : "disabled"}</small>
          </div>
        ))}
        {backends.length === 0 && <p>No peer backends.</p>}
      </div>

      <header>
        <span><TerminalSquare size={15} /> Commands</span>
        <b>{commands.length}</b>
      </header>
      <div className="commandChipList">
        {commands.slice(0, 10).map((command) => (
          <span title={command.summary} key={`${command.source}:${command.name}`}>
            {command.slash}
          </span>
        ))}
      </div>
    </section>
  );
}

function AgentsPanel({
  agents,
  backends
}: {
  agents: WorkbenchAgent[];
  backends: WorkbenchBackend[];
}) {
  return (
    <section className="agentSurfacePanel" aria-label="Agents">
      <header>
        <span>Agents</span>
        <b>{agents.length}</b>
      </header>
      <div className="agentSurfaceList">
        {agents.map((agent) => (
          <div className="agentSurfaceRow" key={`${agent.source}:${agent.name}`}>
            <div>
              <strong>{agent.name}</strong>
              <span>{agent.description}</span>
            </div>
            <small>{agent.entrypoints.join("/") || agent.source}</small>
          </div>
        ))}
        {agents.length === 0 && <p>No agents configured.</p>}
      </div>

      <header>
        <span><PlugZap size={15} /> Backends</span>
        <b>{backends.length}</b>
      </header>
      <div className="agentSurfaceList">
        {backends.map((backend) => (
          <div className="agentSurfaceRow" key={backend.id}>
            <div>
              <strong>{backend.label || backend.id}</strong>
              <span>{backend.command || backend.description || backend.kind}</span>
            </div>
            <small>{backend.enabled ? "enabled" : "disabled"}</small>
          </div>
        ))}
        {backends.length === 0 && <p>No peer backends.</p>}
      </div>
    </section>
  );
}

function CommandsPanel({
  commands,
  feedback,
  onExecute
}: {
  commands: WorkbenchCommand[];
  feedback: CommandFeedback;
  onExecute: (slash: string) => void;
}) {
  return (
    <section className="agentSurfacePanel commandSurfacePanel" aria-label="Commands">
      <header>
        <span><TerminalSquare size={15} /> Commands</span>
        <b>{commands.length}</b>
      </header>
      {feedback && (
        <div className={`commandFeedback ${feedback.accepted ? "is-ok" : "is-error"}`}>
          <strong>{feedback.command}</strong>
          <span>{feedback.message}</span>
        </div>
      )}
      <div className="commandSurfaceList">
        {commands.map((command) => (
          <button
            className="commandSurfaceRow"
            key={`${command.source}:${command.name}`}
            onClick={() => onExecute(command.slash)}
            title={command.usage || command.summary}
            type="button"
          >
            <code>{command.slash}</code>
            <span>{command.summary}</span>
            {command.aliases.length > 0 && <small>{command.aliases.map((alias) => `/${alias}`).join(" ")}</small>}
          </button>
        ))}
        {commands.length === 0 && <p>No commands available.</p>}
      </div>
    </section>
  );
}

function rightTabLabel(value: RightTab): string {
  switch (value) {
    case "files":
      return "Files";
    case "debug":
      return "Debug";
    case "status":
    default:
      return "Status";
  }
}

function readWorkbenchPrefs(): WorkbenchPrefs {
  try {
    const raw = window.localStorage.getItem(PREFS_KEY);
    const value = raw ? JSON.parse(raw) as Partial<WorkbenchPrefs> : {};
    return {
      appearance: value.appearance === "light" ? "light" : "dark",
      debug: value.debug === true
    };
  } catch {
    return { appearance: "dark", debug: false };
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

function diffPreviewState(diff: WorkspaceDiffResult): PreviewState {
  return {
    body: diff.unifiedDiff,
    kind: "diff",
    path: diff.selectedPath,
    title: diff.selectedPath ? `Diff: ${diff.selectedPath}` : "Workspace Diff"
  };
}

function filePreviewState(file: WorkspaceFileReadResult): PreviewState {
  return {
    binary: file.binary,
    body: file.content ?? file.unreadable ?? "",
    kind: "file",
    path: file.path,
    title: file.path,
    truncated: file.truncated
  };
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

function isKnownCommand(command: string, commands: WorkbenchCommand[]): boolean {
  const name = command
    .trim()
    .replace(/^\/+/, "")
    .split(/\s+/, 1)[0]
    ?.toLowerCase();
  if (!name) {
    return true;
  }
  if (name === "commands") {
    return true;
  }
  return commands.some((candidate) => (
    candidate.name.toLowerCase() === name ||
    candidate.slash.replace(/^\/+/, "").toLowerCase() === name ||
    candidate.aliases.some((alias) => alias.toLowerCase() === name)
  ));
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
          entrypoints: stringArray(item.entrypoints)
        };
      }).filter((backend) => backend.id)
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
          source: stringField(item.source)
        };
      }).filter((command) => command.name)
    : [];
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
