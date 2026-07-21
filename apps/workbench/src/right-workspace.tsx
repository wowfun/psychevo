import { lazy, Suspense, type ReactNode } from "react";
import { Bot, Bug, FileText, FolderTree, GitPullRequest, Globe2, Home, MessageSquare, Plus, RefreshCw, TerminalSquare, Users, X } from "lucide-react";
import { ActionButton, DismissibleDetails, IconButton, type TranscriptAgentSession, type WorkspaceFileLinkContext } from "@psychevo/components";
import type { GatewayClient } from "@psychevo/client";
import type {
  ContextReadResult,
  GatewayRequestScope,
  SessionUsageSummaryView,
  ThreadSnapshot,
  WorkspaceChangesResult,
  WorkspaceDiffResult,
  WorkspaceFileEntry,
  WorkspaceFileWriteResult
} from "@psychevo/protocol";
import type { GatewayThreadEventFeed } from "./gateway-event-feed";
import type { Appearance, DebugEvent, RightWorkspaceBrowserState, RightWorkspacePreview, RightWorkspaceTab, RightWorkspaceTabKind, TerminalNotificationEvent, TraceState } from "./types";
import { BrowserPanel } from "./right-workspace/browser";
import { DebugPanel } from "./right-workspace/debug";
import { FilesPanel } from "./right-workspace/files";
import { WorkspaceFileGatewayAdapterProvider } from "./right-workspace/workspace-file-gateway-adapter";
import { PreviewPanel } from "./right-workspace/preview";
import { ReviewPanel } from "./right-workspace/review";
import { TeamPanel } from "./right-workspace/team";
import { ThreadPanel } from "./right-workspace/thread";
import { SessionObservability } from "./right-workspace/usage";
import {
  rightWorkspaceTabLabel,
  rightWorkspaceTabVisibleForSession
} from "./right-workspace-model";

export {
  createRightTabId,
  fileBasename,
  rightWorkspaceDefaultTitle,
  rightWorkspaceTabLabel,
  rightWorkspaceTabVisibleForSession
} from "./right-workspace-model";
export { SessionUsageGrid, normalizedPercent } from "./right-workspace/usage";

const TerminalPanel = lazy(async () => ({
  default: (await import("./right-workspace/terminal")).TerminalPanel
}));

