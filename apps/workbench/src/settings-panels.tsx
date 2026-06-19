import { useState, type ReactNode } from "react";
import {
  Activity,
  Archive,
  ArrowLeft,
  BarChart3,
  Bot,
  Bug,
  Edit3,
  Moon,
  Palette,
  PlugZap,
  Plus,
  RotateCcw,
  Save,
  Search,
  Sun,
  Trash2,
  Wrench,
  X
} from "lucide-react";
import type { SessionSummary } from "@psychevo/protocol";
import { prettyJson } from "./data";
import type {
  Appearance,
  BackendCommandJson,
  BackendDraft,
  SettingsSection,
  WorkbenchUsageStats,
  WorkbenchBackend,
  WorkbenchBackendDoctor
} from "./types";

const BACKEND_ENTRYPOINTS = ["peer", "subagent"] as const;
const BACKEND_CLIENT_CAPABILITIES = ["fs.read", "fs.write", "terminal"] as const;
const BACKEND_COMMAND_JSON_TEMPLATE = `{
  "command": "opencode",
  "args": ["acp"],
  "env": {}
}`;
export const EMPTY_BACKEND_DRAFT: BackendDraft = {
  id: "",
  enabled: true,
  label: "",
  description: "",
  commandJsonText: BACKEND_COMMAND_JSON_TEMPLATE,
  cwd: "",
  entrypoints: ["peer", "subagent"],
  clientCapabilities: ["fs.read", "fs.write", "terminal"],
  mcpServersText: ""
};
const SETTINGS_SECTIONS: Array<{ id: SettingsSection; label: string; description: string }> = [
  { id: "appearance", label: "Appearance", description: "Theme" },
  { id: "usage", label: "Usage", description: "Tokens and cost" },
  { id: "debug", label: "Debug", description: "Developer diagnostics" },
  { id: "agents", label: "Agents", description: "Profile ACP backends" },
  { id: "archived", label: "Archived sessions", description: "Restore or delete" }
];
export function SettingsPage({
  appearance,
  archivedSessions,
  backendDraft,
  backendDoctor,
  backends,
  debugEnabled,
  disabled,
  section,
  usageStats,
  usageStatsError,
  usageStatsLoading,
  onAppearanceChange,
  onCancelBackendEdit,
  onChangeBackendDraft,
  onDebugChange,
  onDeleteArchivedSession,
  onDeleteBackend,
  onDoctorBackend,
  onEditBackend,
  onNewBackend,
  onOpenTranscript,
  onRestoreArchivedSession,
  onSaveBackendDraft,
  onSectionChange,
  onRefreshUsageStats,
  onSetBackendEnabled,
  onSetBackendEntrypoints,
  workdir
}: {
  appearance: Appearance;
  archivedSessions: SessionSummary[];
  backendDraft: BackendDraft | null;
  backendDoctor: Record<string, WorkbenchBackendDoctor>;
  backends: WorkbenchBackend[];
  debugEnabled: boolean;
  disabled: boolean;
  section: SettingsSection;
  usageStats: WorkbenchUsageStats | null;
  usageStatsError: string | null;
  usageStatsLoading: boolean;
  onAppearanceChange(value: Appearance): void;
  onCancelBackendEdit(): void;
  onChangeBackendDraft(draft: BackendDraft): void;
  onDebugChange(value: boolean): void;
  onDeleteArchivedSession(threadId: string): void;
  onDeleteBackend(backend: WorkbenchBackend): void;
  onDoctorBackend(backend: WorkbenchBackend): void;
  onEditBackend(backend: WorkbenchBackend): void;
  onNewBackend(): void;
  onOpenTranscript(): void;
  onRestoreArchivedSession(threadId: string): void;
  onSaveBackendDraft(draft: BackendDraft): void;
  onSectionChange(value: SettingsSection): void;
  onRefreshUsageStats(): void;
  onSetBackendEnabled(backend: WorkbenchBackend, enabled: boolean): void;
  onSetBackendEntrypoints(backend: WorkbenchBackend, entrypoints: string[]): void;
  workdir: string;
}) {
  const [query, setQuery] = useState("");
  const active = SETTINGS_SECTIONS.find((item) => item.id === section) ?? SETTINGS_SECTIONS[0]!;
  const normalizedQuery = query.trim().toLowerCase();
  const primarySections = SETTINGS_SECTIONS.filter((item) => item.id !== "archived");
  const archivedSection = SETTINGS_SECTIONS.find((item) => item.id === "archived")!;
  const sectionMatches = (item: (typeof SETTINGS_SECTIONS)[number]) => (
    !normalizedQuery
    || item.label.toLowerCase().includes(normalizedQuery)
    || item.description.toLowerCase().includes(normalizedQuery)
  );
  const visiblePrimarySections = primarySections.filter(sectionMatches);
  const showArchivedSection = sectionMatches(archivedSection);
  return (
    <section className="centerPage settingsPage" aria-label="Settings">
      <div className="settingsShell">
        <aside className="settingsNav" aria-label="Settings sections">
          <div className="settingsNavTop">
            <button className="settingsBackButton" onClick={onOpenTranscript} type="button">
              <ArrowLeft size={15} />
              <span>Back to app</span>
            </button>
            <label className="settingsSearch">
              <Search size={14} aria-hidden />
              <input
                aria-label="Search settings"
                onChange={(event) => setQuery(event.currentTarget.value)}
                placeholder="Search settings"
                type="search"
                value={query}
              />
            </label>
          </div>
          <div className="settingsNavGroups">
            {visiblePrimarySections.map((item) => (
              <button
                aria-current={item.id === section ? "page" : undefined}
                className={item.id === section ? "is-selected" : ""}
                key={item.id}
                onClick={() => onSectionChange(item.id)}
                type="button"
              >
                {settingsSectionIcon(item.id, 15)}
                <span>{item.label}</span>
              </button>
            ))}
            {visiblePrimarySections.length === 0 && !showArchivedSection && (
              <p className="settingsNavEmpty">No settings found</p>
            )}
          </div>
          <div className="settingsNavFooter">
            {showArchivedSection && (
              <button
                aria-current={section === archivedSection.id ? "page" : undefined}
                className={section === archivedSection.id ? "is-selected" : ""}
                onClick={() => onSectionChange(archivedSection.id)}
                type="button"
              >
                {settingsSectionIcon(archivedSection.id, 15)}
                <span>{archivedSection.label}</span>
              </button>
            )}
          </div>
        </aside>
        <div className="settingsContent">
          <header className="settingsSectionHeader">
            <span>{settingsSectionIcon(active.id, 17)}</span>
            <div>
              <h3>{active.label}</h3>
            </div>
          </header>
          <SettingsSectionPanel
            appearance={appearance}
            archivedSessions={archivedSessions}
            backendDraft={backendDraft}
            backendDoctor={backendDoctor}
            backends={backends}
            debugEnabled={debugEnabled}
            disabled={disabled}
            section={section}
            usageStats={usageStats}
            usageStatsError={usageStatsError}
            usageStatsLoading={usageStatsLoading}
            onAppearanceChange={onAppearanceChange}
            onCancelBackendEdit={onCancelBackendEdit}
            onChangeBackendDraft={onChangeBackendDraft}
            onDebugChange={onDebugChange}
            onDeleteArchivedSession={onDeleteArchivedSession}
            onDeleteBackend={onDeleteBackend}
            onDoctorBackend={onDoctorBackend}
            onEditBackend={onEditBackend}
            onNewBackend={onNewBackend}
            onRestoreArchivedSession={onRestoreArchivedSession}
            onSaveBackendDraft={onSaveBackendDraft}
            onRefreshUsageStats={onRefreshUsageStats}
            onSetBackendEnabled={onSetBackendEnabled}
            onSetBackendEntrypoints={onSetBackendEntrypoints}
            workdir={workdir}
          />
        </div>
      </div>
    </section>
  );
}

