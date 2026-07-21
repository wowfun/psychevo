import type {
  Dispatch,
  PointerEvent as ReactPointerEvent,
  SetStateAction
} from "react";
import type { GatewayClient } from "@psychevo/client";
import type { ConfirmAction, TranscriptAgentSession } from "@psychevo/components";
import type { GatewayRequestScope, WorkspaceDiffResult } from "@psychevo/protocol";
import {
  createRightTabId,
  fileBasename,
  rightWorkspaceDefaultTitle
} from "./right-workspace-model";
import { clampRightWidth } from "./storage";
import type {
  CommandOverlay,
  MainView,
  RightWorkspaceTab,
  RightWorkspaceTabKind
} from "./types";

type RightWorkspaceActionsParams = {
  activeRightTabId: string | null;
  client: GatewayClient | null;
  confirmAction: ConfirmAction;
  currentThreadId: string | null;
  debugEnabled: boolean;
  dirtyRightTabs: Record<string, boolean>;
  rightTabs: RightWorkspaceTab[];
  rightWidthPx: number;
  scope: GatewayRequestScope;
  runAction(action: () => Promise<void>): Promise<void>;
  setActiveCommandOverlay: Dispatch<SetStateAction<CommandOverlay | null>>;
  setActiveRightTabId: Dispatch<SetStateAction<string | null>>;
  setDirtyRightTabs: Dispatch<SetStateAction<Record<string, boolean>>>;
  setMobilePanel: Dispatch<SetStateAction<"history" | "transcript" | "status">>;
  setRightCollapsed: Dispatch<SetStateAction<boolean>>;
  setRightTabs: Dispatch<SetStateAction<RightWorkspaceTab[]>>;
  setRightWidthPx: Dispatch<SetStateAction<number>>;
  updateMainView(value: MainView): void;
};

