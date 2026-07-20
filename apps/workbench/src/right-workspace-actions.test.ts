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
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(confirmReplacement);
  const actions = createRightWorkspaceActions({
    activeRightTabId: filesTab.id,
    client: null,
    currentThreadId: null,
    debugEnabled: false,
    dirtyRightTabs: { [filesTab.id]: true },
    rightTabs: [filesTab],
    rightWidthPx: 420,
    scope,
    runAction: async (action) => action(),
    setActiveCommandOverlay,
    setActiveRightTabId,
    setDirtyRightTabs: vi.fn(),
    setMobilePanel,
    setRightCollapsed,
    setRightTabs,
    setRightWidthPx: vi.fn(),
    updateMainView: vi.fn()
  });
  return {
    actions,
    confirm,
    setActiveCommandOverlay,
    setActiveRightTabId,
    setMobilePanel,
    setRightCollapsed,
    setRightTabs
  };
}

describe("right workspace file actions", () => {
  it("keeps a dirty Files target when replacing it with another path is declined", () => {
    const {
      actions,
      confirm,
      setActiveCommandOverlay,
      setActiveRightTabId,
      setMobilePanel,
      setRightCollapsed,
      setRightTabs
    } = createFilesActionHarness(false);

    actions.openRightWorkspaceTab("files", {
      path: "report.pdf",
      title: "report.pdf"
    });

    expect(confirm).toHaveBeenCalledOnce();
    expect(confirm).toHaveBeenCalledWith("Discard unsaved file edits?");
    expect(setRightTabs).not.toHaveBeenCalled();
    expect(setActiveCommandOverlay).not.toHaveBeenCalled();
    expect(setRightCollapsed).not.toHaveBeenCalled();
    expect(setActiveRightTabId).not.toHaveBeenCalled();
    expect(setMobilePanel).not.toHaveBeenCalled();
  });

  it("does not confirm when reopening the same Files path or only revealing its tree", () => {
    const { actions, confirm, setRightTabs } = createFilesActionHarness(false);

    actions.openRightWorkspaceTab("files", {
      path: "notes.md",
      title: "notes.md"
    });
    actions.openRightWorkspaceTab("files", { fileTreeOpen: true });

    expect(confirm).not.toHaveBeenCalled();
    expect(setRightTabs).toHaveBeenCalledTimes(2);
  });

  it("replaces a dirty Files target after confirmation", () => {
    const { actions, confirm, setRightTabs } = createFilesActionHarness(true);

    actions.openRightWorkspaceTab("files", {
      path: "report.pdf",
      title: "report.pdf"
    });

    expect(confirm).toHaveBeenCalledOnce();
    expect(setRightTabs).toHaveBeenCalledOnce();
  });
});
