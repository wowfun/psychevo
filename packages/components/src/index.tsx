import {
  Activity,
  Archive,
  Bot,
  Brain,
  Bug,
  Check,
  CircleSlash,
  Download,
  FileText,
  History,
  Pencil,
  Plus,
  RefreshCw,
  RotateCcw,
  Send,
  Share2,
  Square,
  Terminal,
  Trash2,
  User,
  Wrench,
  X
} from "lucide-react";
import { useMemo, useState, type FormEvent } from "react";
import type {
  GatewayActivity,
  GatewayEvent,
  PendingClarify,
  PendingPermission,
  PermissionDecision,
  SessionSummary,
  SettingsReadResult,
  TimelineDebugEvent,
  TimelineItem
} from "@psychevo/protocol";

export interface HistoryPanelProps {
  archived: boolean;
  currentThreadId?: string | undefined;
  disabled?: boolean;
  sessions: SessionSummary[];
  onArchive(sessionId: string): void;
  onDelete(sessionId: string): void;
  onExport(sessionId: string): void;
  onNew(): void;
  onRename(sessionId: string, title: string): void;
  onRestore(sessionId: string): void;
  onResume(sessionId: string): void;
  onShare(sessionId: string): void;
  onToggleArchived(): void;
}

export function HistoryPanel(props: HistoryPanelProps) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");

  return (
    <section className="pevo-panel pevo-history" aria-label="History">
      <header className="pevo-panelHeader">
        <div className="pevo-titleLine">
          <History size={17} aria-hidden />
          <h2>History</h2>
        </div>
        <div className="pevo-iconRow">
          <IconButton title="New thread" onClick={props.onNew} disabled={props.disabled}>
            <Plus size={17} />
          </IconButton>
          <IconButton title={props.archived ? "Show active" : "Show archived"} onClick={props.onToggleArchived}>
            {props.archived ? <RotateCcw size={17} /> : <Archive size={17} />}
          </IconButton>
        </div>
      </header>
      <div className="pevo-sessionList">
        {props.sessions.length === 0 ? (
          <div className="pevo-empty">No sessions</div>
        ) : (
          props.sessions.map((session) => {
            const active = session.id === props.currentThreadId;
            const title = session.title?.trim() || shortId(session.id);
            const editing = editingId === session.id;
            return (
              <article className={`pevo-sessionRow ${active ? "is-active" : ""}`} key={session.id}>
                {editing ? (
                  <form
                    className="pevo-rename"
                    onSubmit={(event) => {
                      event.preventDefault();
                      const next = draft.trim();
                      if (next) {
                        props.onRename(session.id, next);
                      }
                      setEditingId(null);
                    }}
                  >
                    <input value={draft} onChange={(event) => setDraft(event.target.value)} autoFocus />
                    <IconButton title="Save title" type="submit">
                      <Check size={16} />
                    </IconButton>
                    <IconButton title="Cancel rename" type="button" onClick={() => setEditingId(null)}>
                      <X size={16} />
                    </IconButton>
                  </form>
                ) : (
                  <>
                    <button
                      className="pevo-sessionMain"
                      onClick={() => props.onResume(session.id)}
                      disabled={props.disabled}
                      type="button"
                    >
                      <span>{title}</span>
                      <small>{session.source} · {session.messageCount} msg · {dateLabel(session.updatedAtMs)}</small>
                    </button>
                    <div className="pevo-sessionActions">
                      <IconButton
                        title="Rename"
                        onClick={() => {
                          setDraft(title);
                          setEditingId(session.id);
                        }}
                      >
                        <Pencil size={15} />
                      </IconButton>
                      <IconButton title="Export" onClick={() => props.onExport(session.id)}>
                        <Download size={15} />
                      </IconButton>
                      <IconButton title="Share" onClick={() => props.onShare(session.id)}>
                        <Share2 size={15} />
                      </IconButton>
                      {props.archived ? (
                        <IconButton title="Restore" onClick={() => props.onRestore(session.id)} disabled={props.disabled}>
                          <RotateCcw size={15} />
                        </IconButton>
                      ) : (
                        <IconButton title="Archive" onClick={() => props.onArchive(session.id)} disabled={props.disabled}>
                          <Archive size={15} />
                        </IconButton>
                      )}
                      <IconButton title="Delete" danger onClick={() => props.onDelete(session.id)} disabled={props.disabled || active}>
                        <Trash2 size={15} />
                      </IconButton>
                    </div>
                  </>
                )}
              </article>
            );
          })
        )}
      </div>
    </section>
  );
}

export interface TranscriptPanelProps {
  debugEvents?: TimelineDebugEvent[];
  events: GatewayEvent[];
  items: TimelineItem[];
  onRefreshDebug?: () => void;
}

