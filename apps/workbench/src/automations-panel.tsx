import { useEffect, useMemo, useState } from "react";
import { CalendarClock, Pause, Pencil, Play, Plus, RefreshCw, Save, Sparkles, Trash2, X } from "lucide-react";
import type {
  AutomationDraftParams,
  AutomationDraftView,
  AutomationExecutionPolicy,
  AutomationScheduleInput,
  AutomationTaskView,
  AutomationWriteParams,
  GatewayRequestScope,
  SessionSummary
} from "@psychevo/protocol";
import type { SessionBrowserWorkspaceState, WorkbenchAutomation } from "./types";

type AutomationDraft = {
  id: string | null;
  cwd: string;
  targetKind: "project" | "threadHeartbeat";
  targetThreadId: string | null;
  title: string;
  prompt: string;
  scheduleKind: "interval" | "delay" | "once" | "daily" | "weekly";
  everyMinutes: number;
  afterMinutes: number;
  onceAt: string;
  time: string;
  weekdays: number[];
  executionPolicy: AutomationExecutionPolicy;
};

type AutomationsPageProps = {
  automations: WorkbenchAutomation[];
  currentThreadId: string | null;
  disabled: boolean;
  error: string | null;
  loading: boolean;
  scope: GatewayRequestScope | null;
  sessionBrowserWorkspaces: SessionBrowserWorkspaceState[];
  sessions: SessionSummary[];
  cwd: string;
  onDelete(id: string): Promise<void>;
  onDraft(params: AutomationDraftParams): Promise<AutomationDraftView>;
  onOpenSession(threadId: string): void;
  onPause(id: string): Promise<void>;
  onRefresh(): Promise<void>;
  onResume(id: string): Promise<void>;
  onRun(id: string): Promise<void>;
  onSave(params: AutomationWriteParams): Promise<void>;
};

const WEEKDAYS = [
  { label: "Mon", value: 1 },
  { label: "Tue", value: 2 },
  { label: "Wed", value: 3 },
  { label: "Thu", value: 4 },
  { label: "Fri", value: 5 },
  { label: "Sat", value: 6 },
  { label: "Sun", value: 7 }
];

