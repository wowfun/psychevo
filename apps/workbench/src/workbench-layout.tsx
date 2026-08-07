import {
  lazy,
  Suspense,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type Dispatch,
  type MutableRefObject,
  type RefObject,
  type SetStateAction
} from "react";
import { AlertTriangle, GripVertical, MessageSquare, PanelLeft, PanelRight, Search } from "lucide-react";
import {
  ActionButton,
  Composer,
  HistoryPanel,
  IconButton,
  NavItem,
  TranscriptPanel,
  type HistoryDraftSession,
  type WorkspaceFileLinkContext
} from "@psychevo/components";
import { scopeForCwd, type GatewayClient } from "@psychevo/client";
import type { GatewayEndpoint, PsychevoHost } from "@psychevo/host";
import type {
  ContextReadResult,
  GatewayActivity,
  GatewayRequestScope,
  InitializeResult,
  ModelOptionView,
  PendingActionView,
  SessionSummary,
  SessionUsageSummaryView,
  SettingsReadResult,
  ThreadContextReadResult,
  ThreadControlDescriptorView,
  ThreadEditableInputPart,
  ThreadSnapshot,
  UsageReadResult,
  WorkspaceChangesResult,
  WorkspaceDiffResult,
  WorkspaceFilesResult
} from "@psychevo/protocol";
import { LeftUtilityRail, MainSurface, PinnedPanel } from "./app-shell";
import { CommandFeedbackView, CommandOverlayView } from "./command-overlay";
import { ComposerRequests, ComposerSubmitControls } from "./composer-controls";
import { ComposerEnvironment } from "./composer-environment";
import { WorkspacePickerDialog } from "./workspace-picker-dialog";
import { ComposerRuntimeControls } from "./runtime-controls";
import { ComposerDictationButton, ComposerVoiceOptionSwitches } from "./voice-controls";
import { rightWorkspaceTabLabel } from "./right-workspace-model";
import { DEFAULT_RIGHT_WIDTH_PX } from "./storage";
import { EMPTY_BACKEND_DRAFT, backendDraftFromBackend } from "./capabilities-agents-config";
import { confirmedSteerTurnId } from "./gateway-event-feed";
import {
  enabledThreadAction
} from "./thread-application";
import type { PendingAttachment, RightWorkspaceTab } from "./types";
import type {
  Appearance,
  BackendDraft,
  CapabilityTab,
  CommandFeedback,
  CommandOverlay,
  DebugEvent,
  MainView,
  SessionBrowserWorkspaceState,
  SettingsSection,
  TerminalNotificationEvent,
  TraceState,
  WorkbenchBackend,
  WorkbenchBackendDoctor,
  WorkbenchChannelDoctor,
  WorkbenchCommand
} from "./types";
import { DeleteSessionDialog } from "./delete-session-dialog";
import { SessionArchivePanel } from "./session-archive-panel";
import type { GatewayThreadEventFeed } from "./gateway-event-feed";
import type { ReturnTypeOfAppActions } from "./app-actions";
import type { ReturnTypeOfAutomations } from "./app-automations";
import type { ReturnTypeOfCommandActions } from "./command-actions";
import type { ReturnTypeOfRightWorkspaceActions } from "./right-workspace-actions";
import type { ReturnTypeOfSurfaceActions } from "./surface-actions";
import type { WorkbenchIntentOwner } from "./workbench-intents";

const logoUrl = new URL("../../../assets/psychevo-logo.svg", import.meta.url).href;
const RightWorkspace = lazy(async () => ({
  default: (await import("./right-workspace")).RightWorkspace
}));

type SetState<T> = Dispatch<SetStateAction<T>>;
type MobilePanel = "history" | "transcript" | "status";
type AppActions = ReturnTypeOfAppActions;
type SurfaceActions = ReturnTypeOfSurfaceActions;
type RightActions = ReturnTypeOfRightWorkspaceActions;
type AutomationModel = ReturnTypeOfAutomations;
type CommandActions = ReturnTypeOfCommandActions;

export type ThreadViewModel = {
  activeCommandOverlay: CommandOverlay | null;
  activeScope: GatewayRequestScope | null;
  activeWorkbenchCwd: string;
  activity: GatewayActivity;
  attachments: PendingAttachment[];
  changeRunnableTarget(targetId: string): Promise<void>;
  changeRuntimeControl(control: ThreadControlDescriptorView, value: unknown): Promise<void>;
  clearCommandTransientUi(): void;
  client: GatewayClient | null;
  commandFeedback: CommandFeedback;
  commands: WorkbenchCommand[];
  composerDraftPatch: {
    id: number;
    text: string;
    inputParts?: ThreadEditableInputPart[];
  } | null;
  composerPresentationReady: boolean;
  composerShellVisible: boolean;
  contextMatchesTarget: boolean;
  contextUsage: ContextReadResult | null;
  controls: SettingsReadResult["controls"] | null;
  currentThreadId: string | undefined;
  disabled: boolean;
  error: string | null;
  executeCommand: CommandActions["executeCommand"];
  fallbackCwd: string;
  handleAttachment: AppActions["handleAttachment"];
  handleAttachmentFiles: AppActions["handleAttachmentFiles"];
  init: InitializeResult | null;
  latestGatewayEvent: GatewayThreadEventFeed;
  liveTranscriptEntries: ThreadSnapshot["entries"];
  loadOlderHistory(): Promise<void>;
  onComposerRetry(): void | Promise<void>;
  onGatewayRetry(): void | Promise<void>;
  onReadAloudText(text: string): void;
  olderHistoryLoading: boolean;
  onVoiceAutoSpeakToggle(): void;
  onVoiceDictationToggle(): void;
  onVoiceRealtimeToggle(): void;
  patchComposerDraft(text: string, inputParts?: ThreadEditableInputPart[]): void;
  pendingClarifyActions: PendingActionView[];
  pendingPermissionActions: PendingActionView[];
  refreshObservability: SurfaceActions["refreshObservability"];
  runAction: SurfaceActions["runAction"];
  runCommandAlternateAction: CommandActions["runCommandAlternateAction"];
  running: boolean;
  runtimeContext: ThreadContextReadResult | null;
  runtimeControlDrafts: Record<string, unknown>;
  runtimeControls: ThreadControlDescriptorView[];
  runtimeOptionsError: string | null;
  runtimeOptionsLoading: boolean;
  runtimeProfiles: NonNullable<ThreadContextReadResult["profiles"]>;
  selectedTargetId: string;
  sessionUsage: SessionUsageSummaryView | null;
  setAttachments: SetState<PendingAttachment[]>;
  setCommandFeedback: SetState<CommandFeedback>;
  settings: SettingsReadResult | undefined;
  snapshot: ThreadSnapshot;
  startShell: AppActions["startShell"];
  status: string;
  submitTurn: AppActions["submitTurn"];
  transcriptEntries: ThreadSnapshot["entries"];
  turnBlockReason: string;
  turnSendable: boolean;
  voiceAutoSpeak: boolean;
  voiceListening: boolean;
  voiceRealtimeActive: boolean;
  workbenchIntents: WorkbenchIntentOwner;
};

