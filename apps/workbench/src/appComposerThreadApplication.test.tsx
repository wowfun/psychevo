// @vitest-environment jsdom

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { GatewayClientError, ThreadController } from "@psychevo/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { deferred, gatewayMock, sessionSummary } from "./appComposerAgent.fixture";
import { App } from "./App";

afterEach(() => vi.restoreAllMocks());

describe("Workbench public Thread Application interactions", () => {
  it("keeps an atomic New Session open authoritative without a redundant context read", async () => {
    gatewayMock.draftOpen = () => draftOpenResult();
    render(<App />);

    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/draft/open"))
        .toHaveLength(1);
    });
    const contextReadsBefore = gatewayMock.requestLog.filter((entry) => (
      entry.method === "thread/context/read"
    )).length;

    fireEvent.click(screen.getByRole("button", { name: "New Session" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/draft/open"))
        .toHaveLength(2);
      expect(screen.getByRole("button", { name: "Agent target" }).textContent)
        .toContain("Psychevo");
    });
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/context/read"))
      .toHaveLength(contextReadsBefore);
  });

  it("commits draft controls and the current branch as one environment generation", async () => {
    const draftOpen = deferred<Record<string, unknown>>();
    const branches = deferred<Record<string, unknown>>();
    gatewayMock.draftOpen = () => draftOpen.promise;
    gatewayMock.workspaceGitBranches = () => branches.promise;

    render(<App />);

    await waitFor(() => {
      expect(gatewayMock.requestLog.map((entry) => entry.method)).toEqual(expect.arrayContaining([
        "thread/draft/open",
        "workspace/git/branches"
      ]));
    });
    await act(async () => {
      draftOpen.resolve(draftOpenResult());
      await draftOpen.promise;
    });
    expect((screen.getByRole("button", { name: "Agent target" }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.queryByRole("button", { name: "Git branch" })).toBeNull();

    await act(async () => {
      branches.resolve(gatewayMock.workspaceGitBranchesResult);
      await branches.promise;
    });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Agent target" }).textContent)
        .toContain("Psychevo");
      expect(screen.getByRole("button", { name: "Git branch" }).textContent)
        .toContain(gatewayMock.projectBranch);
    });
  });

  it("queues one first-turn click while draft open is pending and clears only after acceptance", async () => {
    const draftOpen = deferred<Record<string, unknown>>();
    const turnStart = deferred<Record<string, unknown>>();
    gatewayMock.turnStart = () => turnStart.promise;

    render(<App />);

    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/draft/open"))
        .toHaveLength(1);
    });
    const input = await screen.findByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement;
    gatewayMock.draftOpen = () => draftOpen.promise;
    fireEvent.click(screen.getByRole("button", { name: "New Session" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/draft/open"))
        .toHaveLength(2);
    });
    fireEvent.change(input, { target: { value: "send after ready" } });
    const send = screen.getByRole("button", { name: "Send message" }) as HTMLButtonElement;
    expect(send.disabled).toBe(false);
    fireEvent.click(send);
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
    expect(input.value).toBe("send after ready");
    expect(screen.getByLabelText("Submission preparing elapsed").textContent).toContain("Preparing");
    expect(screen.queryByRole("button", { name: "Interrupt active turn" })).toBeNull();

    await act(async () => {
      draftOpen.resolve(draftOpenResult());
      await draftOpen.promise;
    });
    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "turn/start"))
        .toHaveLength(1);
    });
    expect(input.value).toBe("send after ready");

    await act(async () => {
      turnStart.resolve(turnStartResult());
      await turnStart.promise;
    });
    await waitFor(() => expect(input.value).toBe(""));
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "turn/start"))
      .toHaveLength(1);
  });

  it("cancels pending first-turn auto-submit when the input changes", async () => {
    const draftOpen = deferred<Record<string, unknown>>();

    render(<App />);

    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/draft/open"))
        .toHaveLength(1);
    });
    const input = await screen.findByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement;
    gatewayMock.draftOpen = () => draftOpen.promise;
    fireEvent.click(screen.getByRole("button", { name: "New Session" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/draft/open"))
        .toHaveLength(2);
    });
    fireEvent.change(input, { target: { value: "original pending text" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    fireEvent.change(input, { target: { value: "edited while loading" } });

    await act(async () => {
      draftOpen.resolve(draftOpenResult());
      await draftOpen.promise;
    });
    await waitFor(() => {
      expect((screen.getByRole("button", { name: "Send message" }) as HTMLButtonElement).disabled)
        .toBe(false);
    });
    expect(input.value).toBe("edited while loading");
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
  });

  it("cancels a pending draft submit when an existing Session is selected", async () => {
    const abandonedOpen = deferred<Record<string, unknown>>();
    let openCount = 0;
    gatewayMock.draftOpen = () => {
      openCount += 1;
      return openCount === 1 ? draftOpenResult() : abandonedOpen.promise;
    };
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Existing session")];
    render(<App />);

    await screen.findByRole("button", { name: "Agent target" });
    fireEvent.click(screen.getByRole("button", { name: "New Session" }));
    await waitFor(() => expect(openCount).toBe(2));
    const input = screen.getByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "abandoned draft" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    fireEvent.click(await screen.findByText("Existing session"));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/context/read",
        params: expect.objectContaining({ threadId: "thread-1" })
      });
    });
    const sessionInput = screen.getByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement;
    fireEvent.change(sessionInput, { target: { value: "send in the existing session" } });
    await waitFor(() => {
      expect((screen.getByRole("button", { name: "Send message" }) as HTMLButtonElement).disabled)
        .toBe(false);
    });

    await act(async () => {
      abandonedOpen.resolve(draftOpenResult());
      await abandonedOpen.promise;
    });
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
  });

  it("starts the initial Session browse without waiting for initialize or draft open", async () => {
    const initialize = deferred<Record<string, unknown>>();
    const draftOpen = deferred<Record<string, unknown>>();
    gatewayMock.initialize = () => initialize.promise;
    gatewayMock.draftOpen = () => draftOpen.promise;
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Early session")];

    render(<App />);

    await waitFor(() => {
      expect(gatewayMock.requestLog.map((entry) => entry.method)).toEqual(expect.arrayContaining([
        "initialize",
        "thread/browser"
      ]));
    });

    await act(async () => {
      initialize.resolve({
        server: "test",
        version: "0.0.0",
        cwd: gatewayMock.scope.cwd,
        displayCwd: gatewayMock.scope.cwd,
        scope: gatewayMock.scope,
        source: gatewayMock.source,
        capabilities: {}
      });
      await initialize.promise;
    });
    expect(await screen.findByText("Early session")).toBeTruthy();
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/draft/open")).toHaveLength(1);
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/context/read")).toHaveLength(0);
    expect(gatewayMock.requestLog.some((entry) => [
      "settings/read",
      "workspace/files",
      "agent/list",
      "backend/list",
      "command/list"
    ].includes(entry.method))).toBe(false);

    await act(async () => {
      draftOpen.resolve({
        snapshot: {
          ...gatewayMock.snapshot,
          thread: null,
          entries: [],
          activity: { ...gatewayMock.snapshot.activity }
        },
        context: {
          selectedTargetId: "target:default:native",
          suggestedTargetId: null,
          runtimeProfileRef: "native",
          selectionState: "draft",
          profiles: [],
          binding: null,
          controls: [],
          stability: "stable",
          capabilities: [],
          compatibleTargets: [{
            targetId: "target:default:native",
            agentRef: null,
            runtimeProfileRef: "native",
            agentLabel: "Psychevo",
            profileLabel: "Psychevo (Native)",
            label: "Psychevo · Psychevo (Native)",
            ready: true,
            unavailableReason: null
          }],
          inputCapabilities: [{ kind: "text", enabled: true, unavailableReason: null }],
          actions: [],
          sendability: { allowed: true, reason: null, recoveryAction: null },
          history: { owner: "psychevo", fidelity: "unavailable", cursor: null, hint: null },
          pendingInteractions: [],
          contextRevision: "context-native",
          controlRevision: "controls-native"
        },
        problem: null
      });
      await draftOpen.promise;
    });
    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/context/read")).toHaveLength(0);
    });
    expect(gatewayMock.requestLog.some((entry) => [
      "workspace/files",
      "agent/list",
      "backend/list",
      "command/list"
    ].includes(entry.method))).toBe(false);
  });

  it("renders the connected Composer and opens its draft without waiting for Session history", async () => {
    const history = deferred<Record<string, unknown>>();
    gatewayMock.threadBrowser = () => history.promise;

    render(<App />);

    const input = await screen.findByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement;
    await waitFor(() => expect(input.disabled).toBe(false));
    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/draft/open"))
        .toHaveLength(1);
    });

    await act(async () => {
      history.resolve({ workspaces: [] });
      await history.promise;
    });
  });

  it("keeps the Workbench and draft visible while reconnecting, then rehydrates on Retry", async () => {
    render(<App />);
    const input = await screen.findByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement;
    await waitFor(() => expect(input.disabled).toBe(false));
    fireEvent.change(input, { target: { value: "keep this draft" } });

    act(() => {
      gatewayMock.connectionState = "reconnecting";
      for (const callback of gatewayMock.connectionSubscribers) {
        callback({
          state: "reconnecting",
          generation: 1,
          attempt: 1,
          nextRetryAtMs: Date.now() + 250,
          lastError: "bridge closed"
        });
      }
    });

    expect(screen.getByText("Connection interrupted. Reconnecting…")).toBeTruthy();
    expect(input.value).toBe("keep this draft");
    expect(input.disabled).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Retry now" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "thread/resume")).toBe(true);
      expect(screen.queryByText("Connection interrupted. Reconnecting…")).toBeNull();
    });
    expect(input.value).toBe("keep this draft");
  });

  it("preserves an unknown Send until reconnect proves it was not accepted", async () => {
    gatewayMock.turnStart = () => Promise.reject(new GatewayClientError(
      "disconnected",
      "unknown",
      "bridge closed after send"
    ));
    render(<App />);
    const input = await screen.findByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement;
    await waitFor(() => expect(input.disabled).toBe(false));
    fireEvent.change(input, { target: { value: "verify this send" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByText(/Send status is unknown/)).toBeTruthy();
    expect(input.value).toBe("verify this send");

    act(() => {
      gatewayMock.connectionState = "reconnecting";
      for (const callback of gatewayMock.connectionSubscribers) {
        callback({
          state: "reconnecting",
          generation: 1,
          attempt: 1,
          nextRetryAtMs: Date.now() + 250,
          lastError: "bridge closed"
        });
      }
      gatewayMock.snapshot.thread = null as never;
      gatewayMock.snapshot.activity = { running: false, activeTurnId: null, queuedTurns: 0 };
      gatewayMock.snapshot.entries = [];
      gatewayMock.connectionGeneration = 2;
      gatewayMock.connectionState = "connected";
      for (const callback of gatewayMock.connectionSubscribers) {
        callback({
          state: "connected",
          generation: 2,
          attempt: 1,
          nextRetryAtMs: null,
          lastError: null
        });
      }
    });

    expect(await screen.findByText(/did not accept that Send/)).toBeTruthy();
    expect(input.value).toBe("verify this send");
  });

  it("immediately verifies an unknown timeout on the connected generation", async () => {
    let clientTurnId: string | null = null;
    gatewayMock.turnStart = (params) => {
      clientTurnId = (params as { clientTurnId?: string }).clientTurnId ?? null;
      gatewayMock.snapshot.turnStartReceipts = clientTurnId
        ? [{ clientTurnId, turnId: "turn-accepted" }]
        : [];
      return Promise.reject(new GatewayClientError(
        "request_timeout",
        "unknown",
        "turn/start timed out after send"
      ));
    };
    render(<App />);
    const input = await screen.findByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement;
    await waitFor(() => expect(input.disabled).toBe(false));
    fireEvent.change(input, { target: { value: "verify this timeout" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "thread/resume")).toBe(true);
    });
    expect(clientTurnId).toEqual(expect.any(String));
    await waitFor(() => expect(input.value).toBe(""));
  });

  it("keeps a failed generation recovery retryable until it succeeds", async () => {
    let resumeAttempts = 0;
    gatewayMock.threadResume = () => {
      resumeAttempts += 1;
      if (resumeAttempts === 1) {
        return Promise.reject(new Error("transient resume failure"));
      }
      return gatewayMock.snapshot;
    };
    render(<App />);
    const input = await screen.findByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement;
    await waitFor(() => expect(input.disabled).toBe(false));

    act(() => {
      gatewayMock.connectionGeneration = 2;
      gatewayMock.connectionState = "connected";
      for (const callback of gatewayMock.connectionSubscribers) {
        callback({
          state: "connected",
          generation: 2,
          attempt: 1,
          nextRetryAtMs: null,
          lastError: null
        });
      }
    });

    expect(await screen.findByText("Thread recovery failed. Retry to refresh authoritative state."))
      .toBeTruthy();
    expect(resumeAttempts).toBe(1);
    fireEvent.click(screen.getByRole("button", { name: "Retry now" }));

    await waitFor(() => expect(resumeAttempts).toBe(2));
    await waitFor(() => {
      expect(screen.queryByText("Thread recovery failed. Retry to refresh authoritative state."))
        .toBeNull();
      expect(input.disabled).toBe(false);
    });
  });

  it("does not start a stale startup draft after a Session is selected during initialize", async () => {
    const initialize = deferred<Record<string, unknown>>();
    gatewayMock.initialize = () => initialize.promise;
    gatewayMock.sessionSummaries = [sessionSummary("thread-early", "Selected before initialize")];

    const { container } = render(<App />);
    fireEvent.click(await screen.findByText("Selected before initialize"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "thread-early" })
      });
      expect(container.querySelector(".pevo-sessionRow.is-active")?.textContent)
        .toContain("Selected before initialize");
    });

    await act(async () => {
      initialize.resolve({
        server: "test",
        version: "0.0.0",
        cwd: gatewayMock.scope.cwd,
        displayCwd: gatewayMock.scope.cwd,
        scope: gatewayMock.scope,
        source: gatewayMock.source,
        capabilities: {}
      });
      await initialize.promise;
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Agent target" })).toBeTruthy();
    });
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/draft/open")).toHaveLength(0);
    expect(container.querySelector(".pevo-sessionRow.is-active")?.textContent)
      .toContain("Selected before initialize");
  });

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

  it("does not preload auxiliary surfaces when a session activates", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Auxiliary session")];
    render(<App />);

    await screen.findByRole("button", { name: "Agent target" });
    gatewayMock.requestLog.length = 0;
    fireEvent.click(await screen.findByText("Auxiliary session"));

    await waitForBoundContext();
    await act(async () => {
      await Promise.resolve();
    });
    expect(gatewayMock.requestLog.filter((entry) => (
      entry.method === "settings/read"
      && (entry.params as { threadId?: string | null }).threadId === "thread-1"
    ))).toHaveLength(0);
    for (const method of [
      "workspace/files",
      "workspace/diff",
      "workspace/changes",
      "observability/read",
      "command/list"
    ]) {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === method), method).toHaveLength(0);
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
    expect(screen.queryByText("Once · this request only")).toBeNull();
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

  it("keeps filesystem scope collapsed and submits an offered canonical directory", async () => {
    gatewayMock.snapshot.pendingActions = [{
      actionId: "permission-fs-1",
      kind: "permission",
      title: "Write file",
      summary: "Write linked-outside/result.txt",
      payload: {
        toolName: "write",
        summary: "Write linked-outside/result.txt",
        reason: "file write outside the working directory requires approval",
        suggestedRule: "filesystem:linked-outside/result.txt",
        filesystem: {
          targets: [{
            requestedPath: "linked-outside/result.txt",
            resolvedPath: "/tmp/shared/result.txt"
          }],
          scopeCandidates: ["/tmp/shared", "/tmp"]
        }
      },
      threadId: "thread-1",
      turnId: "turn-1"
    }];

    render(<App />);
    await resumeSession();
    const requestHeading = await screen.findByText("Permission · write");
    const requestCard = requestHeading.closest(".composerRequest");
    expect(requestCard).not.toBeNull();
    const requestText = requestCard?.textContent ?? "";
    expect(requestText.match(/linked-outside\/result\.txt/g)).toHaveLength(1);
    expect(requestText.match(/\/tmp\/shared\/result\.txt/g)).toHaveLength(1);
    expect(within(requestCard as HTMLElement).queryByText("Write linked-outside/result.txt")).toBeNull();
    expect(within(requestCard as HTMLElement).queryByText("filesystem:linked-outside/result.txt")).toBeNull();
    expect(within(requestCard as HTMLElement).getByText("source: thread-1")).toBeTruthy();
    expect(screen.queryByLabelText("Canonical directory")).toBeNull();
    expect(await screen.findByText("/tmp/shared/result.txt")).toBeTruthy();
    fireEvent.click(await screen.findByRole("button", { name: "Directory scope" }));
    fireEvent.change(await screen.findByLabelText("Canonical directory"), {
      target: { value: "/tmp" }
    });
    fireEvent.click(await screen.findByRole("button", { name: "Current session" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/interaction/respond",
        params: {
          scope: gatewayMock.scope,
          threadId: "thread-1",
          interactionId: "permission-fs-1",
          response: {
            kind: "permission",
            decision: "allowSession",
            directory: "/tmp"
          }
        }
      });
    });
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

  it("interrupts a running snapshot even when the cached action descriptor is stale", async () => {
    render(<App />);
    await resumeSession();
    await waitForBoundContext();
    emit("gateway/event", {
      type: "activityChanged",
      threadId: "thread-1",
      activity: { running: true, activeTurnId: "turn-1", queuedTurns: 0 }
    });
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

    emit("gateway/event", turnCompletedEvent("Completed before acceptance."));
    expect(await screen.findByText("Completed before acceptance.")).toBeTruthy();
    expect(applyGatewayEvent).toHaveBeenCalled();

    await act(async () => {
      pending.resolve(turnStartResult());
      await pending.promise;
    });
    await waitFor(() => expect(acceptTurnStart).toHaveBeenCalledTimes(1));
    expect(screen.getByText("Completed before acceptance.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Interrupt active turn" })).toBeNull();
  });

  it("routes a failed turnCompleted event without resurrecting its turn on acceptance", async () => {
    const pending = deferred<Record<string, unknown>>();
    gatewayMock.turnStart = () => pending.promise;
    const acceptTurnStart = vi.spyOn(ThreadController.prototype, "acceptTurnStart");
    const applyGatewayEvent = vi.spyOn(ThreadController.prototype, "applyGatewayEvent");

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
    emit("gateway/event", turnCompletedEvent("Turn failed before acceptance.", "failed"));
    expect((await screen.findAllByText("Turn failed before acceptance.")).length).toBeGreaterThan(0);
    expect(document.querySelector(".errorBand")?.textContent).toContain("Turn failed before acceptance.");
    expect(applyGatewayEvent).toHaveBeenCalled();

    await act(async () => {
      pending.resolve(turnStartResult());
      await pending.promise;
    });
    await waitFor(() => expect(acceptTurnStart).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("button", { name: "Interrupt active turn" })).toBeNull();
  });

  it("keeps an interrupted Turn in the Transcript without showing a global error", async () => {
    gatewayMock.turnStart = () => turnStartResult();

    render(<App />);
    await waitForDraftContext();
    fireEvent.change(screen.getByPlaceholderText("Ask Psychevo..."), {
      target: { value: "interrupt this turn" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(true);
    });

    emit("gateway/event", turnCompletedEvent("The turn was interrupted.", "interrupted"));

    const transcript = screen.getByRole("region", { name: "Transcript" });
    expect(await within(transcript).findByText("Turn interrupted")).toBeTruthy();
    expect(within(transcript).getAllByText("The turn was interrupted.").length).toBeGreaterThan(0);
    expect(document.querySelector(".errorBand")).toBeNull();
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
    expect((screen.getByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement).value).toBe("say hi");
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

function draftOpenResult(): Record<string, unknown> {
  return {
    snapshot: {
      ...gatewayMock.snapshot,
      thread: null,
      entries: [],
      activity: { ...gatewayMock.snapshot.activity }
    },
    context: {
      selectedTargetId: "target:default:native",
      suggestedTargetId: null,
      runtimeProfileRef: "native",
      selectionState: "draft",
      profiles: [],
      binding: null,
      controls: [],
      stability: "stable",
      capabilities: [],
      compatibleTargets: [{
        targetId: "target:default:native",
        agentRef: null,
        runtimeProfileRef: "native",
        agentLabel: "Psychevo",
        profileLabel: "Psychevo (Native)",
        label: "Psychevo · Psychevo (Native)",
        ready: true,
        unavailableReason: null
      }],
      inputCapabilities: [{ kind: "text", enabled: true, unavailableReason: null }],
      actions: [],
      sendability: { allowed: true, reason: null, recoveryAction: null },
      history: { owner: "psychevo", fidelity: "unavailable", cursor: null, hint: null },
      pendingInteractions: [],
      contextRevision: "context-native",
      controlRevision: "controls-native"
    },
    problem: null
  };
}

function turnCompletedEvent(
  body: string,
  status: "completed" | "failed" | "interrupted" = "completed"
): Record<string, unknown> {
  return {
    type: "turnCompleted",
    threadId: "thread-first",
    turnId: "turn-first",
    turn: {
      completedAtMs: 2_000,
      error: status === "completed" ? null : { message: body },
      id: "turn-first",
      outcome: status,
      startedAtMs: 1_000,
      status,
      threadId: "thread-first"
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
      method: "thread/draft/open",
      params: expect.objectContaining({ targetIntent: { kind: "default" } })
    });
    expect((screen.getByRole("button", { name: "Agent target" }) as HTMLButtonElement).disabled).toBe(false);
  });
}