export function AutomationsPage({
  automations,
  currentThreadId,
  disabled,
  error,
  loading,
  scope,
  sessionBrowserWorkspaces,
  sessions,
  cwd,
  onDelete,
  onDraft,
  onOpenSession,
  onPause,
  onRefresh,
  onResume,
  onRun,
  onSave
}: AutomationsPageProps) {
  const [draft, setDraft] = useState<AutomationDraft | null>(null);
  const [selectedCwd, setSelectedCwd] = useState(scope?.cwd ?? cwd);
  const [requestText, setRequestText] = useState("");
  const [draftError, setDraftError] = useState<string | null>(null);
  const [pendingAction, setPendingAction] = useState<string | null>(null);
  const sorted = useMemo(() => [...automations].sort(sortAutomations), [automations]);
  const surfaceMode = draft ? "has-draft" : "is-list-only";
  const workspaceOptions = useMemo(
    () => automationWorkspaceOptions(cwd, scope?.cwd ?? null, sessionBrowserWorkspaces, sessions, automations),
    [automations, cwd, scope?.cwd, sessionBrowserWorkspaces, sessions]
  );
  const selectedThreadOptions = useMemo(
    () => automationThreadOptions(sessions, selectedCwd, currentThreadId),
    [currentThreadId, selectedCwd, sessions]
  );

  useEffect(() => {
    if (!workspaceOptions.includes(selectedCwd)) {
      const nextCwd = workspaceOptions[0] ?? cwd;
      setSelectedCwd(nextCwd);
      setDraft((current) => current ? retargetDraft(current, nextCwd, preferredThreadId(sessions, nextCwd, currentThreadId)) : current);
    }
  }, [currentThreadId, cwd, selectedCwd, sessions, workspaceOptions]);

  function selectCwd(nextCwd: string) {
    const nextThreadId = preferredThreadId(sessions, nextCwd, currentThreadId);
    setSelectedCwd(nextCwd);
    setDraft((current) => current ? retargetDraft(current, nextCwd, nextThreadId) : current);
  }

  async function runPending<T>(key: string, action: () => Promise<T>): Promise<T | undefined> {
    if (disabled || pendingAction) {
      return undefined;
    }
    setPendingAction(key);
    try {
      return await action();
    } finally {
      setPendingAction(null);
    }
  }

  async function saveDraft() {
    if (!draft) {
      return;
    }
    const params = draftToWriteParams(draft, scope);
    await runPending("save", async () => {
      await onSave(params);
      setDraft(null);
    });
  }

  async function generateDraft() {
    const request = requestText.trim();
    if (!request || disabled || pendingAction) {
      return;
    }
    setPendingAction("draft");
    setDraftError(null);
    try {
      const generated = await onDraft({
        scope: scopeForCwd(scope, selectedCwd),
        request,
        currentThreadId: preferredThreadId(sessions, selectedCwd, currentThreadId)
      });
      setDraft(draftFromGenerated(generated, selectedCwd));
    } catch (error) {
      setDraftError(error instanceof Error ? error.message : String(error));
    } finally {
      setPendingAction(null);
    }
  }

  return (
    <section aria-label="Automations" className="automationsPage">
      <header className="automationToolbar">
        <div className="automationTitleBlock">
          <span><CalendarClock size={18} aria-hidden /></span>
          <div>
            <h2>Automations</h2>
          </div>
        </div>
        <div className="automationToolbarActions">
          <label className="automationWorkspaceSelect">
            <span>Workspace</span>
            <select
              aria-label="Workspace"
              disabled={disabled || Boolean(pendingAction)}
              onChange={(event) => selectCwd(event.target.value)}
              value={selectedCwd}
            >
              {workspaceOptions.map((option) => (
                <option key={option} value={option}>{option}</option>
              ))}
            </select>
          </label>
          <button disabled={disabled || Boolean(pendingAction)} onClick={() => void runPending("refresh", onRefresh)} type="button">
            <RefreshCw size={15} aria-hidden /> Refresh
          </button>
          <button disabled={disabled || Boolean(pendingAction)} onClick={() => setDraft(projectDraft(selectedCwd))} type="button">
            <Plus size={15} aria-hidden /> New
          </button>
        </div>
      </header>

      {error && <div className="automationError" role="alert">{error}</div>}
      {draftError && <div className="automationError" role="alert">{draftError}</div>}

      <form
        aria-label="Create automation from description"
        className="automationNaturalDraft"
        onSubmit={(event) => {
          event.preventDefault();
          void generateDraft();
        }}
      >
        <textarea
          aria-label="Automation description"
          disabled={disabled || pendingAction === "draft"}
          onChange={(event) => setRequestText(event.target.value)}
          placeholder="Every weekday at 9, review the repo before standup"
          value={requestText}
        />
        <button disabled={disabled || Boolean(pendingAction) || !requestText.trim()} type="submit">
          <Sparkles size={15} aria-hidden /> {pendingAction === "draft" ? "Drafting" : "Draft"}
        </button>
      </form>

      <div className={`automationSurface ${surfaceMode} ${sorted.length === 0 ? "is-empty-list" : ""}`}>
        <div className="automationListPane">
          {loading ? (
            <div className="automationEmpty">Loading automations</div>
          ) : sorted.length === 0 ? (
            <div className="automationEmpty">
              <div className="automationTemplateActions">
                <button disabled={disabled} onClick={() => setDraft(projectDraft(selectedCwd))} type="button">
                  Project check
                </button>
                <button
                  disabled={disabled || selectedThreadOptions.length === 0}
                  onClick={() => setDraft(threadDraft(selectedCwd, selectedThreadOptions[0]?.id ?? null))}
                  type="button"
                >
                  Thread heartbeat
                </button>
              </div>
            </div>
          ) : (
            <div className="automationRows">
              {sorted.map((automation) => {
                const threadId = automationOpenThreadId(automation);
                return (
                  <article
                    className="automationRow"
                    data-last-status={automation.lastStatus ?? "idle"}
                    data-lifecycle={automation.enabled ? "enabled" : "paused"}
                    key={automation.id}
                  >
                    <div className="automationRowMain">
                      <div className="automationRowTitle">
                        <strong>{automation.title}</strong>
                        <span>{automation.enabled ? "enabled" : "paused"}</span>
                      </div>
                      <p>{automation.prompt}</p>
                      <div className="automationMeta">
                        <span>{automation.kind === "threadHeartbeat" ? "thread" : "project"}</span>
                        <span>{formatSchedule(automation.schedule)}</span>
                        <span>{automation.nextRunAtMs ? `next ${formatDateTime(automation.nextRunAtMs)}` : "no next run"}</span>
                        <span>{automation.lastRunAtMs ? `last run ${formatDateTime(automation.lastRunAtMs)}` : "never run"}</span>
                        {automation.lastStatus && <span data-run-status={automation.lastStatus}>{automation.lastStatus}</span>}
                      </div>
                    </div>
                    <div className="automationRowActions">
                      {threadId && (
                        <button disabled={disabled} onClick={() => onOpenSession(threadId)} type="button">
                          Open thread
                        </button>
                      )}
                      <button disabled={disabled || Boolean(pendingAction)} onClick={() => void runPending(`run:${automation.id}`, () => onRun(automation.id))} type="button">
                        <Play size={14} aria-hidden /> Run
                      </button>
                      <button
                        disabled={disabled || Boolean(pendingAction)}
                        onClick={() => void runPending(
                          `${automation.enabled ? "pause" : "resume"}:${automation.id}`,
                          () => automation.enabled ? onPause(automation.id) : onResume(automation.id)
                        )}
                        type="button"
                      >
                        {automation.enabled ? <Pause size={14} aria-hidden /> : <Play size={14} aria-hidden />}
                        {automation.enabled ? " Pause" : " Resume"}
                      </button>
                      <button
                        disabled={disabled || Boolean(pendingAction)}
                        onClick={() => {
                          setSelectedCwd(automation.cwd);
                          setDraft(draftFromAutomation(automation));
                        }}
                        type="button"
                      >
                        <Pencil size={14} aria-hidden /> Edit
                      </button>
                      <button
                        disabled={disabled || Boolean(pendingAction)}
                        onClick={() => {
                          if (window.confirm(`Delete ${automation.title}?`)) {
                            void runPending(`delete:${automation.id}`, () => onDelete(automation.id));
                          }
                        }}
                        type="button"
                      >
                        <Trash2 size={14} aria-hidden /> Delete
                      </button>
                    </div>
                  </article>
                );
              })}
            </div>
          )}
        </div>

        {draft && (
          <form
            aria-label="Automation draft"
            className="automationDraft"
            onSubmit={(event) => {
              event.preventDefault();
              void saveDraft();
            }}
          >
            <>
              <header>
                <strong>{draft.id ? "Edit automation" : "New automation"}</strong>
                <button aria-label="Close automation draft" onClick={() => setDraft(null)} title="Close" type="button">
                  <X size={15} />
                </button>
              </header>
              <label>
                <span>Workspace</span>
                <select
                  aria-label="Draft workspace"
                  onChange={(event) => {
                    const nextCwd = event.target.value;
                    selectCwd(nextCwd);
                    setDraft((current) => current ? retargetDraft(current, nextCwd, preferredThreadId(sessions, nextCwd, currentThreadId)) : current);
                  }}
                  value={draft.cwd}
                >
                  {workspaceOptions.map((option) => (
                    <option key={option} value={option}>{option}</option>
                  ))}
                </select>
              </label>
              <label>
                <span>Bind to</span>
                <select
                  aria-label="Bind to"
                  onChange={(event) => {
                    const value = event.target.value;
                    setDraft(value === "project"
                      ? { ...draft, targetKind: "project", targetThreadId: null }
                      : { ...draft, targetKind: "threadHeartbeat", targetThreadId: value });
                  }}
                  value={draft.targetKind === "threadHeartbeat" ? draft.targetThreadId ?? "" : "project"}
                >
                  <option value="project">Project</option>
                  {automationThreadOptions(sessions, draft.cwd, currentThreadId).map((session) => (
                    <option key={session.id} value={session.id}>{sessionLabel(session, currentThreadId)}</option>
                  ))}
                </select>
              </label>
              <label>
                <span>Title</span>
                <input
                  onChange={(event) => setDraft({ ...draft, title: event.target.value })}
                  required
                  value={draft.title}
                />
              </label>
              <label>
                <span>Prompt</span>
                <textarea
                  onChange={(event) => setDraft({ ...draft, prompt: event.target.value })}
                  required
                  value={draft.prompt}
                />
              </label>
              <div className="automationSegments" role="group" aria-label="Schedule type">
                {(["interval", "delay", "once", "daily", "weekly"] as const).map((kind) => (
                  <button className={draft.scheduleKind === kind ? "is-selected" : ""} key={kind} onClick={() => setDraft({ ...draft, scheduleKind: kind })} type="button">
                    {kind}
                  </button>
                ))}
              </div>
              {draft.scheduleKind === "interval" && (
                <label>
                  <span>Every minutes</span>
                  <input
                    min={1}
                    onChange={(event) => setDraft({ ...draft, everyMinutes: Math.max(1, Number(event.target.value) || 1) })}
                    type="number"
                    value={draft.everyMinutes}
                  />
                </label>
              )}
              {draft.scheduleKind === "delay" && (
                <label>
                  <span>After minutes</span>
                  <input
                    min={1}
                    onChange={(event) => setDraft({ ...draft, afterMinutes: Math.max(1, Number(event.target.value) || 1) })}
                    type="number"
                    value={draft.afterMinutes}
                  />
                </label>
              )}
              {draft.scheduleKind === "once" && (
                <label>
                  <span>Run once at</span>
                  <input
                    onChange={(event) => setDraft({ ...draft, onceAt: event.target.value || defaultOnceAt() })}
                    type="datetime-local"
                    value={draft.onceAt}
                  />
                </label>
              )}
              {(draft.scheduleKind === "daily" || draft.scheduleKind === "weekly") && (
                <label>
                  <span>Time</span>
                  <input
                    onChange={(event) => setDraft({ ...draft, time: event.target.value || "09:00" })}
                    type="time"
                    value={draft.time}
                  />
                </label>
              )}
              {draft.scheduleKind === "weekly" && (
                <div className="automationWeekdays" role="group" aria-label="Weekdays">
                  {WEEKDAYS.map((day) => (
                    <label key={day.value}>
                      <input
                        checked={draft.weekdays.includes(day.value)}
                        onChange={(event) => {
                          const weekdays = event.target.checked
                            ? [...draft.weekdays, day.value].sort()
                            : draft.weekdays.filter((value) => value !== day.value);
                          setDraft({ ...draft, weekdays: weekdays.length ? weekdays : [day.value] });
                        }}
                        type="checkbox"
                      />
                      <span>{day.label}</span>
                    </label>
                  ))}
                </div>
              )}
              <div className="automationSegments" role="group" aria-label="Execution">
                <button className={draft.executionPolicy === "autoSandbox" ? "is-selected" : ""} onClick={() => setDraft({ ...draft, executionPolicy: "autoSandbox" })} type="button">
                  Auto in sandbox
                </button>
                <button className={draft.executionPolicy === "askFirst" ? "is-selected" : ""} onClick={() => setDraft({ ...draft, executionPolicy: "askFirst" })} type="button">
                  Ask first
                </button>
              </div>
              <footer>
                <button disabled={Boolean(pendingAction)} onClick={() => setDraft(null)} type="button">
                  Cancel
                </button>
                <button disabled={Boolean(pendingAction) || !draft.title.trim() || !draft.prompt.trim()} type="submit">
                  <Save size={15} aria-hidden /> Save
                </button>
              </footer>
            </>
          </form>
        )}
      </div>
    </section>
  );
}

