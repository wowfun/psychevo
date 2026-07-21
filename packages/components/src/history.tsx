import {
  Archive,
  Check,
  ChevronDown,
  ChevronRight,
  Download,
  FolderOpen,
  History,
  Inbox,
  GitFork,
  MoreHorizontal,
  Pencil,
  Pin,
  Plus,
  RotateCcw,
  Share2,
  Trash2,
  X
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { SessionSummary } from "@psychevo/protocol";
import { DismissibleDetails } from "./dismissibleDetails";
import { IconButton } from "./primitives";

export interface HistoryPanelProps {
  archived: boolean;
  currentThreadId?: string | undefined;
  disabled?: boolean;
  draftSession?: HistoryDraftSession | null;
  pinnedSessionIds?: string[];
  browserWorkspaces?: HistoryBrowserWorkspace[];
  loading?: boolean;
  loadingOlderCwd?: string | null;
  sessions: SessionSummary[];
  onArchive(sessionId: string): void;
  onDelete(sessionId: string): void;
  onExport(sessionId: string): void;
  onFork?(sessionId: string): void;
  onImportSessions?(): void;
  onNew(): void;
  onCreateWorkspace?(): void;
  onLoadOlderSessions?(cwd: string): void;
  onNewInCwd?(cwd: string): void;
  onTogglePinned?(sessionId: string): void;
  onRename(sessionId: string, title: string): void;
  onRestore(sessionId: string): void;
  onResume(sessionId: string): void;
  onShare(sessionId: string): void;
  onResumeDraft?(): void;
}

export interface HistoryDraftSession {
  id: string;
  title: string;
  createdAtMs: number;
  cwd: string;
}

export interface HistoryBrowserWorkspace {
  cwd: string;
  hiddenCount: number;
}

const ACTIVITY_SPINNER = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

export function HistoryPanel(props: HistoryPanelProps) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [collapsedProjects, setCollapsedProjects] = useState<Set<string>>(() => new Set());
  const [activityTick, setActivityTick] = useState(0);
  const sessions = Array.isArray(props.sessions) ? props.sessions : [];
  const draftSession = props.archived ? null : props.draftSession ?? null;
  const pinnedSessionIds = new Set(props.pinnedSessionIds ?? []);
  const browserByCwd = useMemo(
    () => new Map((props.browserWorkspaces ?? []).map((workspace) => [workspace.cwd, workspace])),
    [props.browserWorkspaces]
  );
  const groupedSessions = useMemo(
    () => groupSessionsByProject(sessions, draftSession),
    [draftSession, sessions]
  );
  const groupSignature = groupedSessions.map((group) => group.cwd).join("\n");
  const hasCollapsedProjects = groupedSessions.some((group) => collapsedProjects.has(group.cwd));

  useEffect(() => {
    const visibleCwds = new Set(groupedSessions.map((group) => group.cwd));
    setCollapsedProjects((current) => {
      const next = new Set([...current].filter((cwd) => visibleCwds.has(cwd)));
      return next.size === current.size ? current : next;
    });
  }, [groupSignature, groupedSessions]);

  useEffect(() => {
    if (!sessions.some((session) => session.activity?.running === true)) {
      return;
    }
    const timer = window.setInterval(() => setActivityTick((value) => value + 1), 120);
    return () => window.clearInterval(timer);
  }, [sessions]);

  function startProjectSession(cwd: string) {
    if (props.onNewInCwd) {
      props.onNewInCwd(cwd);
      return;
    }
    props.onNew();
  }

  function toggleProject(cwd: string) {
    setCollapsedProjects((current) => {
      const next = new Set(current);
      if (next.has(cwd)) {
        next.delete(cwd);
      } else {
        next.add(cwd);
      }
      return next;
    });
  }

  function toggleAllProjects() {
    setCollapsedProjects(hasCollapsedProjects
      ? new Set()
      : new Set(groupedSessions.map((group) => group.cwd)));
  }

  return (
    <section
      aria-busy={props.loading || undefined}
      aria-label={props.archived ? "Archived sessions" : "Sessions"}
      className="pevo-panel pevo-history"
    >
      <header className="pevo-panelHeader pevo-sessionsHeader">
        <div className="pevo-titleLine">
          <History size={17} aria-hidden />
          <h2>Sessions</h2>
        </div>
        <div className="pevo-iconRow">
          {props.onCreateWorkspace && (
            <IconButton
              disabled={props.disabled}
              icon={<FolderOpen size={17} />}
              label="Open workspace"
              onClick={props.onCreateWorkspace}
              size="compact"
            />
          )}
          {props.onImportSessions && !props.archived && (
            <IconButton
              disabled={props.disabled}
              icon={<Inbox size={17} />}
              label="Imported and archived sessions"
              onClick={props.onImportSessions}
              size="compact"
            />
          )}
          <IconButton
            disabled={groupedSessions.length === 0}
            icon={hasCollapsedProjects ? <ChevronDown size={17} /> : <ChevronRight size={17} />}
            label={hasCollapsedProjects ? "Expand all workspaces" : "Collapse all workspaces"}
            onClick={toggleAllProjects}
          />
        </div>
      </header>
      <div className="pevo-sessionList">
        {groupedSessions.length === 0 && !props.loading ? (
          <div className="pevo-empty">No sessions</div>
        ) : groupedSessions.length > 0 ? (
          groupedSessions.map((group) => {
            const collapsed = collapsedProjects.has(group.cwd);
            const groupDraftSession = draftSession?.cwd === group.cwd ? draftSession : null;
            const browser = browserByCwd.get(group.cwd);
            return (
              <section className={`pevo-sessionGroup ${collapsed ? "is-collapsed" : ""}`} key={group.cwd}>
                <header className="pevo-sessionGroupHeader">
                  <button
                    aria-expanded={!collapsed}
                    className="pevo-sessionGroupToggle"
                    onClick={() => toggleProject(group.cwd)}
                    type="button"
                  >
                    {collapsed ? <ChevronRight size={14} aria-hidden /> : <ChevronDown size={14} aria-hidden />}
                    <span>{group.label}</span>
                  </button>
                  <IconButton
                    className="pevo-sessionProjectNew"
                    disabled={props.disabled}
                    icon={<Plus size={15} />}
                    label={`New session in ${group.label}`}
                    onClick={() => startProjectSession(group.cwd)}
                    size="compact"
                  />
                </header>
                {!collapsed && groupDraftSession && (
                  <article className="pevo-sessionRow is-active is-draft" key={groupDraftSession.id}>
                    <button
                      aria-current="page"
                      className="pevo-sessionMain"
                      onClick={() => props.onResumeDraft?.()}
                      disabled={props.disabled}
                      type="button"
                    >
                      <span className="pevo-sessionTitleRow">
                        <span className="pevo-sessionTitle">{groupDraftSession.title.trim() || "New session"}</span>
                        <time className="pevo-sessionTime" dateTime={new Date(groupDraftSession.createdAtMs).toISOString()}>
                          {sessionAgeLabel(groupDraftSession.createdAtMs)}
                        </time>
                      </span>
                    </button>
                  </article>
                )}
                {!collapsed && group.sessions.map((session) => {
                  const active = session.id === props.currentThreadId;
                  const running = session.activity?.running === true;
                  const title = session.displayTitle?.trim() || session.title?.trim() || shortId(session.id);
                  const editing = editingId === session.id;
                  const pinned = pinnedSessionIds.has(session.id);
                  const forkAction = session.lifecycle?.actions.find((action) => action.id === "fork");
                  const deleteAction = session.lifecycle?.actions.find((action) => action.id === "delete");
                  const forkSource = session.forkedFromThreadId
                    ? sessions.find((candidate) => candidate.id === session.forkedFromThreadId) ?? null
                    : null;
                  const forkSourceLabel = forkSource
                    ? forkSource.displayTitle?.trim() || forkSource.title?.trim() || shortId(forkSource.id)
                    : session.forkedFromThreadId
                      ? shortId(session.forkedFromThreadId)
                      : null;
                  return (
                    <article className={`pevo-sessionRow ${active ? "is-active" : ""} ${running ? "is-running" : ""}`} key={session.id}>
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
                          <input className="pevo-fieldControl pevo-fieldControl--compact" value={draft} onChange={(event) => setDraft(event.target.value)} autoFocus />
                          <IconButton icon={<Check size={16} />} label="Save title" type="submit" />
                          <IconButton icon={<X size={16} />} label="Cancel rename" type="button" onClick={() => setEditingId(null)} />
                        </form>
                      ) : (
                        <>
                          <button
                            aria-current={active ? "page" : undefined}
                            className="pevo-sessionMain"
                            onClick={() => props.onResume(session.id)}
                            disabled={props.disabled}
                            type="button"
                          >
                            <span className="pevo-sessionTitleRow">
                              <span className="pevo-sessionTitleAnchor">
                                <span className="pevo-sessionTitle" title={title}>{title}</span>
                              </span>
                              <span className="pevo-sessionMeta">
                                <time className="pevo-sessionTime" dateTime={dateTimeAttribute(session.updatedAtMs)}>
                                  {sessionAgeLabel(session.updatedAtMs)}
                                </time>
                                {running && (
                                  <b className="pevo-sessionRunning" aria-label="running">
                                    <span className="pevo-sessionSpinner" aria-hidden="true">
                                      {ACTIVITY_SPINNER[activityTick % ACTIVITY_SPINNER.length]}
                                    </span>
                                  </b>
                                )}
                              </span>
                            </span>
                            {forkSourceLabel && (
                              <span className="pevo-sessionProvenance">Forked from {forkSourceLabel}</span>
                            )}
                          </button>
                          <DismissibleDetails
                            className="pevo-sessionMenu"
                            summary={<MoreHorizontal size={16} aria-hidden />}
                            summaryProps={{ "aria-label": "Session actions", title: "Session actions" }}
                          >
                            {({ close }) => (
                              <div className="pevo-sessionMenuPopover pevo-controlPopover" role="menu" aria-label="Session actions">
                                <button
                                  role="menuitem"
                                  title={pinned ? "Unpin" : "Pin"}
                                  onClick={() => {
                                    close();
                                    props.onTogglePinned?.(session.id);
                                  }}
                                  disabled={props.disabled || !props.onTogglePinned}
                                  type="button"
                                >
                                  <Pin size={15} fill={pinned ? "currentColor" : "none"} aria-hidden />
                                  <span>{pinned ? "Unpin" : "Pin"}</span>
                                </button>
                                <button
                                  role="menuitem"
                                  title="Rename"
                                  onClick={() => {
                                    close();
                                    setDraft(title);
                                    setEditingId(session.id);
                                  }}
                                  disabled={props.disabled}
                                  type="button"
                                >
                                  <Pencil size={15} aria-hidden />
                                  <span>Rename</span>
                                </button>
                                <button
                                  role="menuitem"
                                  title="Export"
                                  onClick={() => {
                                    close();
                                    props.onExport(session.id);
                                  }}
                                  disabled={props.disabled}
                                  type="button"
                                >
                                  <Download size={15} aria-hidden />
                                  <span>Export</span>
                                </button>
                                <button
                                  role="menuitem"
                                  title="Share"
                                  onClick={() => {
                                    close();
                                    props.onShare(session.id);
                                  }}
                                  disabled={props.disabled}
                                  type="button"
                                >
                                  <Share2 size={15} aria-hidden />
                                  <span>Share</span>
                                </button>
                                {forkAction?.enabled && props.onFork ? (
                                  <button
                                    role="menuitem"
                                    title="Fork session"
                                    onClick={() => {
                                      close();
                                      props.onFork?.(session.id);
                                    }}
                                    disabled={props.disabled || running}
                                    type="button"
                                  >
                                    <GitFork size={15} aria-hidden />
                                    <span>Fork</span>
                                  </button>
                                ) : null}
                                {props.archived ? (
                                  <button
                                    role="menuitem"
                                    title="Restore"
                                    onClick={() => {
                                      close();
                                      props.onRestore(session.id);
                                    }}
                                    disabled={props.disabled || running}
                                    type="button"
                                  >
                                    <RotateCcw size={15} aria-hidden />
                                    <span>Restore</span>
                                  </button>
                                ) : (
                                  <button
                                    role="menuitem"
                                    title="Archive"
                                    onClick={() => {
                                      close();
                                      props.onArchive(session.id);
                                    }}
                                    disabled={props.disabled || running}
                                    type="button"
                                  >
                                    <Archive size={15} aria-hidden />
                                    <span>Archive</span>
                                  </button>
                                )}
                                <button
                                  className="is-danger"
                                  role="menuitem"
                                  title={deleteAction?.enabled === false
                                    ? deleteAction.unavailableReason ?? "Delete unavailable"
                                    : "Delete"}
                                  onClick={() => {
                                    close();
                                    props.onDelete(session.id);
                                  }}
                                  disabled={props.disabled || running || deleteAction?.enabled === false}
                                  type="button"
                                >
                                  <Trash2 size={15} aria-hidden />
                                  <span>Delete</span>
                                </button>
                              </div>
                            )}
                          </DismissibleDetails>
                        </>
                      )}
                    </article>
                  );
                })}
                {!collapsed && !props.archived && browser && browser.hiddenCount > 0 && (
                  <button
                    className="pevo-sessionOlderRow"
                    disabled={props.disabled || props.loadingOlderCwd === group.cwd || !props.onLoadOlderSessions}
                    onClick={() => props.onLoadOlderSessions?.(group.cwd)}
                    type="button"
                  >
                    <span>Older sessions</span>
                    <span>{props.loadingOlderCwd === group.cwd ? "Loading" : `${browser.hiddenCount}`}</span>
                  </button>
                )}
              </section>
            );
          })
        ) : null}
      </div>
    </section>
  );
}