export type HistoryViewModel = {
  archivedSessions: SessionSummary[];
  createWorkspace: AppActions["createWorkspace"];
  endpoint: GatewayEndpoint | null;
  historyLoading: boolean;
  host: PsychevoHost | null;
  leftCollapsed: boolean;
  loadingOlderCwd: string | null;
  loadOlderSessions(cwd: string): Promise<void>;
  pinnedSessionIds: string[];
  pinnedSessions: SessionSummary[];
  refreshHistory: SurfaceActions["refreshHistory"];
  sessionBrowserWorkspaces: SessionBrowserWorkspaceState[];
  sessions: SessionSummary[];
  setDraftSession: SetState<HistoryDraftSession | null>;
  setLeftCollapsed: SetState<boolean>;
  startNewThread: AppActions["startNewThread"];
  switchMainView(value: MainView): void;
  togglePinnedSession(threadId: string): void;
  workspaceDialogOpen: boolean;
  setWorkspaceDialogOpen: SetState<boolean>;
};

export type WorkspaceViewModel = {
  acceptWorkspaceChange: AppActions["acceptWorkspaceChange"];
  activeRightTab: RightWorkspaceTab | null;
  activeRightTabId: string | null;
  beginRightResize: RightActions["beginRightResize"];
  clearRightWorkspaceTabPendingPrompt: RightActions["clearRightWorkspaceTabPendingPrompt"];
  closeRightWorkspaceTab: RightActions["closeRightWorkspaceTab"];
  copyText: AppActions["copyText"];
  debugEnabled: boolean;
  debugEvents: DebugEvent[];
  openDiffPreview: AppActions["openDiffPreview"];
  openAgentSessionTab: RightActions["openAgentSessionTab"];
  openFilePreview: AppActions["openFilePreview"];
  openRightWorkspaceTab: RightActions["openRightWorkspaceTab"];
  pinnedMessageKeys: ReadonlySet<string>;
  readWorkspaceFolders: AppActions["readWorkspaceFolders"];
  readWorkspaceGitBranches: AppActions["readWorkspaceGitBranches"];
  checkoutWorkspaceGitBranch: AppActions["checkoutWorkspaceGitBranch"];
  refreshAgentSurface(
    nextClient?: GatewayClient | null,
    scope?: GatewayRequestScope
  ): Promise<void>;
  refreshSnapshot: SurfaceActions["refreshSnapshot"];
  refreshTrace: SurfaceActions["refreshTrace"];
  refreshWorkspaceSurface: SurfaceActions["refreshWorkspaceSurface"];
  rejectWorkspaceChange: AppActions["rejectWorkspaceChange"];
  revealRightWorkspace: RightActions["revealRightWorkspace"];
  rightCollapsed: boolean;
  rightTabs: RightWorkspaceTab[];
  rightWidthPx: number;
  saveFileFromEditor: AppActions["saveFileFromEditor"];
  setActiveRightTabId: SetState<string | null>;
  setDirtyRightTabs: SetState<Record<string, boolean>>;
  setError: SetState<string | null>;
  setRightCollapsed: SetState<boolean>;
  setRightTabs: SetState<RightWorkspaceTab[]>;
  setRightWidthPx: SetState<number>;
  terminalEvents: TerminalNotificationEvent[];
  traceState: TraceState;
  togglePinnedMessage: RightActions["togglePinnedMessage"];
  workspaceBranch: string | null | undefined;
  workspaceIsGitRepo: boolean | undefined;
  workspaceChanges: WorkspaceChangesResult | null;
  workspaceDiff: WorkspaceDiffResult | null;
  workspaceFiles: WorkspaceFilesResult | null;
};

export type CapabilityViewModel = {
  appearance: Appearance;
  automations: AutomationModel["automations"];
  automationsError: AutomationModel["automationsError"];
  automationsLoading: AutomationModel["automationsLoading"];
  backendDoctor: Record<string, WorkbenchBackendDoctor>;
  backendDraft: BackendDraft | null;
  backends: WorkbenchBackend[];
  capabilitiesTab: CapabilityTab;
  channelDoctor: Record<string, WorkbenchChannelDoctor>;
  deleteAutomation: AutomationModel["deleteAutomation"];
  deleteBackend: AppActions["deleteBackend"];
  deleteChannel: AppActions["deleteChannel"];
  doctorBackend: AppActions["doctorBackend"];
  doctorChannel: AppActions["doctorChannel"];
  doctorChannels: AppActions["doctorChannels"];
  draftAutomation: AutomationModel["draftAutomation"];
  loadChannelSources: AppActions["loadChannelSources"];
  loadThreadSearchText: AppActions["loadThreadSearchText"];
  mainView: MainView;
  mobilePanel: MobilePanel;
  onModelAssignmentSaved(): Promise<void>;
  onModelCatalogLoaded(options: ModelOptionView[]): void;
  openAutomationThread: AutomationModel["openAutomationThread"];
  openCapabilitiesTab(tab?: CapabilityTab): void;
  openSettingsSection(section: SettingsSection): void;
  pauseAutomation: AutomationModel["pauseAutomation"];
  pollWechatQrSetup: AppActions["pollWechatQrSetup"];
  refreshAutomations: AutomationModel["refreshAutomations"];
  refreshUsageStats(nextClient?: GatewayClient | null): Promise<void>;
  resumeAutomation: AutomationModel["resumeAutomation"];
  runAutomation: AutomationModel["runAutomation"];
  saveAutomation: AutomationModel["saveAutomation"];
  saveBackendDraft: AppActions["saveBackendDraft"];
  setAppearance: SetState<Appearance>;
  setBackendDraft: SetState<BackendDraft | null>;
  setCapabilitiesTab: SetState<CapabilityTab>;
  setChannelEnabled: AppActions["setChannelEnabled"];
  setDebugEnabled: SetState<boolean>;
  setMobilePanel: SetState<MobilePanel>;
  setSettingsSection: SetState<SettingsSection>;
  settingsSection: SettingsSection;
  showSessionChrome: boolean;
  startWechatQrSetup: AppActions["startWechatQrSetup"];
  updateBackendDraftFields: AppActions["updateBackendDraftFields"];
  updateChannel: AppActions["updateChannel"];
  updateMainView(value: MainView): void;
  usageStats: UsageReadResult | null;
  usageStatsError: string | null;
  usageStatsLoading: boolean;
};

