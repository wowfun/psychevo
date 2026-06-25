import { useMemo, useState } from "react";
import { CalendarClock, Pencil, Play, Plus, RefreshCw, Save, Sparkles, Trash2, X } from "lucide-react";
import type {
  AutomationDraftParams,
  AutomationDraftView,
  AutomationExecutionPolicy,
  AutomationScheduleInput,
  AutomationTaskView,
  AutomationWriteParams,
  GatewayRequestScope
} from "@psychevo/protocol";
import type { WorkbenchAutomation } from "./types";

type AutomationDraft = {
  id: string | null;
  targetKind: "project" | "threadHeartbeat";
  title: string;
  prompt: string;
  scheduleKind: "interval" | "daily" | "weekly";
  everyMinutes: number;
  time: string;
  weekdays: number[];
  executionPolicy: AutomationExecutionPolicy;
  enabled: boolean;
};

type AutomationsPageProps = {
  automations: WorkbenchAutomation[];
  currentThreadId: string | null;
  disabled: boolean;
  error: string | null;
  loading: boolean;
  scope: GatewayRequestScope | null;
  workdir: string;
  onDelete(id: string): Promise<void>;
  onDraft(params: AutomationDraftParams): Promise<AutomationDraftView>;
  onOpenSession(threadId: string): void;
  onRefresh(): Promise<void>;
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
  workdir,
  onDelete,
  onDraft,
  onOpenSession,
  onRefresh,
  onRun,
  onSave
}: AutomationsPageProps) {
  const [draft, setDraft] = useState<AutomationDraft | null>(null);
  const [requestText, setRequestText] = useState("");
  const [draftError, setDraftError] = useState<string | null>(null);
  const [pendingAction, setPendingAction] = useState<string | null>(null);
  const sorted = useMemo(() => [...automations].sort(sortAutomations), [automations]);

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
    const params = draftToWriteParams(draft, scope, currentThreadId);
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
        scope,
        request,
        currentThreadId
      });
      setDraft(draftFromGenerated(generated));
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
            <p title={workdir}>{workdir}</p>
          </div>
        </div>
        <div className="automationToolbarActions">
          <button disabled={disabled || Boolean(pendingAction)} onClick={() => void runPending("refresh", onRefresh)} type="button">
            <RefreshCw size={15} aria-hidden /> Refresh
          </button>
          <button disabled={disabled || Boolean(pendingAction)} onClick={() => setDraft(projectDraft())} type="button">
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

      <div className="automationSurface">
        <div className="automationListPane">
          {loading ? (
            <div className="automationEmpty">Loading automations</div>
          ) : sorted.length === 0 ? (
            <div className="automationEmpty">
              <div className="automationTemplateActions">
                <button disabled={disabled} onClick={() => setDraft(projectDraft())} type="button">
                  Project check
                </button>
                <button disabled={disabled || !currentThreadId} onClick={() => setDraft(threadDraft())} type="button">
                  Thread heartbeat
                </button>
              </div>
            </div>
          ) : (
            <div className="automationRows">
              {sorted.map((automation) => {
                const threadId = automation.targetThreadId ?? automation.runs[0]?.threadId ?? null;
                return (
                  <article className="automationRow" data-status={automation.lastStatus ?? "idle"} key={automation.id}>
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
                        {automation.lastStatus && <span>{automation.lastStatus}</span>}
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
                      <button disabled={disabled || Boolean(pendingAction)} onClick={() => setDraft(draftFromAutomation(automation))} type="button">
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

        <form
          aria-label="Automation draft"
          className={`automationDraft ${draft ? "" : "is-empty"}`}
          onSubmit={(event) => {
            event.preventDefault();
            void saveDraft();
          }}
        >
          {draft ? (
            <>
              <header>
                <strong>{draft.id ? "Edit automation" : "New automation"}</strong>
                <button aria-label="Close automation draft" onClick={() => setDraft(null)} title="Close" type="button">
                  <X size={15} />
                </button>
              </header>
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
              <div className="automationSegments" role="group" aria-label="Target">
                <button className={draft.targetKind === "project" ? "is-selected" : ""} onClick={() => setDraft({ ...draft, targetKind: "project" })} type="button">
                  Project
                </button>
                <button
                  className={draft.targetKind === "threadHeartbeat" ? "is-selected" : ""}
                  disabled={!currentThreadId}
                  onClick={() => setDraft({ ...draft, targetKind: "threadHeartbeat" })}
                  type="button"
                >
                  Current thread
                </button>
              </div>
              <div className="automationSegments" role="group" aria-label="Schedule type">
                {(["interval", "daily", "weekly"] as const).map((kind) => (
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
              {draft.scheduleKind !== "interval" && (
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
              <label className="automationCheck">
                <input
                  checked={draft.enabled}
                  onChange={(event) => setDraft({ ...draft, enabled: event.target.checked })}
                  type="checkbox"
                />
                <span>Enabled</span>
              </label>
              <footer>
                <button disabled={Boolean(pendingAction)} onClick={() => setDraft(null)} type="button">
                  Cancel
                </button>
                <button disabled={Boolean(pendingAction) || !draft.title.trim() || !draft.prompt.trim()} type="submit">
                  <Save size={15} aria-hidden /> Save
                </button>
              </footer>
            </>
          ) : (
            <div className="automationDraftPlaceholder">
              <button disabled={disabled} onClick={() => setDraft(projectDraft())} type="button">
                <Plus size={15} aria-hidden /> Project check
              </button>
              <button disabled={disabled || !currentThreadId} onClick={() => setDraft(threadDraft())} type="button">
                <Plus size={15} aria-hidden /> Thread heartbeat
              </button>
            </div>
          )}
        </form>
      </div>
    </section>
  );
}

function projectDraft(): AutomationDraft {
  return {
    id: null,
    targetKind: "project",
    title: "Project check",
    prompt: "Review the current repository state and summarize anything that needs attention.",
    scheduleKind: "interval",
    everyMinutes: 60,
    time: "09:00",
    weekdays: [1, 2, 3, 4, 5],
    executionPolicy: "autoSandbox",
    enabled: true
  };
}

function threadDraft(): AutomationDraft {
  return {
    ...projectDraft(),
    targetKind: "threadHeartbeat",
    title: "Thread heartbeat",
    prompt: "Continue this thread with a concise status check.",
    everyMinutes: 30
  };
}

function draftFromAutomation(automation: AutomationTaskView): AutomationDraft {
  const schedule = automation.schedule;
  return {
    id: automation.id,
    targetKind: automation.kind === "threadHeartbeat" ? "threadHeartbeat" : "project",
    title: automation.title,
    prompt: automation.prompt,
    scheduleKind: schedule.kind,
    everyMinutes: schedule.kind === "interval" ? schedule.everyMinutes : 60,
    time: schedule.kind === "interval" ? "09:00" : schedule.time,
    weekdays: schedule.kind === "weekly" ? schedule.weekdays : [1, 2, 3, 4, 5],
    executionPolicy: automation.execution.policy,
    enabled: automation.enabled
  };
}

function draftFromGenerated(generated: AutomationDraftView): AutomationDraft {
  const schedule = generated.schedule;
  return {
    id: null,
    targetKind: generated.target.kind === "threadHeartbeat" ? "threadHeartbeat" : "project",
    title: generated.title,
    prompt: generated.prompt,
    scheduleKind: schedule.kind,
    everyMinutes: schedule.kind === "interval" ? schedule.everyMinutes : 60,
    time: schedule.kind === "interval" ? "09:00" : schedule.time,
    weekdays: schedule.kind === "weekly" ? schedule.weekdays : [1, 2, 3, 4, 5],
    executionPolicy: generated.execution.policy,
    enabled: generated.enabled
  };
}

function draftToWriteParams(
  draft: AutomationDraft,
  scope: GatewayRequestScope | null,
  currentThreadId: string | null
): AutomationWriteParams {
  return {
    automationId: draft.id,
    scope,
    target: draft.targetKind === "threadHeartbeat" && currentThreadId
      ? { kind: "threadHeartbeat", threadId: currentThreadId }
      : { kind: "project" },
    title: draft.title.trim(),
    prompt: draft.prompt.trim(),
    schedule: scheduleFromDraft(draft),
    enabled: draft.enabled,
    execution: { policy: draft.executionPolicy },
    model: null,
    reasoningEffort: null
  };
}

function scheduleFromDraft(draft: AutomationDraft): AutomationScheduleInput {
  switch (draft.scheduleKind) {
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