export function TranscriptPanel({ debugEvents = [], events, items, onRefreshDebug }: TranscriptPanelProps) {
  const [debugOpen, setDebugOpen] = useState(false);
  const debugAvailable = useMemo(
    () => events.filter((event) => event.type === "debugAvailable").length,
    [events]
  );
  const itemCountLabel = items.length === 1 ? "1 item" : `${items.length} items`;
  return (
    <section className="pevo-panel pevo-transcript" aria-label="Transcript">
      <header className="pevo-panelHeader pevo-transcriptHeader">
        <div className="pevo-titleLine">
          <Activity size={17} aria-hidden />
          <h2>Timeline</h2>
        </div>
        <span className="pevo-countPill">{itemCountLabel}</span>
      </header>
      <div className="pevo-threadItems">
        {items.length === 0 ? (
          <div className="pevo-empty pevo-emptyThread">No messages yet</div>
        ) : (
          items.map((item) => <TranscriptItem item={item} key={item.id} />)
        )}
      </div>
      <div className="pevo-debugDock">
        <button
          className="pevo-debugToggle"
          onClick={() => {
            setDebugOpen((value) => !value);
            onRefreshDebug?.();
          }}
          type="button"
        >
          <Bug size={16} aria-hidden />
          <span>Debug</span>
          {debugAvailable + debugEvents.length > 0 && <b>{debugAvailable + debugEvents.length}</b>}
        </button>
        {debugOpen && (
          <div className="pevo-debugDrawer">
            <div className="pevo-debugHeader">
              <strong>Runtime Debug</strong>
              <button onClick={onRefreshDebug} type="button">Refresh</button>
            </div>
            {debugEvents.length === 0 ? (
              <p className="pevo-muted">No debug events</p>
            ) : (
              debugEvents.map((event) => (
                <article className="pevo-debugEvent" key={event.id}>
                  <div>
                    <code>{event.eventType}</code>
                    <span>{event.status ?? "observed"}</span>
                  </div>
                  {event.summary && <p>{event.summary}</p>}
                </article>
              ))
            )}
          </div>
        )}
      </div>
    </section>
  );
}

export interface ComposerProps {
  disabled?: boolean;
  running: boolean;
  onInterrupt(): void;
  onSteer(text: string): void;
  onSubmit(text: string): void;
}

export function Composer({ disabled, running, onInterrupt, onSteer, onSubmit }: ComposerProps) {
  const [draft, setDraft] = useState("");
  const [mode, setMode] = useState<"turn" | "steer">("turn");
  const trimmed = draft.trim();

  function submit(event: FormEvent) {
    event.preventDefault();
    if (!trimmed || disabled) {
      return;
    }
    if (running && mode === "steer") {
      onSteer(trimmed);
    } else {
      onSubmit(trimmed);
    }
    setDraft("");
  }

  return (
    <form className="pevo-composer" onSubmit={submit}>
      {running && (
        <div className="pevo-segmented" role="tablist" aria-label="Turn mode">
          <button className={mode === "turn" ? "is-selected" : ""} onClick={() => setMode("turn")} type="button">
            Queue
          </button>
          <button className={mode === "steer" ? "is-selected" : ""} onClick={() => setMode("steer")} type="button">
            Steer
          </button>
        </div>
      )}
      <textarea
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        placeholder="Ask pevo..."
        rows={3}
        disabled={disabled}
      />
      <div className="pevo-composerActions">
        {running && (
          <IconButton title="Interrupt active turn" onClick={onInterrupt} type="button">
            <Square size={17} />
          </IconButton>
        )}
        <button className="pevo-primaryButton" disabled={!trimmed || disabled} type="submit">
          <Send size={17} aria-hidden />
          <span>{running && mode === "steer" ? "Steer" : "Send"}</span>
        </button>
      </div>
    </form>
  );
}

export interface StatusPanelProps {
  activity: GatewayActivity;
  pendingClarifies: PendingClarify[];
  pendingPermissions: PendingPermission[];
  settings?: SettingsReadResult | undefined;
  status: string;
  onClarify(requestId: string, answer: string): void;
  onPermission(requestId: string, decision: PermissionDecision): void;
  onRefresh(): void;
}

