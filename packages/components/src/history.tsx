import {
  Archive,
  Check,
  ChevronDown,
  ChevronRight,
  Download,
  FolderPlus,
  History,
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
import { IconButton } from "./primitives";

export interface HistoryPanelProps {
  archived: boolean;
  currentThreadId?: string | undefined;
  disabled?: boolean;
  draftSession?: HistoryDraftSession | null;
  pinnedSessionIds?: string[];
  sessions: SessionSummary[];
  onArchive(sessionId: string): void;
  onDelete(sessionId: string): void;
  onExport(sessionId: string): void;
  onNew(): void;
  onCreateWorkspace?(): void;
  onNewInWorkdir?(workdir: string): void;
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
  workdir: string;
}

export function HistoryPanel(props: HistoryPanelProps) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [collapsedProjects, setCollapsedProjects] = useState<Set<string>>(() => new Set());
  const sessions = Array.isArray(props.sessions) ? props.sessions : [];
  const draftSession = props.archived ? null : props.draftSession ?? null;
  const pinnedSessionIds = new Set(props.pinnedSessionIds ?? []);
  const groupedSessions = useMemo(
    () => groupSessionsByProject(sessions, draftSession),
    [draftSession, sessions]
  );
  const groupSignature = groupedSessions.map((group) => group.workdir).join("\n");
  const hasCollapsedProjects = groupedSessions.some((group) => collapsedProjects.has(group.workdir));

  useEffect(() => {
    const visibleWorkdirs = new Set(groupedSessions.map((group) => group.workdir));
    setCollapsedProjects((current) => {
      const next = new Set([...current].filter((workdir) => visibleWorkdirs.has(workdir)));
      return next.size === current.size ? current : next;
    });
  }, [groupSignature, groupedSessions]);

  function startProjectSession(workdir: string) {
    if (props.onNewInWorkdir) {
      props.onNewInWorkdir(workdir);
      return;
    }
    props.onNew();
  }

  function toggleProject(workdir: string) {
    setCollapsedProjects((current) => {
      const next = new Set(current);
      if (next.has(workdir)) {
        next.delete(workdir);
      } else {
        next.add(workdir);
      }
      return next;
    });
  }

  function toggleAllProjects() {
    setCollapsedProjects(hasCollapsedProjects
      ? new Set()
      : new Set(groupedSessions.map((group) => group.workdir)));
  }

  return (
    <section className="pevo-panel pevo-history" aria-label={props.archived ? "Archived sessions" : "Sessions"}>
      <header className="pevo-panelHeader pevo-sessionsHeader">
        <div className="pevo-titleLine">
          <History size={17} aria-hidden />
          <h2>Sessions</h2>
        </div>
        <div className="pevo-iconRow">
          {props.onCreateWorkspace && (
            <IconButton title="New Workspace" onClick={props.onCreateWorkspace} disabled={props.disabled}>
              <FolderPlus size={17} />
            </IconButton>
          )}
          <IconButton title={hasCollapsedProjects ? "Expand all workspaces" : "Collapse all workspaces"} onClick={toggleAllProjects} disabled={groupedSessions.length === 0}>
            {hasCollapsedProjects ? <ChevronDown size={17} /> : <ChevronRight size={17} />}
          </IconButton>
        </div>
      </header>
      <div className="pevo-sessionList">
        {groupedSessions.length === 0 ? (
          <div className="pevo-empty">No sessions</div>
        ) : (
          groupedSessions.map((group) => {
            const collapsed = collapsedProjects.has(group.workdir);
            const groupDraftSession = draftSession?.workdir === group.workdir ? draftSession : null;
            return (
              <section className={`pevo-sessionGroup ${collapsed ? "is-collapsed" : ""}`} key={group.workdir}>
                <header className="pevo-sessionGroupHeader">
                  <button
                    className="pevo-sessionGroupToggle"
                    onClick={() => toggleProject(group.workdir)}
                    type="button"
                  >
                    {collapsed ? <ChevronRight size={14} aria-hidden /> : <ChevronDown size={14} aria-hidden />}
                    <span>{group.label}</span>
                  </button>
                  <IconButton
                    className="pevo-sessionProjectNew"
                    title={`New session in ${group.label}`}
                    onClick={() => startProjectSession(group.workdir)}
                    disabled={props.disabled}
                  >
                    <Plus size={15} />
                  </IconButton>
                </header>
                {!collapsed && groupDraftSession && (
                  <article className="pevo-sessionRow is-active is-draft" key={groupDraftSession.id}>
                    <button
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
                            <span className="pevo-sessionTitleRow">
                              <span className="pevo-sessionTitle">{title}</span>
                              <span className="pevo-sessionMeta">
                                <time className="pevo-sessionTime" dateTime={dateTimeAttribute(session.updatedAtMs)}>
                                  {sessionAgeLabel(session.updatedAtMs)}
                                </time>
                                {running && <b>running</b>}
                              </span>
                            </span>
                          </button>
                          <details className="pevo-sessionMenu">
                            <summary aria-label="Session actions" title="Session actions">
                              <MoreHorizontal size={16} aria-hidden />
                            </summary>
                            <div className="pevo-sessionMenuPopover" role="menu" aria-label="Session actions">
                              <button
                                role="menuitem"
                                title={pinned ? "Unpin" : "Pin"}
                                onClick={(event) => {
                                  closeSessionMenu(event);
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
                                onClick={(event) => {
                                  closeSessionMenu(event);
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
                                onClick={(event) => {
                                  closeSessionMenu(event);
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
                                onClick={(event) => {
                                  closeSessionMenu(event);
                                  props.onShare(session.id);
                                }}
                                disabled={props.disabled}
                                type="button"
                              >
                                <Share2 size={15} aria-hidden />
                                <span>Share</span>
                              </button>
                              {props.archived ? (
                                <button
                                  role="menuitem"
                                  title="Restore"
                                  onClick={(event) => {
                                    closeSessionMenu(event);
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
                                  onClick={(event) => {
                                    closeSessionMenu(event);
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
                                title="Delete"
                                onClick={(event) => {
                                  closeSessionMenu(event);
                                  props.onDelete(session.id);
                                }}
                                disabled={props.disabled || active || running}
                                type="button"
                              >
                                <Trash2 size={15} aria-hidden />
                                <span>Delete</span>
                              </button>
                            </div>
                          </details>
                        </>
                      )}
                    </article>
                  );
                })}
              </section>
            );
          })
        )}
      </div>
    </section>
  );
}

function closeSessionMenu(event: { currentTarget: HTMLElement }) {
  event.currentTarget.closest("details")?.removeAttribute("open");
}

type SessionProjectGroup = {
  workdir: string;
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
    const workdir = session.project?.workdir || session.workdir || "";
    const label = session.project?.label || projectLabelFromWorkdir(workdir);
    const updatedAt = session.updatedAtMs ?? session.startedAtMs ?? 0;
    const existing = groups.get(workdir);
    if (existing) {
      existing.sessions.push(session);
      existing.latestAt = Math.max(existing.latestAt, updatedAt);
    } else {
      groups.set(workdir, {
        workdir,
        label,
        latestAt: updatedAt,
        sessions: [session]
      });
    }
  }
  if (draftSession) {
    const workdir = draftSession.workdir;
    const existing = groups.get(workdir);
    if (existing) {
      existing.latestAt = Math.max(existing.latestAt, draftSession.createdAtMs);
    } else {
      groups.set(workdir, {
        workdir,
        label: projectLabelFromWorkdir(workdir),
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

function projectLabelFromWorkdir(workdir: string): string {
  const parts = workdir.split(/[\\/]/).filter(Boolean);
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
