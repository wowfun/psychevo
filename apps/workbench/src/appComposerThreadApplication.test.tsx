// @vitest-environment jsdom

import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { ThreadController } from "@psychevo/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { deferred, gatewayMock, sessionSummary } from "./appComposerAgent.fixture";
import { App } from "./App";

afterEach(() => vi.restoreAllMocks());

describe("Workbench public Thread Application interactions", () => {
  it("renders the resumed snapshot without waiting for paginated history", async () => {
    const history = deferred<Record<string, unknown>>();
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Snapshot session")];
    (gatewayMock.snapshot as { entries: Array<Record<string, unknown>> }).entries = [
      transcriptEntry("Snapshot first paint.", "completed", "thread-1", 1)
    ];
    gatewayMock.threadHistoryRead = () => history.promise;

    render(<App />);
    fireEvent.click(await screen.findByText("Snapshot session"));

    expect(await screen.findByText("Snapshot first paint.")).toBeTruthy();
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/resume")).toHaveLength(1);
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/history/read")).toHaveLength(0);
  });

  it("deduplicates auxiliary refreshes when a session activates", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Auxiliary session")];
    render(<App />);

    await screen.findByRole("button", { name: "Agent target" });
    gatewayMock.requestLog.length = 0;
    fireEvent.click(await screen.findByText("Auxiliary session"));

    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "observability/read")).toBe(true);
      expect(gatewayMock.requestLog.some((entry) => entry.method === "command/list")).toBe(true);
    });
    await act(async () => {
      await Promise.resolve();
    });
    for (const method of [
      "settings/read",
      "workspace/files",
      "workspace/diff",
      "workspace/changes",
      "observability/read",
      "command/list"
    ]) {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === method), method).toHaveLength(1);
    }
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "agent/list")).toHaveLength(0);
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "backend/list")).toHaveLength(0);
  });

  it("responds to permission through the sealed Thread interaction union", async () => {
    gatewayMock.snapshot.pendingActions = [{
      actionId: "permission-1",
      kind: "permission",
      title: "Run command",
      summary: "Run cargo test",
      payload: { toolName: "exec_command", summary: "Run cargo test" },
      threadId: "thread-1",
      turnId: "turn-1"
    }];

    render(<App />);
    await resumeSession();
    fireEvent.click(await screen.findByRole("button", { name: "Once" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/interaction/respond",
        params: {
          scope: gatewayMock.scope,
          threadId: "thread-1",
          interactionId: "permission-1",
          response: { kind: "permission", decision: "allowOnce" }
        }
      });
    });
    expect(gatewayMock.requestLog.some((entry) => entry.method === "permission/respond")).toBe(false);
    expect(await screen.findByText("Permission response accepted.")).toBeTruthy();
  });

  it("submits clarification answers through the sealed Thread interaction union", async () => {
    gatewayMock.snapshot.pendingActions = [{
      actionId: "clarify-1",
      kind: "clarify",
      title: "Clarify",
      summary: "Choose a target",
      payload: {
        raw: {
          questions: [{
            question: "Which target?",
            options: [{ label: "Workspace", description: "Current workspace" }],
            multiple: false,
            custom: false,
            secret: false
          }]
        }
      },
      threadId: "thread-1",
      turnId: "turn-1"
    }];

    render(<App />);
    await resumeSession();
    fireEvent.click(await screen.findByRole("radio", { name: /Workspace/ }));
    fireEvent.click(screen.getByRole("button", { name: "Submit" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/interaction/respond",
        params: {
          scope: gatewayMock.scope,
          threadId: "thread-1",
          interactionId: "clarify-1",
          response: { kind: "clarify", answers: [["Workspace"]] }
        }
      });
    });
    expect(gatewayMock.requestLog.some((entry) => entry.method === "clarify/respond")).toBe(false);
    expect(await screen.findByText("Clarify response accepted.")).toBeTruthy();
  });

  it("renders a safe Codex App URL elicitation and returns its typed acceptance", async () => {
    gatewayMock.snapshot.pendingActions = [{
      actionId: "codex-elicitation:1",
      kind: "clarify",
      title: "Codex App request",
      summary: "Finish sign-in",
      payload: {
        owner: "codex_capability_broker",
        raw: {
          url: "https://example.com/complete",
          questions: [{
            question: "Finish sign-in",
            options: [{ label: "Open", description: "https://example.com/complete" }],
            multiple: false,
            custom: false,
            secret: false,
            required: true
          }]
        }
      },
      threadId: "thread-1",
      turnId: "turn-1"
    }];

    render(<App />);
    await resumeSession();
    const link = await screen.findByRole("link", { name: "Open Codex App link" });
    expect(link.getAttribute("href")).toBe("https://example.com/complete");
    expect(link.getAttribute("target")).toBe("_blank");
    fireEvent.click(screen.getByRole("button", { name: "Submit" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/interaction/respond",
        params: {
          scope: gatewayMock.scope,
          threadId: "thread-1",
          interactionId: "codex-elicitation:1",
          response: { kind: "clarify", answers: [["Open"]] }
        }
      });
    });
  });

  it("renders the pinned Codex openai image-picker elicitation", async () => {
    gatewayMock.snapshot.pendingActions = [{
      actionId: "codex-elicitation:image",
      kind: "clarify",
      title: "Codex App request",
      summary: "Choose a report",
      payload: {
        owner: "codex_capability_broker",
        raw: {
          questions: [{
            question: "Choose a report",
            options: [{
              label: "monthly-review",
              description: "Monthly review",
              image: "data:image/png;base64,AA=="
            }],
            multiple: false,
            custom: false,
            secret: false,
            required: true
          }]
        }
      },
      threadId: "thread-1",
      turnId: "turn-1"
    }];

    const { container } = render(<App />);
    await resumeSession();
    expect(await screen.findByText("Monthly review")).toBeTruthy();
    const image = container.querySelector(".composerClarifyOptionImage") as HTMLImageElement | null;
    expect(image?.getAttribute("src")).toBe("data:image/png;base64,AA==");
    fireEvent.click(screen.getByRole("button", { name: "Submit" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/interaction/respond",
        params: {
          scope: gatewayMock.scope,
          threadId: "thread-1",
          interactionId: "codex-elicitation:image",
          response: { kind: "clarify", answers: [["monthly-review"]] }
        }
      });
    });
  });

  it("interrupts only through an enabled Thread action descriptor", async () => {
    gatewayMock.snapshot.activity = { running: true, activeTurnId: "turn-1", queuedTurns: 0 };

    render(<App />);
    await resumeSession();
    await waitForBoundContext();
    fireEvent.click(await screen.findByRole("button", { name: "Interrupt active turn" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/action/run",
        params: {
          scope: gatewayMock.scope,
          threadId: "thread-1",
          action: { kind: "interrupt" }
        }
      });
    });
  });

  it("replaces the ThreadController snapshot when the selected session changes", async () => {
    const reset = vi.spyOn(ThreadController.prototype, "reset");

    render(<App />);
    await resumeSession();

    await waitFor(() => {
      expect(reset.mock.calls.some(([snapshot]) => snapshot?.thread?.id === "thread-1")).toBe(true);
    });
    expect(await screen.findByText("Active session")).toBeTruthy();
  });

  it("routes first-turn streaming and terminal completion through ThreadController before acceptance", async () => {
    const pending = deferred<Record<string, unknown>>();
    gatewayMock.turnStart = () => pending.promise;
    const beginTurn = vi.spyOn(ThreadController.prototype, "beginTurn");
    const acceptTurnStart = vi.spyOn(ThreadController.prototype, "acceptTurnStart");
    const applyGatewayEvent = vi.spyOn(ThreadController.prototype, "applyGatewayEvent");
    const applyTurnResult = vi.spyOn(ThreadController.prototype, "applyTurnResult");

    render(<App />);
    await waitForDraftContext();
    fireEvent.change(screen.getByPlaceholderText("Ask Psychevo..."), {
      target: { value: "first turn" }
    });
    await waitFor(() => {
      expect((screen.getByRole("button", { name: "Send message" }) as HTMLButtonElement).disabled).toBe(false);
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => expect(beginTurn).toHaveBeenCalledTimes(1));
    emit("gateway/event", {
      selectedSkills: [],
      threadId: null,
      turnId: "turn-first",
      type: "turnStarted"
    });
    emit("gateway/event", {
      entry: transcriptEntry("Streaming before acceptance.", "running"),
      turnId: "turn-first",
      type: "entryUpdated"
    });
    expect(await screen.findByText("Streaming before acceptance.")).toBeTruthy();

    emit("turn/result", turnResult("Completed before acceptance."));
    expect(await screen.findByText("Completed before acceptance.")).toBeTruthy();
    expect(applyGatewayEvent).toHaveBeenCalled();
    expect(applyTurnResult).toHaveBeenCalledTimes(1);

    await act(async () => {
      pending.resolve(turnStartResult());
      await pending.promise;
    });
    await waitFor(() => expect(acceptTurnStart).toHaveBeenCalledTimes(1));
    expect(screen.getByText("Completed before acceptance.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Interrupt active turn" })).toBeNull();
  });

  it("routes a terminal error through ThreadController without resurrecting its turn on acceptance", async () => {
    const pending = deferred<Record<string, unknown>>();
    gatewayMock.turnStart = () => pending.promise;
    const acceptTurnStart = vi.spyOn(ThreadController.prototype, "acceptTurnStart");
    const applyTurnError = vi.spyOn(ThreadController.prototype, "applyTurnError");

    render(<App />);
    await waitForDraftContext();
    fireEvent.change(screen.getByPlaceholderText("Ask Psychevo..."), {
      target: { value: "fail first turn" }
    });
    await waitFor(() => {
      expect((screen.getByRole("button", { name: "Send message" }) as HTMLButtonElement).disabled).toBe(false);
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(true);
    });

    emit("gateway/event", {
      selectedSkills: [],
      threadId: null,
      turnId: "turn-first",
      type: "turnStarted"
    });
    emit("turn/error", {
      error: {
        code: "fixture_failure",
        delivery: "notDelivered",
        diagnosticRef: null,
        message: "Turn failed before acceptance.",
        recoveryAction: null,
        retryClass: "retry",
        stage: "runtime"
      },
      threadId: "thread-first",
      turnId: "turn-first"
    });
    expect(await screen.findByText("Turn failed before acceptance.")).toBeTruthy();
    expect(applyTurnError).toHaveBeenCalledTimes(1);

    await act(async () => {
      pending.resolve(turnStartResult());
      await pending.promise;
    });
    await waitFor(() => expect(acceptTurnStart).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("button", { name: "Interrupt active turn" })).toBeNull();
  });

  it("rolls back optimistic input and refreshes Thread Context after turn/start rejects", async () => {
    gatewayMock.turnStart = () => Promise.reject(new Error(
      "Thread Context changed; refresh it before starting the turn."
    ));
    const rejectTurnStart = vi.spyOn(ThreadController.prototype, "rejectTurnStart");

    render(<App />);
    await waitForDraftContext();
    const readsBefore = gatewayMock.requestLog.filter((entry) => (
      entry.method === "thread/context/read"
    )).length;
    fireEvent.change(screen.getByPlaceholderText("Ask Psychevo..."), {
      target: { value: "say hi" }
    });
    await waitFor(() => {
      expect((screen.getByRole("button", { name: "Send message" }) as HTMLButtonElement).disabled)
        .toBe(false);
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByText(
      "Thread Context changed; refresh it before starting the turn."
    )).toBeTruthy();
    await waitFor(() => expect(rejectTurnStart).toHaveBeenCalledTimes(1));
    expect(screen.queryByText("say hi")).toBeNull();
    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => (
        entry.method === "thread/context/read"
      )).length).toBeGreaterThan(readsBefore);
    });
  });
});

