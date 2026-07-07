import { useState, type ReactNode } from "react";
import { CalendarClock, FolderPlus, Pin, Settings, Wrench, X } from "lucide-react";
import type {
  GatewayClient,
} from "@psychevo/client";
import type {
  ChannelUpdateParams,
  ChannelWechatQrPollResult,
  ChannelWechatQrStartResult,
  ModelOptionView,
  AutomationDraftParams,
  AutomationDraftView,
  AutomationWriteParams,
  GatewayRequestScope,
  SessionSummary,
  SettingsReadResult
} from "@psychevo/protocol";
import { ActionButton, CreatePanel, FormField } from "@psychevo/components";
import { AutomationsPage } from "./automations-panel";
import { CapabilitiesPage } from "./capabilities-page";
import { SearchPage } from "./search";
import { SettingsPage } from "./settings-panels";
import { shortSessionId } from "./session-utils";
import type {
  Appearance,
  BackendDraft,
  CapabilityTab,
  MainView,
  SettingsSection,
  SessionBrowserWorkspaceState,
  WorkbenchBackend,
  WorkbenchBackendDoctor,
  WorkbenchAutomation,
  WorkbenchChannel,
  WorkbenchChannelDoctor,
  WorkbenchChannelSource,
  WorkbenchUsageStats
} from "./types";

type ChannelUpdateDraft = Partial<Omit<ChannelUpdateParams, "id" | "scope">>;