function SettingsSectionPanel({
  appearance,
  archivedSessions,
  backendDraft,
  backendDoctor,
  backends,
  debugEnabled,
  disabled,
  section,
  usageStats,
  usageStatsError,
  usageStatsLoading,
  onAppearanceChange,
  onCancelBackendEdit,
  onChangeBackendDraft,
  onDebugChange,
  onDeleteArchivedSession,
  onDeleteBackend,
  onDoctorBackend,
  onEditBackend,
  onNewBackend,
  onRefreshUsageStats,
  onRestoreArchivedSession,
  onSaveBackendDraft,
  onSetBackendEnabled,
  onSetBackendEntrypoints,
  workdir
}: {
  appearance: Appearance;
  archivedSessions: SessionSummary[];
  backendDraft: BackendDraft | null;
  backendDoctor: Record<string, WorkbenchBackendDoctor>;
  backends: WorkbenchBackend[];
  debugEnabled: boolean;
  disabled: boolean;
  section: SettingsSection;
  usageStats: WorkbenchUsageStats | null;
  usageStatsError: string | null;
  usageStatsLoading: boolean;
  onAppearanceChange(value: Appearance): void;
  onCancelBackendEdit(): void;
  onChangeBackendDraft(draft: BackendDraft): void;
  onDebugChange(value: boolean): void;
  onDeleteArchivedSession(threadId: string): void;
  onDeleteBackend(backend: WorkbenchBackend): void;
  onDoctorBackend(backend: WorkbenchBackend): void;
  onEditBackend(backend: WorkbenchBackend): void;
  onNewBackend(): void;
  onRefreshUsageStats(): void;
  onRestoreArchivedSession(threadId: string): void;
  onSaveBackendDraft(draft: BackendDraft): void;
  onSetBackendEnabled(backend: WorkbenchBackend, enabled: boolean): void;
  onSetBackendEntrypoints(backend: WorkbenchBackend, entrypoints: string[]): void;
  workdir: string;
}) {
  switch (section) {
    case "appearance":
      return (
        <div className="settingsRows">
          <SettingsOptionRow title="Theme" description="Dark, neutral light, or warm paper Workbench appearance.">
            <div className="segmentedControl">
              <button className={appearance === "dark" ? "is-selected" : ""} onClick={() => onAppearanceChange("dark")} type="button">
                <Moon size={15} /> Dark
              </button>
              <button className={appearance === "light" ? "is-selected" : ""} onClick={() => onAppearanceChange("light")} type="button">
                <Sun size={15} /> Light
              </button>
              <button className={appearance === "warm" ? "is-selected" : ""} onClick={() => onAppearanceChange("warm")} type="button">
                <Palette size={15} /> Warm
              </button>
            </div>
          </SettingsOptionRow>
        </div>
      );
    case "usage":
      return (
        <UsageSettingsPanel
          loading={usageStatsLoading}
          stats={usageStats}
          error={usageStatsError}
          onRefresh={onRefreshUsageStats}
        />
      );
    case "archived":
      return (
        <ArchivedSessionsPanel
          disabled={disabled}
          sessions={archivedSessions}
          onDelete={onDeleteArchivedSession}
          onRestore={onRestoreArchivedSession}
        />
      );
    case "debug":
      return (
        <div className="settingsRows">
          <SettingsOptionRow title="Show debug tab" description="Recent Gateway notifications in the right inspector.">
            <label className="switchControl">
              <input checked={debugEnabled} onChange={(event) => onDebugChange(event.target.checked)} type="checkbox" />
              <span>{debugEnabled ? "On" : "Off"}</span>
            </label>
          </SettingsOptionRow>
        </div>
      );
    case "agents":
      return (
        <AgentsConfigPanel
          backendDraft={backendDraft}
          backendDoctor={backendDoctor}
          backends={backends}
          disabled={disabled}
          onCancelBackendEdit={onCancelBackendEdit}
          onChangeBackendDraft={onChangeBackendDraft}
          onDeleteBackend={onDeleteBackend}
          onDoctorBackend={onDoctorBackend}
          onEditBackend={onEditBackend}
          onNewBackend={onNewBackend}
          onSaveBackendDraft={onSaveBackendDraft}
          onSetBackendEnabled={onSetBackendEnabled}
          onSetBackendEntrypoints={onSetBackendEntrypoints}
        />
      );
  }
}

