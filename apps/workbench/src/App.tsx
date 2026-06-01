import { useEffect, useMemo, useState } from "react";
import { AlertTriangle, MessageSquare, PanelLeft, PanelRight } from "lucide-react";
import {
  Composer,
  HistoryPanel,
  StatusPanel,
  TranscriptPanel
} from "@psychevo/components";
import {
  GatewayClient,
  scopeForWorkdir
} from "@psychevo/client";
import {
  createBrowserHost,
  downloadUrl,
  type GatewayEndpoint,
  type PsychevoHost
} from "@psychevo/host";
import {
  DebugEventsResultSchema,
  GatewayEventSchema,
  InitializeResultSchema,
  SettingsReadResultSchema,
  ThreadListResultSchema,
  ThreadSnapshotSchema,
  type GatewayEvent,
  type InitializeResult,
  type PermissionDecision,
  type SessionSummary,
  type SettingsReadResult,
  type TimelineDebugEvent,
  type ThreadSnapshot
} from "@psychevo/protocol";

const EMPTY_SNAPSHOT: ThreadSnapshot = {
  source: { kind: "web", rawId: "pending", lifetime: "persistent", rawIdentity: null, visibleName: null },
  thread: null,
  items: [],
  activity: { running: false, activeTurnId: null, queuedTurns: 0 },
  pendingPermissions: [],
  pendingClarifies: []
};

export function App() {
  const [client, setClient] = useState<GatewayClient | null>(null);
  const [host, setHost] = useState<PsychevoHost | null>(null);
  const [endpoint, setEndpoint] = useState<GatewayEndpoint | null>(null);
  const [init, setInit] = useState<InitializeResult | null>(null);
  const [snapshot, setSnapshot] = useState<ThreadSnapshot>(EMPTY_SNAPSHOT);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [settings, setSettings] = useState<SettingsReadResult | undefined>();
  const [events, setEvents] = useState<GatewayEvent[]>([]);
  const [debugEvents, setDebugEvents] = useState<TimelineDebugEvent[]>([]);
  const [archived, setArchived] = useState(false);
  const [status, setStatus] = useState("connecting");
  const [error, setError] = useState<string | null>(null);
  const [mobilePanel, setMobilePanel] = useState<"history" | "transcript" | "status">("transcript");

  const turnModel = useMemo(() => {
    const value = new URLSearchParams(window.location.search).get("model")?.trim();
    return value || undefined;
  }, []);
  const running = snapshot.activity.running;
  const disabled = status !== "connected";
  const currentThreadId = snapshot.thread?.id;

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
          setEvents((items) => [...items, event].slice(-80));
          if (event.type === "itemCompleted" && "item" in event) {
            setSnapshot((current) => ({
              ...current,
              items: [...current.items.filter((item) => item.id !== event.item.id), event.item]
            }));
          }
          if ((event.type === "turnStarted" || event.type === "turnCompleted") && event.threadId) {
            const threadId = event.threadId;
            void refreshSnapshot(nextClient, threadId, undefined, true);
            for (const delay of [1_500, 3_000, 7_500, 15_000, 30_000, 60_000, 120_000]) {
              window.setTimeout(() => {
                void refreshSnapshot(nextClient, threadId, undefined, true);
              }, delay);
            }
          }
          if (event.type === "turnCompleted" && event.threadId) {
            const threadId = event.threadId;
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

  async function refreshSnapshot(nextClient = client, threadId?: string, scope = init?.scope, readOnly = false) {
    if (!nextClient) {
      return;
    }
    if (threadId && readOnly) {
      const nextSnapshot = ThreadSnapshotSchema.parse(await nextClient.request("thread/read", { threadId }));
      setSnapshot(nextSnapshot);
      return;
    }
    const nextScope = scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
    const params = threadId ? { threadId, scope: nextScope } : { scope: nextScope };
    const nextSnapshot = ThreadSnapshotSchema.parse(await nextClient.request("thread/resume", params));
    setSnapshot(nextSnapshot);
  }

  async function refreshHistory(nextClient = client, includeArchived = archived, workdir = init?.scope.workdir) {
    if (!nextClient) {
      return;
    }
    const result = ThreadListResultSchema.parse(
      await nextClient.request("thread/list", { archived: includeArchived, limit: 100, workdir: workdir ?? null })
    );
    setSessions(result.sessions);
  }

  async function refreshDebug(nextClient = client, threadId = snapshot.thread?.id) {
    if (!nextClient || !threadId) {
      setDebugEvents([]);
      return;
    }
    const result = DebugEventsResultSchema.parse(
      await nextClient.request("debug/events", { threadId, limit: 200 })
    );
    setDebugEvents(result.events);
  }

  async function runAction(action: () => Promise<void>) {
    try {
      setError(null);
      await action();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
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
          Timeline
        </button>
        <button className={mobilePanel === "status" ? "is-selected" : ""} onClick={() => setMobilePanel("status")} type="button">
          <PanelRight size={17} />
          Status
        </button>
      </nav>

      <div className="workbench">
        <aside className={`historyColumn ${mobilePanel === "history" ? "is-mobileSelected" : ""}`}>
          <HistoryPanel
            archived={archived}
            currentThreadId={currentThreadId}
            disabled={running || disabled}
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
              const nextSnapshot = ThreadSnapshotSchema.parse(await client?.request("thread/start", { scope }));
              setSnapshot(nextSnapshot);
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
            debugEvents={debugEvents}
            events={events}
            items={snapshot.items}
            onRefreshDebug={() => void runAction(async () => refreshDebug())}
          />
          <Composer
            disabled={disabled}
            running={running}
            onInterrupt={() => void runAction(async () => {
              await client?.request("turn/interrupt", { threadId: snapshot.thread?.id ?? null });
              await refreshSnapshot();
            })}
            onSteer={(text) => void runAction(async () => {
              if (!snapshot.activity.activeTurnId) {
                return;
              }
              await client?.request("turn/steer", {
                expectedTurnId: snapshot.activity.activeTurnId,
                threadId: snapshot.thread?.id ?? null,
                text
              });
              await refreshSnapshot();
            })}
            onSubmit={(text) => void runAction(async () => {
              const scope = init?.scope ?? scopeForWorkdir(settings?.workdir ?? window.location.pathname);
              await client?.request("turn/start", {
                model: turnModel ?? null,
                scope,
                threadId: snapshot.thread?.id ?? null,
                text
              });
              await refreshSnapshot();
            })}
          />
        </section>

        <aside className={`statusColumn ${mobilePanel === "status" ? "is-mobileSelected" : ""}`}>
          <StatusPanel
            activity={snapshot.activity}
            pendingClarifies={snapshot.pendingClarifies}
            pendingPermissions={snapshot.pendingPermissions}
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
            })}
          />
        </aside>
      </div>
    </main>
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