function projectDraft(cwd: string): AutomationDraft {
  return {
    id: null,
    cwd,
    targetKind: "project",
    targetThreadId: null,
    title: "Project check",
    prompt: "Review the current repository state and summarize anything that needs attention.",
    scheduleKind: "interval",
    everyMinutes: 60,
    afterMinutes: 30,
    onceAt: defaultOnceAt(),
    time: "09:00",
    weekdays: [1, 2, 3, 4, 5],
    executionPolicy: "autoSandbox"
  };
}

function threadDraft(cwd: string, threadId: string | null): AutomationDraft {
  return {
    ...projectDraft(cwd),
    targetKind: "threadHeartbeat",
    targetThreadId: threadId,
    title: "Thread heartbeat",
    prompt: "Continue this thread with a concise status check.",
    everyMinutes: 30
  };
}

function draftFromAutomation(automation: AutomationTaskView): AutomationDraft {
  const schedule = automation.schedule;
  return {
    id: automation.id,
    cwd: automation.cwd,
    targetKind: automation.kind === "threadHeartbeat" ? "threadHeartbeat" : "project",
    targetThreadId: automation.targetThreadId,
    title: automation.title,
    prompt: automation.prompt,
    scheduleKind: schedule.kind,
    everyMinutes: schedule.kind === "interval" ? schedule.everyMinutes : 60,
    afterMinutes: schedule.kind === "delay" ? schedule.afterMinutes : 30,
    onceAt: schedule.kind === "once" ? dateTimeLocalValue(schedule.at) : defaultOnceAt(),
    time: schedule.kind === "daily" || schedule.kind === "weekly" ? schedule.time : "09:00",
    weekdays: schedule.kind === "weekly" ? schedule.weekdays : [1, 2, 3, 4, 5],
    executionPolicy: automation.execution.policy
  };
}