function SettingsOptionRow({
  children,
  description,
  title
}: {
  children: ReactNode;
  description: string;
  title: string;
}) {
  return (
    <div className="settingsRow">
      <div>
        <strong>{title}</strong>
        <span>{description}</span>
      </div>
      {children}
    </div>
  );
}

function UsageSettingsPanel({
  error,
  loading,
  stats,
  onRefresh
}: {
  error: string | null;
  loading: boolean;
  stats: WorkbenchUsageStats | null;
  onRefresh(): void;
}) {
  const windows = stats?.windows ?? [];
  const primaryWindows = ["all", "30d", "7d"]
    .map((id) => windows.find((window) => window.id === id))
    .filter((window): window is WorkbenchUsageStats["windows"][number] => Boolean(window));
  return (
    <section className="usageSettingsPanel" aria-label="Usage">
      <div className="usageSettingsToolbar">
        <span>{stats ? `Updated ${formatShortDateTime(stats.generatedAtMs)}` : loading ? "Loading" : "No data"}</span>
        <button aria-label="Refresh usage" disabled={loading} onClick={onRefresh} title="Refresh usage" type="button">
          <RotateCcw size={13} />
          <span>Refresh</span>
        </button>
      </div>
      {error && <div className="usageSettingsError" role="alert">{error}</div>}
      {primaryWindows.length > 0 ? (
        <div className="usageWindowGrid">
          {primaryWindows.map((window) => <UsageWindowCard key={window.id} window={window} />)}
        </div>
      ) : (
        <div className="usageSettingsEmpty">{loading ? "Loading usage" : "No usage recorded"}</div>
      )}
      {stats && <UsageActivityHeatmap activity={stats.activity} />}
    </section>
  );
}