function emit(method: string, params: unknown) {
  act(() => {
    for (const subscriber of gatewayMock.subscribers) subscriber({ method, params });
  });
}

function turnStartResult(): Record<string, unknown> {
  return {
    accepted: true,
    threadId: "thread-first",
    turnId: "turn-first",
    thread: gatewayThread()
  };
}

function turnResult(body: string): Record<string, unknown> {
  return {
    thread: gatewayThread(),
    turn: {
      completedAtMs: 2_000,
      error: null,
      id: "turn-first",
      outcome: "completed",
      startedAtMs: 1_000,
      status: "completed",
      threadId: "thread-first"
    },
    result: {
      finalAnswer: body,
      model: "fixture/default",
      outcome: "completed",
      provider: "fixture",
      sessionId: "thread-first",
      toolFailures: 0
    },
    committedEntries: [transcriptEntry(body, "completed", "thread-first", 2)]
  };
}

function gatewayThread(): Record<string, unknown> {
  return {
    backend: { kind: "native", runtimeRef: "native", sessionHandle: "thread-first" },
    id: "thread-first",
    sourceKey: null
  };
}

function transcriptEntry(
  body: string,
  status: "running" | "completed",
  threadId = "",
  messageSeq: number | null = null
): Record<string, unknown> {
  return {
    accounting: null,
    blocks: [{
      artifactIds: [],
      body,
      createdAtMs: 1_000,
      detail: body,
      id: "assistant:first:text",
      kind: "text",
      metadata: null,
      order: 0,
      preview: body,
      result: null,
      source: "runtime.message",
      status,
      title: null,
      updatedAtMs: status === "completed" ? 2_000 : 1_100
    }],
    createdAtMs: 1_000,
    id: "assistant:first",
    messageSeq,
    metadata: null,
    role: "assistant",
    source: "runtime.message",
    status,
    threadId,
    turnId: "turn-first",
    updatedAtMs: status === "completed" ? 2_000 : 1_100,
    usage: null
  };
}

async function resumeSession() {
  gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Active session")];
  fireEvent.click(await screen.findByText("Active session"));
  await waitFor(() => {
    expect(gatewayMock.requestLog).toContainEqual({
      method: "thread/resume",
      params: expect.objectContaining({ threadId: "thread-1" })
    });
  });
}

async function waitForBoundContext() {
  await waitFor(() => {
    expect(gatewayMock.requestLog).toContainEqual({
      method: "thread/context/read",
      params: expect.objectContaining({ threadId: "thread-1" })
    });
    expect((screen.getByRole("button", { name: "Agent target" }) as HTMLButtonElement).disabled).toBe(false);
  });
}

async function waitForDraftContext() {
  await waitFor(() => {
    expect(gatewayMock.requestLog).toContainEqual({
      method: "thread/context/read",
      params: expect.objectContaining({ threadId: null })
    });
    expect((screen.getByRole("button", { name: "Agent target" }) as HTMLButtonElement).disabled).toBe(false);
  });
}