export type WorkbenchLayoutProps = {
  capabilities: CapabilityViewModel;
  history: HistoryViewModel;
  thread: ThreadViewModel;
  workspace: WorkspaceViewModel;
};

export function WorkbenchLayout(props: WorkbenchLayoutProps) {
  const groupedProps = {
    ...props.capabilities,
    ...props.history,
    ...props.thread,
    ...props.workspace
  };
  const {
    activeCommandOverlay,
    activeRightTab,
    activeRightTabId,
    activeScope,
    activeWorkbenchCwd,
    activity,
    appearance,
    archivedSessions,
    automations,
    automationsError,
    automationsLoading,
    attachments,
    backendDoctor,
    backendDraft,
    backends,
    beginRightResize,
    capabilitiesTab,
    changeRuntimeControl,
    changeRunnableTarget,
    clearRightWorkspaceTabPendingPrompt,
    closeRightWorkspaceTab,
    channelDoctor,
    client,
    commandFeedback,
    commands,
    composerDraftPatch,
    composerPresentationReady,
    composerShellVisible,
    contextUsage,
    controls,
    copyText,
    checkoutWorkspaceGitBranch,
    createWorkspace,
    currentThreadId,
    debugEnabled,
    debugEvents,
    deleteAutomation,
    deleteBackend,
    deleteChannel,
    disabled,
    doctorBackend,
    doctorChannel,
    doctorChannels,
    draftAutomation,
    endpoint,
    error,
    executeCommand,
    fallbackCwd,
    handleAttachment,
    handleAttachmentFiles,
    host,
    historyLoading,
    init,
    leftCollapsed,
    latestGatewayEvent,
    liveTranscriptEntries,
    loadOlderHistory,
    loadingOlderCwd,
    loadChannelSources,
    loadOlderSessions,
    loadThreadSearchText,
    mainView,
    mobilePanel,
    turnSendable,
    turnBlockReason,
    openDiffPreview,
    openCapabilitiesTab,
    openAgentSessionTab,
    openAutomationThread,
    openFilePreview,
    openRightWorkspaceTab,
    openSettingsSection,
    onModelAssignmentSaved,
    onModelCatalogLoaded,
    olderHistoryLoading,
    pendingClarifyActions,
    pendingPermissionActions,
    patchComposerDraft,
    pinnedSessionIds,
    pinnedSessions,
    pinnedMessageKeys,
    pauseAutomation,
    pollWechatQrSetup,
    refreshAutomations,
    refreshAgentSurface,
    refreshHistory,
    refreshObservability,
    refreshSnapshot,
    refreshTrace,
    refreshUsageStats,
    refreshWorkspaceSurface,
    readWorkspaceFolders,
    readWorkspaceGitBranches,
    rejectWorkspaceChange,
    resumeAutomation,
    rightCollapsed,
    rightTabs,
    rightWidthPx,
    revealRightWorkspace,
    runAction,
    runAutomation,
    runCommandAlternateAction,
    running,
    runtimeContext,
    runtimeControls,
    runtimeControlDrafts,
    runtimeOptionsLoading,
    runtimeOptionsError,
    runtimeProfiles,
    saveBackendDraft,
    saveAutomation,
    saveFileFromEditor,
    selectedTargetId,
    workspaceBranch,
    workspaceIsGitRepo,
    contextMatchesTarget,
    sessionBrowserWorkspaces,
    sessionUsage,
    sessions,
    setActiveRightTabId,
    setAppearance,
    setAttachments,
    setBackendDraft,
    setCapabilitiesTab,
    setChannelEnabled,
    setDebugEnabled,
    setDirtyRightTabs,
    setDraftSession,
    setError,
    setLeftCollapsed,
    setMobilePanel,
    setRightCollapsed,
    setRightTabs,
    setRightWidthPx,
    setCommandFeedback,
    setSettingsSection,
    setWorkspaceDialogOpen,
    settings,
    settingsSection,
    showSessionChrome,
    snapshot,
    startNewThread,
    startShell,
    startWechatQrSetup,
    status,
    submitTurn,
    switchMainView,
    terminalEvents,
    togglePinnedSession,
    traceState,
    transcriptEntries,
    togglePinnedMessage,
    voiceAutoSpeak,
    voiceListening,
    voiceRealtimeActive,
    workbenchIntents,
    updateBackendDraftFields,
    updateChannel,
    updateMainView,
    usageStats,
    usageStatsError,
    usageStatsLoading,
    workspaceChanges,
    workspaceDialogOpen,
    workspaceDiff,
    workspaceFiles,
    acceptWorkspaceChange,
    clearCommandTransientUi,
    onReadAloudText,
    onVoiceAutoSpeakToggle,
    onVoiceDictationToggle,
    onVoiceRealtimeToggle,
    onGatewayRetry,
    onComposerRetry
  } = groupedProps;

  const workspaceFileLinks: WorkspaceFileLinkContext | undefined = workspaceFiles
    ? {
        entries: workspaceFiles.entries,
        onOpen: (path) => runAction(async () => openFilePreview(path, { hideFileTree: true })),
        root: workspaceFiles.root
      }
    : undefined;
  const selectedRuntimeRef = runtimeContext?.compatibleTargets?.find((target) => (
    target.targetId === selectedTargetId
  ))?.runtimeProfileRef ?? null;
  const selectedRuntimeProfile = runtimeProfiles.find((profile) => (
    profile.id === selectedRuntimeRef
  )) ?? null;
  const runtimeSafetyParts = [selectedRuntimeProfile?.sandbox]
    .filter((value): value is string => typeof value === "string" && Boolean(value.trim()));
  const runtimeSafetyLabel = runtimeSafetyParts.length > 0
    ? ["Profile safety", ...runtimeSafetyParts].join(" · ")
    : null;
  const activeRuntimeControls = runtimeControls ?? [];
  const modelControl = activeRuntimeControls.find((control) => control.surfaceRole === "model") ?? null;
  const reasoningControl = activeRuntimeControls.find((control) => control.surfaceRole === "reasoning") ?? null;
  const inputCapabilities = contextMatchesTarget ? runtimeContext?.inputCapabilities ?? [] : [];
  const textCapability = inputCapabilities.find((capability) => capability.kind === "text") ?? null;
  const promptTextUnavailableReason = !currentThreadId && runtimeOptionsLoading
    ? null
    : textCapability?.enabled
    ? null
    : textCapability?.unavailableReason ?? turnBlockReason;
  const attachmentCapabilities = inputCapabilities.filter((capability) => (
    capability.kind === "image"
    || capability.kind === "resource"
    || capability.kind === "embeddedContext"
  ));
  const attachmentsEnabled = attachmentCapabilities.some((capability) => capability.enabled);
  const attachmentUnavailableReason = attachmentsEnabled
    ? null
    : attachmentCapabilities.find((capability) => capability.unavailableReason)?.unavailableReason
      ?? turnBlockReason;
  const steerTurnId = confirmedSteerTurnId(
    latestGatewayEvent,
    snapshot.thread?.id ?? null,
    activity.activeTurnId ?? null
  );
  const steerAvailable = Boolean(steerTurnId)
    && contextMatchesTarget
    && enabledThreadAction(runtimeContext, "steer") !== null;
  const historyEditAvailable = enabledThreadAction(runtimeContext, "revertConversation") !== null;
  const pointForkAvailable = enabledThreadAction(runtimeContext, "forkBefore") !== null;
  const forkSource = snapshot.thread?.forkedFromThreadId
    ? [...sessions, ...archivedSessions].find((session) => (
        session.id === snapshot.thread?.forkedFromThreadId
      )) ?? null
    : null;
  const [sessionArchiveView, setSessionArchiveView] = useState(false);
  const [pendingDeleteSession, setPendingDeleteSession] = useState<SessionSummary | null>(null);
  const [deleteSessionPending, setDeleteSessionPending] = useState(false);
  const importScope = activeScope ?? init?.scope ?? scopeForCwd(activeWorkbenchCwd);
  const draftSession = showSessionChrome && !currentThreadId;
  const composerInteractionDisabled = disabled || !composerPresentationReady;
  const composerJourneyState = currentThreadId
    ? "bound"
    : runtimeOptionsError
      ? "blocked"
      : !composerPresentationReady || runtimeOptionsLoading || !contextMatchesTarget
        ? "opening"
        : turnSendable
          ? "ready"
          : "blocked";
  const composerDockRef = useRef<HTMLDivElement | null>(null);
  useComposerDockTransition(composerDockRef, draftSession, composerPresentationReady);

  return (
    <main
      className="appShell"
      data-composer-state={composerJourneyState}
      data-gateway-status={status}
      data-main-view={mainView}
      data-turn-state={running ? "running" : "idle"}
    >
      {error && (
        <div className="errorBand" role="alert">
          <AlertTriangle size={17} aria-hidden />
          <span>{error}</span>
        </div>
      )}
      {["reconnecting", "recovering", "recovery-error"].includes(status) && (
        <div className="connectionBand" role="status">
          <span>{status === "reconnecting"
            ? "Connection interrupted. Reconnecting…"
            : status === "recovering"
              ? "Refreshing authoritative Thread state…"
              : "Thread recovery failed. Retry to refresh authoritative state."}</span>
          {status !== "recovering" && (
            <ActionButton
              onClick={() => void onGatewayRetry?.()}
              size="compact"
              type="button"
            >
              Retry now
            </ActionButton>
          )}
        </div>
      )}
      {workspaceDialogOpen && (
        <WorkspacePickerDialog
          ariaLabel="Open workspace"
          disabled={disabled}
          onCancel={() => setWorkspaceDialogOpen(false)}
          onCreate={async (parent, name) => {
            await createWorkspace(name, parent);
            setWorkspaceDialogOpen(false);
          }}
          onOpen={async (cwd) => {
            await startNewThread(cwd);
            setWorkspaceDialogOpen(false);
          }}
          onReadFolders={readWorkspaceFolders}
          title="Open workspace"
        />
      )}
      {pendingDeleteSession && (
        <DeleteSessionDialog
          disabled={deleteSessionPending}
          onCancel={() => setPendingDeleteSession(null)}
          onConfirm={() => void runAction(async () => {
            setDeleteSessionPending(true);
            try {
              await workbenchIntents.deleteSession(pendingDeleteSession);
              setPendingDeleteSession(null);
            } finally {
              setDeleteSessionPending(false);
            }
          })}
          session={pendingDeleteSession}
        />
      )}
      <nav className="mobileTabs" aria-label="Workbench panels">
        <button aria-current={mobilePanel === "history" ? "page" : undefined} className={mobilePanel === "history" ? "is-selected" : ""} onClick={() => setMobilePanel("history")} type="button">
          <PanelLeft size={17} />
          History
        </button>
        <button aria-current={mobilePanel === "transcript" ? "page" : undefined} className={mobilePanel === "transcript" ? "is-selected" : ""} onClick={() => setMobilePanel("transcript")} type="button">
          <MessageSquare size={17} />
          Transcript
        </button>
        {showSessionChrome && (
          <button aria-current={mobilePanel === "status" ? "page" : undefined} className={mobilePanel === "status" ? "is-selected" : ""} onClick={() => setMobilePanel("status")} type="button">
            <PanelRight size={17} />
            {activeRightTab ? rightWorkspaceTabLabel(activeRightTab.kind) : "Status"}
          </button>
        )}
      </nav>

      <div
        className={`workbench ${leftCollapsed ? "is-leftCollapsed" : ""} ${rightCollapsed || !showSessionChrome ? "is-rightCollapsed" : ""}`}
        style={{ "--right-column-width": `${rightWidthPx}px` } as CSSProperties}
      >
        <aside className={`historyColumn ${leftCollapsed ? "is-collapsed" : ""} ${mobilePanel === "history" ? "is-mobileSelected" : ""}`} id="workbench-left-sidebar">
          <div className="leftChrome">
            <div className="leftBrandRow">
              <div className="brandMark">
                <span className="brandGlyph"><img alt="Psychevo" src={logoUrl} /></span>
                <div>
                  <h1>Psychevo</h1>
                </div>
              </div>
              <IconButton
                aria-controls="workbench-left-sidebar"
                aria-expanded={!leftCollapsed}
                className={`sidebarToggle ${leftCollapsed ? "is-logoToggle" : ""}`}
                icon={leftCollapsed ? <img alt="" aria-hidden className="sidebarToggleLogo" src={logoUrl} /> : <PanelLeft size={16} />}
                label={leftCollapsed ? "Expand left sidebar" : "Collapse left sidebar"}
                onClick={() => setLeftCollapsed((value: boolean) => !value)}
                size="compact"
              />
            </div>
            <div className="leftActions" aria-label="Session actions">
              {leftCollapsed ? (
                <IconButton icon={<MessageSquare size={16} />} label="New Session" onClick={() => void runAction(async () => startNewThread())} shape="rounded" variant="primary" />
              ) : (
                <ActionButton block className="newSessionAction" icon={<MessageSquare size={16} />} onClick={() => void runAction(async () => startNewThread())} variant="primary">New Session</ActionButton>
              )}
              {leftCollapsed ? (
                <IconButton icon={<Search size={16} />} label="Search" onClick={() => switchMainView("search")} shape="rounded" />
              ) : (
                <NavItem current={mainView === "search"} icon={<Search size={16} />} label="Search" onSelect={() => switchMainView("search")} />
              )}
            </div>
            {!leftCollapsed && (
              <>
                <PinnedPanel
                  currentThreadId={currentThreadId}
                  disabled={disabled}
                  sessions={pinnedSessions}
                  onResume={(threadId) => void runAction(async () => workbenchIntents.openThread(threadId))}
                  onUnpin={togglePinnedSession}
                />
                {sessionArchiveView ? (
                  <SessionArchivePanel
                    archivedSessions={archivedSessions}
                    client={client}
                    currentThreadId={currentThreadId ?? null}
                    disabled={disabled}
                    scope={importScope}
                    onActivateArchived={workbenchIntents.activateArchived}
                    onDeleteArchived={(session) => setPendingDeleteSession(session)}
                    onImportSession={workbenchIntents.importSession}
                    onOpenArchived={(threadId) => workbenchIntents.openThread(threadId, {
                      allowDetachedAdoption: true,
                      readOnly: true
                    })}
                    onOpenWorkspace={() => setWorkspaceDialogOpen(true)}
                    onRefreshArchived={() => refreshHistory(client, true)}
                    onShowActive={() => setSessionArchiveView(false)}
                  />
                ) : (
                <HistoryPanel
                  archived={false}
                  currentThreadId={currentThreadId}
                  disabled={disabled}
                  draftSession={null}
                  pinnedSessionIds={pinnedSessionIds}
                  browserWorkspaces={sessionBrowserWorkspaces}
                  loadingOlderCwd={loadingOlderCwd}
                  loading={historyLoading}
                  sessions={sessions}
                  onArchive={(threadId) => void runAction(async () => workbenchIntents.archiveSession(threadId))}
                  onDelete={(threadId) => void runAction(async () => {
                    const session = [...sessions, ...archivedSessions]
                      .find((candidate) => candidate.id === threadId);
                    if (session) setPendingDeleteSession(session);
                  })}
                  onExport={(threadId) => {
                    if (endpoint) {
                      void host?.open.downloadSession(endpoint, threadId, "export");
                    }
                  }}
                  onFork={(threadId) => void runAction(async () => workbenchIntents.forkSession(threadId))}
                  onImportSessions={() => setSessionArchiveView(true)}
                  onNew={() => void runAction(async () => {
                    await startNewThread();
                  })}
                  onCreateWorkspace={() => setWorkspaceDialogOpen(true)}
                  onNewInCwd={(cwd) => void runAction(async () => {
                    await startNewThread(cwd);
                  })}
                  onLoadOlderSessions={(cwd) => void runAction(async () => loadOlderSessions(cwd))}
                  onTogglePinned={togglePinnedSession}
                  onRename={(threadId, title) => void runAction(async () => workbenchIntents.renameSession(threadId, title))}
                  onRestore={(threadId) => void runAction(async () => workbenchIntents.restoreSession(threadId))}
                  onResumeDraft={() => {
                    switchMainView("transcript");
                    setMobilePanel("transcript");
                  }}
                  onResume={(threadId) => void runAction(async () => workbenchIntents.openThread(threadId))}
                  onShare={(threadId) => {
                    if (endpoint) {
                      void host?.open.downloadSession(endpoint, threadId, "share");
                    }
                  }}
                />
                )}
              </>
            )}
            <LeftUtilityRail
              value={mainView}
              onChange={(value) => {
                if (value === "settings") {
                  openSettingsSection(settingsSection);
                } else {
                  switchMainView(value);
                  setMobilePanel("transcript");
                }
              }}
            />
          </div>
        </aside>

        <section className={`conversationColumn ${mobilePanel === "transcript" ? "is-mobileSelected" : ""} ${draftSession ? "is-draftSession" : ""}`}>
          <div className="conversationChrome">
            {snapshot.thread?.forkedFromThreadId && (
              <button
                className="forkProvenance"
                disabled={!forkSource}
                onClick={() => void runAction(async () => {
                  if (!forkSource) return;
                  await workbenchIntents.openThread(forkSource.id);
                })}
                title={forkSource ? "Open source thread" : `Source thread ${snapshot.thread.forkedFromThreadId} is unavailable`}
                type="button"
              >
                Forked from {forkSource?.displayTitle ?? forkSource?.title ?? snapshot.thread.forkedFromThreadId.slice(0, 8)}
              </button>
            )}
            {showSessionChrome && (
              <IconButton
                aria-controls="workbench-right-inspector"
                aria-expanded={!rightCollapsed}
                className="rightInspectorToggle"
                icon={<PanelRight size={16} />}
                label="Right inspector"
                onClick={() => setRightCollapsed((value: boolean) => !value)}
                size="compact"
              />
            )}
          </div>
          <div className="centerWorkspace">
            <MainSurface
              appearance={appearance}
              automations={automations}
              automationsError={automationsError}
              automationsLoading={automationsLoading}
              backendDraft={backendDraft}
              backendDoctor={backendDoctor}
              backends={backends}
              capabilitiesTab={capabilitiesTab}
              channelDoctor={channelDoctor}
              channels={settings?.channels?.channels ?? []}
              client={client}
              controls={controls}
              currentThreadId={currentThreadId ?? null}
              debugEnabled={debugEnabled}
              disabled={disabled}
              mainView={mainView}
              runtimeProfiles={runtimeProfiles}
              scope={activeScope ?? init?.scope ?? null}
              sessions={sessions}
              settingsSection={settingsSection}
              sessionBrowserWorkspaces={sessionBrowserWorkspaces}
              usageStats={usageStats}
              usageStatsError={usageStatsError}
              usageStatsLoading={usageStatsLoading}
              cwd={activeWorkbenchCwd}
              loadThreadSearchText={loadThreadSearchText}
              onCopyText={copyText}
              onAppearanceChange={setAppearance}
              onAgentSurfaceChanged={() => refreshAgentSurface()}
              onDeleteAutomation={(id) => deleteAutomation(id)}
              onDraftAutomation={(params) => draftAutomation(params)}
              onDebugChange={setDebugEnabled}
              onCancelBackendEdit={() => setBackendDraft(null)}
              onChangeBackendDraft={setBackendDraft}
              onDeleteBackend={(backend) => void runAction(async () => deleteBackend(backend))}
              onDeleteChannel={(channel) => deleteChannel(channel)}
              onDoctorBackend={(backend) => void runAction(async () => doctorBackend(backend))}
              onDoctorChannel={(channel) => void runAction(async () => doctorChannel(channel))}
              onDoctorChannels={() => void runAction(async () => doctorChannels())}
              onEditBackend={(backend) => setBackendDraft(backendDraftFromBackend(backend))}
              onCapabilitiesTabChange={setCapabilitiesTab}
              onLoadChannelSources={(channel) => loadChannelSources(channel)}
              onPollWechatQrSetup={(sessionId) => pollWechatQrSetup(sessionId)}
              onSetChannelEnabled={(channel, enabled) => void runAction(async () => setChannelEnabled(channel, enabled))}
              onSetBackendEnabled={(backend, enabled) => void runAction(async () => updateBackendDraftFields(backend, { enabled }))}
              onSetBackendEntrypoints={(backend, entrypoints) => void runAction(async () => updateBackendDraftFields(backend, { entrypoints }))}
              onSlashSettingsSaved={() => refreshAgentSurface()}
              onStartWechatQrSetup={() => startWechatQrSetup()}
              onUpdateChannel={(channel, draft) => updateChannel(channel, draft)}
              onMainViewChange={switchMainView}
              onModelAssignmentSaved={onModelAssignmentSaved}
              onModelCatalogLoaded={onModelCatalogLoaded}
              onNewBackend={() => {
                openCapabilitiesTab("agents");
                setBackendDraft({ ...EMPTY_BACKEND_DRAFT });
              }}
              onOpenSession={(threadId, readOnly = false) => void runAction(async () => (
                workbenchIntents.openThread(threadId, {
                  allowDetachedAdoption: readOnly,
                  readOnly
                })
              ))}
              onOpenAutomationThread={openAutomationThread}
              onSettingsSectionChange={setSettingsSection}
              onSaveBackendDraft={(draft) => void runAction(async () => saveBackendDraft(draft))}
              onSaveAutomation={(params) => saveAutomation(params)}
              onPauseAutomation={(id) => pauseAutomation(id)}
              onRefreshAutomations={() => refreshAutomations()}
              onResumeAutomation={(id) => resumeAutomation(id)}
              onRunAutomation={(id) => runAutomation(id)}
              onRefreshUsageStats={() => void runAction(async () => refreshUsageStats())}
              transcript={(
                <TranscriptPanel
                  activity={activity}
                  entries={transcriptEntries}
                  history={snapshot.history}
                  liveEntries={liveTranscriptEntries}
                  onLoadOlderHistory={() => void runAction(loadOlderHistory)}
                  onCopyText={copyText}
                  {...(historyEditAvailable && pointForkAvailable ? {
                    onReadUserMessageDraft: workbenchIntents.readUserMessageDraft,
                    onUpdateUserMessage: workbenchIntents.updateUserMessage,
                    onForkUserMessage: workbenchIntents.forkUserMessage
                  } : {})}
                  onOpenAgentSession={openAgentSessionTab}
                  onPinnedMessageChange={(message, pinned) => togglePinnedMessage(
                    message,
                    pinnedMessageSourceTitle(message.threadId, sessions),
                    pinned
                  )}
                  pinnedMessageKeys={pinnedMessageKeys}
                  threadId={snapshot.thread?.id ?? null}
                  onReadAloudText={onReadAloudText}
                  olderHistoryLoading={olderHistoryLoading}
                  {...(workspaceFileLinks ? { workspaceFileLinks } : {})}
                />
              )}
            />
            {showSessionChrome && activeCommandOverlay && (
              <CommandOverlayView
                commands={commands}
                feedback={commandFeedback}
                onAlternateAction={(action) => void runAction(async () => runCommandAlternateAction(action))}
                onClose={clearCommandTransientUi}
                onExecute={(slash) => void runAction(async () => executeCommand(slash, "commandOverlay"))}
              />
            )}
          </div>
          {showSessionChrome && composerShellVisible && <div
            aria-busy={!composerPresentationReady}
            className="composerDock"
            ref={composerDockRef}
          >
            {!composerPresentationReady && runtimeOptionsError && (
              <div className="composerPreparingError" role="alert">
                <span>{runtimeOptionsError}</span>
                <ActionButton
                  disabled={runtimeOptionsLoading}
                  onClick={() => void runAction(async () => onComposerRetry?.())}
                  size="compact"
                  type="button"
                  variant="caution"
                >
                  Retry
                </ActionButton>
              </div>
            )}
            {snapshot.historyEditing?.kind === "conversationEdit" && (
              <div className="historyEditingStrip" role="status">
                <span>{snapshot.historyEditing.hiddenEntryCount} hidden {snapshot.historyEditing.hiddenEntryCount === 1 ? "entry" : "entries"}</span>
                <ActionButton onClick={() => void runAction(async () => {
                  await workbenchIntents.restoreEditedHistory();
                })} size="compact" type="button" variant="caution">
                  Restore history
                </ActionButton>
              </div>
            )}
            {(commandFeedback?.feedbackAnchor === "composer" || commandFeedback?.feedbackAnchor === "status") && (
              <CommandFeedbackView
                className="composerCommandFeedback"
                feedback={commandFeedback}
                onAlternateAction={(action) => void runAction(async () => runCommandAlternateAction(action))}
              />
            )}
            <Composer
              attachmentUnavailableReason={attachmentUnavailableReason}
              attachments={attachments}
              completionProvider={workbenchIntents.completion}
              disabled={composerInteractionDisabled}
              draftPatch={composerDraftPatch ?? undefined}
              placeholder={composerPresentationReady ? "Ask Psychevo..." : "Preparing runtime environment…"}
              leftControls={(
                <>
                  <ComposerRuntimeControls
                    binding={runtimeContext?.binding ?? null}
                    controls={activeRuntimeControls}
                    profiles={runtimeContext?.profiles ?? []}
                    targets={runtimeContext?.compatibleTargets ?? []}
                    controlValues={runtimeControlDrafts}
                    disabled={composerInteractionDisabled}
                    targetId={selectedTargetId}
                    contextError={runtimeOptionsError}
                    contextLoading={runtimeOptionsLoading}
                    preparing={!composerPresentationReady}
                    onTargetChange={(value) => void runAction(async () => changeRunnableTarget(value))}
                    onControlChange={(control, value) => void runAction(async () => changeRuntimeControl(control, value))}
                  />
                </>
              )}
              addMenuOptions={(
                <ComposerVoiceOptionSwitches
                  autoSpeak={Boolean(voiceAutoSpeak)}
                  disabled={composerInteractionDisabled}
                  realtimeActive={Boolean(voiceRealtimeActive)}
                  onToggleAutoSpeak={onVoiceAutoSpeakToggle}
                  onToggleRealtime={onVoiceRealtimeToggle}
                />
              )}
              mode="default"
              modeControlVisible={false}
              planModeAvailable={false}
              preActionControls={(
                <ComposerDictationButton
                  disabled={composerInteractionDisabled}
                  listening={Boolean(voiceListening)}
                  onToggle={onVoiceDictationToggle}
                />
              )}
              promptSubmitBlockReason={turnBlockReason}
              promptSubmitDisabled={!turnSendable || !composerPresentationReady}
              promptTextUnavailableReason={promptTextUnavailableReason}
              retainDraftUntilAccepted
              rightControls={(
                <>
                  <ComposerSubmitControls
                    context={contextUsage}
                    controls={controls}
                    usage={sessionUsage}
                    controlValues={runtimeControlDrafts}
                    disabled={composerInteractionDisabled || runtimeOptionsLoading}
                    modelControl={modelControl}
                    reasoningControl={reasoningControl}
                    onContextOpen={() => void refreshObservability(
                      client,
                      activeScope ?? init?.scope,
                      currentThreadId ?? null
                    )}
                    onControlChange={(control, value) => void runAction(async () => changeRuntimeControl(control, value))}
                  />
                </>
              )}
              requestPanel={(pendingClarifyActions.length > 0 || pendingPermissionActions.length > 0) ? (
                <ComposerRequests
                  clarifies={pendingClarifyActions}
                  permissions={pendingPermissionActions}
                  onClarify={(request, answers, cancel) => void runAction(async () => workbenchIntents.respondClarify(request, answers, cancel))}
                  onPermission={(request, decision, directory) => void runAction(async () => workbenchIntents.respondPermission(request, decision, directory))}
                />
              ) : null}
              running={running}
              runningStartedAtMs={activity.startedAtMs ?? null}
              steerAvailable={steerAvailable}
              {...(attachmentsEnabled ? {
                onAttach: () => void runAction(async () => handleAttachment()),
                onAttachFiles: (files: File[]) => void runAction(async () => handleAttachmentFiles(files))
              } : {})}
              onCommand={(command) => void runAction(async () => executeCommand(command, "composer"))}
              onInterrupt={() => void runAction(workbenchIntents.interrupt)}
              onModeChange={() => {}}
              onRemoveAttachment={(id) => setAttachments((current: PendingAttachment[]) => current.filter((attachment) => attachment.id !== id))}
              onShell={(command) => void runAction(async () => startShell(command))}
              onSteer={(text) => void runAction(async () => workbenchIntents.steer(text))}
              onSubmit={(text, mentions, orderedInput, isInputCurrent) => runAction(
                async () => submitTurn(text, mentions, undefined, orderedInput, isInputCurrent)
              ).then((accepted: unknown) => accepted === true)}
            />
            <ComposerEnvironment
              branch={workspaceBranch !== undefined
                ? workspaceBranch
                : settings?.project?.branch ?? null}
              branchDisabled={running}
              isGitRepo={workspaceIsGitRepo}
              controlValues={runtimeControlDrafts}
              controls={activeRuntimeControls}
              cwd={activeWorkbenchCwd}
              disabled={composerInteractionDisabled || runtimeOptionsLoading}
              draft={draftSession}
              preparing={!composerPresentationReady}
              path={settings?.cwd === activeWorkbenchCwd
                ? settings?.project?.displayPath ?? activeWorkbenchCwd
                : init?.scope.cwd === activeWorkbenchCwd
                  ? init.displayCwd
                  : sessionBrowserWorkspaces.find((workspace) => workspace.cwd === activeWorkbenchCwd)?.displayPath
                    ?? activeWorkbenchCwd}
              runtimeSafetyLabel={runtimeSafetyLabel}
              profile={init?.profile ?? null}
              workspaces={sessionBrowserWorkspaces}
              onBranchChange={(nextBranch, create) => checkoutWorkspaceGitBranch(nextBranch, create)}
              onOpenFiles={() => openRightWorkspaceTab("files", { fileTreeOpen: true })}
              onReadBranches={() => readWorkspaceGitBranches()}
              onReadFolders={(folderPath) => readWorkspaceFolders(folderPath)}
              onRuntimeControlChange={(control, value) => void runAction(async () => changeRuntimeControl(control, value))}
              onWorkspaceChange={(cwd) => startNewThread(cwd)}
            />
          </div>}
        </section>

        {showSessionChrome && !rightCollapsed && (
          <aside className={`statusColumn ${mobilePanel === "status" ? "is-mobileSelected" : ""}`} id="workbench-right-inspector">
            <button
              aria-label="Resize right workspace"
              className="rightResizeHandle"
              onDoubleClick={() => setRightWidthPx(DEFAULT_RIGHT_WIDTH_PX)}
              onPointerDown={(event) => beginRightResize(event)}
              title="Resize right workspace"
              type="button"
            >
              <GripVertical size={15} />
            </button>
            <Suspense fallback={<div className="rightPanelLoading" role="status">Loading workspace…</div>}>
              <RightWorkspace
                activeTabId={activeRightTabId}
                activity={activity}
                appearance={appearance}
                client={client}
                context={contextUsage}
                debugEnabled={debugEnabled}
                debugEvents={debugEvents}
                files={workspaceFiles?.entries ?? []}
                hostKind={host?.platform?.kind ?? "browser"}
                latestGatewayEvent={latestGatewayEvent}
                root={workspaceFiles?.root ?? settings?.cwd ?? ""}
                scope={activeScope ?? init?.scope ?? null}
                sessionId={snapshot.thread?.id ?? null}
                status={status}
                usage={sessionUsage}
                tabs={rightTabs}
                terminalEvents={terminalEvents}
                trace={traceState}
                truncated={workspaceFiles?.truncated ?? false}
                cwd={settings?.project?.displayPath ?? settings?.cwd ?? ""}
                workspaceChanges={workspaceChanges}
                workspaceDiff={workspaceDiff}
                workspaceFileLinks={workspaceFileLinks}
                onActivate={setActiveRightTabId}
                onAcceptChange={(turnId, path) => void runAction(async () => acceptWorkspaceChange(turnId, path))}
                onChangedFile={(path) => void runAction(async () => openDiffPreview(path))}
                onClose={closeRightWorkspaceTab}
                onCopyText={copyText}
                onDirtyTabChange={(tabId, dirty) => {
                  setDirtyRightTabs((current: Record<string, boolean>) => current[tabId] === dirty ? current : { ...current, [tabId]: dirty });
                }}
                onFileTreeOpenChange={(tabId, open) => {
                  setRightTabs((current: RightWorkspaceTab[]) => current.map((tab) => (
                    tab.id === tabId ? { ...tab, fileTreeOpen: open } : tab
                  )));
                }}
                onOpenFile={(path) => void runAction(async () => openFilePreview(path))}
                onOpenAgentSession={openAgentSessionTab}
                onBrowserStateChange={(tabId, browser) => {
                  setRightTabs((current: RightWorkspaceTab[]) => current.map((tab) => (
                    tab.id === tabId ? { ...tab, browser } : tab
                  )));
                }}
                onOpenExternal={(url) => void runAction(async () => {
                  const result = await host?.open.openExternal(url);
                  if (!result?.ok) {
                    setError(result?.message ?? "Open externally is not supported by this host.");
                  }
                })}
                onOpenKind={(kind) => {
                  if (kind === "sideConversation") {
                    void runAction(async () => executeCommand("/btw", "commandsPanel"));
                    return;
                  }
                  openRightWorkspaceTab(
                    kind,
                    kind === "files" ? { fileTreeOpen: true } : {},
                    kind !== "browser"
                  );
                }}
                onOpenPreview={(preview) => openRightWorkspaceTab("preview", { preview, title: preview.title }, true)}
                onPinnedMessageChange={togglePinnedMessage}
                onRejectChange={(turnId, path) => void runAction(async () => rejectWorkspaceChange(turnId, path))}
                onConsumePendingPrompt={clearRightWorkspaceTabPendingPrompt}
                onRefresh={() => void runAction(async () => {
                  await refreshSnapshot();
                  await refreshHistory();
                  await refreshAgentSurface();
                  await refreshWorkspaceSurface();
                })}
                onRefreshTrace={() => void refreshTrace()}
                onSaveFile={(path, content, expectedRevision, force) => saveFileFromEditor(path, content, expectedRevision, force)}
                onShowHome={() => revealRightWorkspace(null)}
                pinnedMessageKeys={pinnedMessageKeys}
              />
            </Suspense>
          </aside>
        )}
      </div>
    </main>
  );
}