function UsageWindowCard({ window }: { window: WorkbenchUsageStats["windows"][number] }) {
  const inputTokens = window.billableInputTokens + window.cacheReadTokens + window.cacheWriteTokens;
  return (
    <section className="usageWindowCard" aria-label={window.label}>
      <header>
        <span>{window.label}</span>
        <strong>{formatCompactNumber(window.reportedTotalTokens)}</strong>
      </header>
      <div className="usageWindowMetrics">
        <div>
          <span>Cost</span>
          <strong>{formatUsageCost(window)}</strong>
        </div>
        <div>
          <span>Cache read</span>
          <strong>{formatPercent(window.cacheReadPercent)}</strong>
        </div>
        <div>
          <span>Sessions</span>
          <strong>{formatCompactNumber(window.sessionCount)}</strong>
        </div>
      </div>
      <dl className="usageBreakdown">
        <div><dt>Input</dt><dd>{formatCompactNumber(inputTokens)}</dd></div>
        <div><dt>Output</dt><dd>{formatCompactNumber(window.billableOutputTokens)}</dd></div>
        <div><dt>Reasoning</dt><dd>{formatCompactNumber(window.reasoningTokens)}</dd></div>
        <div><dt>Cache write</dt><dd>{formatCompactNumber(window.cacheWriteTokens)}</dd></div>
        {window.unknownPricingCount > 0 && (
          <div><dt>Unknown pricing</dt><dd>{formatCompactNumber(window.unknownPricingCount)}</dd></div>
        )}
      </dl>
    </section>
  );
}