export function createRightWorkspaceActions(params: RightWorkspaceActionsParams) {
  const confirmDiscardedEdits = () => params.confirmAction({
    confirmLabel: "Discard edits",
    description: "The unsaved file changes will be lost.",
    title: "Discard unsaved file edits?",
    tone: "caution"
  });

  function revealRightWorkspace(tabId: string | null = params.activeRightTabId) {
    params.setActiveCommandOverlay(null);
    params.setRightCollapsed(false);
    params.setActiveRightTabId(tabId);
    params.setMobilePanel("status");
  }

  async function openRightWorkspaceTab(kind: RightWorkspaceTabKind, patch: Partial<RightWorkspaceTab> = {}, forceNew = false) {
    if (kind === "debug" && !params.debugEnabled) {
      return;
    }
    const reusable = kind === "review" || kind === "files" || kind === "debug";
    const ownedThreadId = kind === "browser" ? (patch.threadId ?? params.currentThreadId) : patch.threadId;
    if (kind === "browser" && !ownedThreadId) {
      return;
    }
    const threadReusable = (kind === "agentSession" || kind === "browser") && ownedThreadId;
    const existingThreadTab = threadReusable
      ? params.rightTabs.find((tab) => tab.kind === kind && tab.threadId === ownedThreadId)
      : null;
    const nextId = existingThreadTab?.id
      ?? (reusable && !forceNew ? params.rightTabs.find((tab) => tab.kind === kind)?.id ?? createRightTabId(kind) : createRightTabId(kind));
    const replacedFileTab = kind === "files"
      ? params.rightTabs.find((tab) => tab.id === nextId) ?? null
      : null;
    if (
      replacedFileTab
      && params.dirtyRightTabs[nextId]
      && patch.path !== undefined
      && (replacedFileTab.path ?? null) !== (patch.path ?? null)
      && !await confirmDiscardedEdits()
    ) {
      return;
    }
    const nextTab: RightWorkspaceTab = {
      id: nextId,
      kind,
      title: patch.title ?? rightWorkspaceDefaultTitle(kind),
      threadId: ownedThreadId ?? null,
      parentThreadId: patch.parentThreadId ?? (kind === "team" ? params.currentThreadId : null),
      pendingPrompt: patch.pendingPrompt ?? null,
      path: patch.path ?? null,
      diff: patch.diff ?? null,
      preview: patch.preview ?? null,
      message: patch.message ?? null
    };
    if (kind === "files" && patch.fileTreeOpen !== undefined) {
      nextTab.fileTreeOpen = patch.fileTreeOpen;
    }
    params.setRightTabs((current) => {
      const existing = current.find((tab) => tab.id === nextId);
      if (!existing) {
        return [
          ...current,
          kind === "files" && nextTab.fileTreeOpen === undefined
            ? { ...nextTab, fileTreeOpen: true }
            : nextTab
        ];
      }
      return current.map((tab) => (
        tab.id === nextId
          ? {
              ...tab,
              ...nextTab,
              ...(kind === "files" && patch.path === undefined
                ? { message: tab.message, path: tab.path, title: tab.title }
                : {}),
              id: tab.id,
              kind: tab.kind
            }
          : tab
      ));
    });
    revealRightWorkspace(nextId);
  }

  function clearRightWorkspaceTabPendingPrompt(tabId: string) {
    params.setRightTabs((current) => current.map((tab) => (
      tab.id === tabId ? { ...tab, pendingPrompt: null } : tab
    )));
  }

  async function closeRightWorkspaceTab(tabId: string) {
    if (params.dirtyRightTabs[tabId] && !await confirmDiscardedEdits()) {
      return;
    }
    const closingTab = params.rightTabs.find((tab) => tab.id === tabId) ?? null;
    if (closingTab?.kind === "sideConversation" && closingTab.threadId) {
      const threadId = closingTab.threadId;
      void params.runAction(async () => {
        if (!params.client) {
          return;
        }
        const context = await params.client.request("thread/context/read", {
          threadId,
          target: null,
          scope: params.scope
        });
        if (context.actions.some((action) => action.id === "interrupt" && action.enabled)) {
          await params.client.request("thread/action/run", {
            scope: params.scope,
            threadId,
            action: { kind: "interrupt" }
          });
        }
        await params.client?.request("thread/delete", { threadId });
      });
    }
    params.setRightTabs((current) => current.filter((tab) => tab.id !== tabId));
    params.setDirtyRightTabs((current) => {
      const next = { ...current };
      delete next[tabId];
      return next;
    });
    params.setActiveRightTabId((current) => {
      if (current !== tabId) {
        return current;
      }
      const remaining = params.rightTabs.filter((tab) => tab.id !== tabId);
      return remaining.at(-1)?.id ?? null;
    });
  }

  function openReviewTab(diff: WorkspaceDiffResult, path?: string | null) {
    const selectedPath = diff.selectedPath ?? path ?? null;
    openRightWorkspaceTab("review", {
      diff,
      path: selectedPath,
      title: selectedPath ? fileBasename(selectedPath) : "Review"
    });
  }

  function openAgentSessionTab(session: TranscriptAgentSession) {
    openRightWorkspaceTab("agentSession", {
      parentThreadId: session.parentSessionId ?? params.currentThreadId ?? null,
      threadId: session.childSessionId,
      title: session.taskName ?? session.agentName ?? session.title ?? "Agent"
    });
  }

  function beginRightResize(event: ReactPointerEvent<HTMLButtonElement>) {
    if (window.matchMedia("(max-width: 780px)").matches) {
      return;
    }
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = params.rightWidthPx;
    const pointerId = event.pointerId;
    event.currentTarget.setPointerCapture(pointerId);
    function onPointerMove(moveEvent: PointerEvent) {
      const nextWidth = clampRightWidth(startWidth + startX - moveEvent.clientX);
      params.setRightWidthPx(nextWidth);
    }
    function onPointerUp() {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
    }
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp, { once: true });
  }

  return {
    beginRightResize,
    clearRightWorkspaceTabPendingPrompt,
    closeRightWorkspaceTab,
    openAgentSessionTab,
    openReviewTab,
    openRightWorkspaceTab,
    revealRightWorkspace
  };
}