function useComposerDockTransition(
  ref: RefObject<HTMLDivElement | null>,
  draftSession: boolean,
  present: boolean
) {
  const previousRectRef = useRef<DOMRect | null>(null);
  const previousDraftRef = useRef(draftSession);

  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const nextRect = element.getBoundingClientRect();
    const previousRect = previousRectRef.current;
    const stateChanged = previousDraftRef.current !== draftSession;
    const reducedMotion = typeof window.matchMedia === "function"
      && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (previousRect && stateChanged && !reducedMotion && typeof element.animate === "function") {
      const x = previousRect.left - nextRect.left;
      const y = previousRect.top - nextRect.top;
      if (Math.abs(x) > 0.5 || Math.abs(y) > 0.5) {
        element.getAnimations?.().forEach((animation) => animation.cancel());
        element.animate(
          [
            { transform: `translate(${x}px, ${y}px)` },
            { transform: "translate(0, 0)" }
          ],
          {
            duration: 360,
            easing: "cubic-bezier(0.16, 1, 0.3, 1)"
          }
        );
      }
    }
    previousRectRef.current = nextRect;
    previousDraftRef.current = draftSession;
  }, [draftSession, present, ref]);
}

function pinnedMessageSourceTitle(threadId: string, sessions: SessionSummary[]): string {
  const session = sessions.find((candidate) => candidate.id === threadId);
  return session?.displayTitle?.trim()
    || session?.title?.trim()
    || threadId.slice(0, 8)
    || "Thread";
}
