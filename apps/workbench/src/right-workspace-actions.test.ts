// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";
import type { GatewayRequestScope } from "@psychevo/protocol";
import type { TranscriptPinnedMessage } from "@psychevo/components";
import { createRightWorkspaceActions } from "./right-workspace-actions";
import { rightWorkspaceTabVisibleForSession } from "./right-workspace-model";
import type { RightWorkspaceTab } from "./types";

const scope: GatewayRequestScope = {
  cwd: "/workspace",
  source: {
    kind: "web",
    lifetime: "persistent",
    rawId: null,
    rawIdentity: null,
    visibleName: null
  }
};

afterEach(() => {
  vi.restoreAllMocks();
});

function createFilesActionHarness(confirmReplacement: boolean) {
  const filesTab: RightWorkspaceTab = {
    id: "files:existing",
    kind: "files",
    path: "notes.md",
    title: "notes.md"
  };
  const setActiveCommandOverlay = vi.fn();
  const setActiveRightTabId = vi.fn();
  const setMobilePanel = vi.fn();
  const setRightCollapsed = vi.fn();
  const setRightTabs = vi.fn();
  const setDirtyRightTabs = vi.fn();
  const confirmAction = vi.fn().mockResolvedValue(confirmReplacement);
  const nativeConfirm = vi.spyOn(window, "confirm").mockImplementation(() => {
    throw new Error("native confirmation must not be used");
  });
  const actions = createRightWorkspaceActions({
    activeRightTabId: filesTab.id,
    client: null,
    confirmAction,
    currentThreadId: null,
    debugEnabled: false,
    dirtyRightTabs: { [filesTab.id]: true },
    rightTabs: [filesTab],
    rightWidthPx: 420,
    scope,
    runAction: async (action) => action(),
    setActiveCommandOverlay,
    setActiveRightTabId,
    setDirtyRightTabs,
    setMobilePanel,
    setRightCollapsed,
    setRightTabs,
    setRightWidthPx: vi.fn(),
    updateMainView: vi.fn()
  });
  return {
    actions,
    confirmAction,
    nativeConfirm,
    setActiveCommandOverlay,
    setActiveRightTabId,
    setMobilePanel,
    setRightCollapsed,
    setRightTabs,
    setDirtyRightTabs
  };
}

describe("right workspace file actions", () => {
  it("keeps a dirty Files target when replacing it with another path is declined", async () => {
    const {
      actions,
      confirmAction,
      nativeConfirm,
      setActiveCommandOverlay,
      setActiveRightTabId,
      setMobilePanel,
      setRightCollapsed,
      setRightTabs
    } = createFilesActionHarness(false);

    await actions.openRightWorkspaceTab("files", {
      path: "report.pdf",
      title: "report.pdf"
    });

    expect(confirmAction).toHaveBeenCalledOnce();
    expect(confirmAction).toHaveBeenCalledWith({
      confirmLabel: "Discard edits",
      description: "The unsaved file changes will be lost.",
      title: "Discard unsaved file edits?",
      tone: "caution"
    });
    expect(nativeConfirm).not.toHaveBeenCalled();
    expect(setRightTabs).not.toHaveBeenCalled();
    expect(setActiveCommandOverlay).not.toHaveBeenCalled();
    expect(setRightCollapsed).not.toHaveBeenCalled();
    expect(setActiveRightTabId).not.toHaveBeenCalled();
    expect(setMobilePanel).not.toHaveBeenCalled();
  });

  it("does not confirm when reopening the same Files path or only revealing its tree", () => {
    const { actions, confirmAction, nativeConfirm, setRightTabs } = createFilesActionHarness(false);

    void actions.openRightWorkspaceTab("files", {
      path: "notes.md",
      title: "notes.md"
    });
    void actions.openRightWorkspaceTab("files", { fileTreeOpen: true });

    expect(confirmAction).not.toHaveBeenCalled();
    expect(nativeConfirm).not.toHaveBeenCalled();
    expect(setRightTabs).toHaveBeenCalledTimes(2);
  });

  it("replaces a dirty Files target after confirmation", async () => {
    const { actions, confirmAction, nativeConfirm, setRightTabs } = createFilesActionHarness(true);

    await actions.openRightWorkspaceTab("files", {
      path: "report.pdf",
      title: "report.pdf"
    });

    expect(confirmAction).toHaveBeenCalledOnce();
    expect(nativeConfirm).not.toHaveBeenCalled();
    expect(setRightTabs).toHaveBeenCalledOnce();
  });

  it("keeps or closes a dirty Files tab through product confirmation", async () => {
    const declined = createFilesActionHarness(false);
    await declined.actions.closeRightWorkspaceTab("files:existing");
    expect(declined.confirmAction).toHaveBeenCalledOnce();
    expect(declined.nativeConfirm).not.toHaveBeenCalled();
    expect(declined.setRightTabs).not.toHaveBeenCalled();

    const confirmed = createFilesActionHarness(true);
    await confirmed.actions.closeRightWorkspaceTab("files:existing");
    expect(confirmed.confirmAction).toHaveBeenCalledOnce();
    expect(confirmed.nativeConfirm).not.toHaveBeenCalled();
    expect(confirmed.setRightTabs).toHaveBeenCalledOnce();
    expect(confirmed.setDirtyRightTabs).toHaveBeenCalledOnce();
  });
});