type SessionProjectGroup = {
  cwd: string;
  label: string;
  latestAt: number;
  sessions: SessionSummary[];
};

function groupSessionsByProject(
  sessions: SessionSummary[],
  draftSession: HistoryDraftSession | null
): SessionProjectGroup[] {
  const groups = new Map<string, SessionProjectGroup>();
  for (const session of sessions) {
    const cwd = session.project?.cwd ?? session.cwd;
    const label = session.project?.label || projectLabelFromCwd(cwd);
    const updatedAt = session.updatedAtMs ?? session.startedAtMs ?? 0;
    const existing = groups.get(cwd);
    if (existing) {
      existing.sessions.push(session);
      existing.latestAt = Math.max(existing.latestAt, updatedAt);
    } else {
      groups.set(cwd, {
        cwd,
        label,
        latestAt: updatedAt,
        sessions: [session]
      });
    }
  }
  if (draftSession) {
    const cwd = draftSession.cwd;
    const existing = groups.get(cwd);
    if (existing) {
      existing.latestAt = Math.max(existing.latestAt, draftSession.createdAtMs);
    } else {
      groups.set(cwd, {
        cwd,
        label: projectLabelFromCwd(cwd),
        latestAt: draftSession.createdAtMs,
        sessions: []
      });
    }
  }
  for (const group of groups.values()) {
    group.sessions.sort((left, right) => sessionTime(right) - sessionTime(left) || left.id.localeCompare(right.id));
  }
  return Array.from(groups.values()).sort((a, b) => {
    return b.latestAt - a.latestAt || a.label.localeCompare(b.label);
  });
}

function sessionTime(session: SessionSummary): number {
  return session.updatedAtMs ?? session.startedAtMs ?? 0;
}

function projectLabelFromCwd(cwd: string): string {
  const parts = cwd.split(/[\\/]/).filter(Boolean);
  return parts.at(-1) ?? "workspace";
}

function shortId(value: string): string {
  return value.length > 10 ? value.slice(0, 10) : value;
}

function dateTimeAttribute(value?: number | null): string | undefined {
  return value ? new Date(value).toISOString() : undefined;
}

function sessionAgeLabel(value?: number | null): string {
  if (!value) {
    return "0d";
  }
  const elapsedMs = Math.max(0, Date.now() - value);
  return `${Math.floor(elapsedMs / 86_400_000)}d`;
}
