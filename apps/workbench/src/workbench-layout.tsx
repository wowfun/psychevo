import { lazy, Suspense, useLayoutEffect, useRef, useState, type CSSProperties, type RefObject } from "react";
import { AlertTriangle, GripVertical, MessageSquare, PanelLeft, PanelRight, Search } from "lucide-react";
import { ActionButton, Composer, HistoryPanel, TranscriptPanel, type WorkspaceFileLinkContext } from "@psychevo/components";
import { appendOptimisticPrompt, scopeForCwd } from "@psychevo/client";
import type { ThreadEditableInputPart } from "@psychevo/protocol";
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
  enabledThreadAction,
  runThreadInterrupt,
  snapshotThreadApplicationTarget
} from "./thread-application";
import type { PendingAttachment, RightWorkspaceTab } from "./types";
import { DeleteSessionDialog } from "./delete-session-dialog";
import { SessionArchivePanel } from "./session-archive-panel";

const logoUrl = new URL("../../../assets/psychevo-logo.svg", import.meta.url).href;
const RightWorkspace = lazy(async () => ({
  default: (await import("./right-workspace")).RightWorkspace
}));

export function WorkbenchLayout(props: Record<string, any>) {
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
    beginExplicitViewSwitch,
    beginRightResize,
    capabilitiesTab,
    changeRuntimeControl,
    changeRunnableTarget,
    clearRightWorkspaceTabPendingPrompt,
    channelDoctor,
    client,
    commandFeedback,
    commands,
    composerPresentationReady,
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
    onModelAssignmentSaved,
    onModelCatalogLoaded,
    pendingClarifyActions,
    pendingPermissionActions,
    patchComposerDraft,
    pinnedSessionIds,
    pinnedSessions,
    pauseAutomation,
    pollWechatQrSetup,
    refreshAutomations,
    refreshAgentSurface,
    refreshHistory,
    refreshObservability,
    refreshSnapshot,
    refreshUsageStats,
    refreshWorkspaceSurface,
    readWorkspaceFolders,
    readWorkspaceGitBranches,
    rejectWorkspaceChange,
    resumeAutomation,
    rightCollapsed,
    rightTabs,
    rightWidthPx,
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
    setLeftCollapsed,
    setMainView,
    setMobilePanel,
    setRightCollapsed,
    setRightTabs,
    setRightWidthPx,
    setCommandFeedback,
    setSettingsSection,
    setSnapshot,
    settings,
    settingsSection,
    showSessionChrome,
    snapshot,
    startNewThread,
    startShell,
    startWechatQrSetup,
    status,
    submitTurn,
    submitThreadTurn,
    switchMainView,
    terminalEvents,
    togglePinnedSession,
    traceState,
    transcriptEntries,
    voiceAutoSpeak,
    voiceListening,
    voiceRealtimeActive,
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
    onVoiceRealtimeToggle
  } = props;

  const workspaceFileLinks: WorkspaceFileLinkContext | undefined = workspaceFiles
    ? {
        entries: workspaceFiles.entries,
        onOpen: (path) => runAction(async () => openFilePreview(path)),
        root: workspaceFiles.root
      }
    : undefined;
  const selectedRuntimeRef = runtimeContext?.compatibleTargets.find((target: any) => (
    target.targetId === selectedTargetId
  ))?.runtimeProfileRef ?? null;
  const selectedRuntimeProfile = (runtimeProfiles ?? []).find((profile: any) => (
    profile.id === selectedRuntimeRef
  )) ?? null;
  const runtimeSafetyParts = [selectedRuntimeProfile?.approvalMode, selectedRuntimeProfile?.sandbox]
    .filter((value): value is string => typeof value === "string" && Boolean(value.trim()));
  const runtimeSafetyLabel = runtimeSafetyParts.length > 0
    ? ["Profile safety", ...runtimeSafetyParts].join(" · ")
    : null;
  const activeRuntimeControls = runtimeControls ?? [];
  const modelControl = activeRuntimeControls.find((control: any) => control.surfaceRole === "model") ?? null;
  const reasoningControl = activeRuntimeControls.find((control: any) => control.surfaceRole === "reasoning") ?? null;
  const inputCapabilities = contextMatchesTarget ? runtimeContext?.inputCapabilities ?? [] : [];
  const textCapability = inputCapabilities.find((capability: any) => capability.kind === "text") ?? null;
  const promptTextUnavailableReason = !currentThreadId && runtimeOptionsLoading
    ? null
    : textCapability?.enabled
    ? null
    : textCapability?.unavailableReason ?? turnBlockReason;
  const attachmentCapabilities = inputCapabilities.filter((capability: any) => (
    capability.kind === "image"
    || capability.kind === "resource"
    || capability.kind === "embeddedContext"
  ));
  const attachmentsEnabled = attachmentCapabilities.some((capability: any) => capability.enabled);
  const attachmentUnavailableReason = attachmentsEnabled
    ? null
    : attachmentCapabilities.find((capability: any) => capability.unavailableReason)?.unavailableReason
      ?? turnBlockReason;
  const agentMentionsEnabled = inputCapabilities.some((capability: any) => (
    capability.kind === "agentMention" && capability.enabled
  ));
  const steerTurnId = confirmedSteerTurnId(
    latestGatewayEvent,
    snapshot.thread?.id ?? null,
    activity.activeTurnId
  );
  const steerAvailable = Boolean(steerTurnId)
    && contextMatchesTarget
    && enabledThreadAction(runtimeContext, "steer") !== null;
  const historyEditAvailable = enabledThreadAction(runtimeContext, "revertConversation") !== null;
  const pointForkAvailable = enabledThreadAction(runtimeContext, "forkBefore") !== null;
  const forkSource = snapshot.thread?.forkedFromThreadId
    ? [...sessions, ...archivedSessions].find((session: any) => (
        session.id === snapshot.thread?.forkedFromThreadId
      )) ?? null
    : null;
  const [sessionArchiveView, setSessionArchiveView] = useState(false);
  const [pendingDeleteSession, setPendingDeleteSession] = useState<any | null>(null);
  const [deleteSessionPending, setDeleteSessionPending] = useState(false);
  const importScope = activeScope ?? init?.scope ?? scopeForCwd(activeWorkbenchCwd);
  const draftSession = showSessionChrome && !currentThreadId;
  const composerJourneyState = currentThreadId
    ? "bound"
    : runtimeOptionsError
      ? "blocked"
      : runtimeOptionsLoading || !contextMatchesTarget
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
      {workspaceDialogOpen && (
        <WorkspacePickerDialog
          ariaLabel="Open workspace"
          disabled={disabled}
          onCancel={() => props.setWorkspaceDialogOpen(false)}
          onCreate={async (parent, name) => {
            await createWorkspace(name, parent);
            props.setWorkspaceDialogOpen(false);
          }}
          onOpen={async (cwd) => {
            await startNewThread(cwd);
            props.setWorkspaceDialogOpen(false);
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
              const deletingCurrent = pendingDeleteSession.id === currentThreadId;
              setDraftSession(null);
              await client?.request("thread/delete", { threadId: pendingDeleteSession.id });
              setPendingDeleteSession(null);
              if (deletingCurrent) {
                await startNewThread(undefined, { refreshHistory: false });
              }
              await Promise.all([refreshHistory(), refreshHistory(client, true)]);
            } finally {
              setDeleteSessionPending(false);
            }
          })}
          session={pendingDeleteSession}
        />
      )}
      <nav className="mobileTabs" aria-label="Workbench panels">
        <button className={mobilePanel === "history" ? "is-selected" : ""} onClick={() => setMobilePanel("history")} type="button">
          <PanelLeft size={17} />
          History
        </button>
        <button className={mobilePanel === "transcript" ? "is-selected" : ""} onClick={() => setMobilePanel("transcript")} type="button">
          <MessageSquare size={17} />
          Transcript
        </button>
        {showSessionChrome && (
          <button className={mobilePanel === "status" ? "is-selected" : ""} onClick={() => setMobilePanel("status")} type="button">
            <PanelRight size={17} />
            {activeRightTab ? rightWorkspaceTabLabel(activeRightTab.kind) : "Status"}
          </button>
        )}
      </nav>

      <div
        className={`workbench ${leftCollapsed ? "is-leftCollapsed" : ""} ${rightCollapsed || !showSessionChrome ? "is-rightCollapsed" : ""}`}
        style={{ "--right-column-width": `${rightWidthPx}px` } as CSSProperties}
      >
        <aside className={`historyColumn ${leftCollapsed ? "is-collapsed" : ""} ${mobilePanel === "history" ? "is-mobileSelected" : ""}`}>
          <div className="leftChrome">
            <div className="leftBrandRow">
              <div className="brandMark">
                <span className="brandGlyph"><img alt="Psychevo" src={logoUrl} /></span>
                <div>
                  <h1>Psychevo</h1>
                </div>
              </div>
              <button
                aria-label={leftCollapsed ? "Expand left sidebar" : "Collapse left sidebar"}
                className={`sidebarToggle ${leftCollapsed ? "is-logoToggle" : ""}`}
                onClick={() => setLeftCollapsed((value: boolean) => !value)}
                title={leftCollapsed ? "Expand left sidebar" : "Collapse left sidebar"}
                type="button"
              >
                {leftCollapsed ? <img alt="" aria-hidden className="sidebarToggleLogo" src={logoUrl} /> : <PanelLeft size={16} />}
              </button>
            </div>
            <div className="leftActions" aria-label="Session actions">
              <ActionButton ariaLabel="New Session" icon={<MessageSquare size={16} />} onClick={() => void runAction(async () => startNewThread())} variant="ghost">
                New Session
              </ActionButton>
              <button aria-label="Search" className={mainView === "search" ? "is-selected" : ""} onClick={() => switchMainView("search")} type="button">
                <Search size={16} /> <span>Search</span>
              </button>
            </div>
            {!leftCollapsed && (
              <>
                <PinnedPanel
                  currentThreadId={currentThreadId}
                  disabled={disabled}
                  sessions={pinnedSessions}
                  onResume={(threadId) => void runAction(async () => {
                    const epoch = beginExplicitViewSwitch();
                    await refreshSnapshot(client, threadId, undefined, false, epoch);
                    updateMainView("transcript");
                    setMobilePanel("transcript");
                  })}
                  onUnpin={togglePinnedSession}
                />
                {sessionArchiveView ? (
                  <SessionArchivePanel
                    archivedSessions={archivedSessions}
                    client={client}
                    currentThreadId={currentThreadId}
                    disabled={disabled}
                    scope={importScope}
                    onActivateArchived={async (threadId) => {
                      if (!client) return;
                      await client.request("thread/restore", { threadId });
                      await Promise.all([refreshHistory(client), refreshHistory(client, true)]);
                      const epoch = beginExplicitViewSwitch();
                      await refreshSnapshot(client, threadId, undefined, false, epoch);
                      updateMainView("transcript");
                      setMobilePanel("transcript");
                    }}
                    onDeleteArchived={(session) => setPendingDeleteSession(session)}
                    onImportSession={async (profile, candidateId, targetId, activate) => {
                      if (!client) return;
                      const imported = await client.request("thread/import", {
                        archived: !activate,
                        candidateId,
                        scope: importScope,
                        targetId
                      });
                      const threadId = imported.snapshot.thread?.id;
                      if (!threadId) throw new Error(`Imported ${profile.profileLabel} session did not publish a Thread.`);
                      await Promise.all([refreshHistory(client), refreshHistory(client, true)]);
                      const epoch = beginExplicitViewSwitch();
                      await refreshSnapshot(client, threadId, undefined, !activate, epoch, true);
                      updateMainView("transcript");
                      setMobilePanel("transcript");
                    }}
                    onOpenArchived={async (threadId) => {
                      const epoch = beginExplicitViewSwitch();
                      await refreshSnapshot(client, threadId, undefined, true, epoch, true);
                      updateMainView("transcript");
                      setMobilePanel("transcript");
                    }}
                    onOpenWorkspace={() => props.setWorkspaceDialogOpen(true)}
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
                  onArchive={(threadId) => void runAction(async () => {
                    setDraftSession(null);
                    await client?.request("thread/archive", { threadId });
                    await refreshHistory();
                    await refreshHistory(client, true);
                  })}
                  onDelete={(threadId) => void runAction(async () => {
                    const session = [...sessions, ...archivedSessions]
                      .find((candidate: any) => candidate.id === threadId);
                    if (session) setPendingDeleteSession(session);
                  })}
                  onExport={(threadId) => {
                    if (endpoint) {
                      void host?.open.downloadSession(endpoint, threadId, "export");
                    }
                  }}
                  onFork={(threadId) => void runAction(async () => {
                    const session = sessions.find((candidate: any) => candidate.id === threadId);
                    if (!session) return;
                    const result = await client?.request("thread/action/run", {
                      action: { kind: "fork" },
                      scope: scopeForCwd(session.cwd),
                      threadId
                    });
                    const forkedThreadId = result?.kind === "fork" ? result.snapshot.thread?.id : null;
                    if (!forkedThreadId) return;
                    const epoch = beginExplicitViewSwitch();
                    await refreshSnapshot(client, forkedThreadId, undefined, false, epoch);
                    await refreshHistory();
                    updateMainView("transcript");
                    setMobilePanel("transcript");
                  })}
                  onImportSessions={() => setSessionArchiveView(true)}
                  onNew={() => void runAction(async () => {
                    await startNewThread();
                  })}
                  onCreateWorkspace={() => props.setWorkspaceDialogOpen(true)}
                  onNewInCwd={(cwd) => void runAction(async () => {
                    await startNewThread(cwd);
                  })}
                  onLoadOlderSessions={(cwd) => void runAction(async () => loadOlderSessions(cwd))}
                  onTogglePinned={togglePinnedSession}
                  onRename={(threadId, title) => void runAction(async () => {
                    await client?.request("thread/rename", { threadId, title });
                    await refreshHistory();
                  })}
                  onRestore={(threadId) => void runAction(async () => {
                    setDraftSession(null);
                    await client?.request("thread/restore", { threadId });
                    await refreshHistory();
                    await refreshHistory(client, true);
                  })}
                  onResumeDraft={() => {
                    switchMainView("transcript");
                    setMobilePanel("transcript");
                  }}
                  onResume={(threadId) => void runAction(async () => {
                    const epoch = beginExplicitViewSwitch();
                    await refreshSnapshot(client, threadId, undefined, false, epoch);
                    updateMainView("transcript");
                    setMobilePanel("transcript");
                  })}
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
                  props.openSettingsSection(settingsSection);
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
                  const epoch = beginExplicitViewSwitch();
                  await refreshSnapshot(client, forkSource.id, undefined, false, epoch);
                })}
                title={forkSource ? "Open source thread" : `Source thread ${snapshot.thread.forkedFromThreadId} is unavailable`}
                type="button"
              >
                Forked from {forkSource?.displayTitle ?? forkSource?.title ?? snapshot.thread.forkedFromThreadId.slice(0, 8)}
              </button>
            )}
            {showSessionChrome && (
              <button
                aria-label={rightCollapsed ? "Show right inspector" : "Collapse right inspector"}
                className="rightInspectorToggle"
                onClick={() => setRightCollapsed((value: boolean) => !value)}
                title={rightCollapsed ? "Show right inspector" : "Collapse right inspector"}
                type="button"
              >
                <PanelRight size={16} />
              </button>
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
              channels={settings?.channels.channels ?? []}
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
              onOpenSession={(threadId, readOnly = false) => void runAction(async () => {
                const epoch = beginExplicitViewSwitch();
                await refreshSnapshot(client, threadId, undefined, readOnly, epoch, readOnly);
                updateMainView("transcript");
                setMobilePanel("transcript");
              })}
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
                  onCopyText={copyText}
                  {...(historyEditAvailable && pointForkAvailable ? {
                    onReadUserMessageDraft: async (entry: any) => {
                      const target = snapshotThreadApplicationTarget(snapshot);
                      if (!client || !target) throw new Error("The active Thread is unavailable.");
                      return client.request("thread/history/draft/read", {
                        ...target,
                        messageId: entry.id
                      });
                    },
                    onUpdateUserMessage: async (entry: any, draft: any) => {
                      const target = snapshotThreadApplicationTarget(snapshot);
                      if (!client || !target) throw new Error("The active Thread is unavailable.");
                      const result = await client.request("thread/action/run", {
                        ...target,
                        action: { kind: "revertConversation", messageId: entry.id, draft }
                      });
                      if (result.kind !== "revertConversation") return;
                      if (result.noOp) {
                        setSnapshot(result.snapshot);
                        return;
                      }
                      const text = editableDraftText(draft.parts);
                      await submitThreadTurn(
                        target.threadId,
                        text,
                        [],
                        text,
                        draft.parts
                      );
                    },
                    onForkUserMessage: async (entry: any, draft: any) => {
                      const target = snapshotThreadApplicationTarget(snapshot);
                      if (!client || !target) throw new Error("The active Thread is unavailable.");
                      const result = await client.request("thread/action/run", {
                        ...target,
                        action: { kind: "forkBefore", messageId: entry.id }
                      });
                      if (result.kind !== "forkBefore" || !result.snapshot.thread?.id) return;
                      const epoch = beginExplicitViewSwitch();
                      setSnapshot(result.snapshot);
                      await refreshSnapshot(client, result.snapshot.thread.id, undefined, false, epoch);
                      prefillEditableDraft(draft.parts, patchComposerDraft, setAttachments);
                      await refreshHistory();
                      updateMainView("transcript");
                      setMobilePanel("transcript");
                    }
                  } : {})}
                  onOpenAgentSession={openAgentSessionTab}
                  threadId={snapshot.thread?.id ?? null}
                  onReadAloudText={onReadAloudText}
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
          {showSessionChrome && composerPresentationReady && <div className="composerDock" ref={composerDockRef}>
            {snapshot.historyEditing?.kind === "conversationEdit" && (
              <div className="historyEditingStrip" role="status">
                <span>{snapshot.historyEditing.hiddenEntryCount} hidden {snapshot.historyEditing.hiddenEntryCount === 1 ? "entry" : "entries"}</span>
                <button onClick={() => void runAction(async () => {
                  const target = snapshotThreadApplicationTarget(snapshot);
                  if (!client || !target) return;
                  const result = await client.request("thread/action/run", {
                    ...target,
                    action: { kind: "unrevertConversation" }
                  });
                  if (result.kind !== "unrevertConversation") return;
                  setSnapshot(result.snapshot);
                  prefillEditableDraft(result.draft.parts, patchComposerDraft, setAttachments);
                })} type="button">
                  Restore history
                </button>
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
              completionProvider={async (text, cursor) => {
                const scope = activeScope ?? init?.scope ?? scopeForCwd(settings?.cwd ?? fallbackCwd);
                const result = await client?.request("completion/list", {
                  cursor,
                  scope,
                  text,
                  threadId: snapshot.thread?.id ?? null
                }) ?? { items: [], replacement: null };
                return {
                  ...result,
                  items: result.items.filter((item: any) => (
                    agentMentionsEnabled
                    || item.target?.kind !== "agent"
                  ))
                };
              }}
              disabled={disabled}
              draftPatch={props.composerDraftPatch ?? undefined}
              leftControls={(
                <>
                  <ComposerRuntimeControls
                    binding={runtimeContext?.binding ?? null}
                    controls={activeRuntimeControls}
                    profiles={runtimeContext?.profiles ?? []}
                    targets={runtimeContext?.compatibleTargets ?? []}
                    controlValues={runtimeControlDrafts}
                    disabled={disabled}
                    targetId={selectedTargetId}
                    contextError={runtimeOptionsError}
                    contextLoading={runtimeOptionsLoading}
                    onTargetChange={(value) => void runAction(async () => changeRunnableTarget(value))}
                    onControlChange={(control, value) => void runAction(async () => changeRuntimeControl(control, value))}
                  />
                </>
              )}
              addMenuOptions={(
                <ComposerVoiceOptionSwitches
                  autoSpeak={Boolean(voiceAutoSpeak)}
                  disabled={disabled}
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
                  disabled={disabled}
                  listening={Boolean(voiceListening)}
                  onToggle={onVoiceDictationToggle}
                />
              )}
              promptSubmitBlockReason={turnBlockReason}
              promptSubmitDisabled={!turnSendable}
              promptTextUnavailableReason={promptTextUnavailableReason}
              retainDraftUntilAccepted
              rightControls={(
                <>
                  <ComposerSubmitControls
                    context={contextUsage}
                    controls={controls}
                    usage={sessionUsage}
                    controlValues={runtimeControlDrafts}
                    disabled={disabled || runtimeOptionsLoading}
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
                  onClarify={(request, answers, cancel) => void runAction(async () => {
                    const target = snapshotThreadApplicationTarget(snapshot, request.threadId);
                    if (!client || !target) {
                      setInteractionFeedback(setCommandFeedback, false, "Clarify response does not belong to the active Thread.");
                      return;
                    }
                    const response = await client.request("thread/interaction/respond", {
                      ...target,
                      interactionId: request.actionId,
                      response: cancel
                        ? { kind: "cancelClarify" }
                        : { kind: "clarify", answers: answers ?? [] }
                    });
                    setInteractionFeedback(
                      setCommandFeedback,
                      acceptedInteractionResponse(response),
                      response.accepted ? "Clarify response accepted." : "Clarify response was not accepted."
                    );
                    await refreshSnapshot(client, target.threadId, undefined, true);
                  })}
                  onPermission={(request, decision) => void runAction(async () => {
                    const target = snapshotThreadApplicationTarget(snapshot, request.threadId);
                    if (!client || !target) {
                      setInteractionFeedback(setCommandFeedback, false, "Permission response does not belong to the active Thread.");
                      return;
                    }
                    const response = await client.request("thread/interaction/respond", {
                      ...target,
                      interactionId: request.actionId,
                      response: { kind: "permission", decision }
                    });
                    setInteractionFeedback(
                      setCommandFeedback,
                      acceptedInteractionResponse(response),
                      response.accepted ? "Permission response accepted." : "Permission response was not accepted."
                    );
                    await refreshSnapshot(client, target.threadId, undefined, true);
                  })}
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
              onInterrupt={() => void runAction(async () => {
                const target = snapshotThreadApplicationTarget(snapshot);
                if (!client || !target) {
                  setCommandFeedback?.({
                    accepted: false,
                    command: "interrupt",
                    message: "Interrupt is not available for the active Thread.",
                    feedbackAnchor: "composer"
                  });
                  return;
                }
                await runThreadInterrupt(client, target);
                await refreshSnapshot(client, target.threadId, undefined, true, props.viewEpochRef.current);
              })}
              onModeChange={() => {}}
              onRemoveAttachment={(id) => setAttachments((current: any[]) => current.filter((attachment) => attachment.id !== id))}
              onShell={(command) => void runAction(async () => startShell(command))}
              onSteer={(text) => void runAction(async () => {
                if (!steerTurnId) {
                  return;
                }
                const target = snapshotThreadApplicationTarget(snapshot);
                if (!client || !target || !steerAvailable) {
                  return;
                }
                clearCommandTransientUi();
                const result = await client.request("thread/action/run", {
                  ...target,
                  action: { kind: "steer", expectedTurnId: steerTurnId, text }
                });
                if (result.kind === "steer" && result.accepted) {
                  setSnapshot((current: any) => appendOptimisticPrompt(current, text));
                  await refreshHistory();
                } else {
                  setCommandFeedback?.({
                    accepted: false,
                    command: "/steer",
                    message: "The selected Runtime Profile does not support steering this turn.",
                    feedbackAnchor: "composer"
                  });
                }
              })}
              onSubmit={(text, mentions, orderedInput, isInputCurrent) => runAction(
                async () => submitTurn(text, mentions, undefined, orderedInput, isInputCurrent)
              ).then((accepted: unknown) => accepted === true)}
            />
            <ComposerEnvironment
              branch={workspaceBranch !== undefined
                ? workspaceBranch
                : settings?.project?.branch ?? null}
              branchDisabled={running}
              controlValues={runtimeControlDrafts}
              controls={activeRuntimeControls}
              cwd={activeWorkbenchCwd}
              disabled={disabled || runtimeOptionsLoading}
              draft={draftSession}
              path={settings?.cwd === activeWorkbenchCwd
                ? settings?.project?.displayPath ?? activeWorkbenchCwd
                : init?.scope.cwd === activeWorkbenchCwd
                  ? init.displayCwd
                  : sessionBrowserWorkspaces.find((workspace: any) => workspace.cwd === activeWorkbenchCwd)?.displayPath
                    ?? activeWorkbenchCwd}
              runtimeSafetyLabel={runtimeSafetyLabel}
              profile={init?.profile ?? null}
              workspaces={sessionBrowserWorkspaces}
              onBranchChange={(nextBranch, create) => checkoutWorkspaceGitBranch(nextBranch, create)}
              onOpenFiles={() => openRightWorkspaceTab("files")}
              onReadBranches={() => readWorkspaceGitBranches()}
              onReadFolders={(folderPath) => readWorkspaceFolders(folderPath)}
              onRuntimeControlChange={(control, value) => void runAction(async () => changeRuntimeControl(control, value))}
              onWorkspaceChange={(cwd) => startNewThread(cwd)}
            />
          </div>}
        </section>

        {showSessionChrome && !rightCollapsed && (
          <aside className={`statusColumn ${mobilePanel === "status" ? "is-mobileSelected" : ""}`}>
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
                status={props.status}
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
                onClose={props.closeRightWorkspaceTab}
                onCopyText={copyText}
                onDirtyTabChange={(tabId, dirty) => {
                  setDirtyRightTabs((current: Record<string, boolean>) => current[tabId] === dirty ? current : { ...current, [tabId]: dirty });
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
                    props.setError(result?.message ?? "Open externally is not supported by this host.");
                  }
                })}
                onOpenKind={(kind) => {
                  if (kind === "sideConversation") {
                    void runAction(async () => executeCommand("/btw", "commandsPanel"));
                    return;
                  }
                  openRightWorkspaceTab(kind, {}, kind !== "browser");
                }}
                onOpenPreview={(preview) => openRightWorkspaceTab("preview", { preview, title: preview.title }, true)}
                onRejectChange={(turnId, path) => void runAction(async () => rejectWorkspaceChange(turnId, path))}
                onConsumePendingPrompt={clearRightWorkspaceTabPendingPrompt}
                onRefresh={() => void runAction(async () => {
                  await refreshSnapshot();
                  await refreshHistory();
                  await refreshAgentSurface();
                  await refreshWorkspaceSurface();
                })}
                onRefreshTrace={() => void props.refreshTrace()}
                onSaveFile={(path, content, expectedRevision, force) => saveFileFromEditor(path, content, expectedRevision, force)}
                onShowHome={() => props.revealRightWorkspace(null)}
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

function editableDraftText(parts: ThreadEditableInputPart[]): string {
  return parts
    .filter((part): part is Extract<ThreadEditableInputPart, { type: "text" }> => part.type === "text")
    .map((part) => part.text)
    .join("\n");
}

function prefillEditableDraft(
  parts: ThreadEditableInputPart[],
  patchComposerDraft: (text: string, parts?: ThreadEditableInputPart[]) => void,
  setAttachments: (attachments: PendingAttachment[]) => void
) {
  patchComposerDraft(editableDraftText(parts), parts);
  const attachments = parts.flatMap((part, index): PendingAttachment[] => {
    if (part.type !== "image") return [];
    const source = part.input.kind === "localPath" ? part.input.path : part.input.url;
    const name = source.split(/[\\/]/).pop()?.split(/[?#]/)[0] || `image-${index + 1}`;
    return [{
      id: `history:${index}:${source}`,
      input: part,
      kind: "image",
      name,
      ...(part.input.kind === "url" ? { previewUrl: part.input.url } : {}),
      size: 0,
      sizeLabel: "From history"
    }];
  });
  setAttachments(attachments);
}

function acceptedInteractionResponse(value: unknown): boolean {
  return typeof value === "object"
    && value !== null
    && (value as { accepted?: unknown }).accepted === true;
}

function setInteractionFeedback(
  setCommandFeedback: ((feedback: Record<string, unknown>) => void) | undefined,
  accepted: boolean,
  message: string
) {
  setCommandFeedback?.({
    accepted,
    command: "thread/interaction/respond",
    message,
    feedbackAnchor: "composer"
  });
}