export function RightWorkspace({
  activeTabId,
  activity,
  appearance,
  client,
  context,
  debugEnabled,
  debugEvents,
  files,
  hostKind,
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
  workspaceFileLinks,
  onActivate,
  onAcceptChange,
  onChangedFile,
  onClose,
  onCopyText,
  onDirtyTabChange,
  onFileTreeOpenChange,
  onOpenFile,
  onOpenAgentSession,
  onOpenExternal,
  onBrowserStateChange,
  onOpenKind,
  onOpenPreview,
  onConsumePendingPrompt,
  onRejectChange,
  onRefresh,
  onRefreshTrace,
  onSaveFile,
  onShowHome
}: {
  activeTabId: string | null;
  activity: ThreadSnapshot["activity"];
  appearance: Appearance;
  client: GatewayClient | null;
  context: ContextReadResult | null;
  debugEnabled: boolean;
  debugEvents: DebugEvent[];
  files: WorkspaceFileEntry[];
  hostKind: string;
  latestGatewayEvent: GatewayThreadEventFeed;
  root: string;
  scope: GatewayRequestScope | null;
  sessionId: string | null;
  status: string;
  usage: SessionUsageSummaryView | null;
  tabs: RightWorkspaceTab[];
  terminalEvents: TerminalNotificationEvent[];
  trace: TraceState;
  truncated: boolean;
  cwd: string;
  workspaceChanges: WorkspaceChangesResult | null;
  workspaceDiff: WorkspaceDiffResult | null;
  workspaceFileLinks?: WorkspaceFileLinkContext | undefined;
  onActivate(tabId: string): void;
  onAcceptChange(turnId: string, path: string): void;
  onChangedFile(path: string): void;
  onClose(tabId: string): void;
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  onDirtyTabChange(tabId: string, dirty: boolean): void;
  onFileTreeOpenChange(tabId: string, open: boolean): void;
  onOpenFile(path: string): void;
  onOpenAgentSession(session: TranscriptAgentSession): void;
  onOpenExternal(url: string): void | Promise<void>;
  onBrowserStateChange(tabId: string, state: RightWorkspaceBrowserState): void;
  onOpenKind(kind: RightWorkspaceTabKind): void;
  onOpenPreview(preview: RightWorkspacePreview): void;
  onConsumePendingPrompt(tabId: string): void;
  onRejectChange(turnId: string, path: string): void;
  onRefresh(): void;
  onRefreshTrace(): void;
  onSaveFile(path: string, content: string, expectedRevision: string | null, force: boolean): Promise<WorkspaceFileWriteResult>;
  onShowHome(): void;
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
              <WorkspaceFileGatewayAdapterProvider
                client={client}
                onCopyText={onCopyText}
                onOpenHtmlPreview={(path, content) => onOpenPreview({
                  content,
                  kind: "html",
                  path,
                  title: path.split(/[\\/]/).pop() || "HTML preview"
                })}
                onSave={onSaveFile}
              >
                <FilesPanel
                  client={client}
                  files={files}
                  root={root}
                  scope={scope}
                  selectedPath={tab.path ?? null}
                  tabId={tab.id}
                  truncated={truncated}
                  onCompare={onChangedFile}
                  onDirtyChange={onDirtyTabChange}
                  fileTreeOpen={tab.fileTreeOpen ?? true}
                  htmlExecutionActive={tab.id === activeTab?.id}
                  onFileTreeOpenChange={(open) => onFileTreeOpenChange(tab.id, open)}
                  onOpen={onOpenFile}
                />
              </WorkspaceFileGatewayAdapterProvider>
            )}
            {tab.kind === "preview" && tab.preview && (
              <PreviewPanel
                htmlExecutionActive={tab.id === activeTab?.id}
                onCopyText={onCopyText}
                preview={tab.preview}
              />
            )}
            {tab.kind === "browser" && (
              <BrowserPanel
                hostKind={hostKind}
                sessionId={sessionId}
                onOpenExternal={onOpenExternal}
                onStateChange={(state) => onBrowserStateChange(tab.id, state)}
                state={tab.browser ?? EMPTY_BROWSER_STATE}
              />
            )}
            {tab.kind === "terminal" && (
              <Suspense fallback={<div className="rightPanelLoading" role="status">Loading terminal…</div>}>
                <TerminalPanel
                  appearance={appearance}
                  client={client}
                  scope={scope}
                  terminalEvents={terminalEvents}
                  cwd={cwd}
                />
              </Suspense>
            )}
            {tab.kind === "debug" && debugEnabled && (
              <DebugPanel
                events={debugEvents}
                trace={trace}
                onRefreshTrace={onRefreshTrace}
              />
            )}
            {tab.kind === "team" && (
              <TeamPanel
                client={client}
                disabled={status !== "connected"}
                latestGatewayEvent={latestGatewayEvent}
                nativeActivities={runtimeActivitiesForThread(tabs, tab.parentThreadId ?? sessionId)}
                scope={scope}
                threadId={tab.parentThreadId ?? sessionId}
                onOpenAgentSession={onOpenAgentSession}
              />
            )}
            {(tab.kind === "sideConversation" || tab.kind === "agentSession") && (
              <ThreadPanel
                client={client}
                disabled={status !== "connected"}
                gatewayEventFeed={latestGatewayEvent}
                kind={tab.kind}
                parentThreadId={tab.parentThreadId ?? sessionId}
                historyFidelity={tab.historyFidelity ?? null}
                pendingPrompt={tab.pendingPrompt ?? null}
                scope={scope}
                threadId={tab.threadId ?? null}
                title={tab.title}
                onCopyText={onCopyText}
                onOpenAgentSession={onOpenAgentSession}
                onPendingPromptConsumed={() => onConsumePendingPrompt(tab.id)}
                workspaceFileLinks={workspaceFileLinks}
              />
            )}
          </div>
        ))}
      </div>
    </section>
  );
}

