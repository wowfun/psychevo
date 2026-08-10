// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { SessionSummary } from "@psychevo/protocol";
import type { ComponentProps } from "react";
import { HistoryPanel } from "./history";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

function session(overrides: Partial<SessionSummary> = {}): SessionSummary {
  return {
    id: "session-1234567890",
    cwd: "/work/chat",
    project: {
      cwd: "/work/chat",
      label: "chat",
      displayPath: "/work/chat"
    },
    model: "fake-model",
    provider: "fake-provider",
    startedAtMs: Date.now(),
    updatedAtMs: Date.now(),
    endedAtMs: null,
    endReason: null,
    archivedAtMs: null,
    messageCount: 0,
    toolCallCount: 0,
    activity: {
      running: false,
      activeTurnId: null,
      queuedTurns: 0
    },
    title: null,
    displayTitle: "A very long session title that needs persistent hover disclosure",
    ...overrides
  };
}

function renderHistory(props: Partial<ComponentProps<typeof HistoryPanel>> = {}) {
  return render(
    <HistoryPanel
      archived={false}
      sessions={[session()]}
      onArchive={vi.fn()}
      onDelete={vi.fn()}
      onExport={vi.fn()}
      onNew={vi.fn()}
      onRename={vi.fn()}
      onRestore={vi.fn()}
      onResume={vi.fn()}
      onShare={vi.fn()}
      {...props}
    />
  );
}

