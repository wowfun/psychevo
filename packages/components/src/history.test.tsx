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
    workdir: "/work/chat",
    project: {
      workdir: "/work/chat",
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
    visibleEntryCount: 0,
    activity: {
      running: false,
      activeTurnId: null,
      queuedTurns: 0
    },
    title: null,
    displayTitle: "A very long session title that needs persistent hover disclosure",
    preview: null,
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
  it("uses the native title tooltip for truncated session titles", () => {
    const { container } = renderHistory();

    const title = "A very long session title that needs persistent hover disclosure";
    expect(screen.getByTitle(title)).toBeTruthy();
    expect(container.querySelector(".pevo-sessionTitlePopover")).toBeNull();
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
      browserWorkspaces: [{ workdir: "/work/chat", hiddenCount: 7 }],
      onLoadOlderSessions
    });

    expect(screen.getAllByLabelText("running").length).toBeGreaterThan(0);
    expect(screen.queryByText("1m05s")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /Older sessions/ }));
    expect(onLoadOlderSessions).toHaveBeenCalledWith("/work/chat");
  });
});