function draftFromGenerated(generated: AutomationDraftView, cwd: string): AutomationDraft {
  const schedule = generated.schedule;
  return {
    id: null,
    cwd,
    targetKind: generated.target.kind === "threadHeartbeat" ? "threadHeartbeat" : "project",
    targetThreadId: generated.target.kind === "threadHeartbeat" ? generated.target.threadId : null,
    title: generated.title,
    prompt: generated.prompt,
    scheduleKind: schedule.kind,
    everyMinutes: schedule.kind === "interval" ? schedule.everyMinutes : 60,
    afterMinutes: schedule.kind === "delay" ? schedule.afterMinutes : 30,
    onceAt: schedule.kind === "once" ? dateTimeLocalValue(schedule.at) : defaultOnceAt(),
    time: schedule.kind === "daily" || schedule.kind === "weekly" ? schedule.time : "09:00",
    weekdays: schedule.kind === "weekly" ? schedule.weekdays : [1, 2, 3, 4, 5],
    executionPolicy: generated.execution.policy
  };
}

function draftToWriteParams(
  draft: AutomationDraft,
  scope: GatewayRequestScope | null
): AutomationWriteParams {
  return {
    automationId: draft.id,
    scope: scopeForCwd(scope, draft.cwd),
    target: draft.targetKind === "threadHeartbeat" && draft.targetThreadId
      ? { kind: "threadHeartbeat", threadId: draft.targetThreadId }
      : { kind: "project" },
    title: draft.title.trim(),
    prompt: draft.prompt.trim(),
    schedule: scheduleFromDraft(draft),
    execution: { policy: draft.executionPolicy },
    model: null,
    reasoningEffort: null
  };
}