function UsageActivityHeatmap({ activity }: { activity: WorkbenchUsageStats["activity"] }) {
  const days = activity.days;
  const maxTokens = Math.max(1, ...days.map((day) => day.reportedTotalTokens));
  const startPadding = days[0] ? new Date(`${days[0].date}T00:00:00`).getDay() : 0;
  const cells: Array<null | WorkbenchUsageStats["activity"]["days"][number]> = [
    ...Array.from({ length: startPadding }, () => null),
    ...days
  ];
  const weekCount = Math.max(1, Math.ceil(cells.length / 7));
  const monthLabels = heatmapMonthLabels(cells, weekCount);
  return (
    <section className="usageHeatmapPanel" aria-label="Token activity">
      <header>
        <span><BarChart3 size={14} /> Token activity</span>
        <small>{activity.startDate} to {activity.endDate}</small>
      </header>
      <div className="usageHeatmapScroller">
        <div
          className="usageHeatmapMonths"
          style={{ gridTemplateColumns: `repeat(${weekCount}, 11px)` }}
        >
          {monthLabels.map((label) => (
            <span key={`${label.month}-${label.week}`} style={{ gridColumn: `${label.week + 1} / span ${label.span}` }}>
              {label.month}
            </span>
          ))}
        </div>
        <div className="usageHeatmapBody">
          <div className="usageHeatmapWeekdays" aria-hidden>
            <span>Sun</span>
            <span>Mon</span>
            <span>Tue</span>
            <span>Wed</span>
            <span>Thu</span>
            <span>Fri</span>
            <span>Sat</span>
          </div>
          <div
            className="usageHeatmapGrid"
            style={{ gridTemplateColumns: `repeat(${weekCount}, 11px)` }}
          >
            {cells.map((day, index) => {
              const level = day ? heatmapLevel(day.reportedTotalTokens, maxTokens) : 0;
              return (
                <span
                  aria-label={day ? `${day.date}: ${day.reportedTotalTokens} tokens` : undefined}
                  className={day ? "usageHeatmapCell" : "usageHeatmapCell is-empty"}
                  data-level={level}
                  key={day?.date ?? `pad-${index}`}
                  title={day ? `${day.date}: ${formatCompactNumber(day.reportedTotalTokens)} tokens` : undefined}
                />
              );
            })}
          </div>
        </div>
      </div>
    </section>
  );
}

function heatmapLevel(tokens: number, maxTokens: number): number {
  if (tokens <= 0) {
    return 0;
  }
  return Math.max(1, Math.min(4, Math.ceil((tokens / maxTokens) * 4)));
}

function heatmapMonthLabels(
  cells: Array<null | WorkbenchUsageStats["activity"]["days"][number]>,
  weekCount: number
): Array<{ month: string; span: number; week: number }> {
  const labels: Array<{ month: string; span: number; week: number }> = [];
  let lastMonth = "";
  for (let index = 0; index < cells.length; index += 1) {
    const day = cells[index];
    if (!day) {
      continue;
    }
    const date = new Date(`${day.date}T00:00:00`);
    const month = date.toLocaleString(undefined, { month: "short" });
    const week = Math.floor(index / 7);
    if (month !== lastMonth) {
      labels.push({ month, span: 1, week });
      lastMonth = month;
    }
  }
  return labels;
}

function formatUsageCost(window: WorkbenchUsageStats["windows"][number]): string {
  if (window.costStatus === "unknown" && window.estimatedCostNanodollars === 0) {
    return "Unknown";
  }
  const value = formatNanodollars(window.estimatedCostNanodollars);
  return window.unknownPricingCount > 0 ? `${value} + unknown` : value;
}

function formatCompactNumber(value: number): string {
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 1, notation: "compact" }).format(value);
}

function formatPercent(value: number | null): string {
  return value === null ? "-" : `${Math.round(value)}%`;
}

function formatNanodollars(value: number): string {
  return `$${(value / 1_000_000_000).toFixed(6)}`;
}

function formatShortDateTime(value: number): string {
  return new Date(value).toLocaleString(undefined, {
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    month: "short"
  });
}

