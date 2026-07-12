import { useEffect, useMemo, useState } from "react";
import { Inbox, RefreshCw, Trash2 } from "lucide-react";
import type { GatewayClient } from "@psychevo/client";
import type {
  GatewayRequestScope,
  SessionSummary,
  ThreadImportListResult,
  ThreadImportProfileView
} from "@psychevo/protocol";
import { ActionButton, CreatePanel } from "@psychevo/components";

export function AgentSessionImportDialog({
  client,
  disabled,
  onClose,
  onImported,
  scope
}: {
  client: GatewayClient | null;
  disabled: boolean;
  onClose(): void;
  onImported(threadId: string): void;
  scope: GatewayRequestScope;
}) {
  const [result, setResult] = useState<ThreadImportListResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);
  const [importingId, setImportingId] = useState<string | null>(null);
  const [selectedTargets, setSelectedTargets] = useState<Record<string, string>>({});

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    void client?.request("thread/import/list", { scope, cursors: {} }).then((next) => {
      if (!cancelled) {
        setResult(next);
        setSelectedTargets((current) => ({ ...defaultTargetSelections(next), ...current }));
      }
    }).catch((nextError: unknown) => {
      if (!cancelled) {
        setError(nextError instanceof Error ? nextError.message : String(nextError));
      }
    }).finally(() => {
      if (!cancelled) setLoading(false);
    });
    if (!client) {
      setLoading(false);
      setError("Connect to Gateway to import Agent sessions.");
    }
    return () => {
      cancelled = true;
    };
  }, [client, refreshKey, scope]);

  const sessionCount = useMemo(
    () => result?.profiles.reduce((total, profile) => total + profile.sessions.length, 0) ?? 0,
    [result]
  );

  async function importSession(profile: ThreadImportProfileView, candidateId: string) {
    if (!client) return;
    const targetId = selectedTargets[profile.runtimeProfileRef]
      ?? profile.targets.find((target) => target.ready)?.targetId;
    if (!targetId) return;
    setImportingId(candidateId);
    setError(null);
    try {
      const imported = await client.request("thread/import", { candidateId, scope, targetId });
      const threadId = imported.snapshot.thread?.id;
      if (!threadId) throw new Error("Imported Agent session did not publish a Thread.");
      onImported(threadId);
      onClose();
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : String(nextError));
    } finally {
      setImportingId(null);
    }
  }

  return (
    <div className="modalBackdrop agentSessionImportBackdrop" role="presentation" onMouseDown={(event) => {
      if (event.target === event.currentTarget && !importingId) onClose();
    }}>
      <CreatePanel
        className="agentSessionImportDialog"
        description="Bring an Agent-owned conversation into Psychevo. Opening this window is the only action that scans external Agent sessions."
        icon={<Inbox size={18} />}
        layout="dialog"
        onClose={importingId ? undefined : onClose}
        title="Import Agent session"
        footer={
          <>
            <span className="agentSessionImportCount">{loading ? "Scanning Agent profiles…" : `${sessionCount} available`}</span>
            <ActionButton
              disabled={disabled || loading || Boolean(importingId)}
              icon={<RefreshCw size={15} />}
              onClick={() => setRefreshKey((value) => value + 1)}
              variant="ghost"
            >
              Refresh
            </ActionButton>
          </>
        }
      >
        {error && <div className="errorBand" role="alert">{error}</div>}
        <div className="agentSessionImportProfiles">
          {loading && !result ? <p className="agentSessionImportEmpty">Initializing enabled ACP Agents…</p> : null}
          {!loading && result?.profiles.length === 0 ? (
            <p className="agentSessionImportEmpty">No enabled ACP Agent profiles can list sessions.</p>
          ) : null}
          {result?.profiles.map((profile) => {
            const selectedTarget = selectedTargets[profile.runtimeProfileRef]
              ?? profile.targets.find((target) => target.ready)?.targetId
              ?? "";
            return (
              <section className={`agentSessionImportProfile is-${profile.status}`} key={profile.runtimeProfileRef}>
                <header>
                  <div>
                    <h4>{profile.profileLabel}</h4>
                    <p>{profile.status === "ready" ? `${profile.sessions.length} sessions` : "Unavailable"}</p>
                  </div>
                  {profile.targets.length > 1 ? (
                    <label>
                      <span>Import as</span>
                      <select
                        aria-label={`Agent target for ${profile.profileLabel}`}
                        disabled={Boolean(importingId)}
                        onChange={(event) => setSelectedTargets((current) => ({
                          ...current,
                          [profile.runtimeProfileRef]: event.target.value
                        }))}
                        value={selectedTarget}
                      >
                        {profile.targets.map((target) => (
                          <option disabled={!target.ready} key={target.targetId} value={target.targetId}>
                            {target.label}
                          </option>
                        ))}
                      </select>
                    </label>
                  ) : null}
                </header>
                {profile.error ? <p className="agentSessionImportError">{profile.error.message}</p> : null}
                {profile.status === "ready" && profile.sessions.length === 0 ? (
                  <p className="agentSessionImportEmpty">
                    {profile.alreadyImportedCount > 0
                      ? "All discovered sessions are already in Psychevo."
                      : "No sessions found in this workspace."}
                  </p>
                ) : null}
                <div className="agentSessionImportRows">
                  {profile.sessions.map((session) => (
                    <button
                      className="agentSessionImportRow"
                      disabled={disabled || Boolean(importingId) || !selectedTarget}
                      key={session.candidateId}
                      onClick={() => void importSession(profile, session.candidateId)}
                      type="button"
                    >
                      <span>
                        <strong>{session.title?.trim() || "Untitled Agent session"}</strong>
                        <small>{session.cwd}</small>
                      </span>
                      <span>{importingId === session.candidateId ? "Importing…" : formatUpdatedAt(session.updatedAt)}</span>
                    </button>
                  ))}
                </div>
              </section>
            );
          })}
        </div>
      </CreatePanel>
    </div>
  );
}