describe("HistoryPanel", () => {
  it("suppresses the successful-empty state while the first history request is pending", () => {
    const { container, rerender } = renderHistory({ loading: true, sessions: [] });

    const panel = screen.getByRole("region", { name: "Sessions" });
    expect(panel.getAttribute("aria-busy")).toBe("true");
    expect(screen.queryByText("No sessions")).toBeNull();

    rerender(
      <HistoryPanel
        archived={false}
        loading={false}
        sessions={[]}
        onArchive={vi.fn()}
        onDelete={vi.fn()}
        onExport={vi.fn()}
        onNew={vi.fn()}
        onRename={vi.fn()}
        onRestore={vi.fn()}
        onResume={vi.fn()}
        onShare={vi.fn()}
      />
    );
    expect(container.querySelector('[aria-busy="true"]')).toBeNull();
    expect(screen.getByText("No sessions")).toBeTruthy();
  });

  it("opens imported-and-archived history and renders lifecycle actions from product descriptors", () => {
    const onImportSessions = vi.fn();
    const onFork = vi.fn();
    const onDelete = vi.fn();
    const { container } = renderHistory({
      onDelete,
      onFork,
      onImportSessions,
      sessions: [session({
        lifecycle: {
          targetLabel: "OpenCode",
          actions: [
            { id: "fork", enabled: true, unavailableReason: null },
            { id: "delete", enabled: false, unavailableReason: "OpenCode cannot delete sessions." }
          ]
        }
      })]
    });

    fireEvent.click(screen.getByRole("button", { name: "Imported and archived sessions" }));
    expect(onImportSessions).toHaveBeenCalledTimes(1);
    fireEvent.click(container.querySelector(".pevo-sessionMenu summary") as HTMLElement);
    fireEvent.click(screen.getByRole("menuitem", { name: "Fork" }));
    expect(onFork).toHaveBeenCalledWith("session-1234567890");
    const deleteButton = screen.getByRole("menuitem", { name: "Delete" });
    expect((deleteButton as HTMLButtonElement).disabled).toBe(true);
    expect(deleteButton.getAttribute("title")).toBe("OpenCode cannot delete sessions.");
    expect(onDelete).not.toHaveBeenCalled();
  });

  it("allows archive and delete for the idle current session but not while it is running", () => {
    const onArchive = vi.fn();
    const onDelete = vi.fn();
    const { container, rerender } = renderHistory({
      currentThreadId: "session-1234567890",
      onArchive,
      onDelete
    });

    fireEvent.click(container.querySelector(".pevo-sessionMenu summary") as HTMLElement);
    const idleArchive = screen.getByRole("menuitem", { name: "Archive" }) as HTMLButtonElement;
    const idleDelete = screen.getByRole("menuitem", { name: "Delete" }) as HTMLButtonElement;
    expect(idleArchive.disabled).toBe(false);
    expect(idleDelete.disabled).toBe(false);

    fireEvent.click(idleArchive);
    expect(onArchive).toHaveBeenCalledWith("session-1234567890");
    fireEvent.click(container.querySelector(".pevo-sessionMenu summary") as HTMLElement);
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(onDelete).toHaveBeenCalledWith("session-1234567890");

    rerender(
      <HistoryPanel
        archived={false}
        currentThreadId="session-1234567890"
        sessions={[session({ activity: { running: true, activeTurnId: "turn-1", queuedTurns: 0 } })]}
        onArchive={onArchive}
        onDelete={onDelete}
        onExport={vi.fn()}
        onNew={vi.fn()}
        onRename={vi.fn()}
        onRestore={vi.fn()}
        onResume={vi.fn()}
        onShare={vi.fn()}
      />
    );
    fireEvent.click(container.querySelector(".pevo-sessionMenu summary") as HTMLElement);
    expect((screen.getByRole("menuitem", { name: "Archive" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("menuitem", { name: "Delete" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("labels the workspace action as opening rather than creating", () => {
    const onCreateWorkspace = vi.fn();
    renderHistory({ onCreateWorkspace });

    fireEvent.click(screen.getByRole("button", { name: "Open workspace" }));
    expect(onCreateWorkspace).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "New Workspace" })).toBeNull();
  });

  it("uses the native title tooltip for truncated session titles", () => {
    const { container } = renderHistory();

    const title = "A very long session title that needs persistent hover disclosure";
    expect(screen.getByTitle(title)).toBeTruthy();
    expect(container.querySelector(".pevo-sessionTitlePopover")).toBeNull();
  });

  it("renders pinned sessions through the shared row and complete action menu", () => {
    const onTogglePinned = vi.fn();
    const { container } = renderHistory({
      pinned: true,
      pinnedSessionIds: ["session-1234567890"],
      onTogglePinned
    });

    expect(container.querySelector(".pevo-sessionRow .pevo-sessionMain")).toBeTruthy();
    expect(container.querySelector(".pinnedSessionRow")).toBeNull();
    fireEvent.click(container.querySelector('.pevo-sessionMenu summary[aria-label="Session actions"]') as HTMLElement);
    expect(screen.getByRole("menuitem", { name: "Unpin" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "Rename" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "Export" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "Share" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "Archive" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "Delete" })).toBeTruthy();

    fireEvent.click(screen.getByRole("menuitem", { name: "Unpin" }));
    expect(onTogglePinned).toHaveBeenCalledWith("session-1234567890");
    expect((container.querySelector(".pevo-sessionMenu") as HTMLDetailsElement).open).toBe(false);
  });

  it("renders pinned Threads before complete pinned Workspace groups", () => {
    const onLoadOlderSessions = vi.fn();
    const pinnedThread = session({
      id: "thread-pinned",
      displayTitle: "Pinned Thread",
      updatedAtMs: 20
    });
    const workspaceThread = session({
      id: "workspace-thread",
      displayTitle: "Workspace Thread",
      updatedAtMs: 10
    });
    const { container } = renderHistory({
      pinned: true,
      pinnedSessionIds: [pinnedThread.id],
      pinnedWorkspaces: [{
        id: "workspace-1",
        name: "Pinned Workspace",
        roots: ["/work/chat"],
        sessionIds: [workspaceThread.id],
        revision: 0,
        cwd: "/work/chat",
        hiddenCount: 4,
        pinned: true
      }],
      sessions: [workspaceThread, pinnedThread],
      onLoadOlderSessions
    });

    const text = container.textContent ?? "";
    expect(text.indexOf("Pinned Thread")).toBeLessThan(text.indexOf("Pinned Workspace"));
    expect(text.indexOf("Pinned Workspace")).toBeLessThan(text.indexOf("Workspace Thread"));
    fireEvent.click(screen.getByRole("button", { name: /Older sessions/ }));
    expect(onLoadOlderSessions).toHaveBeenCalledWith("/work/chat");
  });

  it("keeps pinned Workspace row controls equivalent to ordinary placement", () => {
    const onNewInWorkspace = vi.fn();
    renderHistory({
      onNewInWorkspace,
      pinned: true,
      pinnedWorkspaces: [{
        id: "workspace-1",
        name: "Pinned Workspace",
        roots: ["/work/chat"],
        sessionIds: ["workspace-thread"],
        revision: 0,
        cwd: "/work/chat",
        hiddenCount: 0,
        pinned: true
      }],
      sessions: [session({ id: "workspace-thread" })]
    });

    const toggle = screen.getByRole("button", { name: "Pinned Workspace" });
    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(onNewInWorkspace).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "New session in Pinned Workspace" }));
    expect(onNewInWorkspace).toHaveBeenCalledWith(expect.objectContaining({ id: "workspace-1" }));
  });

  it("keeps pinned Threads in the persisted navigation order", () => {
    const olderFirst = session({
      id: "thread-new-pin",
      displayTitle: "Newly pinned older Thread",
      updatedAtMs: 10
    });
    const newerSecond = session({
      id: "thread-old-pin",
      displayTitle: "Previously pinned newer Thread",
      updatedAtMs: 20
    });
    const { container } = renderHistory({
      pinned: true,
      pinnedSessionIds: [olderFirst.id, newerSecond.id],
      sessions: [newerSecond, olderFirst]
    });

    const text = container.textContent ?? "";
    expect(text.indexOf("Newly pinned older Thread"))
      .toBeLessThan(text.indexOf("Previously pinned newer Thread"));
  });

  it("renders a Workspace and its pagination control when all sessions are hidden", () => {
    const onLoadOlderSessions = vi.fn();
    renderHistory({
      sessions: [],
      browserWorkspaces: [{
        id: "workspace-hidden",
        name: "Archived in time",
        roots: ["/work/hidden"],
        sessionIds: [],
        revision: 1,
        cwd: "/work/hidden",
        hiddenCount: 3
      }],
      onLoadOlderSessions
    });

    expect(screen.getByText("Archived in time")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Older sessions/ }));
    expect(onLoadOlderSessions).toHaveBeenCalledWith("/work/hidden");
  });

  it("resolves explicit Workspace membership before cwd containment fallback", () => {
    renderHistory({
      sessions: [session({
        id: "bound-thread",
        cwd: "/adopted/root",
        project: { cwd: "/adopted/root", label: "retained", displayPath: "/adopted/root" },
        displayTitle: "Explicitly bound Thread"
      })],
      browserWorkspaces: [
        {
          id: "cwd-owner",
          name: "Cwd owner",
          roots: ["/adopted/root"],
          sessionIds: [],
          revision: 1,
          cwd: "/adopted/root",
          hiddenCount: 0
        },
        {
          id: "bound-owner",
          name: "Bound owner",
          roots: ["/different/root"],
          sessionIds: ["bound-thread"],
          revision: 2,
          cwd: "/different/root",
          hiddenCount: 0
        }
      ]
    });

    expect(screen.getByRole("button", { name: "Cwd owner" }).closest("section")?.textContent)
      .not.toContain("Explicitly bound Thread");
    expect(screen.getByRole("button", { name: "Bound owner" }).closest("section")?.textContent)
      .toContain("Explicitly bound Thread");
  });

  it("indexes Workspace membership instead of scanning every id list per Session", () => {
    const sessionIds = new Proxy(["indexed-thread"], {
      get(target, property, receiver) {
        if (property === "includes") throw new Error("linear membership scan");
        return Reflect.get(target, property, receiver);
      }
    });
    renderHistory({
      sessions: [session({ id: "indexed-thread", displayTitle: "Indexed Thread" })],
      browserWorkspaces: [{
        id: "indexed-workspace",
        name: "Indexed Workspace",
        roots: ["/other"],
        sessionIds,
        revision: 1,
        cwd: "/other",
        hiddenCount: 0
      }]
    });

    expect(screen.getByRole("button", { name: "Indexed Workspace" }).closest("section")?.textContent)
      .toContain("Indexed Thread");
  });

  it("keeps scalar fork provenance visible when the source row is unavailable", () => {
    renderHistory({
      sessions: [session({ forkedFromThreadId: "source-thread-abcdef" })]
    });

    expect(screen.getByText("Forked from source-thr")).toBeTruthy();
  });

  it("keeps long session titles separate from time and running status", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-06-14T00:01:05.000Z"));
    const { container } = renderHistory({
      sessions: [
        session({
          displayTitle: "A very long session title that should truncate before covering session metadata",
          updatedAtMs: new Date("2026-06-14T00:00:00.000Z").getTime(),
          activity: {
            running: true,
            activeTurnId: "turn-1",
            queuedTurns: 0,
            startedAtMs: new Date("2026-06-14T00:00:00.000Z").getTime()
          }
        })
      ]
    });

    const row = container.querySelector(".pevo-sessionRow");
    const title = container.querySelector(".pevo-sessionTitle");
    const meta = container.querySelector(".pevo-sessionMeta");
    expect(row).toBeTruthy();
    expect(title?.getAttribute("title")).toContain("should truncate");
    expect(meta?.querySelector(".pevo-sessionTime")?.textContent).toBeTruthy();
    expect(meta?.querySelector('[aria-label="running"]')).toBeTruthy();
  });

  it("shows only the running spinner in rows and loads older sessions by workspace", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-06-14T00:01:05.000Z"));
    const onLoadOlderSessions = vi.fn();
    renderHistory({
      sessions: [
        session({
          activity: {
            running: true,
            activeTurnId: "turn-1",
            queuedTurns: 0,
            startedAtMs: new Date("2026-06-14T00:00:00.000Z").getTime()
          }
        })
      ],
      browserWorkspaces: [{
        id: "workspace-1",
        name: "chat",
        roots: ["/work/chat"],
        revision: 0,
        cwd: "/work/chat",
        hiddenCount: 7
      }],
      onLoadOlderSessions
    });

    expect(screen.getAllByLabelText("running").length).toBeGreaterThan(0);
    expect(screen.queryByText("1m05s")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /Older sessions/ }));
    expect(onLoadOlderSessions).toHaveBeenCalledWith("/work/chat");
  });

  it("keeps a bound session in its stable workspace after its cwd leaves the root list", () => {
    renderHistory({
      sessions: [session({ cwd: "/work/old", project: { cwd: "/work/old", label: "old", displayPath: "/work/old" } })],
      browserWorkspaces: [{
        id: "workspace-1",
        name: "Renamed workspace",
        roots: ["/work/new"],
        sessionIds: ["session-1234567890"],
        revision: 2,
        cwd: "/work/new",
        hiddenCount: 0
      }]
    });

    expect(screen.getByText("Renamed workspace")).toBeTruthy();
    expect(screen.queryByText("old")).toBeNull();
  });

  it("groups a nested draft under the longest containing Workspace root", () => {
    renderHistory({
      sessions: [],
      draftSession: {
        id: "draft-nested",
        cwd: "/repo/apps/web",
        createdAtMs: 5,
        title: ""
      },
      browserWorkspaces: [
        {
          id: "workspace-repo",
          name: "Repository",
          roots: ["/repo"],
          sessionIds: [],
          revision: 1,
          cwd: "/repo",
          hiddenCount: 0
        },
        {
          id: "workspace-apps",
          name: "Applications",
          roots: ["/repo/apps"],
          sessionIds: [],
          revision: 1,
          cwd: "/repo/apps",
          hiddenCount: 0
        }
      ]
    });

    const draft = screen.getByText("New session").closest(".pevo-sessionGroup");
    expect(draft?.textContent).toContain("Applications");
    expect(draft?.textContent).not.toContain("Repository");
  });
});
