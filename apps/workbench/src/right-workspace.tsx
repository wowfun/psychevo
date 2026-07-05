import { type ReactNode } from "react";
import { Bot, Bug, FolderTree, GitPullRequest, Home, MessageSquare, Plus, RefreshCw, TerminalSquare, X } from "lucide-react";
import { DismissibleDetails, type TranscriptAgentSession } from "@psychevo/components";
import type { GatewayClient } from "@psychevo/client";
import type {
  ContextReadResult,
  GatewayMention,
  GatewayRequestScope,
  SessionUsageSummaryView,
  ThreadSnapshot,
  WorkspaceChangesResult,
  WorkspaceDiffResult,
  WorkspaceFileEntry,
  WorkspaceFileWriteResult
} from "@psychevo/protocol";
import type { Appearance, DebugEvent, GatewayEventFeed, RightWorkspaceTab, RightWorkspaceTabKind, TerminalNotificationEvent, TraceState } from "./types";
import { DebugPanel } from "./right-workspace/debug";
import { FilesPanel } from "./right-workspace/files";
import { ReviewPanel } from "./right-workspace/review";
import { ThreadPanel } from "./right-workspace/thread";
import { TerminalPanel } from "./right-workspace/terminal";
import { SessionObservability } from "./right-workspace/usage";

export { isUnsupportedPreviewFile } from "./right-workspace/files";
export { fileBasename } from "./right-workspace/tree";
export { SessionUsageGrid, normalizedPercent } from "./right-workspace/usage";

export function RightWorkspace({
  activeTabId,
  activity,
  appearance,
  client,
  context,
  debugEnabled,
  debugEvents,
  files,
  latestGatewayEvent,
  root,
  scope,
  sessionId,
  status,
  usage,
  tabs,
  terminalEvents,
  trace,
  truncated,
  cwd,
  workspaceChanges,
  workspaceDiff,
  onActivate,
  onAcceptChange,
  onChangedFile,
  onClose,
  onCopyText,
  onDirtyTabChange,
  onOpenFile,
  onOpenAgentSession,
  onOpenKind,
  onConsumePendingPrompt,
  onRejectChange,
  onRefresh,
  onRefreshTrace,
  onSaveFile,
  onShowHome,
  onSubmitThreadTurn,
  promptSubmitBlockReason,
  promptSubmitDisabled
}: {
  activeTabId: string | null;
  activity: ThreadSnapshot["activity"];
  appearance: Appearance;
  client: GatewayClient | null;
  context: ContextReadResult | null;
  debugEnabled: boolean;
  debugEvents: DebugEvent[];
  files: WorkspaceFileEntry[];
  latestGatewayEvent: GatewayEventFeed | null;
  root: string;
  scope: GatewayRequestScope | null;
  sessionId: string | null;
  status: string;
  promptSubmitBlockReason?: string | undefined;
  promptSubmitDisabled?: boolean | undefined;
  usage: SessionUsageSummaryView | null;
  tabs: RightWorkspaceTab[];
  terminalEvents: TerminalNotificationEvent[];
  trace: TraceState;
  truncated: boolean;
  cwd: string;
  workspaceChanges: WorkspaceChangesResult | null;
  workspaceDiff: WorkspaceDiffResult | null;
  onActivate(tabId: string): void;
  onAcceptChange(turnId: string, path: string): void;
  onChangedFile(path: string): void;
  onClose(tabId: string): void;
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  onDirtyTabChange(tabId: string, dirty: boolean): void;
  onOpenFile(path: string): void;
  onOpenAgentSession(session: TranscriptAgentSession): void;
  onOpenKind(kind: RightWorkspaceTabKind): void;
  onConsumePendingPrompt(tabId: string): void;
  onRejectChange(turnId: string, path: string): void;
  onRefresh(): void;
  onRefreshTrace(): void;
  onSaveFile(path: string, content: string, expectedRevision: string | null, force: boolean): Promise<WorkspaceFileWriteResult>;
  onShowHome(): void;
  onSubmitThreadTurn(threadId: string, text: string, mentions: GatewayMention[]): Promise<void>;
}) {
  const visibleTabs = tabs.filter((tab) => rightWorkspaceTabVisibleForSession(tab, sessionId));
  const activeTab = visibleTabs.find((tab) => tab.id === activeTabId) ?? null;
  const visibleActiveTabId = activeTab?.id ?? null;
  return (
    <section className="rightWorkspace" aria-label="Right workspace">
      {visibleTabs.length > 0 && (
        <RightWorkspaceTabs
          activeTabId={visibleActiveTabId}
          tabs={visibleTabs}
          onActivate={onActivate}
          onClose={onClose}
          sessionId={sessionId}
          onOpenKind={onOpenKind}
          onShowHome={onShowHome}
        />
      )}
      <div className="rightTabPanels">
        <div className="rightTabPanel" hidden={activeTab !== null}>
          <RightWorkspaceHome
            context={context}
            files={workspaceDiff?.files ?? []}
            sessionId={sessionId}
            usage={usage}
            onChangedFile={onChangedFile}
            onOpenKind={onOpenKind}
            onRefresh={onRefresh}
          />
        </div>
        {visibleTabs.map((tab) => (
          <div className="rightTabPanel" hidden={tab.id !== activeTab?.id} key={tab.id}>
            {tab.kind === "review" && (
              <ReviewPanel
                activity={activity}
                changedFiles={workspaceDiff?.files ?? []}
                changes={workspaceChanges}
                context={context}
                diff={tab.diff ?? workspaceDiff}
                root={root || cwd}
                sessionId={sessionId}
                status={status}
                cwd={cwd}
                onAcceptChange={onAcceptChange}
                onChangedFile={onChangedFile}
                onRejectChange={onRejectChange}
                onRefresh={onRefresh}
              />
            )}
            {tab.kind === "files" && (
              <FilesPanel
                files={files}
                preview={tab.file ?? null}
                previewMessage={tab.message ?? null}
                root={root}
                selectedPath={tab.path ?? null}
                tabId={tab.id}
                truncated={truncated}
                onCompare={onChangedFile}
                onCopyText={onCopyText}
                onDirtyChange={onDirtyTabChange}
                onOpen={onOpenFile}
                onSave={onSaveFile}
              />
            )}
            {tab.kind === "terminal" && (
              <TerminalPanel
                appearance={appearance}
                client={client}
                scope={scope}
                terminalEvents={terminalEvents}
                cwd={cwd}
              />
            )}
            {tab.kind === "debug" && debugEnabled && (
              <DebugPanel
                events={debugEvents}
                trace={trace}
                onRefreshTrace={onRefreshTrace}
              />
            )}
            {(tab.kind === "sideConversation" || tab.kind === "agentSession") && (
              <ThreadPanel
                client={client}
                disabled={status !== "connected"}
                kind={tab.kind}
                latestGatewayEvent={latestGatewayEvent}
                parentThreadId={tab.parentThreadId ?? sessionId}
                pendingPrompt={tab.pendingPrompt ?? null}
                promptSubmitBlockReason={promptSubmitBlockReason}
                promptSubmitDisabled={promptSubmitDisabled}
                scope={scope}
                threadId={tab.threadId ?? null}
                title={tab.title}
                onCopyText={onCopyText}
                onOpenAgentSession={onOpenAgentSession}
                onPendingPromptConsumed={() => onConsumePendingPrompt(tab.id)}
                onSubmitThreadTurn={onSubmitThreadTurn}
              />
            )}
          </div>
        ))}
      </div>
    </section>
  );
}