export function LeftUtilityRail({
  value,
  onChange
}: {
  value: MainView;
  onChange(value: MainView): void;
}) {
  const items: Array<{ icon: ReactNode; label: string; value: MainView }> = [
    { icon: <Wrench size={16} />, label: "Capabilities", value: "capabilities" },
    { icon: <CalendarClock size={16} />, label: "Automations", value: "automations" },
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
        <CreatePanel
          icon={<FolderPlus size={18} />}
          layout="dialog"
          onClose={onCancel}
          title="New Workspace"
          footer={
            <>
              <ActionButton disabled={disabled} onClick={onCancel} variant="ghost">
                Cancel
              </ActionButton>
              <ActionButton disabled={disabled || !trimmed} type="submit" variant="primary">
                Create
              </ActionButton>
            </>
          }
        >
          <FormField label="Name">
          <input
            autoFocus
            disabled={disabled}
            onChange={(event) => setName(event.target.value)}
            placeholder="general notes"
            value={name}
          />
          </FormField>
        </CreatePanel>
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
  automations,
  automationsError,
  automationsLoading,
  archivedSessions,
  backendDraft,
  backendDoctor,
  backends,
  capabilitiesTab,
  channelDoctor,
  channels,
  client,
  controls,
  debugEnabled,
  disabled,
  currentThreadId,
  loadThreadSearchText,
  mainView,
  scope,
  onCopyText,
  onAppearanceChange,
  onDeleteAutomation,
  onAgentSurfaceChanged,
  onDraftAutomation,
  onCancelBackendEdit,
  onChangeBackendDraft,
  onDebugChange,
  onDeleteArchivedSession,
  onDeleteBackend,
  onDeleteChannel,
  onDoctorChannel,
  onDoctorChannels,
  onDoctorBackend,
  onEditBackend,
  onMainViewChange,
  onCapabilitiesTabChange,
  onOpenAutomationThread,
  onOpenCapabilitiesAgents,
  onModelAssignmentSaved,
  onModelCatalogLoaded,
  onNewBackend,
  onOpenSession,
  onPauseAutomation,
  onLoadChannelSources,
  onPollWechatQrSetup,
  onRestoreArchivedSession,
  onRefreshUsageStats,
  onRefreshAutomations,
  onResumeAutomation,
  onRunAutomation,
  onSaveBackendDraft,
  onSaveAutomation,
  onSetBackendEnabled,
  onSetBackendEntrypoints,
  onSetChannelEnabled,
  onSlashSettingsSaved,
  onSettingsSectionChange,
  onStartWechatQrSetup,
  onUpdateChannel,
  settingsSection,
  sessionBrowserWorkspaces,
  usageStats,
  usageStatsError,
  usageStatsLoading,
  sessions,
  transcript,
  cwd
}: {
  appearance: Appearance;
  automations: WorkbenchAutomation[];
  automationsError: string | null;
  automationsLoading: boolean;
  archivedSessions: SessionSummary[];
  backendDraft: BackendDraft | null;
  backendDoctor: Record<string, WorkbenchBackendDoctor>;
  backends: WorkbenchBackend[];
  capabilitiesTab: CapabilityTab;
  channelDoctor: Record<string, WorkbenchChannelDoctor>;
  channels: WorkbenchChannel[];
  client: GatewayClient | null;
  controls: SettingsReadResult["controls"];
  debugEnabled: boolean;
  disabled: boolean;
  currentThreadId: string | null;
  loadThreadSearchText(threadId: string): Promise<string>;
  mainView: MainView;
  scope: GatewayRequestScope | null;
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  onAppearanceChange(value: Appearance): void;
  onDeleteAutomation(id: string): Promise<void>;
  onAgentSurfaceChanged(): Promise<void> | void;
  onDraftAutomation(params: AutomationDraftParams): Promise<AutomationDraftView>;
  onCancelBackendEdit(): void;
  onChangeBackendDraft(draft: BackendDraft): void;
  onDebugChange(value: boolean): void;
  onDeleteArchivedSession(threadId: string): void;
  onDeleteBackend(backend: WorkbenchBackend): void;
  onDeleteChannel(channel: WorkbenchChannel): Promise<void>;
  onDoctorChannel(channel: WorkbenchChannel): void;
  onDoctorChannels(): void;
  onDoctorBackend(backend: WorkbenchBackend): void;
  onEditBackend(backend: WorkbenchBackend): void;
  onMainViewChange(value: MainView): void;
  onCapabilitiesTabChange(value: CapabilityTab): void;
  onOpenAutomationThread(threadId: string): void;
  onOpenCapabilitiesAgents(): void;
  onModelAssignmentSaved(): Promise<void>;
  onModelCatalogLoaded(options: ModelOptionView[]): void;
  onNewBackend(): void;
  onOpenSession(threadId: string): void;
  onPauseAutomation(id: string): Promise<void>;
  onLoadChannelSources(channel: WorkbenchChannel): Promise<WorkbenchChannelSource[]>;
  onPollWechatQrSetup(sessionId: string): Promise<ChannelWechatQrPollResult>;
  onRestoreArchivedSession(threadId: string): void;
  onRefreshUsageStats(): void;
  onRefreshAutomations(): Promise<void>;
  onResumeAutomation(id: string): Promise<void>;
  onRunAutomation(id: string): Promise<void>;
  onSaveBackendDraft(draft: BackendDraft): void;
  onSaveAutomation(params: AutomationWriteParams): Promise<void>;
  onSetBackendEnabled(backend: WorkbenchBackend, enabled: boolean): void;
  onSetBackendEntrypoints(backend: WorkbenchBackend, entrypoints: string[]): void;
  onSetChannelEnabled(channel: WorkbenchChannel, enabled: boolean): void;
  onSlashSettingsSaved(): Promise<void>;
  onSettingsSectionChange(value: SettingsSection): void;
  onStartWechatQrSetup(): Promise<ChannelWechatQrStartResult>;
  onUpdateChannel(channel: WorkbenchChannel, draft: ChannelUpdateDraft): Promise<WorkbenchChannel>;
  settingsSection: SettingsSection;
  sessionBrowserWorkspaces: SessionBrowserWorkspaceState[];
  usageStats: WorkbenchUsageStats | null;
  usageStatsError: string | null;
  usageStatsLoading: boolean;
  sessions: SessionSummary[];
  transcript: ReactNode;
  cwd: string;
}) {
  if (mainView === "transcript") {
    return <>{transcript}</>;
  }
  if (mainView === "settings") {
    return (
      <SettingsPage
        appearance={appearance}
        archivedSessions={archivedSessions}
        channelDoctor={channelDoctor}
        channels={channels}
        client={client}
        controls={controls}
        debugEnabled={debugEnabled}
        disabled={disabled}
        section={settingsSection}
        usageStats={usageStats}
        usageStatsError={usageStatsError}
        usageStatsLoading={usageStatsLoading}
        onAppearanceChange={onAppearanceChange}
        onDebugChange={onDebugChange}
        onDeleteArchivedSession={onDeleteArchivedSession}
        onDeleteChannel={onDeleteChannel}
        onDoctorChannel={onDoctorChannel}
        onDoctorChannels={onDoctorChannels}
        onModelAssignmentSaved={onModelAssignmentSaved}
        onModelCatalogLoaded={onModelCatalogLoaded}
        onOpenAgents={onOpenCapabilitiesAgents}
        onOpenTranscript={() => onMainViewChange("transcript")}
        onLoadChannelSources={onLoadChannelSources}
        onPollWechatQrSetup={onPollWechatQrSetup}
        onRefreshUsageStats={onRefreshUsageStats}
        onRestoreArchivedSession={onRestoreArchivedSession}
        onSectionChange={onSettingsSectionChange}
        onSetChannelEnabled={onSetChannelEnabled}
        onSlashSettingsSaved={onSlashSettingsSaved}
        onStartWechatQrSetup={onStartWechatQrSetup}
        onUpdateChannel={onUpdateChannel}
        sessionBrowserWorkspaces={sessionBrowserWorkspaces}
        cwd={cwd}
      />
    );
  }
  if (mainView === "automations") {
    return (
      <AutomationsPage
        automations={automations}
        currentThreadId={currentThreadId}
        disabled={disabled}
        error={automationsError}
        loading={automationsLoading}
        scope={scope}
        sessionBrowserWorkspaces={sessionBrowserWorkspaces}
        sessions={sessions}
        cwd={cwd}
        onDelete={onDeleteAutomation}
        onDraft={onDraftAutomation}
        onOpenSession={onOpenAutomationThread}
        onPause={onPauseAutomation}
        onRefresh={onRefreshAutomations}
        onResume={onResumeAutomation}
        onRun={onRunAutomation}
        onSave={onSaveAutomation}
      />
    );
  }
  if (mainView === "capabilities") {
    return (
      <CapabilitiesPage
        activeTab={capabilitiesTab}
        backendDraft={backendDraft}
        backendDoctor={backendDoctor}
        backends={backends}
        client={client}
        cwd={cwd}
        disabled={disabled}
        onActiveTabChange={onCapabilitiesTabChange}
        onAgentSurfaceChanged={onAgentSurfaceChanged}
        onCancelBackendEdit={onCancelBackendEdit}
        onChangeBackendDraft={onChangeBackendDraft}
        onCopyText={onCopyText}
        onDeleteBackend={onDeleteBackend}
        onDoctorBackend={onDoctorBackend}
        onEditBackend={onEditBackend}
        onNewBackend={onNewBackend}
        onSaveBackendDraft={onSaveBackendDraft}
        onSetBackendEnabled={onSetBackendEnabled}
        onSetBackendEntrypoints={onSetBackendEntrypoints}
        scope={scope}
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
