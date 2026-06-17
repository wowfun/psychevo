import { useState, type ReactNode } from "react";
import { FolderPlus, Pin, Settings, X } from "lucide-react";
import type { SessionSummary } from "@psychevo/protocol";
import { SearchPage } from "./search";
import { SettingsPage } from "./settings-panels";
import { shortSessionId } from "./session-utils";
import type {
  Appearance,
  BackendDraft,
  MainView,
  SettingsSection,
  WorkbenchBackend,
  WorkbenchBackendDoctor,
  WorkbenchUsageStats
} from "./types";

export function LeftUtilityRail({
  value,
  onChange
}: {
  value: MainView;
  onChange(value: MainView): void;
}) {
  const items: Array<{ icon: ReactNode; label: string; value: MainView }> = [
    { icon: <Settings size={16} />, label: "Settings", value: "settings" }
  ];
  return (
    <nav className="leftUtilityRail" aria-label="Workbench utilities">
      {items.map((item) => (
        <button
          className={value === item.value ? "is-selected" : ""}
          key={item.value}
          onClick={() => onChange(item.value)}
          title={item.label}
          type="button"
        >
          {item.icon}
          <span>{item.label}</span>
        </button>
      ))}
    </nav>
  );
}

export function WorkspaceCreateDialog({
  disabled,
  onCancel,
  onCreate
}: {
  disabled: boolean;
  onCancel(): void;
  onCreate(name: string): void;
}) {
  const [name, setName] = useState("");
  const trimmed = name.trim();

  return (
    <div
      className="modalBackdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onCancel();
        }
      }}
      role="presentation"
    >
      <form
        aria-label="New workspace"
        className="workspaceDialog"
        onSubmit={(event) => {
          event.preventDefault();
          if (trimmed && !disabled) {
            onCreate(trimmed);
          }
        }}
      >
        <header>
          <div className="workspaceDialogTitle">
            <FolderPlus size={18} aria-hidden />
            <h2>New Workspace</h2>
          </div>
          <button aria-label="Close" onClick={onCancel} title="Close" type="button">
            <X size={15} />
          </button>
        </header>
        <label>
          <span>Name</span>
          <input
            autoFocus
            disabled={disabled}
            onChange={(event) => setName(event.target.value)}
            placeholder="general notes"
            value={name}
          />
        </label>
        <footer>
          <button disabled={disabled} onClick={onCancel} type="button">
            Cancel
          </button>
          <button disabled={disabled || !trimmed} type="submit">
            Create
          </button>
        </footer>
      </form>
    </div>
  );
}

export function PinnedPanel({
  currentThreadId,
  disabled,
  sessions,
  onResume,
  onUnpin
}: {
  currentThreadId: string | undefined;
  disabled: boolean;
  sessions: SessionSummary[];
  onResume(threadId: string): void;
  onUnpin(threadId: string): void;
}) {
  return (
    <section className="leftPinnedPanel" aria-label="Pinned sessions">
      <header>
        <Pin size={16} />
        <span>Pinned</span>
      </header>
      {sessions.length === 0 ? (
        <p>No pinned sessions</p>
      ) : (
        <div className="pinnedSessionList">
          {sessions.map((session) => (
            <div className={`pinnedSessionRow ${session.id === currentThreadId ? "is-active" : ""}`} key={session.id}>
              <button disabled={disabled} onClick={() => onResume(session.id)} type="button">
                <span>{session.displayTitle?.trim() || session.title?.trim() || shortSessionId(session.id)}</span>
                <small>{session.project?.label ?? "workspace"}</small>
              </button>
              <button aria-label="Unpin session" disabled={disabled} onClick={() => onUnpin(session.id)} title="Unpin" type="button">
                <X size={13} />
              </button>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

export function MainSurface({
  appearance,
  archivedSessions,
  backendDraft,
  backendDoctor,
  backends,
  debugEnabled,
  disabled,
  loadThreadSearchText,
  mainView,
  onAppearanceChange,
  onCancelBackendEdit,
  onChangeBackendDraft,
  onDebugChange,
  onDeleteArchivedSession,
  onDeleteBackend,
  onDoctorBackend,
  onEditBackend,
  onMainViewChange,
  onNewBackend,
  onOpenSession,
  onRestoreArchivedSession,
  onRefreshUsageStats,
  onSaveBackendDraft,
  onSetBackendEnabled,
  onSetBackendEntrypoints,
  onSettingsSectionChange,
  settingsSection,
  usageStats,
  usageStatsError,
  usageStatsLoading,
  sessions,
  transcript,
  workdir
}: {
  appearance: Appearance;
  archivedSessions: SessionSummary[];
  backendDraft: BackendDraft | null;
  backendDoctor: Record<string, WorkbenchBackendDoctor>;
  backends: WorkbenchBackend[];
  debugEnabled: boolean;
  disabled: boolean;
  loadThreadSearchText(threadId: string): Promise<string>;
  mainView: MainView;
  onAppearanceChange(value: Appearance): void;
  onCancelBackendEdit(): void;
  onChangeBackendDraft(draft: BackendDraft): void;
  onDebugChange(value: boolean): void;
  onDeleteArchivedSession(threadId: string): void;
  onDeleteBackend(backend: WorkbenchBackend): void;
  onDoctorBackend(backend: WorkbenchBackend): void;
  onEditBackend(backend: WorkbenchBackend): void;
  onMainViewChange(value: MainView): void;
  onNewBackend(): void;
  onOpenSession(threadId: string): void;
  onRestoreArchivedSession(threadId: string): void;
  onRefreshUsageStats(): void;
  onSaveBackendDraft(draft: BackendDraft): void;
  onSetBackendEnabled(backend: WorkbenchBackend, enabled: boolean): void;
  onSetBackendEntrypoints(backend: WorkbenchBackend, entrypoints: string[]): void;
  onSettingsSectionChange(value: SettingsSection): void;
  settingsSection: SettingsSection;
  usageStats: WorkbenchUsageStats | null;
  usageStatsError: string | null;
  usageStatsLoading: boolean;
  sessions: SessionSummary[];
  transcript: ReactNode;
  workdir: string;
}) {
  if (mainView === "transcript") {
    return <>{transcript}</>;
  }
  if (mainView === "settings") {
    return (
      <SettingsPage
        appearance={appearance}
        archivedSessions={archivedSessions}
        backendDraft={backendDraft}
        backendDoctor={backendDoctor}
        backends={backends}
        debugEnabled={debugEnabled}
        disabled={disabled}
        section={settingsSection}
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
        onOpenTranscript={() => onMainViewChange("transcript")}
        onRefreshUsageStats={onRefreshUsageStats}
        onRestoreArchivedSession={onRestoreArchivedSession}
        onSaveBackendDraft={onSaveBackendDraft}
        onSectionChange={onSettingsSectionChange}
        onSetBackendEnabled={onSetBackendEnabled}
        onSetBackendEntrypoints={onSetBackendEntrypoints}
        workdir={workdir}
      />
    );
  }
  if (mainView === "search") {
    return (
      <SearchPage
        loadThreadSearchText={loadThreadSearchText}
        sessions={sessions}
        onOpenSession={onOpenSession}
        onOpenTranscript={() => onMainViewChange("transcript")}
      />
    );
  }
  return <>{transcript}</>;
}
