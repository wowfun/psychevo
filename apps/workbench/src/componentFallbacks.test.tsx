// @vitest-environment jsdom

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { HistoryPanel, StatusPanel, TranscriptPanel } from "@psychevo/components";
import type { SessionSummary, TranscriptBlock } from "@psychevo/protocol";
import {
  cwdPath,
  noop,
  sessionSummary,
  setupComponentFallbackTests,
  transcriptBlock,
  transcriptEntry
} from "./componentFallbacks.test-support";

setupComponentFallbackTests();

describe("component fallback rendering", () => {
  it("renders older session summaries without activity metadata", () => {
    const session = {
      id: "thread-old",
      cwd: cwdPath("/tmp/project"),
      project: { cwd: cwdPath("/tmp/project"), label: "project", displayPath: "/tmp/project" },
      model: null,
      provider: null,
      startedAtMs: 1,
      updatedAtMs: null,
      endedAtMs: null,
      endReason: null,
      archivedAtMs: null,
      messageCount: 1,
      toolCallCount: 0,
      visibleEntryCount: 1,
      activity: { running: false, activeTurnId: null, queuedTurns: 0 },
      displayTitle: "Old session",
      preview: null,
      title: "Old session"
    } as SessionSummary;

    const html = renderToStaticMarkup(
      <HistoryPanel
        archived={false}
        sessions={[session]}
        onArchive={noop}
        onDelete={noop}
        onExport={noop}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onShare={noop}
      />
    );

    expect(html).toContain("Old session");
    expect(html).not.toContain("entries");
    expect(html).toContain("title=\"Collapse all workspaces\"");
    expect(html).not.toContain("title=\"Expand all workspaces\"");
  });

  it("renders pin and unpin controls inside the session actions menu", () => {
    const html = renderToStaticMarkup(
      <HistoryPanel
        archived={false}
        pinnedSessionIds={["thread-1"]}
        sessions={[
          sessionSummary({ id: "thread-1", title: "Pinned session" }),
          sessionSummary({ id: "thread-2", title: "Unpinned session" })
        ]}
        onArchive={noop}
        onDelete={noop}
        onExport={noop}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onShare={noop}
        onTogglePinned={noop}
      />
    );

    expect(html).toContain("pevo-sessionMenu");
    expect(html).toContain("role=\"menu\"");
    expect(html).toContain("role=\"menuitem\"");
    expect(html).toContain("title=\"Unpin\"");
    expect(html).toContain("title=\"Pin\"");
    expect(html).not.toContain("pevo-sessionActions");
  });

  it("closes the session actions menu on outside click", async () => {
    const { container } = render(
      <HistoryPanel
        archived={false}
        sessions={[sessionSummary({ id: "thread-1", title: "Menu session" })]}
        onArchive={noop}
        onDelete={noop}
        onExport={noop}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onShare={noop}
      />
    );
    const menu = container.querySelector(".pevo-sessionMenu") as HTMLDetailsElement | null;
    const trigger = container.querySelector(".pevo-sessionMenu summary") as HTMLElement | null;

    fireEvent.click(trigger!);
    await waitFor(() => expect(menu?.open).toBe(true));
    fireEvent.mouseDown(document.body);

    await waitFor(() => expect(menu?.open).toBe(false));
  });

  it("closes the session actions menu on Escape and restores trigger focus", async () => {
    const { container } = render(
      <HistoryPanel
        archived={false}
        sessions={[sessionSummary({ id: "thread-1", title: "Escape session" })]}
        onArchive={noop}
        onDelete={noop}
        onExport={noop}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onShare={noop}
      />
    );
    const trigger = container.querySelector(".pevo-sessionMenu summary") as HTMLElement | null;
    const menu = container.querySelector(".pevo-sessionMenu") as HTMLDetailsElement | null;

    fireEvent.click(trigger!);
    await waitFor(() => expect(menu?.open).toBe(true));
    fireEvent.keyDown(document, { key: "Escape" });

    await waitFor(() => expect(menu?.open).toBe(false));
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it("closes the session actions menu after an enabled menu item fires", async () => {
    const onExport = vi.fn();
    const { container } = render(
      <HistoryPanel
        archived={false}
        sessions={[sessionSummary({ id: "thread-1", title: "Export session" })]}
        onArchive={noop}
        onDelete={noop}
        onExport={onExport}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onShare={noop}
      />
    );
    const menu = container.querySelector(".pevo-sessionMenu") as HTMLDetailsElement | null;
    const trigger = container.querySelector(".pevo-sessionMenu summary") as HTMLElement | null;

    fireEvent.click(trigger!);
    await waitFor(() => expect(menu?.open).toBe(true));
    fireEvent.click(screen.getByRole("menuitem", { name: "Export" }));

    expect(onExport).toHaveBeenCalledWith("thread-1");
    await waitFor(() => expect(menu?.open).toBe(false));
  });

  it("renders workspace creation as a sessions header action", () => {
    const html = renderToStaticMarkup(
      <HistoryPanel
        archived={false}
        sessions={[sessionSummary({ id: "thread-1", title: "Workspace session" })]}
        onArchive={noop}
        onCreateWorkspace={noop}
        onDelete={noop}
        onExport={noop}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onShare={noop}
      />
    );

    expect(html).toContain("title=\"New Workspace\"");
    expect(html.indexOf("title=\"New Workspace\"")).toBeLessThan(html.indexOf("title=\"Collapse all workspaces\""));
  });

  it("does not mark the first history row active without a current thread", () => {
    const sessions = [
      sessionSummary({ id: "thread-1", title: "First session" }),
      sessionSummary({ id: "thread-2", title: "Second session" })
    ];

    const html = renderToStaticMarkup(
      <HistoryPanel
        archived={false}
        sessions={sessions}
        onArchive={noop}
        onDelete={noop}
        onExport={noop}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onShare={noop}
      />
    );

    expect(html).toContain("First session");
    expect(html).toContain("Second session");
    expect(html).not.toContain("is-active");
  });

  it("renders a local draft row without session actions", () => {
    const html = renderToStaticMarkup(
        <HistoryPanel
          archived={false}
          draftSession={{ id: "draft:1", title: "New session", createdAtMs: Date.now(), cwd: "/tmp/project" }}
          sessions={[]}
        onArchive={noop}
        onDelete={noop}
        onExport={noop}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onResumeDraft={noop}
        onShare={noop}
      />
    );

    expect(html).toContain("New session");
    expect(html).toContain("project");
    expect(html).toContain("0d");
    expect(html).toContain("is-active is-draft");
    expect(html).not.toContain("title=\"Session actions\"");
    expect(html).not.toContain("title=\"Rename\"");
    expect(html).not.toContain("title=\"Export\"");
    expect(html).not.toContain("title=\"Share\"");
    expect(html).not.toContain("title=\"Archive\"");
    expect(html).not.toContain("title=\"Delete\"");
  });

  it("renders the local draft row inside its project group", () => {
    const html = renderToStaticMarkup(
      <HistoryPanel
        archived={false}
        draftSession={{ id: "draft:1", title: "New session", createdAtMs: Date.now(), cwd: "/tmp/other" }}
        sessions={[
          sessionSummary({ id: "thread-1", title: "First session" }),
          sessionSummary({
            id: "thread-2",
            title: "Other session",
            cwd: cwdPath("/tmp/other"),
            project: { cwd: cwdPath("/tmp/other"), label: "other", displayPath: "/tmp/other" }
          })
        ]}
        onArchive={noop}
        onDelete={noop}
        onExport={noop}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onResumeDraft={noop}
        onShare={noop}
      />
    );

    expect(html.indexOf("other")).toBeLessThan(html.indexOf("New session"));
    expect(html.indexOf("New session")).toBeLessThan(html.indexOf("Other session"));
  });

  it("does not lift a project group just because its session is active", () => {
    const html = renderToStaticMarkup(
      <HistoryPanel
        archived={false}
        currentThreadId="thread-older"
        sessions={[
          sessionSummary({
            id: "thread-newer",
            title: "Newer session",
            cwd: cwdPath("/tmp/newer-project"),
            project: { cwd: cwdPath("/tmp/newer-project"), label: "newer-project", displayPath: "/tmp/newer-project" },
            startedAtMs: 3_000,
            updatedAtMs: 3_000
          }),
          sessionSummary({
            id: "thread-older",
            title: "Older active session",
            cwd: cwdPath("/tmp/older-project"),
            project: { cwd: cwdPath("/tmp/older-project"), label: "older-project", displayPath: "/tmp/older-project" },
            startedAtMs: 1_000,
            updatedAtMs: 1_000
          })
        ]}
        onArchive={noop}
        onDelete={noop}
        onExport={noop}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onShare={noop}
      />
    );

    expect(html.indexOf("newer-project")).toBeLessThan(html.indexOf("older-project"));
    expect(html).toContain("pevo-sessionRow is-active");
  });

  it("renders compact relative day labels for persisted sessions", () => {
    const nowSpy = vi.spyOn(Date, "now").mockReturnValue(86_400_000 * 5);
    try {
      const html = renderToStaticMarkup(
        <HistoryPanel
          archived={false}
          sessions={[
            sessionSummary({
              id: "thread-1",
              title: "Recent session",
              startedAtMs: 86_400_000 * 2,
              updatedAtMs: 86_400_000 * 2
            })
          ]}
          onArchive={noop}
          onDelete={noop}
          onExport={noop}
          onNew={noop}
          onRename={noop}
          onRestore={noop}
          onResume={noop}
          onShare={noop}
        />
      );

      expect(html).toContain(">3d</time>");
    } finally {
      nowSpy.mockRestore();
    }
  });
});