export function StatusPanel(props: StatusPanelProps) {
  return (
    <section className="pevo-panel pevo-utility" aria-label="Status">
      <header className="pevo-panelHeader">
        <div className="pevo-titleLine">
          <CircleSlash size={17} aria-hidden />
          <h2>Status</h2>
        </div>
        <IconButton title="Refresh" onClick={props.onRefresh}>
          <RefreshCw size={17} />
        </IconButton>
      </header>

      <dl className="pevo-statusGrid">
        <div><dt>Connection</dt><dd>{props.status}</dd></div>
        <div><dt>Turn</dt><dd>{props.activity.running ? "running" : "idle"}</dd></div>
        <div><dt>Queued</dt><dd>{props.activity.queuedTurns}</dd></div>
      </dl>

      <div className="pevo-stack">
        <h3>Permissions</h3>
        {props.pendingPermissions.length === 0 ? (
          <p className="pevo-muted">None</p>
        ) : (
          props.pendingPermissions.map((permission) => (
            <div className="pevo-request" key={permission.requestId}>
              <strong>{permission.toolName}</strong>
              <p>{permission.reason}</p>
              <div className="pevo-buttonRow">
                <button onClick={() => props.onPermission(permission.requestId, "allowOnce")} type="button">Once</button>
                <button onClick={() => props.onPermission(permission.requestId, "allowSession")} type="button">Session</button>
                <button onClick={() => props.onPermission(permission.requestId, "deny")} type="button">Deny</button>
              </div>
            </div>
          ))
        )}
      </div>

      <div className="pevo-stack">
        <h3>Clarify</h3>
        {props.pendingClarifies.length === 0 ? (
          <p className="pevo-muted">None</p>
        ) : (
          props.pendingClarifies.map((clarify) => (
            <ClarifyRequest key={clarify.requestId} request={clarify} onSubmit={props.onClarify} />
          ))
        )}
      </div>

      <div className="pevo-stack">
        <h3>Settings</h3>
        <dl className="pevo-settings">
          <div><dt>Workdir</dt><dd>{props.settings?.workdir ?? "unknown"}</dd></div>
          <div><dt>Memory</dt><dd>{stringSetting(props.settings?.memoryResources.mode, "status_only")}</dd></div>
          <div><dt>Secrets</dt><dd>{stringSetting(props.settings?.secrets.frontendPersistence, "disabled")}</dd></div>
        </dl>
      </div>
    </section>
  );
}

function TranscriptItem({ item }: { item: TimelineItem }) {
  const text = item.body ?? item.detail ?? item.preview ?? "";
  if (item.kind === "prompt") {
    return (
      <article className="pevo-message is-user">
        <User size={16} aria-hidden />
        <p>{text}</p>
      </article>
    );
  }
  if (item.kind === "assistant") {
    return (
      <article className="pevo-message is-assistant">
        <Bot size={16} aria-hidden />
        <p>{text}</p>
      </article>
    );
  }
  if (item.kind === "reasoning") {
    return (
      <article className="pevo-reasoning">
        <Brain size={16} aria-hidden />
        <p>{item.preview ?? text}</p>
      </article>
    );
  }
  const Icon = item.kind === "shell" ? Terminal : item.kind === "file" || item.kind === "diff" ? FileText : Wrench;
  return (
    <article className={`pevo-evidence is-${item.status}`}>
      <div className="pevo-evidenceLine">
        <Icon size={16} aria-hidden />
        <code>{item.title ?? item.kind}</code>
        <em>{item.status}</em>
      </div>
      {item.preview && <p>{item.preview}</p>}
      {item.detail && <pre>{item.detail}</pre>}
      {item.artifactIds.length > 0 && (
        <div className="pevo-artifactRefs">
          {item.artifactIds.map((artifactId) => <span key={artifactId}>{artifactId}</span>)}
        </div>
      )}
    </article>
  );
}

function ClarifyRequest({
  request,
  onSubmit
}: {
  request: PendingClarify;
  onSubmit(requestId: string, answer: string): void;
}) {
  const [answer, setAnswer] = useState("");
  return (
    <form
      className="pevo-request"
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit(request.requestId, answer);
        setAnswer("");
      }}
    >
      <pre>{JSON.stringify(request.raw, null, 2)}</pre>
      <input value={answer} onChange={(event) => setAnswer(event.target.value)} />
      <button type="submit">Submit</button>
    </form>
  );
}

function IconButton({
  children,
  danger,
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { danger?: boolean }) {
  const label = props["aria-label"] ?? (typeof props.title === "string" ? props.title : undefined);
  return (
    <button
      {...props}
      aria-label={label}
      className={`pevo-iconButton ${danger ? "is-danger" : ""} ${props.className ?? ""}`.trim()}
    >
      {children}
    </button>
  );
}

function shortId(value: string): string {
  return value.length > 10 ? value.slice(0, 10) : value;
}

function dateLabel(value?: number | null): string {
  if (!value) {
    return "pending";
  }
  return new Intl.DateTimeFormat(undefined, {
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    month: "short"
  }).format(new Date(value));
}

function stringSetting(value: unknown, fallback: string): string {
  return typeof value === "string" && value.trim() ? value : fallback;
}
