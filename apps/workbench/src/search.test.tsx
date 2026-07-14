// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { SessionSummary } from "@psychevo/protocol";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SearchPage } from "./search";

afterEach(cleanup);

const session: SessionSummary = {
  id: "session-12345678",
  cwd: "/work/psychevo",
  project: {
    cwd: "/work/psychevo",
    label: "psychevo",
    displayPath: "/work/psychevo"
  },
  model: null,
  provider: null,
  startedAtMs: 1,
  updatedAtMs: 2,
  endedAtMs: null,
  endReason: null,
  archivedAtMs: null,
  messageCount: 3,
  toolCallCount: 0,
  activity: { running: false, activeTurnId: null, queuedTurns: 0 },
  title: "Optimize session browser",
  displayTitle: "Optimize session browser"
};

describe("SearchPage", () => {
  it("indexes lightweight session fields and uses persisted message count", async () => {
    render(
      <SearchPage
        loadThreadSearchText={vi.fn().mockResolvedValue("")}
        sessions={[session]}
        onOpenSession={vi.fn()}
        onOpenTranscript={vi.fn()}
      />
    );

    fireEvent.change(screen.getByPlaceholderText("Search current workspace"), {
      target: { value: "session browser" }
    });

    expect(await screen.findByText("Optimize session browser")).toBeTruthy();
    expect(screen.getByText("psychevo · 3 entries")).toBeTruthy();
  });

  it("keeps message-body search on the authoritative thread read", async () => {
    const loadThreadSearchText = vi.fn().mockResolvedValue("A hidden authoritative message body");
    render(
      <SearchPage
        loadThreadSearchText={loadThreadSearchText}
        sessions={[session]}
        onOpenSession={vi.fn()}
        onOpenTranscript={vi.fn()}
      />
    );

    fireEvent.change(screen.getByPlaceholderText("Search current workspace"), {
      target: { value: "authoritative message" }
    });

    expect(await screen.findByText("Message match")).toBeTruthy();
    expect(loadThreadSearchText).toHaveBeenCalledWith(session.id);
  });
});
