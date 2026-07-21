// @vitest-environment jsdom

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  agentRecord,
  commandItem,
  deferred,
  gatewayMock,
  observabilityResult,
  openAgentRuntimePopover,
  selectMainAgent,
  selectRuntime,
  sessionSummary,
  workspaceDiffAction
} from "./appComposerAgent.fixture";
import { App } from "./App";

async function openRightInspector() {
  const toggle = await screen.findByRole("button", { name: "Right inspector" });
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  fireEvent.click(toggle);
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
}

describe("Workbench session status observability", () => {
  it("renders the full session id in the Status panel", async () => {
    const longSessionId = "019ebc20-1234-5678-9abc-def0123492dd";
    gatewayMock.sessionSummaries = [sessionSummary(longSessionId, "Long session")];

    render(<App />);

    fireEvent.click(await screen.findByText("Long session"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: longSessionId })
      });
    });
    await openRightInspector();
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "observability/read",
        params: expect.objectContaining({ threadId: longSessionId })
      });
    });
    const home = await screen.findByRole("region", { name: "Workspace status" });
    expect(await within(home).findByText(longSessionId)).toBeTruthy();
    expect(within(home).queryByText("/tmp/project")).toBeNull();
    expect(home.querySelector(".rightStatusMetrics")).toBeNull();
    expect(within(home).queryByText("Session")).toBeNull();
    expect(within(home).queryByText("Connection")).toBeNull();
    expect(within(home).queryByText("Turn")).toBeNull();
    expect(within(home).queryByText("Queued")).toBeNull();
    expect(within(home).getByText("Session tokens")).toBeTruthy();
    expect(within(home).getByText("250")).toBeTruthy();
    expect(within(home).getByText("40%")).toBeTruthy();
    expect(within(home).getByText("$0.010000")).toBeTruthy();
    expect(within(home).queryByText("Messages")).toBeNull();
    expect(within(home).queryByText("Provider")).toBeNull();
    expect(within(home).queryByText("Model")).toBeNull();
    expect(home.querySelector(".sessionContextRing")).toBeNull();
    const stack = home.querySelector(".promptTokenStack");
    expect(stack).toBeTruthy();
    expect(stack?.querySelectorAll(".promptTokenSegment").length).toBe(3);
    expect(stack?.querySelector('[title^="Developer prompt:"]')).toBeTruthy();
    const promptTokens = within(home).getByText("Prompt tokens").closest("details") as HTMLDetailsElement | null;
    expect(promptTokens).toBeTruthy();
    expect(promptTokens?.classList.contains("promptTokensDisclosure")).toBe(true);
    expect(promptTokens?.open).toBe(false);
    const promptSummary = promptTokens!.querySelector("summary") as HTMLElement;
    expect(within(promptSummary).queryByText("3")).toBeNull();
    expect(promptTokens?.querySelectorAll(".promptTokenCategory details").length).toBe(0);
    fireEvent.click(promptSummary);
    expect(promptTokens?.open).toBe(true);
    expect(within(promptTokens as HTMLElement).getByText("Developer prompt")).toBeTruthy();
    expect(within(promptTokens as HTMLElement).getByText("design")).toBeTruthy();
    expect(screen.queryByText("019ebc20...92dd")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Context usage" }));
    const contextPopover = await screen.findByRole("dialog", { name: "Context usage" });
    expect(within(contextPopover).getByText("Session tokens")).toBeTruthy();
    expect(within(contextPopover).getByText("Cache read")).toBeTruthy();
    expect(within(contextPopover).getByText("Cost")).toBeTruthy();
    expect(within(contextPopover).queryByText("Developer prompt")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "New Session" }));
    await waitFor(() => {
      expect(within(home).getByText("No session usage yet.")).toBeTruthy();
    });
  });

  it("does not apply stale session observability after creating a new draft", async () => {
    const staleObservability = deferred<Record<string, unknown>>();
    gatewayMock.sessionSummaries = [sessionSummary("old-thread", "Old session")];
    gatewayMock.observabilityRead = (params: unknown) => {
      const threadId = (params as { threadId?: string | null } | undefined)?.threadId ?? null;
      if (threadId === "old-thread") {
        return staleObservability.promise;
      }
      return observabilityResult(threadId);
    };

    render(<App />);

    fireEvent.click(await screen.findByText("Old session"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "old-thread" })
      });
    });
    await openRightInspector();
    fireEvent.click(screen.getByRole("button", { name: "New Session" }));

    const home = await screen.findByRole("region", { name: "Workspace status" });
    await waitFor(() => {
      expect(within(home).getByText("draft")).toBeTruthy();
      expect(within(home).getByText("No active session")).toBeTruthy();
    });

    await act(async () => {
      staleObservability.resolve(observabilityResult("old-thread", true));
      await staleObservability.promise;
    });

    expect(within(home).getByText("No active session")).toBeTruthy();
    expect(within(home).queryByText("reported by ACP peer")).toBeNull();
    expect(within(home).queryByText("8.0k/200.0k (4.0%)")).toBeNull();
  });

  it("ignores late completed-turn observability refreshes for a previous session", async () => {
    gatewayMock.observabilityRead = (params: unknown) => {
      const threadId = (params as { threadId?: string | null } | undefined)?.threadId ?? null;
      return observabilityResult(threadId, threadId === "old-thread");
    };

    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "New Session" }));
    await openRightInspector();
    const home = await screen.findByRole("region", { name: "Workspace status" });
    await waitFor(() => {
      expect(within(home).getByText("draft")).toBeTruthy();
      expect(within(home).getByText("No active session")).toBeTruthy();
    });

    await act(async () => {
      for (const subscriber of gatewayMock.subscribers) {
        subscriber({
          method: "gateway/event",
          params: {
            type: "turnCompleted",
            threadId: "old-thread",
            turnId: "old-turn",
            turn: {
              id: "old-turn",
              threadId: "old-thread",
              status: "completed",
              outcome: "normal",
              error: null,
              startedAtMs: 1,
              completedAtMs: 2
            },
            committedEntries: []
          }
        });
      }
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "observability/read",
        params: expect.objectContaining({ threadId: "old-thread" })
      });
    });
    expect(within(home).getByText("No active session")).toBeTruthy();
    expect(within(home).queryByText("reported by ACP peer")).toBeNull();
    expect(within(home).queryByText("8.0k/200.0k (4.0%)")).toBeNull();
  });

  it("applies workspace inventory refreshes from completed child turns", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("parent-thread", "Parent session")];
    gatewayMock.workspaceFilesResult = {
      root: gatewayMock.scope.cwd,
      entries: [],
      truncated: false
    };

    render(<App />);

    fireEvent.click(await screen.findByText("Parent session"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "parent-thread" })
      });
    });
    await openRightInspector();
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Files" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "workspace/files")).toBe(true);
    });
    const files = await screen.findByRole("region", { name: "Workspace files" });
    expect(within(files).queryByRole("treeitem", { name: /result\.html/u })).toBeNull();

    gatewayMock.workspaceFilesResult = {
      root: gatewayMock.scope.cwd,
      entries: [
        { path: "generated", name: "generated", kind: "directory", depth: 0 },
        { path: "generated/result.html", name: "result.html", kind: "file", depth: 1 }
      ],
      truncated: false
    };
    const workspaceRequestCount = gatewayMock.requestLog.filter((entry) => (
      entry.method === "workspace/files"
    )).length;

    await act(async () => {
      for (const subscriber of gatewayMock.subscribers) {
        subscriber({
          method: "gateway/event",
          params: {
            type: "turnCompleted",
            threadId: "child-thread",
            turnId: "child-turn",
            turn: {
              id: "child-turn",
              threadId: "child-thread",
              status: "completed",
              outcome: "normal",
              error: null,
              startedAtMs: 1,
              completedAtMs: 2
            },
            committedEntries: []
          }
        });
      }
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => (
        entry.method === "workspace/files"
      )).length).toBeGreaterThan(workspaceRequestCount);
    });
    expect(await within(files).findByRole("treeitem", { name: /result\.html/u })).toBeTruthy();
  });

  it("opens the composer context usage popover without revealing Status", async () => {
    render(<App />);

    expect(screen.queryByRole("region", { name: "Workspace status" })).toBeNull();
    fireEvent.click(await screen.findByRole("button", { name: "Context usage" }));

    const contextPopover = await screen.findByRole("dialog", { name: "Context usage" });
    expect(within(contextPopover).getByText("Context unavailable.")).toBeTruthy();
    expect(within(contextPopover).queryByText("0%")).toBeNull();
    expect(screen.queryByRole("region", { name: "Workspace status" })).toBeNull();
    expect(screen.getByRole("button", { name: "Right inspector" }).getAttribute("aria-expanded")).toBe("false");
  });

  it("shows token usage and Limit unavailable when a runtime reports no context ceiling", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("runtime-context-thread", "Runtime context")];
    gatewayMock.observabilityRead = () => ({
      ...observabilityResult("runtime-context-thread", true),
      context: {
        available: true,
        label: "Runtime context",
        status: "reported by runtime",
        basis: "agent_reported_context",
        appliesToSessionSeq: null,
        usedTokens: 12_345,
        contextLimit: null,
        percent: null,
        categories: [],
        advice: []
      }
    });

    render(<App />);
    fireEvent.click(await screen.findByText("Runtime context"));
    fireEvent.click(await screen.findByRole("button", { name: "Context usage" }));

    const popover = await screen.findByRole("dialog", { name: "Context usage" });
    expect(await within(popover).findByText("12.3k")).toBeTruthy();
    expect(within(popover).getByText("12,345 tokens · Limit unavailable")).toBeTruthy();
    expect(within(popover).queryByText("0%")).toBeNull();
  });
});
