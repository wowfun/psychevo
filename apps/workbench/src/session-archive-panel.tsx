import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  Archive,
  Check,
  ChevronDown,
  ChevronRight,
  FolderOpen,
  History,
  Inbox,
  MoreHorizontal,
  RefreshCw,
  RotateCcw,
  Trash2
} from "lucide-react";
import type { GatewayClient } from "@psychevo/client";
import type {
  GatewayRequestScope,
  SessionSummary,
  ThreadImportListResult,
  ThreadImportProfileView
} from "@psychevo/protocol";
import { ActionButton, DismissibleDetails } from "@psychevo/components";

export function SessionArchivePanel({
  archivedSessions,
  client,
  currentThreadId,
  disabled,
  onActivateArchived,
  onDeleteArchived,
  onImportSession,
  onOpenArchived,
  onOpenWorkspace,
  onRefreshArchived,
  onShowActive,
  scope
}: {
  archivedSessions: SessionSummary[];
  client: GatewayClient | null;
  currentThreadId?: string | null;
  disabled: boolean;
  onActivateArchived(threadId: string): Promise<unknown>;
  onDeleteArchived(session: SessionSummary): void;
  onImportSession(profile: ThreadImportProfileView, candidateId: string, targetId: string, activate: boolean): Promise<unknown>;
  onOpenArchived(threadId: string): Promise<unknown>;
  onOpenWorkspace(): void;
  onRefreshArchived(): Promise<unknown>;
  onShowActive(): void;
  scope: GatewayRequestScope;
}) {
  const [result, setResult] = useState<ThreadImportListResult | null>(null);
  const [loadingAgents, setLoadingAgents] = useState(true);
  const [loadingArchived, setLoadingArchived] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [archivedError, setArchivedError] = useState<string | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);
  const [selectedTargets, setSelectedTargets] = useState<Record<string, string>>({});
  const [pendingId, setPendingId] = useState<string | null>(null);
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(() => new Set());
  const refreshArchivedRef = useRef(onRefreshArchived);
  refreshArchivedRef.current = onRefreshArchived;

  useEffect(() => {
    let cancelled = false;
    setLoadingArchived(true);
    setArchivedError(null);
    void refreshArchivedRef.current().catch((cause: unknown) => {
      if (!cancelled) setArchivedError(errorMessage(cause));
    }).finally(() => {
      if (!cancelled) setLoadingArchived(false);
    });
    return () => {
      cancelled = true;
    };
  }, [refreshKey, scope.cwd]);

  useEffect(() => {
    let cancelled = false;
    setLoadingAgents(true);
    setError(null);
    if (!client) {
      setLoadingAgents(false);
      setError("Connect to Gateway to read Agent sessions.");
      return () => {
        cancelled = true;
      };
    }
    void client.request("thread/import/list", { scope, cursors: {} }).then((next) => {
      if (cancelled) return;
      setResult(next);
      setSelectedTargets((current) => ({ ...defaultTargetSelections(next), ...current }));
    }).catch((cause: unknown) => {
      if (!cancelled) setError(errorMessage(cause));
    }).finally(() => {
      if (!cancelled) setLoadingAgents(false);
    });
    return () => {
      cancelled = true;
    };
  }, [client, refreshKey, scope.cwd, scope.source.kind, scope.source.lifetime, scope.source.rawId]);

  const groupIds = useMemo(
    () => ["archived", ...(result?.profiles.map((profile) => profile.runtimeProfileRef) ?? [])],
    [result]
  );
  const hasCollapsedGroups = groupIds.some((id) => collapsedGroups.has(id));

  function toggleGroup(id: string) {
    setCollapsedGroups((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function toggleAllGroups() {
    setCollapsedGroups(hasCollapsedGroups ? new Set() : new Set(groupIds));
  }

  async function importSession(
    profile: ThreadImportProfileView,
    candidateId: string,
    activate: boolean
  ) {
    const targetId = selectedTargets[profile.runtimeProfileRef]
      ?? profile.targets.find((target) => target.ready)?.targetId;
    if (!targetId) return;
    setPendingId(candidateId);
    setError(null);
    try {
      await onImportSession(profile, candidateId, targetId, activate);
      setResult((current) => current ? {
        ...current,
        profiles: current.profiles.map((candidateProfile) => (
          candidateProfile.runtimeProfileRef === profile.runtimeProfileRef
            ? {
                ...candidateProfile,
                sessions: candidateProfile.sessions.filter((session) => session.candidateId !== candidateId)
              }
            : candidateProfile
        ))
      } : current);
      if (activate) onShowActive();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setPendingId(null);
    }
  }

  return (
    <section aria-busy={loadingAgents || loadingArchived || undefined} aria-label="Sessions" className="pevo-panel pevo-history pevo-sessionArchivePanel">
      <header className="pevo-panelHeader pevo-sessionsHeader">
        <div className="pevo-titleLine">
          <History size={17} aria-hidden />
          <h2>Sessions</h2>
        </div>
        <div className="pevo-iconRow">
          <ActionButton
            ariaLabel="Open workspace"
            disabled={disabled}
            icon={<FolderOpen size={17} />}
            iconOnly
            onClick={onOpenWorkspace}
            size="compact"
            tooltip="Open workspace"
            variant="ghost"
          >
            Open workspace
          </ActionButton>
          <ActionButton
            active
            ariaLabel="Active sessions"
            disabled={disabled}
            icon={<Inbox size={17} />}
            iconOnly
            onClick={onShowActive}
            size="compact"
            tooltip="Active sessions"
            variant="ghost"
          >
            Active sessions
          </ActionButton>
          <ActionButton
            ariaLabel={hasCollapsedGroups ? "Expand all sources" : "Collapse all sources"}
            disabled={groupIds.length === 0}
            icon={hasCollapsedGroups ? <ChevronDown size={17} /> : <ChevronRight size={17} />}
            iconOnly
            onClick={toggleAllGroups}
            size="compact"
            title={hasCollapsedGroups ? "Expand all sources" : "Collapse all sources"}
            variant="ghost"
          >
            {hasCollapsedGroups ? "Expand all sources" : "Collapse all sources"}
          </ActionButton>
        </div>
      </header>
      <div className="pevo-sessionList">
        <SourceGroup
          collapsed={collapsedGroups.has("archived")}
          icon={<Archive size={13} aria-hidden />}
          label="Archived"
          onToggle={() => toggleGroup("archived")}
        >
          {archivedSessions.map((session) => (
            <ArchivedSessionRow
              active={session.id === currentThreadId}
              disabled={disabled || Boolean(pendingId)}
              key={session.id}
              session={session}
              onActivate={() => void activateArchived(session.id)}
              onDelete={() => onDeleteArchived(session)}
              onOpen={() => void onOpenArchived(session.id)}
            />
          ))}
          {!loadingArchived && archivedSessions.length === 0 ? <p className="pevo-sessionSourceState">No archived sessions</p> : null}
          {loadingArchived ? <p className="pevo-sessionSourceState">Loading archived sessions...</p> : null}
          {archivedError ? <p className="pevo-sessionSourceState is-error" role="alert">{archivedError}</p> : null}
        </SourceGroup>

        {result?.profiles.map((profile) => {
          const selectedTarget = selectedTargets[profile.runtimeProfileRef]
            ?? profile.targets.find((target) => target.ready)?.targetId
            ?? "";
          return (
            <SourceGroup
              collapsed={collapsedGroups.has(profile.runtimeProfileRef)}
              key={profile.runtimeProfileRef}
              label={profile.profileLabel}
              onToggle={() => toggleGroup(profile.runtimeProfileRef)}
              trailing={profile.targets.length > 1 ? (
                <select
                  aria-label={`Agent target for ${profile.profileLabel}`}
                  disabled={disabled || Boolean(pendingId)}
                  onChange={(event) => setSelectedTargets((current) => ({
                    ...current,
                    [profile.runtimeProfileRef]: event.target.value
                  }))}
                  value={selectedTarget}
                >
                  {profile.targets.map((target) => (
                    <option disabled={!target.ready} key={target.targetId} value={target.targetId}>{target.label}</option>
                  ))}
                </select>
              ) : null}
            >
              {profile.sessions.map((session) => (
                <AgentCandidateRow
                  disabled={disabled || Boolean(pendingId) || !selectedTarget}
                  key={session.candidateId}
                  pending={pendingId === session.candidateId}
                  title={session.title?.trim() || "Untitled Agent session"}
                  updatedAt={session.updatedAt}
                  onActivate={() => void importSession(profile, session.candidateId, true)}
                  onOpen={() => void importSession(profile, session.candidateId, false)}
                />
              ))}
              {profile.status === "ready" && profile.sessions.length === 0 ? (
                <p className="pevo-sessionSourceState">
                  {profile.alreadyImportedCount > 0 ? "All sessions are already in Psychevo" : "No sessions found"}
                </p>
              ) : null}
              {profile.error ? <p className="pevo-sessionSourceState is-error" role="alert">{profile.error.message}</p> : null}
            </SourceGroup>
          );
        })}
        {loadingAgents && !result ? <p className="pevo-sessionSourceState">Reading ACP Agent sessions...</p> : null}
        {!loadingAgents && result?.profiles.length === 0 ? <p className="pevo-sessionSourceState">No ACP Agents can list sessions</p> : null}
        {error ? (
          <div className="pevo-sessionSourceRecovery" role="alert">
            <span>{error}</span>
            <button disabled={loadingAgents} onClick={() => setRefreshKey((value) => value + 1)} type="button">
              <RefreshCw size={13} aria-hidden /> Retry
            </button>
          </div>
        ) : null}
      </div>
    </section>
  );

  async function activateArchived(threadId: string) {
    setPendingId(threadId);
    setArchivedError(null);
    try {
      await onActivateArchived(threadId);
      onShowActive();
    } catch (cause) {
      setArchivedError(errorMessage(cause));
    } finally {
      setPendingId(null);
    }
  }
}

function SourceGroup({
  children,
  collapsed,
  icon,
  label,
  onToggle,
  trailing
}: {
  children: ReactNode;
  collapsed: boolean;
  icon?: ReactNode;
  label: string;
  onToggle(): void;
  trailing?: ReactNode;
}) {
  return (
    <section className={`pevo-sessionGroup pevo-sessionSourceGroup ${collapsed ? "is-collapsed" : ""}`}>
      <header className="pevo-sessionGroupHeader">
        <button className="pevo-sessionGroupToggle" onClick={onToggle} type="button">
          {collapsed ? <ChevronRight size={14} aria-hidden /> : <ChevronDown size={14} aria-hidden />}
          {icon}
          <span>{label}</span>
        </button>
        {trailing ? <div className="pevo-sessionSourceTarget">{trailing}</div> : null}
      </header>
      {!collapsed ? children : null}
    </section>
  );
}

function ArchivedSessionRow({
  active,
  disabled,
  onActivate,
  onDelete,
  onOpen,
  session
}: {
  active: boolean;
  disabled: boolean;
  onActivate(): void;
  onDelete(): void;
  onOpen(): void;
  session: SessionSummary;
}) {
  const title = session.displayTitle?.trim() || session.title?.trim() || shortId(session.id);
  const deleteAction = session.lifecycle?.actions.find((action) => action.id === "delete");
  return (
    <article className={`pevo-sessionRow ${active ? "is-active" : ""}`}>
      <button className="pevo-sessionMain" disabled={disabled} onClick={onOpen} type="button">
        <span className="pevo-sessionTitleRow">
          <span className="pevo-sessionTitleAnchor"><span className="pevo-sessionTitle" title={title}>{title}</span></span>
          <time className="pevo-sessionTime" dateTime={dateTimeAttribute(session.updatedAtMs)}>{sessionAgeLabel(session.updatedAtMs)}</time>
        </span>
      </button>
      <DismissibleDetails
        className="pevo-sessionMenu"
        summary={<MoreHorizontal size={16} aria-hidden />}
        summaryProps={{ "aria-label": "Session actions", title: "Session actions" }}
      >
        {({ close }) => (
          <div aria-label="Session actions" className="pevo-sessionMenuPopover pevo-controlPopover" role="menu">
            <button disabled={disabled} onClick={() => { close(); onActivate(); }} role="menuitem" type="button">
              <RotateCcw size={15} aria-hidden /><span>Activate</span>
            </button>
            <button
              className="is-danger"
              disabled={disabled || active || deleteAction?.enabled === false}
              onClick={() => { close(); onDelete(); }}
              role="menuitem"
              title={deleteAction?.enabled === false ? deleteAction.unavailableReason ?? "Delete unavailable" : "Delete"}
              type="button"
            >
              <Trash2 size={15} aria-hidden /><span>Delete</span>
            </button>
          </div>
        )}
      </DismissibleDetails>
    </article>
  );
}

function AgentCandidateRow({
  disabled,
  onActivate,
  onOpen,
  pending,
  title,
  updatedAt
}: {
  disabled: boolean;
  onActivate(): void;
  onOpen(): void;
  pending: boolean;
  title: string;
  updatedAt: string | null;
}) {
  return (
    <article className="pevo-sessionRow is-importCandidate">
      <button className="pevo-sessionMain" disabled={disabled} onClick={onOpen} type="button">
        <span className="pevo-sessionTitleRow">
          <span className="pevo-sessionTitleAnchor">
            <span className="pevo-sessionTitle" title={title}>{pending ? "Opening..." : title}</span>
          </span>
          <time className="pevo-sessionTime" dateTime={updatedAt ?? undefined}>{agentSessionAgeLabel(updatedAt)}</time>
        </span>
      </button>
      <DismissibleDetails
        className="pevo-sessionMenu"
        summary={<MoreHorizontal size={16} aria-hidden />}
        summaryProps={{ "aria-label": "Session actions", title: "Session actions" }}
      >
        {({ close }) => (
          <div aria-label="Session actions" className="pevo-sessionMenuPopover pevo-controlPopover" role="menu">
            <button disabled={disabled} onClick={() => { close(); onActivate(); }} role="menuitem" type="button">
              <Check size={15} aria-hidden /><span>Activate</span>
            </button>
          </div>
        )}
      </DismissibleDetails>
    </article>
  );
}

function defaultTargetSelections(result: ThreadImportListResult): Record<string, string> {
  return Object.fromEntries(result.profiles.flatMap((profile) => {
    const target = profile.targets.find((candidate) => candidate.ready);
    return target ? [[profile.runtimeProfileRef, target.targetId]] : [];
  }));
}

function shortId(value: string): string {
  return value.length > 10 ? value.slice(0, 10) : value;
}

function dateTimeAttribute(value?: number | null): string | undefined {
  return value ? new Date(value).toISOString() : undefined;
}

function sessionAgeLabel(value?: number | null): string {
  if (!value) return "0d";
  return `${Math.floor(Math.max(0, Date.now() - value) / 86_400_000)}d`;
}

function agentSessionAgeLabel(value: string | null): string {
  if (!value) return "";
  const timestamp = Date.parse(value);
  return Number.isFinite(timestamp) ? sessionAgeLabel(timestamp) : "";
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
