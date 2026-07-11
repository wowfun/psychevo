import { type CSSProperties } from "react";
import { AlertTriangle, GripVertical, MessageSquare, PanelLeft, PanelRight, Search } from "lucide-react";
import { ActionButton, Composer, HistoryPanel, TranscriptPanel, type WorkspaceFileLinkContext } from "@psychevo/components";
import { appendOptimisticPrompt, scopeForCwd } from "@psychevo/client";
import { WorkspaceCreateDialog, LeftUtilityRail, MainSurface, PinnedPanel } from "./app-shell";
import { CommandFeedbackView, CommandOverlayView } from "./command-overlay";
import { ComposerRequests, ComposerStatusLine, ComposerSubmitControls } from "./composer-controls";
import { ComposerRuntimeControls } from "./runtime-controls";
import { ComposerDictationButton, ComposerVoiceOptionSwitches } from "./voice-controls";
import { RightWorkspace, rightWorkspaceTabLabel } from "./right-workspace";
import { DEFAULT_RIGHT_WIDTH_PX } from "./storage";
import { EMPTY_BACKEND_DRAFT, backendDraftFromBackend } from "./capabilities-agents-config";
import { confirmedSteerTurnId } from "./gateway-event-feed";
import type { RightWorkspaceTab } from "./types";