export function DeleteSessionDialog({
  disabled,
  onCancel,
  onConfirm,
  session
}: {
  disabled: boolean;
  onCancel(): void;
  onConfirm(): void;
  session: SessionSummary;
}) {
  const targetLabel = session.lifecycle?.targetLabel;
  const remote = Boolean(targetLabel && targetLabel !== "Psychevo (Native)");
  const title = session.displayTitle?.trim() || session.title?.trim() || session.id.slice(0, 8);
  return (
    <div className="modalBackdrop" role="presentation" onMouseDown={(event) => {
      if (event.target === event.currentTarget && !disabled) onCancel();
    }}>
      <CreatePanel
        className="agentSessionDeleteDialog"
        description={remote
          ? `This permanently deletes the Psychevo Thread and its ${targetLabel} session.`
          : "This permanently deletes the Psychevo Thread from local history."}
        icon={<Trash2 size={18} />}
        layout="dialog"
        onClose={disabled ? undefined : onCancel}
        title="Delete session?"
        footer={
          <>
            <ActionButton disabled={disabled} onClick={onCancel} variant="ghost">Cancel</ActionButton>
            <ActionButton disabled={disabled} onClick={onConfirm} variant="danger">Delete session</ActionButton>
          </>
        }
      >
        <p className="agentSessionDeleteName">{title}</p>
        {remote ? <p className="agentSessionDeleteWarning">Remote deletion must succeed before Psychevo removes local history.</p> : null}
      </CreatePanel>
    </div>
  );
}

function defaultTargetSelections(result: ThreadImportListResult): Record<string, string> {
  return Object.fromEntries(result.profiles.flatMap((profile) => {
    const target = profile.targets.find((candidate) => candidate.ready);
    return target ? [[profile.runtimeProfileRef, target.targetId]] : [];
  }));
}

function formatUpdatedAt(value: string | null): string {
  if (!value) return "";
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return "";
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" })
    .format(new Date(timestamp));
}