function scopeForCwd(scope: GatewayRequestScope | null, cwd: string): GatewayRequestScope | null {
  return scope ? { ...scope, cwd } : null;
}

function retargetDraft(draft: AutomationDraft, cwd: string, threadId: string | null): AutomationDraft {
  return {
    ...draft,
    cwd,
    targetThreadId: draft.targetKind === "threadHeartbeat" ? threadId : null
  };
}

function automationWorkspaceOptions(
  cwd: string,
  scopeCwd: string | null,
  browserWorkspaces: SessionBrowserWorkspaceState[],
  sessions: SessionSummary[],
  automations: WorkbenchAutomation[]
): string[] {
  return uniqueNonEmpty([
    scopeCwd,
    cwd,
    ...browserWorkspaces.map((workspace) => workspace.cwd),
    ...sessions.map((session) => session.cwd),
    ...automations.map((automation) => automation.cwd)
  ]);
}

function automationThreadOptions(
  sessions: SessionSummary[],
  cwd: string,
  currentThreadId: string | null
): SessionSummary[] {
  return [...sessions]
    .filter((session) => session.cwd === cwd)
    .sort((left, right) => {
      if (left.id === currentThreadId) {
        return -1;
      }
      if (right.id === currentThreadId) {
        return 1;
      }
      const rightTime = right.updatedAtMs ?? right.startedAtMs ?? 0;
      const leftTime = left.updatedAtMs ?? left.startedAtMs ?? 0;
      return rightTime - leftTime || left.id.localeCompare(right.id);
    });
}

