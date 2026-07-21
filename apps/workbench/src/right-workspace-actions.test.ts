// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";
import type { GatewayRequestScope } from "@psychevo/protocol";
import { createRightWorkspaceActions } from "./right-workspace-actions";
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