function runtimeActivitiesForThread(
  tabs: RightWorkspaceTab[],
  rootThreadId: string | null
): RightWorkspaceTab[] {
  if (!rootThreadId) return [];
  const reachable = new Set([rootThreadId]);
  const activities: RightWorkspaceTab[] = [];
  let changed = true;
  while (changed) {
    changed = false;
    for (const tab of tabs) {
      if (
        tab.id.startsWith("runtime-child:")
        && tab.threadId
        && tab.parentThreadId
        && reachable.has(tab.parentThreadId)
        && !reachable.has(tab.threadId)
      ) {
        reachable.add(tab.threadId);
        activities.push(tab);
        changed = true;
      }
    }
  }
  return activities;
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
    menuItems.push({ icon: <Globe2 size={14} />, kind: "browser", label: "Browser" });
    menuItems.push({ icon: <MessageSquare size={14} />, kind: "sideConversation", label: "Side chat" });
    menuItems.push({ icon: <Users size={14} />, kind: "team", label: "Team" });
  }
  return (
    <div className="rightWorkspaceTabs" aria-label="Right workspace tabs" role="tablist">
      <button
        aria-label="Workspace home"
        aria-selected={activeTabId === null}
        className={activeTabId === null ? "is-selected" : ""}
        onClick={onShowHome}
        role="tab"
        title="Workspace home"
        type="button"
      >
        <Home size={14} />
      </button>
      {tabs.map((tab) => (
        <div className={`rightWorkspaceTab ${tab.id === activeTabId ? "is-selected" : ""}`} key={tab.id}>
          <button aria-selected={tab.id === activeTabId} onClick={() => onActivate(tab.id)} role="tab" title={tab.title} type="button">
            {rightWorkspaceTabIcon(tab.kind)}
            <span>{tab.title}</span>
          </button>
          <IconButton icon={<X size={12} />} label={`Close ${tab.title}`} onClick={() => onClose(tab.id)} size="compact" />
        </div>
      ))}
      <DismissibleDetails
        className="rightAddMenu"
        summary={<Plus size={15} />}
        summaryProps={{ "aria-label": "Open right workspace tab", title: "Open tab" }}
      >
        {({ close }) => (
          <div className="pevo-controlPopover" role="menu" aria-label="Open right workspace tab">
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
        <IconButton icon={<RefreshCw size={15} />} label="Refresh workspace" onClick={onRefresh} size="compact" />
      </header>
      <SessionObservability
        context={context}
        hasActiveSession={Boolean(sessionId)}
        usage={usage}
        showCategories
      />
      <nav className="rightHomeNav" aria-label="Open workspace tab">
        <ActionButton block icon={<GitPullRequest size={16} />} onClick={() => onOpenKind("review")} type="button" variant="ghost">Review</ActionButton>
        <ActionButton block icon={<TerminalSquare size={16} />} onClick={() => onOpenKind("terminal")} type="button" variant="ghost">Terminal</ActionButton>
        <ActionButton block icon={<FolderTree size={16} />} onClick={() => onOpenKind("files")} type="button" variant="ghost">Files</ActionButton>
        {sessionId && (
          <ActionButton block icon={<Globe2 size={16} />} onClick={() => onOpenKind("browser")} type="button" variant="ghost">Browser</ActionButton>
        )}
        {sessionId && (
          <ActionButton block icon={<MessageSquare size={16} />} onClick={() => onOpenKind("sideConversation")} type="button" variant="ghost">Side chat</ActionButton>
        )}
        {sessionId && (
          <ActionButton block icon={<Users size={16} />} onClick={() => onOpenKind("team")} type="button" variant="ghost">Team</ActionButton>
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
    case "team":
      return <Users size={14} />;
    case "browser":
      return <Globe2 size={14} />;
    case "preview":
      return <FileText size={14} />;
    case "review":
    default:
      return <GitPullRequest size={14} />;
  }
}

const EMPTY_BROWSER_STATE: RightWorkspaceBrowserState = {
  address: "",
  currentUrl: null,
  reloadKey: 0
};