export function rightWorkspaceTabVisibleForSession(tab: RightWorkspaceTab, sessionId: string | null): boolean {
  if (tab.kind !== "sideConversation" && tab.kind !== "agentSession") {
    return true;
  }
  return Boolean(sessionId) && (tab.parentThreadId ?? null) === sessionId;
}

function RightWorkspaceTabs({
  activeTabId,
  sessionId,
  tabs,
  onActivate,
  onClose,
  onOpenKind,
  onShowHome
}: {
  activeTabId: string | null;
  sessionId: string | null;
  tabs: RightWorkspaceTab[];
  onActivate(tabId: string): void;
  onClose(tabId: string): void;
  onOpenKind(kind: RightWorkspaceTabKind): void;
  onShowHome(): void;
}) {
  const menuItems: Array<{ icon: ReactNode; kind: RightWorkspaceTabKind; label: string }> = [
    { icon: <GitPullRequest size={14} />, kind: "review", label: "Review" },
    { icon: <TerminalSquare size={14} />, kind: "terminal", label: "Terminal" },
    { icon: <FolderTree size={14} />, kind: "files", label: "Files" }
  ];
  if (sessionId) {
    menuItems.push({ icon: <MessageSquare size={14} />, kind: "sideConversation", label: "Side chat" });
  }
  return (
    <div className="rightWorkspaceTabs" aria-label="Right workspace tabs">
      <button
        aria-label="Workspace home"
        className={activeTabId === null ? "is-selected" : ""}
        onClick={onShowHome}
        title="Workspace home"
        type="button"
      >
        <Home size={14} />
      </button>
      {tabs.map((tab) => (
        <div className={`rightWorkspaceTab ${tab.id === activeTabId ? "is-selected" : ""}`} key={tab.id}>
          <button onClick={() => onActivate(tab.id)} title={tab.title} type="button">
            {rightWorkspaceTabIcon(tab.kind)}
            <span>{tab.title}</span>
          </button>
          <button aria-label={`Close ${tab.title}`} onClick={() => onClose(tab.id)} title="Close" type="button">
            <X size={12} />
          </button>
        </div>
      ))}
      <DismissibleDetails
        className="rightAddMenu"
        summary={<Plus size={15} />}
        summaryProps={{ "aria-label": "Open right workspace tab", title: "Open tab" }}
      >
        {({ close }) => (
          <div role="menu" aria-label="Open right workspace tab">
            {menuItems.map((item) => (
              <button
                key={item.kind}
                onClick={() => {
                  close();
                  onOpenKind(item.kind);
                }}
                role="menuitem"
                type="button"
              >
                {item.icon}
                <span>{item.label}</span>
              </button>
            ))}
          </div>
        )}
      </DismissibleDetails>
    </div>
  );
}