function preferredThreadId(
  sessions: SessionSummary[],
  cwd: string,
  currentThreadId: string | null
): string | null {
  return automationThreadOptions(sessions, cwd, currentThreadId)[0]?.id ?? null;
}

function automationOpenThreadId(automation: AutomationTaskView): string | null {
  return automation.targetThreadId ?? automation.runs.find((run) => run.threadId)?.threadId ?? null;
}

function sessionLabel(session: SessionSummary, currentThreadId: string | null): string {
  const title = session.displayTitle ?? session.title ?? session.id;
  return session.id === currentThreadId ? `${title} (current)` : title;
}

function uniqueNonEmpty(values: Array<string | null | undefined>): string[] {
  return Array.from(new Set(values.map((value) => value?.trim()).filter((value): value is string => Boolean(value))));
}

function scheduleFromDraft(draft: AutomationDraft): AutomationScheduleInput {
  switch (draft.scheduleKind) {
    case "delay":
      return { kind: "delay", afterMinutes: Math.max(1, draft.afterMinutes || 1) };
    case "once":
      return { kind: "once", at: draft.onceAt || defaultOnceAt() };
    case "daily":
      return { kind: "daily", time: draft.time || "09:00" };
    case "weekly":
      return { kind: "weekly", weekdays: draft.weekdays.length ? draft.weekdays : [1], time: draft.time || "09:00" };
    case "interval":
    default:
      return { kind: "interval", everyMinutes: Math.max(1, draft.everyMinutes || 1) };
  }
}

function sortAutomations(left: AutomationTaskView, right: AutomationTaskView): number {
  if (left.enabled !== right.enabled) {
    return left.enabled ? -1 : 1;
  }
  const leftNext = left.nextRunAtMs ?? Number.MAX_SAFE_INTEGER;
  const rightNext = right.nextRunAtMs ?? Number.MAX_SAFE_INTEGER;
  return leftNext - rightNext || right.updatedAtMs - left.updatedAtMs;
}

function formatSchedule(schedule: AutomationScheduleInput): string {
  if (schedule.kind === "interval") {
    return `every ${schedule.everyMinutes}m`;
  }
  if (schedule.kind === "delay") {
    return `after ${schedule.afterMinutes}m`;
  }
  if (schedule.kind === "once") {
    return `once ${formatOnceAt(schedule.at)}`;
  }
  if (schedule.kind === "daily") {
    return `daily ${schedule.time}`;
  }
  return `${schedule.weekdays.map((day) => WEEKDAYS.find((item) => item.value === day)?.label ?? String(day)).join(" ")} ${schedule.time}`;
}

function formatDateTime(value: number): string {
  return new Intl.DateTimeFormat(undefined, {
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    month: "short"
  }).format(new Date(value));
}

function defaultOnceAt(): string {
  const value = new Date(Date.now() + 60 * 60_000);
  value.setSeconds(0, 0);
  return dateTimeLocalFromDate(value);
}

function dateTimeLocalValue(value: string): string {
  const parsed = Date.parse(value);
  if (!Number.isNaN(parsed)) {
    return dateTimeLocalFromDate(new Date(parsed));
  }
  return /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/.test(value) ? value : defaultOnceAt();
}

function dateTimeLocalFromDate(value: Date): string {
  const offsetMs = value.getTimezoneOffset() * 60_000;
  return new Date(value.getTime() - offsetMs).toISOString().slice(0, 16);
}

function formatOnceAt(value: string): string {
  const parsed = Date.parse(value);
  if (!Number.isNaN(parsed)) {
    return formatDateTime(parsed);
  }
  return value;
}
