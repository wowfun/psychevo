// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { GatewayClient } from "@psychevo/client";
import type { GatewayRequestScope, SessionSummary, ThreadImportListResult } from "@psychevo/protocol";
import { SessionArchivePanel } from "./session-archive-panel";

afterEach(cleanup);

const scope: GatewayRequestScope = {
  cwd: "/workspace",
  source: {
    kind: "web",
    rawId: "archive-panel-test",
    lifetime: "persistent",
    rawIdentity: null,
    visibleName: null
  }
};

const archived = {
  id: "thread-archived",
  cwd: "/workspace",
  project: { cwd: "/workspace", label: "workspace", displayPath: "/workspace" },
  model: null,
  provider: null,
  startedAtMs: 1,
  updatedAtMs: 1,
  endedAtMs: null,
  endReason: null,
  archivedAtMs: 2,
  messageCount: 2,
  toolCallCount: 0,
  activity: { running: false, activeTurnId: null, queuedTurns: 0 },
  title: "Archived transcript",
  displayTitle: "Archived transcript",
  lifecycle: { targetLabel: "OpenCode", actions: [{ id: "delete", enabled: true, unavailableReason: null }] }
} satisfies SessionSummary;

const imported: ThreadImportListResult = {
  profiles: [{
    runtimeProfileRef: "opencode",
    profileLabel: "OpenCode",
    targets: [{
      targetId: "target:opencode",
      agentRef: "opencode",
      runtimeProfileRef: "opencode",
      agentLabel: "OpenCode",
      profileLabel: "OpenCode",
      label: "OpenCode",
      ready: true,
      unavailableReason: null
    }],
    status: "ready",
    sessions: [{
      candidateId: "candidate:opaque",
      cwd: "/workspace",
      title: "Agent transcript",
      updatedAt: null
    }],
    nextCursor: null,
    alreadyImportedCount: 0,
    error: null
  }]
};

function renderPanel(overrides: Partial<Parameters<typeof SessionArchivePanel>[0]> = {}) {
  const request = vi.fn(async (method: string) => {
    if (method === "thread/import/list") return imported;
    throw new Error(`unexpected request: ${method}`);
  });
  const props = {
    archivedSessions: [archived],
    client: { request } as unknown as GatewayClient,
    currentThreadId: null,
    disabled: false,
    onActivateArchived: vi.fn(async () => undefined),
    onDeleteArchived: vi.fn(),
    onImportSession: vi.fn(async () => undefined),
    onOpenArchived: vi.fn(async () => undefined),
    onOpenWorkspace: vi.fn(),
    onRefreshArchived: vi.fn(async () => undefined),
    onShowActive: vi.fn(),
    scope,
    ...overrides
  };
  return { ...render(<SessionArchivePanel {...props} />), props, request };
}

describe("SessionArchivePanel", () => {
  it("renders archived Threads immediately while ACP discovery remains asynchronous", () => {
    const pending = new Promise<ThreadImportListResult>(() => undefined);
    const request = vi.fn(async () => pending);
    renderPanel({ client: { request } as unknown as GatewayClient });

    expect(screen.getByText("Archived transcript")).toBeTruthy();
    expect(screen.getByText("Reading ACP Agent sessions...")).toBeTruthy();
    expect(request).toHaveBeenCalledWith("thread/import/list", { scope, cursors: {} });
  });

  it("opens archived history without activation and exposes Activate in its row menu", async () => {
    const { props } = renderPanel();
    fireEvent.click(screen.getByRole("button", { name: /Archived transcript/ }));
    expect(props.onOpenArchived).toHaveBeenCalledWith("thread-archived");

    const row = screen.getByText("Archived transcript").closest("article")!;
    fireEvent.click(row.querySelector("summary")!);
    fireEvent.click(within(row).getByRole("menuitem", { name: "Activate" }));
    await waitFor(() => expect(props.onActivateArchived).toHaveBeenCalledWith("thread-archived"));
    expect(props.onShowActive).toHaveBeenCalled();
  });

  it("imports a clicked ACP candidate as archived and removes the stale discovery row", async () => {
    const { props } = renderPanel();
    const candidate = await screen.findByText("Agent transcript");
    fireEvent.click(candidate.closest("button")!);
    await waitFor(() => expect(props.onImportSession).toHaveBeenCalledWith(
      imported.profiles[0],
      "candidate:opaque",
      "target:opencode",
      false
    ));
    await waitFor(() => expect(screen.queryByText("Agent transcript")).toBeNull());
  });

  it("activates an ACP candidate only from its secondary menu", async () => {
    const { props } = renderPanel();
    const row = (await screen.findByText("Agent transcript")).closest("article")!;
    fireEvent.click(row.querySelector("summary")!);
    fireEvent.click(within(row).getByRole("menuitem", { name: "Activate" }));
    await waitFor(() => expect(props.onImportSession).toHaveBeenLastCalledWith(
      imported.profiles[0],
      "candidate:opaque",
      "target:opencode",
      true
    ));
  });
});