function ArchivedSessionsPanel({
  disabled,
  sessions,
  onDelete,
  onRestore
}: {
  disabled: boolean;
  sessions: SessionSummary[];
  onDelete(threadId: string): void;
  onRestore(threadId: string): void;
}) {
  return (
    <section className="archivedSessionsPanel" aria-label="Archived sessions">
      {sessions.length === 0 ? (
        <p>No archived sessions.</p>
      ) : (
        <div className="archivedSessionList">
          {sessions.map((session) => {
            const title = session.displayTitle?.trim() || session.title?.trim() || shortSessionId(session.id);
            const workspace = session.project?.label || session.project?.displayPath || session.workdir || "workspace";
            return (
              <div className="archivedSessionRow" key={session.id}>
                <div>
                  <strong>{title}</strong>
                  <span>{workspace}</span>
                </div>
                <div className="archivedSessionActions">
                  <button aria-label={`Restore ${title}`} disabled={disabled} onClick={() => onRestore(session.id)} title="Restore" type="button">
                    <RotateCcw size={13} />
                  </button>
                  <button aria-label={`Delete ${title}`} disabled={disabled} onClick={() => onDelete(session.id)} title="Delete" type="button">
                    <Trash2 size={13} />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}

function settingsSectionIcon(section: SettingsSection, size: number): ReactNode {
  switch (section) {
    case "appearance":
      return <Sun size={size} />;
    case "usage":
      return <Activity size={size} />;
    case "archived":
      return <Archive size={size} />;
    case "debug":
      return <Bug size={size} />;
    case "agents":
      return <Bot size={size} />;
  }
}

function AgentsConfigPanel({
  backendDraft,
  backendDoctor,
  backends,
  disabled,
  onCancelBackendEdit,
  onChangeBackendDraft,
  onDeleteBackend,
  onDoctorBackend,
  onEditBackend,
  onNewBackend,
  onSaveBackendDraft,
  onSetBackendEnabled,
  onSetBackendEntrypoints,
}: {
  backendDraft: BackendDraft | null;
  backendDoctor: Record<string, WorkbenchBackendDoctor>;
  backends: WorkbenchBackend[];
  disabled: boolean;
  onCancelBackendEdit(): void;
  onChangeBackendDraft(draft: BackendDraft): void;
  onDeleteBackend(backend: WorkbenchBackend): void;
  onDoctorBackend(backend: WorkbenchBackend): void;
  onEditBackend(backend: WorkbenchBackend): void;
  onNewBackend(): void;
  onSaveBackendDraft(draft: BackendDraft): void;
  onSetBackendEnabled(backend: WorkbenchBackend, enabled: boolean): void;
  onSetBackendEntrypoints(backend: WorkbenchBackend, entrypoints: string[]): void;
}) {
  const profileBackends = backends.filter((backend) => backend.sourceTargets.includes("profile"));
  return (
    <section className="agentSurfacePanel agentsConfigPanel" aria-label="Agents">
      <header className="agentSurfaceHeaderWithAction">
        <span><PlugZap size={15} /> Profile ACP Backends <b>{profileBackends.length}</b></span>
        {!backendDraft && (
          <button aria-label="Add ACP backend" disabled={disabled} onClick={onNewBackend} title="Add ACP backend" type="button">
            <Plus size={14} />
          </button>
        )}
      </header>
      {backendDraft && (
        <BackendEditorForm
          draft={backendDraft}
          disabled={disabled}
          onCancel={onCancelBackendEdit}
          onChange={onChangeBackendDraft}
          onSave={onSaveBackendDraft}
        />
      )}
      {(!backendDraft || profileBackends.length > 0) && <div className="agentSurfaceList">
        {profileBackends.map((backend) => {
          const doctor = backendDoctor[backend.id] ?? null;
          return (
            <div className="agentSurfaceRow agentBackendRow" key={backend.id}>
	              <div>
	                <strong>{backend.label || backend.id}</strong>
	                <span>{backend.command ? [backend.command, ...backend.args].join(" ") : backend.description || backend.kind}</span>
	                {backend.diagnostics.length > 0 && (
	                  <small className="agentSurfaceWarning">{backend.diagnostics.map((diagnostic) => diagnostic.message).join(" · ")}</small>
	                )}
                {doctor && (
                  <small className={doctor.ok ? "agentSurfaceOk" : "agentSurfaceWarning"}>
                    {doctor.checks.map((check) => `${check.name}: ${check.ok ? "ok" : check.message}`).join(" · ")}
                  </small>
                )}
              </div>
              <div className="agentBackendSide">
                <div className="agentBackendControls">
                  <label className="backendSwitch">
                    <input
                      aria-label={`${backend.enabled ? "Disable" : "Enable"} ${backend.id}`}
                      checked={backend.enabled}
                      disabled={disabled}
                      onChange={(event) => onSetBackendEnabled(backend, event.currentTarget.checked)}
                      role="switch"
                      type="checkbox"
                    />
                    <span className="backendSwitchTrack" aria-hidden />
                    <span>{backend.enabled ? "Enabled" : "Disabled"}</span>
                  </label>
                  <BackendEntrypointControls
                    backend={backend}
                    disabled={disabled}
                    onChange={(entrypoints) => onSetBackendEntrypoints(backend, entrypoints)}
                  />
                </div>
                <div className="agentBackendActions">
                  <button aria-label={`Edit ${backend.id}`} disabled={disabled} onClick={() => onEditBackend(backend)} title="Edit Profile backend" type="button">
                    <Edit3 size={13} />
                  </button>
                  <button aria-label={`Doctor ${backend.id}`} disabled={disabled} onClick={() => onDoctorBackend(backend)} title="Doctor" type="button">
                    <Wrench size={13} />
                  </button>
                  <button
                    aria-label={`Delete ${backend.id} from Profile`}
                    disabled={disabled}
                    onClick={() => onDeleteBackend(backend)}
                    title="Delete Profile backend"
                    type="button"
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
              </div>
            </div>
          );
        })}
        {profileBackends.length === 0 && <p>No Profile ACP backends configured.</p>}
      </div>}
    </section>
  );
}

function BackendEntrypointControls({
  backend,
  disabled,
  onChange
}: {
  backend: WorkbenchBackend;
  disabled: boolean;
  onChange(entrypoints: string[]): void;
}) {
  const selected = backend.entrypoints.length > 0 ? backend.entrypoints : ["peer", "subagent"];
  return (
    <div className="backendEntrypointControls" aria-label={`${backend.id} entrypoints`}>
      {BACKEND_ENTRYPOINTS.map((entrypoint) => {
        const checked = selected.includes(entrypoint);
        const isLastSelected = checked && selected.length === 1;
        return (
          <label key={entrypoint}>
            <input
              aria-label={`${backend.id} ${entrypoint} entrypoint`}
              checked={checked}
              disabled={disabled || isLastSelected}
              onChange={(event) => {
                const next = event.currentTarget.checked
                  ? [...selected, entrypoint]
                  : selected.filter((item) => item !== entrypoint);
                onChange(BACKEND_ENTRYPOINTS.filter((item) => next.includes(item)));
              }}
              type="checkbox"
            />
            <span>{entrypoint}</span>
          </label>
        );
      })}
    </div>
  );
}

function BackendEditorForm({
  draft,
  disabled,
  onCancel,
  onChange,
  onSave
}: {
  draft: BackendDraft;
  disabled: boolean;
  onCancel(): void;
  onChange(draft: BackendDraft): void;
  onSave(draft: BackendDraft): void;
}) {
  const commandConfig = parseBackendCommandJson(draft.commandJsonText);
  const commandJsonError = draft.commandJsonText.trim() ? commandConfig.error : null;
  const canSave = Boolean(draft.id.trim() && commandConfig.command.trim() && !commandConfig.error);
  function patch(patch: Partial<BackendDraft>) {
    onChange({ ...draft, ...patch });
  }
  function toggleClientCapability(value: string) {
    const current = draft.clientCapabilities;
    patch({
      clientCapabilities: current.includes(value)
        ? current.filter((item) => item !== value)
        : [...current, value]
    });
  }
  return (
    <form
      aria-label="Profile ACP backend"
      className="backendEditor"
      onSubmit={(event) => {
        event.preventDefault();
        if (canSave && !disabled) {
          onSave(draft);
        }
      }}
    >
      <header>
        <div className="workspaceDialogTitle">
          <PlugZap size={18} aria-hidden />
          <h4>Backend details</h4>
        </div>
        <button aria-label="Close backend editor" onClick={onCancel} title="Close" type="button">
          <X size={15} />
        </button>
      </header>
        <label>
          <span>ID</span>
          <input aria-label="ID" disabled={disabled} onChange={(event) => patch({ id: event.currentTarget.value })} value={draft.id} />
        </label>
        <label>
          <span>Label <em>Optional</em></span>
          <input aria-label="Label" disabled={disabled} onChange={(event) => patch({ label: event.currentTarget.value })} value={draft.label} />
        </label>
        <label>
          <span>Description <em>Optional</em></span>
          <input aria-label="Description" disabled={disabled} onChange={(event) => patch({ description: event.currentTarget.value })} value={draft.description} />
        </label>
        <label>
          <span>Command JSON</span>
          <textarea
            aria-describedby={commandJsonError ? "backend-command-json-error" : undefined}
            aria-invalid={commandJsonError ? true : undefined}
            aria-label="Command JSON"
            className="backendJsonInput"
            disabled={disabled}
            onChange={(event) => patch({ commandJsonText: event.currentTarget.value })}
            spellCheck={false}
            value={draft.commandJsonText}
          />
          {commandJsonError && <small className="backendFieldError" id="backend-command-json-error">{commandJsonError}</small>}
        </label>
        <label>
          <span>CWD</span>
          <input
            aria-label="CWD"
            disabled={disabled}
            onChange={(event) => patch({ cwd: event.currentTarget.value })}
            placeholder="Defaults to workspace"
            value={draft.cwd}
          />
        </label>
        <fieldset className="backendDialogChecks">
          <legend>Client Capabilities</legend>
          {BACKEND_CLIENT_CAPABILITIES.map((capability) => (
            <label key={capability}>
              <input
                checked={draft.clientCapabilities.includes(capability)}
                disabled={disabled}
                onChange={() => toggleClientCapability(capability)}
                type="checkbox"
              />
              <span>{capability}</span>
            </label>
          ))}
        </fieldset>
        <label>
          <span>MCP Servers</span>
          <textarea aria-label="MCP Servers" disabled={disabled} onChange={(event) => patch({ mcpServersText: event.currentTarget.value })} value={draft.mcpServersText} />
        </label>
        <footer>
          <button disabled={disabled} onClick={onCancel} type="button">
            <X size={14} />
            Cancel
          </button>
          <button disabled={disabled || !canSave} type="submit">
            <Save size={14} />
            Save
          </button>
        </footer>
    </form>
  );
}

export function backendDraftFromBackend(backend: WorkbenchBackend): BackendDraft {
  return {
    id: backend.id,
    enabled: backend.enabled,
    label: backend.label && backend.label !== backend.id ? backend.label : "",
    description: backend.description ?? "",
    commandJsonText: backend.command ? formatBackendCommandJson({
      command: backend.command,
      args: backend.args,
      env: {}
    }) : "",
    cwd: backend.cwd && backend.cwd !== "invocation" ? backend.cwd : "",
    entrypoints: backend.entrypoints.length > 0 ? backend.entrypoints : ["peer", "subagent"],
    clientCapabilities: backend.clientCapabilities.length > 0
      ? backend.clientCapabilities
      : ["fs.read", "fs.write", "terminal"],
    mcpServersText: backend.mcpServers.join("\n")
  };
}

function formatBackendCommandJson(config: BackendCommandJson): string {
  return prettyJson({
    command: config.command,
    args: config.args,
    env: config.env
  });
}

export function parseBackendCommandJson(value: string): BackendCommandJson & { error: string | null } {
  const trimmed = value.trim();
  if (!trimmed) {
    return { command: "", args: [], env: {}, error: null };
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch {
    return { command: "", args: [], env: {}, error: "Command JSON must be valid JSON." };
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    return { command: "", args: [], env: {}, error: "Command JSON must be an object." };
  }
  const record = parsed as Record<string, unknown>;
  if (typeof record.command !== "string") {
    return { command: "", args: [], env: {}, error: "Command JSON must include a string command." };
  }
  const args = record.args === undefined ? [] : record.args;
  if (!Array.isArray(args) || !args.every((item) => typeof item === "string")) {
    return { command: "", args: [], env: {}, error: "Command JSON args must be an array of strings." };
  }
  const envValue = record.env === undefined ? {} : record.env;
  if (!envValue || typeof envValue !== "object" || Array.isArray(envValue)) {
    return { command: "", args: [], env: {}, error: "Command JSON env must be an object." };
  }
  const env: Record<string, string> = {};
  for (const [key, envItem] of Object.entries(envValue as Record<string, unknown>)) {
    if (typeof envItem !== "string") {
      return { command: "", args: [], env: {}, error: "Command JSON env values must be strings." };
    }
    if (key.trim()) {
      env[key.trim()] = envItem;
    }
  }
  return {
    command: record.command,
    args,
    env,
    error: null
  };
}



function shortSessionId(id: string): string {
  return id.length <= 12 ? id : `${id.slice(0, 8)}...`;
}