const logoUrl = new URL("../../../assets/psychevo-logo.svg", import.meta.url).href;

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
    changeRuntimeProfile,
    changeAgentSelection,
    clearRightWorkspaceTabPendingPrompt,
    channelDoctor,
    client,
    commandFeedback,
    commands,
    contextUsage,
    controls,
    copyText,
    createWorkspace,
    currentThreadId,
    debugEnabled,
    debugEvents,
    deleteAutomation,
    deleteArchivedSession,
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
    init,
    leftCollapsed,
    latestGatewayEvent,
    loadingOlderCwd,
    loadChannelSources,
    loadOlderSessions,
    loadThreadSearchText,
    mainView,
    mobilePanel,
    modelReady,
    modelTurnBlockReason,
    nativeRuntimeSelected,
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
    permissionMode,
    pinnedSessionIds,
    pinnedSessions,
    planModeAvailable,
    pauseAutomation,
    pollWechatQrSetup,
    refreshAutomations,
    refreshAgentSurface,
    refreshHistory,
    refreshSnapshot,
    refreshUsageStats,
    refreshWorkspaceSurface,
    rejectWorkspaceChange,
    restoreArchivedSession,
    resumeAutomation,
    rightCollapsed,
    rightTabs,
    rightWidthPx,
    runnableAgents,
    runAction,
    runAutomation,
    runCommandAlternateAction,
    running,
    runtimeAcceptsAgentPersona,
    runtimeContext,
    runtimeControlValues,
    runtimeOptionsLoading,
    runtimeOptionsError,
    runtimeProfiles,
    saveBackendDraft,
    saveAutomation,
    saveFileFromEditor,
    selectedAgentName,
    selectedModel,
    selectedRuntimeRef,
    selectedVariant,
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
    setPermissionMode,
    setRightCollapsed,
    setRightTabs,
    setRightWidthPx,
    setCommandFeedback,
    setSelectedModel,
    setSelectedModelSelection,
    setSelectedRuntimeMode,
    setSelectedVariant,
    setSettingsSection,
    setSnapshot,
    setWorkMode,
    settings,
    settingsSection,
    showSessionChrome,
    snapshot,
    startNewThread,
    startShell,
    startWechatQrSetup,
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
    workMode,
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
  const selectedRuntimeProfile = (runtimeProfiles ?? []).find((profile: any) => profile.id === selectedRuntimeRef) ?? null;
  const runtimeSafetyLabel = nativeRuntimeSelected
    ? null
    : [
        "Profile safety",
        selectedRuntimeProfile?.approvalMode || "runtime approval",
        selectedRuntimeProfile?.sandbox || "runtime sandbox"
      ].join(" · ");
  const steerTurnId = confirmedSteerTurnId(
    latestGatewayEvent,
    snapshot.thread?.id ?? null,
    activity.activeTurnId
  );
  const steerAvailable = Boolean(steerTurnId) && (
    nativeRuntimeSelected || selectedRuntimeProfile?.runtime === "acp" || (
      runtimeContext?.runtimeRef === selectedRuntimeRef
      && runtimeContext.capabilities.some((capability: any) => capability.id === "turn.steer" && capability.enabled)
    )
  );

  return (
    <main className="appShell" data-main-view={mainView}>
      {error && (
        <div className="errorBand" role="alert">
          <AlertTriangle size={17} aria-hidden />
          <span>{error}</span>
        </div>
      )}
      {workspaceDialogOpen && (
        <WorkspaceCreateDialog
          disabled={disabled}
          onCancel={() => props.setWorkspaceDialogOpen(false)}
          onCreate={(name) => void runAction(async () => {
            await createWorkspace(name);
            props.setWorkspaceDialogOpen(false);
          })}
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
                <HistoryPanel
                  archived={false}
                  currentThreadId={currentThreadId}
                  disabled={disabled}
                  draftSession={null}
                  pinnedSessionIds={pinnedSessionIds}
                  browserWorkspaces={sessionBrowserWorkspaces}
                  loadingOlderCwd={loadingOlderCwd}
                  sessions={sessions}
                  onArchive={(threadId) => void runAction(async () => {
                    setDraftSession(null);
                    await client?.request("thread/archive", { threadId });
                    await refreshHistory();
                    await refreshHistory(client, true);
                  })}
                  onDelete={(threadId) => void runAction(async () => {
                    setDraftSession(null);
                    await client?.request("thread/delete", { threadId });
                    await refreshHistory();
                    await refreshHistory(client, true);
                  })}
                  onExport={(threadId) => {
                    if (endpoint) {
                      void host?.open.downloadSession(endpoint, threadId, "export");
                    }
                  }}
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

        <section className={`conversationColumn ${mobilePanel === "transcript" ? "is-mobileSelected" : ""}`}>
          <div className="conversationChrome">
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
              archivedSessions={archivedSessions}
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
              onDeleteArchivedSession={(threadId) => void runAction(async () => deleteArchivedSession(threadId))}
              onRestoreArchivedSession={(threadId) => void runAction(async () => restoreArchivedSession(threadId))}
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
                  onCopyText={copyText}
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
          {showSessionChrome && <div className="composerDock">
            {(commandFeedback?.feedbackAnchor === "composer" || commandFeedback?.feedbackAnchor === "status") && (
              <CommandFeedbackView
                className="composerCommandFeedback"
                feedback={commandFeedback}
                onAlternateAction={(action) => void runAction(async () => runCommandAlternateAction(action))}
              />
            )}
            <Composer
              attachments={attachments}
              completionProvider={async (text, cursor) => {
                const scope = activeScope ?? init?.scope ?? scopeForCwd(settings?.cwd ?? fallbackCwd);
                const result = await client?.request("completion/list", {
                  cursor,
                  scope,
                  text,
                  threadId: snapshot.thread?.id ?? null
                }) ?? { items: [], replacement: null };
                if (runtimeAcceptsAgentPersona) {
                  return result;
                }
                return {
                  ...result,
                  items: result.items.filter((item: any) => item.target?.kind !== "agent")
                };
              }}
              disabled={disabled}
              draftPatch={props.composerDraftPatch ?? undefined}
              leftControls={(
                <>
                  <ComposerRuntimeControls
                    agents={runnableAgents}
                    profiles={runtimeProfiles}
                    binding={runtimeContext?.binding ?? null}
                    controls={runtimeContext?.runtimeRef === selectedRuntimeRef ? runtimeContext.controls : []}
                    controlValues={runtimeControlValues}
                    disabled={disabled}
                    agentValue={selectedAgentName}
                    runtimeValue={selectedRuntimeRef}
                    contextError={runtimeOptionsError}
                    contextLoading={runtimeOptionsLoading}
                    onAgentChange={(value) => void runAction(async () => changeAgentSelection(value))}
                    onRuntimeChange={(value) => void runAction(async () => changeRuntimeProfile(value))}
                    onControlChange={(control, value) => void runAction(async () => changeRuntimeControl(control, value))}
                    onManageRuntimes={() => openCapabilitiesTab("agents")}
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
              mode={nativeRuntimeSelected ? workMode : "default"}
              modeControlVisible={nativeRuntimeSelected}
              planModeAvailable={nativeRuntimeSelected && planModeAvailable}
              preActionControls={(
                <ComposerDictationButton
                  disabled={disabled}
                  listening={Boolean(voiceListening)}
                  onToggle={onVoiceDictationToggle}
                />
              )}
              promptSubmitBlockReason={modelTurnBlockReason}
              promptSubmitDisabled={!modelReady}
              rightControls={(
                <ComposerSubmitControls
                  context={contextUsage}
                  controls={controls}
                  usage={sessionUsage}
                  model={selectedModel}
                  showNativeModelControls={nativeRuntimeSelected}
                  variant={selectedVariant}
                  onModelChange={setSelectedModel}
                  onModelSelectionChange={setSelectedModelSelection}
                  onVariantChange={setSelectedVariant}
                />
              )}
              requestPanel={(pendingClarifyActions.length > 0 || pendingPermissionActions.length > 0) ? (
                <ComposerRequests
                  clarifies={pendingClarifyActions}
                  permissions={pendingPermissionActions}
                  onClarify={(request, answers, cancel) => void runAction(async () => {
                    const response = await client?.request("clarify/respond", {
                      requestId: request.actionId,
                      threadId: request.threadId ?? snapshot.thread?.id ?? null,
                      sourceKey: request.sourceKey ?? null,
                      activityId: request.activityId ?? null,
                      answers,
                      cancel
                    });
                    if (!acceptedInteractionResponse(response)) {
                      setCommandFeedback?.({
                        accepted: false,
                        command: "clarify/respond",
                        message: "Clarify response was not accepted.",
                        feedbackAnchor: "composer"
                      });
                    }
                    if (request.threadId) {
                      await refreshSnapshot(client, request.threadId, undefined, true);
                    }
                  })}
                  onPermission={(request, decision) => void runAction(async () => {
                    const response = await client?.request("permission/respond", {
                      requestId: request.actionId,
                      threadId: request.threadId ?? snapshot.thread?.id ?? null,
                      sourceKey: request.sourceKey ?? null,
                      activityId: request.activityId ?? null,
                      decision
                    });
                    if (!acceptedInteractionResponse(response)) {
                      setCommandFeedback?.({
                        accepted: false,
                        command: "permission/respond",
                        message: "Permission response was not accepted.",
                        feedbackAnchor: "composer"
                      });
                    }
                    if (request.threadId) {
                      await refreshSnapshot(client, request.threadId, undefined, true);
                    }
                  })}
                />
              ) : null}
              running={running}
              runningStartedAtMs={activity.startedAtMs ?? null}
              steerAvailable={steerAvailable}
              onAttach={() => void runAction(async () => handleAttachment())}
              onAttachFiles={(files) => void runAction(async () => handleAttachmentFiles(files))}
              onCommand={(command) => void runAction(async () => executeCommand(command, "composer"))}
              onInterrupt={() => void runAction(async () => {
                const threadId = snapshot.thread?.id ?? null;
                await client?.request("turn/interrupt", { threadId });
                await refreshSnapshot(client, threadId ?? undefined, undefined, true, props.viewEpochRef.current);
              })}
              onModeChange={(mode) => {
                setWorkMode(mode);
                if (mode === "plan") {
                  setSelectedRuntimeMode("");
                }
              }}
              onRemoveAttachment={(id) => setAttachments((current: any[]) => current.filter((attachment) => attachment.id !== id))}
              onShell={(command) => void runAction(async () => startShell(command))}
              onSteer={(text) => void runAction(async () => {
                if (!steerTurnId) {
                  return;
                }
                clearCommandTransientUi();
                const result = await client?.request("turn/steer", {
                  expectedTurnId: steerTurnId,
                  threadId: snapshot.thread?.id ?? null,
                  text
                });
                if (result?.accepted) {
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
              onSubmit={(text, mentions) => void runAction(async () => submitTurn(text, mentions))}
            />
            <ComposerStatusLine
              branch={settings?.project?.branch ?? null}
              controls={controls}
              path={settings?.project?.displayPath ?? settings?.cwd ?? ""}
              permissionMode={permissionMode}
              runtimeSafetyLabel={runtimeSafetyLabel}
              profile={init?.profile ?? null}
              onBranchClick={() => {
                void runAction(async () => openDiffPreview(null));
              }}
              onPathClick={() => {
                openRightWorkspaceTab("files");
              }}
              onPermissionModeChange={setPermissionMode}
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
              promptSubmitBlockReason={modelTurnBlockReason}
              promptSubmitDisabled={!modelReady}
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
              onSubmitThreadTurn={submitThreadTurn}
            />
          </aside>
        )}
      </div>
    </main>
  );
}

function acceptedInteractionResponse(value: unknown): boolean {
  return typeof value === "object"
    && value !== null
    && (value as { accepted?: unknown }).accepted === true;
}