describe("right workspace pinned messages", () => {
  const message: TranscriptPinnedMessage = {
    blockId: "message:1:block",
    createdAtMs: 1,
    entryId: "message:1",
    key: JSON.stringify(["thread-1", "message:1", "message:1:block"]),
    role: "assistant",
    status: "completed",
    text: "A pinned answer that stays available",
    threadId: "thread-1"
  };

  it("creates one application-scoped snapshot tab and reveals Status", () => {
    const harness = createPinnedActionHarness([]);
    const snapshotInput = { ...message };

    harness.actions.togglePinnedMessage(snapshotInput, "Source thread", true);
    snapshotInput.text = "A later source edit";

    const updater = harness.setRightTabs.mock.calls[0]?.[0] as (tabs: RightWorkspaceTab[]) => RightWorkspaceTab[];
    const tabs = updater([]);
    expect(tabs).toHaveLength(1);
    expect(tabs[0]).toMatchObject({
      kind: "pinnedMessage",
      pinnedMessage: { ...message, sourceTitle: "Source thread" },
      title: "Assistant · A pinned answer that stays available"
    });
    expect(tabs[0]?.pinnedMessage?.text).toBe(message.text);
    expect(rightWorkspaceTabVisibleForSession(tabs[0]!, "another-thread")).toBe(true);
    expect(harness.setRightCollapsed).toHaveBeenCalledWith(false);
    expect(harness.setMobilePanel).toHaveBeenCalledWith("status");
    expect(harness.setActiveRightTabId).toHaveBeenCalledWith(tabs[0]?.id);
  });

  it("focuses an existing source block and closes it when unpinned", () => {
    const existing: RightWorkspaceTab = {
      id: "pinned:existing",
      kind: "pinnedMessage",
      pinnedMessage: { ...message, sourceTitle: "Source thread" },
      title: "Assistant · A pinned answer"
    };
    const harness = createPinnedActionHarness([existing]);

    harness.actions.togglePinnedMessage(message, "Changed title", true);
    expect(harness.setRightTabs).not.toHaveBeenCalled();
    expect(harness.setActiveRightTabId).toHaveBeenCalledWith(existing.id);

    harness.actions.togglePinnedMessage(message, "Changed title", false);
    const updater = harness.setRightTabs.mock.calls[0]?.[0] as (tabs: RightWorkspaceTab[]) => RightWorkspaceTab[];
    expect(updater([existing])).toEqual([]);
  });
});

function createPinnedActionHarness(rightTabs: RightWorkspaceTab[]) {
  const setActiveRightTabId = vi.fn();
  const setMobilePanel = vi.fn();
  const setRightCollapsed = vi.fn();
  const setRightTabs = vi.fn();
  const actions = createRightWorkspaceActions({
    activeRightTabId: null,
    client: null,
    confirmAction: vi.fn().mockResolvedValue(true),
    currentThreadId: null,
    debugEnabled: false,
    dirtyRightTabs: {},
    rightTabs,
    rightWidthPx: 420,
    scope,
    runAction: async (action) => action(),
    setActiveCommandOverlay: vi.fn(),
    setActiveRightTabId,
    setDirtyRightTabs: vi.fn(),
    setMobilePanel,
    setRightCollapsed,
    setRightTabs,
    setRightWidthPx: vi.fn(),
    updateMainView: vi.fn()
  });
  return { actions, setActiveRightTabId, setMobilePanel, setRightCollapsed, setRightTabs };
}
