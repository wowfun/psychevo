import { useEffect, useMemo, useState } from "react";
import { AlertTriangle, Bot, MessageSquare, PanelLeft, PanelRight, PlugZap, TerminalSquare } from "lucide-react";
import {
  Composer,
  HistoryPanel,
  StatusPanel,
  TranscriptPanel
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
  SettingsReadResultSchema,
  ThreadListResultSchema,
  type GatewayMention,
  type InitializeResult,
  type PermissionDecision,
  type SessionSummary,
  type SettingsReadResult,
  type ThreadSnapshot
} from "@psychevo/protocol";

const EMPTY_SNAPSHOT: ThreadSnapshot = {
  source: { kind: "web", rawId: "pending", lifetime: "persistent", rawIdentity: null, visibleName: null },
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

type UtilityPanel = "status" | "agents" | "commands";

type CommandFeedback = {
  accepted: boolean;
  command: string;
  message: string;
} | null;

export function App() {
  const [client, setClient] = useState<GatewayClient | null>(null);
  const [host, setHost] = useState<PsychevoHost | null>(null);
  const [endpoint, setEndpoint] = useState<GatewayEndpoint | null>(null);
  const [init, setInit] = useState<InitializeResult | null>(null);
  const [snapshot, setSnapshot] = useState<ThreadSnapshot>(EMPTY_SNAPSHOT);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [settings, setSettings] = useState<SettingsReadResult | undefined>();
  const [agents, setAgents] = useState<WorkbenchAgent[]>([]);
  const [backends, setBackends] = useState<WorkbenchBackend[]>([]);
  const [commands, setCommands] = useState<WorkbenchCommand[]>([]);
  const [utilityPanel, setUtilityPanel] = useState<UtilityPanel>("status");
  const [commandFeedback, setCommandFeedback] = useState<CommandFeedback>(null);
  const [selectedAgentName, setSelectedAgentName] = useState<string>("");
  const [archived, setArchived] = useState(false);
  const [status, setStatus] = useState("connecting");
  const [error, setError] = useState<string | null>(null);
  const [mobilePanel, setMobilePanel] = useState<"history" | "transcript" | "status">("transcript");

  const turnModel = useMemo(() => {
    const value = new URLSearchParams(window.location.search).get("model")?.trim();
    return value || undefined;
  }, []);
  const activity = normalizeActivity(snapshot.activity);
  const transcriptEntries = Array.isArray(snapshot.entries) ? snapshot.entries : [];
  const pendingClarifies = Array.isArray(snapshot.pendingClarifies) ? snapshot.pendingClarifies : [];
  const pendingPermissions = Array.isArray(snapshot.pendingPermissions) ? snapshot.pendingPermissions : [];
  const running = activity.running;
  const disabled = status !== "connected";
  const currentThreadId = snapshot.thread?.id;
  const peerAgents = useMemo(
    () => agents.filter((agent) => agent.entrypoints.includes("peer") && agent.backend?.ref),
    [agents]
  );

  useEffect(() => {
    if (selectedAgentName && !peerAgents.some((agent) => agent.name === selectedAgentName)) {
      setSelectedAgentName("");
    }
  }, [peerAgents, selectedAgentName]);

  useEffect(() => {
    let alive = true;
    const nextHost = createBrowserHost(window.location, window.localStorage);
    const nextEndpoint = nextHost.endpoint;
    const nextClient = new GatewayClient(nextEndpoint);
    setHost(nextHost);
    setEndpoint(nextEndpoint);

    nextClient.subscribe((notification) => {
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
            void refreshSnapshot(nextClient, threadId, undefined, true);
            void refreshHistory(nextClient, archived);
            for (const delay of [1_500, 3_000, 7_500, 15_000, 30_000, 60_000, 120_000]) {
              window.setTimeout(() => {
                void refreshSnapshot(nextClient, threadId, undefined, true);
                void refreshHistory(nextClient, archived);
              }, delay);
            }
            window.setTimeout(() => {
              void refreshSnapshot(nextClient, threadId, undefined, true);
            }, 750);
          }
          if (["permissionRequested", "permissionResolved", "clarifyRequested", "clarifyResolved"].includes(event.type)) {
            void refreshSnapshot(nextClient);
          }
        }
      }
      if (notification.method === "turn/result" || notification.method === "turn/error") {
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
        await refreshSnapshot(nextClient, undefined, initialize.scope);
        await refreshHistory(nextClient, archived, initialize.scope.workdir);
        const nextSettings = SettingsReadResultSchema.parse(await nextClient.request("settings/read", { workdir: initialize.scope.workdir }));
        setSettings(nextSettings);
        await refreshAgentSurface(nextClient, initialize.scope);
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
    if (client) {
      void refreshHistory(client, archived);
    }
  }, [archived, client]);

  useEffect(() => {
    if (client && init?.scope) {
      void refreshAgentSurface(client, init.scope);
    }
  }, [client, init?.scope, currentThreadId, running]);

  async function refreshSnapshot(nextClient = client, threadId?: string, scope = init?.scope, readOnly = false) {
    if (!nextClient) {
      return;
    }
    if (threadId && readOnly) {
      const nextSnapshot = parseThreadSnapshot(await nextClient.request("thread/read", { threadId }));
      setSnapshot((current) => current.thread?.id === threadId
        || (current.thread === null && current.entries.length === 0)
        ? normalizeSnapshot(reconcileThreadSnapshot(normalizeSnapshot(current), normalizeSnapshot(nextSnapshot)))
        : current);
      return;
    }
    const nextScope = scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    const params = threadId ? { threadId, scope: nextScope } : { scope: nextScope };
    const nextSnapshot = parseThreadSnapshot(await nextClient.request("thread/resume", params));
    setSnapshot((current) => normalizeSnapshot(reconcileThreadSnapshot(normalizeSnapshot(current), normalizeSnapshot(nextSnapshot))));
  }

  async function refreshHistory(nextClient = client, includeArchived = archived, workdir = init?.scope.workdir) {
    if (!nextClient) {
      return;
    }
    const result = ThreadListResultSchema.parse(
      await nextClient.request("thread/list", { archived: includeArchived, limit: 100, workdir: workdir ?? null })
    );
    setSessions(result.sessions.map(normalizeSessionSummary));
  }

  async function refreshAgentSurface(nextClient = client, scope = init?.scope) {
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

  async function runAction(action: () => Promise<void>) {
    try {
      setError(null);
      await action();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  async function executeCommand(command: string) {
    const scope = init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    const result = await client?.request("command/execute", {
      command,
      scope,
      threadId: snapshot.thread?.id ?? null
    });
    if (!result) {
      return;
    }
    const record = asRecord(result);
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
      setUtilityPanel("commands");
      setMobilePanel("status");
      return;
    }
    setCommandFeedback(optionalStringField(record.message) ? {
      accepted: true,
      command,
      message: optionalStringField(record.message) ?? ""
    } : null);
    await runHostAction(record.action);
  }

  async function runHostAction(action: unknown) {
    const record = asRecord(action);
    switch (record.type) {
      case "threadStart": {
        const scope = init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
        const nextSnapshot = parseThreadSnapshot(await client?.request("thread/start", { scope }));
        setSnapshot(normalizeSnapshot(nextSnapshot));
        await refreshHistory();
        setMobilePanel("transcript");
        break;
      }
      case "threadArchive":
        if (snapshot.thread?.id) {
          await client?.request("thread/archive", { threadId: snapshot.thread.id });
          await refreshHistory();
        }
        break;
      case "threadDelete":
        if (snapshot.thread?.id) {
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
      case "downloadSession":
        if (endpoint && snapshot.thread?.id) {
          const kind = stringField(record.kind) === "share" ? "share" : "export";
          void host?.open.openDownload(downloadUrl(endpoint, snapshot.thread.id, kind));
        }
        break;
      case "showPanel":
        switch (stringField(record.panel)) {
          case "history":
          case "sessions":
            setMobilePanel("history");
            break;
          case "agents":
            setUtilityPanel("agents");
            setMobilePanel("status");
            break;
          case "commands":
          case "help":
            setUtilityPanel("commands");
            setMobilePanel("status");
            break;
          case "status":
          default:
            setUtilityPanel("status");
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
    const scope = init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    setSnapshot((current) => appendOptimisticPrompt(current, text));
    await client?.request("turn/start", {
      agentName: selectedAgentName || null,
      input: [],
      mentions,
      model: turnModel ?? null,
      reasoningEffort: null,
      scope,
      threadId: snapshot.thread?.id ?? null,
      text
    });
    await refreshHistory();
  }

  const title = useMemo(() => {
    if (init?.source.visibleName) {
      return init.source.visibleName;
    }
    if (settings?.workdir) {
      return settings.workdir.split("/").filter(Boolean).at(-1) ?? "workbench";
    }
    return "workbench";
  }, [init?.source.visibleName, settings?.workdir]);

  return (
    <main className="appShell">
      <header className="topBar">
        <div className="brandMark">
          <span className="brandGlyph">p</span>
          <div>
            <h1>pevo</h1>
            <p>{title}</p>
          </div>
        </div>
        <div className="topMeta">
          <span className={`statePill ${running ? "is-running" : ""}`}>
            <span className="stateDot" aria-hidden />
            {running ? "running" : status}
          </span>
          <span className="endpointPill">{endpointLabel(endpoint?.httpBase)}</span>
        </div>
      </header>

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
        <button className={mobilePanel === "status" ? "is-selected" : ""} onClick={() => setMobilePanel("status")} type="button">
          <PanelRight size={17} />
          {utilityPanelLabel(utilityPanel)}
        </button>
      </nav>

      <div className="workbench">
        <aside className={`historyColumn ${mobilePanel === "history" ? "is-mobileSelected" : ""}`}>
          <HistoryPanel
            archived={archived}
            currentThreadId={currentThreadId}
            disabled={disabled}
            sessions={sessions}
            onArchive={(threadId) => void runAction(async () => {
              await client?.request("thread/archive", { threadId });
              await refreshHistory();
            })}
            onDelete={(threadId) => void runAction(async () => {
              await client?.request("thread/delete", { threadId });
              await refreshHistory();
            })}
            onExport={(threadId) => {
              if (endpoint) {
                void host?.open.openDownload(downloadUrl(endpoint, threadId, "export"));
              }
            }}
            onNew={() => void runAction(async () => {
              const scope = init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
              const nextSnapshot = parseThreadSnapshot(await client?.request("thread/start", { scope }));
              setSnapshot(normalizeSnapshot(nextSnapshot));
              await refreshHistory();
            })}
            onRename={(threadId, title) => void runAction(async () => {
              await client?.request("thread/rename", { threadId, title });
              await refreshHistory();
            })}
            onRestore={(threadId) => void runAction(async () => {
              await client?.request("thread/restore", { threadId });
              await refreshHistory();
            })}
            onResume={(threadId) => void runAction(async () => {
              await refreshSnapshot(client, threadId);
              setMobilePanel("transcript");
            })}
            onShare={(threadId) => {
              if (endpoint) {
                void host?.open.openDownload(downloadUrl(endpoint, threadId, "share"));
              }
            }}
            onToggleArchived={() => setArchived((value) => !value)}
          />
        </aside>

        <section className={`conversationColumn ${mobilePanel === "transcript" ? "is-mobileSelected" : ""}`}>
          <TranscriptPanel
            activity={activity}
            entries={transcriptEntries}
          />
          <div className="composerDock">
            <AgentRunSelector
              agents={peerAgents}
              disabled={disabled}
              value={selectedAgentName}
              onChange={setSelectedAgentName}
            />
            <Composer
              completionProvider={async (text, cursor) => {
                const scope = init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
                return await client?.request("completion/list", {
                  cursor,
                  scope,
                  text,
                  threadId: snapshot.thread?.id ?? null
                }) ?? { items: [], replacement: null };
              }}
              disabled={disabled}
              running={running}
              onCommand={(command) => void runAction(async () => executeCommand(command))}
              onInterrupt={() => void runAction(async () => {
                await client?.request("turn/interrupt", { threadId: snapshot.thread?.id ?? null });
                await refreshSnapshot();
              })}
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
          </div>
        </section>

        <aside className={`statusColumn ${mobilePanel === "status" ? "is-mobileSelected" : ""}`}>
          <UtilityTabs value={utilityPanel} onChange={setUtilityPanel} />
          {utilityPanel === "status" && (
            <>
              <StatusPanel
                activity={activity}
                pendingClarifies={pendingClarifies}
                pendingPermissions={pendingPermissions}
                settings={settings}
                status={status}
                onClarify={(requestId, answer) => void runAction(async () => {
                  await client?.request("clarify/respond", { requestId, threadId: snapshot.thread?.id ?? null, answers: [[answer]] });
                  await refreshSnapshot();
                })}
                onPermission={(requestId, decision: PermissionDecision) => void runAction(async () => {
                  await client?.request("permission/respond", { requestId, threadId: snapshot.thread?.id ?? null, decision });
                  await refreshSnapshot();
                })}
                onRefresh={() => void runAction(async () => {
                  await refreshSnapshot();
                  await refreshHistory();
                  await refreshAgentSurface();
                })}
              />
              <AgentSurfacePanel agents={agents} backends={backends} commands={commands} />
            </>
          )}
          {utilityPanel === "agents" && <AgentsPanel agents={agents} backends={backends} />}
          {utilityPanel === "commands" && (
            <CommandsPanel
              commands={commands}
              feedback={commandFeedback}
              onExecute={(slash) => void runAction(async () => executeCommand(slash))}
            />
          )}
        </aside>
      </div>
    </main>
  );
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
    <label className="agentRunSelector">
      <span><Bot size={14} aria-hidden /> Agent</span>
      <select
        aria-label="Run agent"
        disabled={disabled || agents.length === 0}
        value={value}
        onChange={(event) => onChange(event.target.value)}
      >
        <option value="">Default</option>
        {agents.map((agent) => (
          <option key={`${agent.source}:${agent.name}`} value={agent.name}>
            {agent.name}
          </option>
        ))}
      </select>
    </label>
  );
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
        <span><Bot size={15} /> Agents</span>
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

function UtilityTabs({
  value,
  onChange
}: {
  value: UtilityPanel;
  onChange: (value: UtilityPanel) => void;
}) {
  return (
    <nav className="utilityTabs" aria-label="Utility panels">
      <button className={value === "status" ? "is-selected" : ""} onClick={() => onChange("status")} type="button">
        Status
      </button>
      <button className={value === "agents" ? "is-selected" : ""} onClick={() => onChange("agents")} type="button">
        Agents
      </button>
      <button className={value === "commands" ? "is-selected" : ""} onClick={() => onChange("commands")} type="button">
        Commands
      </button>
    </nav>
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
        <span><Bot size={15} /> Agents</span>
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

function endpointLabel(value?: string): string {
  if (!value) {
    return "local";
  }
  try {
    return new URL(value).host;
  } catch {
    return value;
  }
}

function utilityPanelLabel(value: UtilityPanel): string {
  switch (value) {
    case "agents":
      return "Agents";
    case "commands":
      return "Commands";
    case "status":
    default:
      return "Status";
  }
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