function RightWorkspaceHome({
  context,
  files,
  sessionId,
  usage,
  onChangedFile,
  onOpenKind,
  onRefresh
}: {
  context: ContextReadResult | null;
  files: WorkspaceDiffResult["files"];
  sessionId: string | null;
  usage: SessionUsageSummaryView | null;
  onChangedFile(path: string): void;
  onOpenKind(kind: RightWorkspaceTabKind): void;
  onRefresh(): void;
}) {
  return (
    <section className="rightWorkspaceHome" aria-label="Workspace status">
      <header>
        <div>
          <h2>Status</h2>
          <p className="rightWorkspaceSessionId" title={sessionId ?? undefined}>{sessionId ?? "draft"}</p>
        </div>
        <button aria-label="Refresh workspace" onClick={onRefresh} title="Refresh" type="button">
          <RefreshCw size={15} />
        </button>
      </header>
      <SessionObservability
        context={context}
        hasActiveSession={Boolean(sessionId)}
        usage={usage}
        showCategories
      />
      <nav className="rightHomeNav" aria-label="Open workspace tab">
        <button onClick={() => onOpenKind("review")} type="button">
          <GitPullRequest size={16} />
          <span>Review</span>
        </button>
        <button onClick={() => onOpenKind("terminal")} type="button">
          <TerminalSquare size={16} />
          <span>Terminal</span>
        </button>
        <button onClick={() => onOpenKind("files")} type="button">
          <FolderTree size={16} />
          <span>Files</span>
        </button>
        {sessionId && (
          <button onClick={() => onOpenKind("sideConversation")} type="button">
            <MessageSquare size={16} />
            <span>Side chat</span>
          </button>
        )}
      </nav>
      <div className="rightChangedFiles">
        <div className="rightSectionLabel">
          <span>Changed files</span>
          <b>{files.length}</b>
        </div>
        {files.slice(0, 8).map((file) => (
          <button key={`${file.status}:${file.path}`} onClick={() => onChangedFile(file.path)} type="button">
            <span>{file.path}</span>
            <small>{file.status}</small>
          </button>
        ))}
        {files.length === 0 && <p>No changed files.</p>}
      </div>
    </section>
  );
}

export function createRightTabId(kind: RightWorkspaceTabKind): string {
  return `${kind}:${Date.now()}:${Math.random().toString(16).slice(2)}`;
}

export function rightWorkspaceDefaultTitle(kind: RightWorkspaceTabKind): string {
  return rightWorkspaceTabLabel(kind);
}

export function rightWorkspaceTabLabel(kind: RightWorkspaceTabKind): string {
  switch (kind) {
    case "files":
      return "Files";
    case "terminal":
      return "Terminal";
    case "debug":
      return "Debug";
    case "sideConversation":
      return "Side chat";
    case "agentSession":
      return "Agent";
    case "review":
    default:
      return "Review";
  }
}

function rightWorkspaceTabIcon(kind: RightWorkspaceTabKind): ReactNode {
  switch (kind) {
    case "files":
      return <FolderTree size={14} />;
    case "terminal":
      return <TerminalSquare size={14} />;
    case "debug":
      return <Bug size={14} />;
    case "sideConversation":
      return <MessageSquare size={14} />;
    case "agentSession":
      return <Bot size={14} />;
    case "review":
    default:
      return <GitPullRequest size={14} />;
  }
}
