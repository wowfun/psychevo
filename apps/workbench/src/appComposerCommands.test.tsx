// @vitest-environment jsdom

import type { TranscriptEntry } from "@psychevo/protocol";
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

describe("Workbench command routing", () => {
  async function resumeSession(threadId = "thread-1", title = "Active session") {
    gatewayMock.sessionSummaries = [sessionSummary(threadId, title)];
    const sessionRow = await screen.findByText(title);
    fireEvent.click(sessionRow);
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId })
      });
    });
  }

  it("groups command panel rows by runtime presentation kind", async () => {
    gatewayMock.commandList = [
      commandItem("sessions", "navigate", "history"),
      commandItem("diff", "inspect", "preview"),
      commandItem("queue", "control", "composer"),
      commandItem("fork", "submit", "composer"),
      commandItem("export", "export", "download"),
      commandItem("x-daily", "extension", "composer", "Fetch X daily posts.")
    ];
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "navigate",
      feedbackAnchor: "commandsPanel",
      action: { type: "showPanel", panel: "commands" }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/help" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByRole("region", { name: "Commands overlay" })).toBeTruthy();
    expect(await screen.findByRole("region", { name: "Commands" })).toBeTruthy();
    expect(screen.getByRole("region", { name: "Transcript" })).toBeTruthy();
    expect(screen.getByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    for (const heading of ["Navigate", "Inspect", "Control", "Submit", "Export", "Extensions"]) {
      expect(screen.getByText(heading)).toBeTruthy();
    }
    expect(screen.getByRole("button", { name: /\/diff/ })).toBeTruthy();
    expect(screen.getByText("Preview")).toBeTruthy();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);

    fireEvent.click(screen.getByRole("button", { name: "Close Commands" }));
    expect(screen.queryByRole("region", { name: "Commands overlay" })).toBeNull();
    expect(screen.getByPlaceholderText("Ask Psychevo...")).toBeTruthy();
  });

  it("opens commands slash results as transcript overlays", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "navigate",
      feedbackAnchor: "commandsPanel",
      action: { type: "showPanel", panel: "commands" }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/commands" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByRole("region", { name: "Commands overlay" })).toBeTruthy();
    expect(screen.getByRole("region", { name: "Transcript" })).toBeTruthy();
    expect(screen.getByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
  });

  it("shows custom slash alias rows in the commands overlay and executes them", async () => {
    gatewayMock.commandList = [
      {
        ...commandItem("st", "inspect", "status", "alias for /status"),
        slash: "/st",
        usage: "/st [args]",
        source: "custom",
        expandsTo: "/status"
      }
    ];
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: command === "/commands" ? "navigate" : "inspect",
      feedbackAnchor: "commandsPanel",
      action: { type: "showPanel", panel: command === "/commands" ? "commands" : "status" }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/commands" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    const overlay = await screen.findByRole("region", { name: "Commands overlay" });
    fireEvent.click(within(overlay).getByRole("button", { name: /\/st/ }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "command/execute",
        params: expect.objectContaining({ command: "/st" })
      });
    });
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
  });

  it("opens side chat host actions as right workspace thread tabs", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "control",
      feedbackAnchor: "composer",
      action: {
        type: "sideConversationStart",
        threadId: "side-thread",
        parentThreadId: "thread-1",
        title: "Side abcd1234",
        prompt: "explain this"
      }
    });

    render(<App />);
    await resumeSession();

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/btw explain this" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    const rightWorkspace = await screen.findByRole("region", { name: "Right workspace" });
    expect(within(rightWorkspace).getByRole("button", { name: "Side chat" })).toBeTruthy();
    expect(within(rightWorkspace).getByRole("region", { name: "Side chat" })).toBeTruthy();
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          input: [{ type: "text", text: "explain this" }],
          threadId: "side-thread"
        })
      });
    });
    expect(gatewayMock.requestLog).toContainEqual({
      method: "command/execute",
      params: expect.objectContaining({
        command: "/btw explain this",
        threadId: "thread-1"
      })
    });
  });

  it("opens agent transcript rows in a child session tab from the explicit Open action", async () => {
    const agentEntry: TranscriptEntry = {
      id: "entry-agent",
      threadId: "thread-1",
      turnId: "turn-1",
      messageSeq: null,
      role: "assistant",
      status: "running",
      source: "psychevo",
      blocks: [
        {
          id: "block-agent",
          kind: "agent",
          status: "running",
          order: 0,
          source: "runtime",
          title: "Planck",
          body: "working",
          preview: "working",
          detail: null,
          artifactIds: [],
          metadata: {
            agent_name: "Planck",
            child_session_id: "child-thread",
            parent_session_id: "thread-1"
          },
          result: null,
          createdAtMs: 1_000,
          updatedAtMs: 1_000
        }
      ],
      metadata: null,
      usage: null,
      accounting: null,
      createdAtMs: 1_000,
      updatedAtMs: 1_000
    };
    (gatewayMock.snapshot as unknown as { entries: TranscriptEntry[] }).entries = [agentEntry];

    render(<App />);
    await resumeSession();

    fireEvent.click(await screen.findByRole("button", { name: "Open Planck agent session" }));

    expect(await screen.findByRole("button", { name: "Planck" })).toBeTruthy();
    expect(screen.getByRole("region", { name: "Planck" })).toBeTruthy();
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/read",
        params: { threadId: "child-thread" }
      });
    });
  });

  it("opens team status in the right workspace and controls child agents", async () => {
    gatewayMock.teamStatusResult = {
      team: {
        id: "team-run-1",
        parentSessionId: "thread-1",
        missionRunId: "mission-1",
        teamName: "release",
        description: "Release team",
        sourcePath: "/tmp/project/.psychevo/teams/release.md",
        leaderAgentName: "general",
        members: [
          { id: "reviewer", agentName: "general", role: "review", description: "Review changes", maxTurns: 2 }
        ],
        maxParallelAgents: 4,
        status: "running",
        startedAtMs: 1_000,
        endedAtMs: null,
        finalSummary: null
      },
      mission: {
        id: "mission-1",
        parentSessionId: "thread-1",
        teamRunId: "team-run-1",
        teamName: "release",
        goal: "Ship release",
        leadAgentName: "general",
        status: "running",
        startedAtMs: 1_000,
        endedAtMs: null,
        finalSummary: null
      },
      agents: [
        {
          id: "agent-1",
          taskName: "Review patch",
          agentName: "general",
          task: "Review patch",
          parentSessionId: "thread-1",
          childSessionId: "child-reviewer",
          role: "subagent",
          background: false,
          status: "running",
          edgeStatus: "running",
          startedAtMs: 1_000,
          endedAtMs: null,
          outcome: null,
          finalAnswer: null,
          error: null,
          effectiveMaxSpawnDepth: 2,
          teamRunId: "team-run-1",
          missionRunId: "mission-1",
          teamName: "release",
          teamMemberId: "reviewer",
          agentPath: ".psychevo/agents/general.md"
        }
      ],
      control: {
        spawningPaused: false,
        maxSpawnDepthCap: 3,
        concurrencyCap: 4
      }
    };

    render(<App />);
    await resumeSession();

    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const workspace = await screen.findByRole("region", { name: "Right workspace" });
    fireEvent.click(within(workspace).getByRole("button", { name: "Team" }));

    const teamPanel = await within(workspace).findByRole("region", { name: "Team" });
    expect(await within(teamPanel).findByText("Ship release")).toBeTruthy();
    expect(within(teamPanel).getByRole("region", { name: "Member reviewer" })).toBeTruthy();
    expect(within(teamPanel).getByText("Review patch")).toBeTruthy();

    fireEvent.click(within(teamPanel).getByRole("button", { name: "Stop reviewer" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "agent/control",
        params: expect.objectContaining({
          action: "stop",
          target: "agent-1"
        })
      });
    });

    const promptSpy = vi.spyOn(window, "prompt").mockReturnValueOnce("Please verify.");
    fireEvent.click(within(teamPanel).getByRole("button", { name: "Send to reviewer" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "agent/control",
        params: expect.objectContaining({
          action: "send",
          target: "agent-1",
          message: "Please verify."
        })
      });
    });
    promptSpy.mockRestore();

    fireEvent.click(within(teamPanel).getByRole("button", { name: "Open Review patch" }));
    expect(await within(workspace).findByRole("region", { name: "Review patch" })).toBeTruthy();
  });

  it("opens completed agent tool rows whose child session is stored in result metadata", async () => {
    const agentEntry: TranscriptEntry = {
      id: "entry-agent",
      threadId: "thread-1",
      turnId: "turn-1",
      messageSeq: 2,
      role: "assistant",
      status: "completed",
      source: "psychevo",
      blocks: [
        {
          id: "block-agent",
          kind: "agent",
          status: "completed",
          order: 0,
          source: "runtime",
          title: "Agent",
          body: "{\"child_thread_id\":\"child-thread\"}",
          preview: "child-thread",
          detail: null,
          artifactIds: [],
          metadata: {
            projection: "tool",
            tool_name: "spawn_agent",
            tool_call_id: "call-agent",
            result: {
              agent_name: "Planck",
              child_thread_id: "child-thread",
              parent_thread_id: "thread-1",
              task_name: "investigate"
            }
          },
          result: null,
          createdAtMs: 1_000,
          updatedAtMs: 2_000
        }
      ],
      metadata: null,
      usage: null,
      accounting: null,
      createdAtMs: 1_000,
      updatedAtMs: 2_000
    };
    (gatewayMock.snapshot as unknown as { entries: TranscriptEntry[] }).entries = [agentEntry];

    render(<App />);
    await resumeSession();

    fireEvent.click(await screen.findByRole("button", { name: "Open investigate agent session" }));

    expect(await screen.findByRole("button", { name: "investigate" })).toBeTruthy();
    expect(screen.getByRole("region", { name: "investigate" })).toBeTruthy();
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/read",
        params: { threadId: "child-thread" }
      });
    });
  });

  it("submits a spawned child follow-up through the visible panel controller", async () => {
    const spawnEntry = spawnedChildEntry();
    (gatewayMock.snapshot as unknown as { entries: TranscriptEntry[] }).entries = [spawnEntry];

    render(<App />);
    await resumeSession();

    const initialUser = childCommittedEntry("message:1", 1, "user", "Main Agent instruction", "turn-initial");
    const initialAssistant = childCommittedEntry("message:2", 2, "assistant", "Initial answer", "turn-initial");
    (gatewayMock.snapshot as unknown as { entries: TranscriptEntry[] }).entries = [initialUser, initialAssistant];
    gatewayMock.runtimeContextRead = () => writableChildContext();
    gatewayMock.turnStart = () => ({
      accepted: true,
      threadId: "child-thread",
      turnId: "turn-follow-up",
      thread: {
        id: "child-thread",
        backend: { kind: "native", runtimeRef: "native", sessionHandle: "child-thread" },
        sourceKey: "source-child-thread"
      }
    });

    fireEvent.click(await screen.findByRole("button", { name: "Open investigate agent session" }));
    const panel = await screen.findByRole("region", { name: "investigate" });
    const composer = await within(panel).findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(composer, { target: { value: "Second question" } });
    fireEvent.submit(composer.closest("form") as HTMLFormElement);

    await waitFor(() => expect(gatewayMock.requestLog).toContainEqual({
      method: "turn/start",
      params: expect.objectContaining({
        input: [{ type: "text", text: "Second question" }],
        threadId: "child-thread"
      })
    }));

    const committedUser = childCommittedEntry("message:3", 3, "user", "Second question", "turn-follow-up");
    const committedAssistant = childCommittedEntry("message:4", 4, "assistant", "Second answer", "turn-follow-up");
    (gatewayMock.snapshot as unknown as { entries: TranscriptEntry[] }).entries = [
      initialUser,
      initialAssistant,
      committedUser,
      committedAssistant
    ];
    await act(async () => {
      for (const subscriber of gatewayMock.subscribers) {
        subscriber({
          method: "gateway/event",
          params: {
            type: "turnStarted",
            threadId: "child-thread",
            turnId: "turn-follow-up",
            selectedSkills: []
          }
        });
        subscriber({
          method: "gateway/event",
          params: { type: "entryUpdated", turnId: "turn-follow-up", entry: committedUser }
        });
        subscriber({
          method: "gateway/event",
          params: {
            type: "turnCompleted",
            threadId: "child-thread",
            turnId: "turn-follow-up",
            turn: {
              id: "turn-follow-up",
              threadId: "child-thread",
              status: "completed",
              outcome: "normal",
              error: null,
              startedAtMs: 10,
              completedAtMs: 20
            },
            committedEntries: [committedUser, committedAssistant]
          }
        });
      }
      await Promise.resolve();
    });

    expect(await within(panel).findByText("Second answer")).toBeTruthy();
    expect(within(panel).getAllByText("Main Agent instruction")).toHaveLength(1);
    expect(within(panel).getAllByText("Second question")).toHaveLength(1);
    expect(panel.querySelectorAll('[data-entry-id="message:1"]')).toHaveLength(1);
    expect(panel.querySelectorAll('[data-entry-id="message:3"]')).toHaveLength(1);
    expect(panel.querySelectorAll('[data-entry-id^="optimistic:"]')).toHaveLength(0);
  });

  it("routes /agents to Capabilities Agents", async () => {
    gatewayMock.commandList = [
      commandItem("commands", "navigate", "commands")
    ];
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      message: "Opened Agents.",
      feedbackAnchor: "composer",
      action: { type: "showPanel", panel: "agents" }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/agents" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    const capabilitiesRegion = await screen.findByRole("region", { name: "Capabilities" });
    expect(within(capabilitiesRegion).getByRole("tab", { name: "Agents" }).getAttribute("aria-selected")).toBe("true");
    expect(screen.queryByRole("region", { name: "Commands overlay" })).toBeNull();
    expect(screen.queryByRole("region", { name: "Agents overlay" })).toBeNull();
    expect(screen.queryByRole("region", { name: "Settings" })).toBeNull();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
  });

  it("routes /compact through native thread compaction without submitting a turn", async () => {
    gatewayMock.commandList = [
      commandItem("compact", "control", "composer")
    ];
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "control",
      feedbackAnchor: "composer",
      action: { type: "threadCompactStart", instructions: "keep decisions" }
    });
    gatewayMock.threadActionRun = (params: unknown) => ({
      kind: "compact",
      threadId: (params as { threadId: string }).threadId,
      result: {
        accepted: true,
        compacted: true,
        reason: "manual",
        message: "context compacted",
        checkpoint: {
          checkpointId: 7,
          reason: "manual",
          createdAtMs: 1_798_650_000_000,
          firstKeptSessionSeq: 3,
          tokensBefore: 120,
          tokensAfter: 42,
          summaryProvider: "fake",
          summaryModel: "fake-model",
          summary: "summary"
        },
        tokensBefore: 120,
        tokensAfter: 42,
        summaryProvider: "fake",
        summaryModel: "fake-model",
        unavailable: false,
        error: null
      }
    });

    render(<App />);
    await resumeSession();

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/compact keep decisions" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/action/run",
        params: {
          scope: gatewayMock.scope,
          threadId: "thread-1",
          action: { kind: "compact", instructions: "keep decisions" }
        }
      });
    });
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
    expect(await screen.findByText("context compacted")).toBeTruthy();
  });

  it("routes commands clicked inside the overlay without submitting transcript turns", async () => {
    gatewayMock.commandList = [
      commandItem("status", "inspect", "status")
    ];
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: command === "/status" ? "inspect" : "navigate",
      feedbackAnchor: command === "/status" ? "status" : "commandsPanel",
      action: { type: "showPanel", panel: command === "/status" ? "status" : "commands" }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/help" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByRole("region", { name: "Commands overlay" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /\/status/ }));

    expect(await screen.findByRole("region", { name: "Workspace status" })).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Commands overlay" })).toBeNull();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
  });

  it("shows composer feedback for known unsupported slash commands without submitting a turn", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: false,
      command,
      known: true,
      message: "/model is managed by the Workbench model controls.",
      presentationKind: "control",
      feedbackAnchor: "composer",
      alternateAction: { type: "openComposerControl", target: "model", label: "Open model controls" },
      action: null
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/model" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByText("/model is managed by the Workbench model controls.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Open model controls" })).toBeTruthy();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
  });

  it("reveals workspace status and shows local feedback for composer-entered status commands", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "inspect",
      feedbackAnchor: "status",
      action: { type: "showPanel", panel: "status" }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/status" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByRole("region", { name: "Workspace status" })).toBeTruthy();
    expect(await screen.findByText("Opened Status.")).toBeTruthy();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
  });

  it("auto-dismisses successful inspect command feedback", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "inspect",
      feedbackAnchor: "status",
      action: { type: "showPanel", panel: "status" }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/context" } });
    vi.useFakeTimers();
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.getByText("Opened Status.")).toBeTruthy();
    await act(async () => {
      vi.advanceTimersByTime(2_999);
    });
    expect(screen.getByText("Opened Status.")).toBeTruthy();
    await act(async () => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.queryByText("Opened Status.")).toBeNull();
  });

  it("shows sandbox status feedback near the composer while revealing workspace status", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      message: "sandbox: workspace-write",
      presentationKind: "inspect",
      feedbackAnchor: "status",
      action: null
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/sandbox" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByRole("region", { name: "Workspace status" })).toBeTruthy();
    expect(await screen.findByText("sandbox: workspace-write")).toBeTruthy();
    fireEvent.mouseDown(document.body);
    await waitFor(() => {
      expect(screen.queryByText("sandbox: workspace-write")).toBeNull();
    });
    expect(screen.getByRole("region", { name: "Workspace status" })).toBeTruthy();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
  });

  it("reveals collapsed History for composer-entered sessions commands", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "navigate",
      feedbackAnchor: "commandsPanel",
      action: { type: "showPanel", panel: "history" }
    });

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Collapse left sidebar" }));
    expect(screen.queryByText("Sessions")).toBeNull();

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/sessions" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByText("Sessions")).toBeTruthy();
    expect(await screen.findByText("Opened History.")).toBeTruthy();
  });

  it("routes history export and share through semantic session downloads", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("thread-history", "History export")];
    const { container } = render(<App />);

    await screen.findByText("History export");
    const menu = container.querySelector(".pevo-sessionMenu") as HTMLDetailsElement | null;
    const trigger = container.querySelector(".pevo-sessionMenu summary") as HTMLElement | null;
    expect(trigger).toBeTruthy();

    fireEvent.click(trigger!);
    await waitFor(() => expect(menu?.open).toBe(true));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Export" }));
    await waitFor(() => {
      expect(gatewayMock.openDownloadLog).toContain(
        "http://127.0.0.1/download/session/thread-history/export"
      );
    });
    await waitFor(() => expect(menu?.open).toBe(false));

    fireEvent.click(trigger!);
    await waitFor(() => expect(menu?.open).toBe(true));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Share" }));
    await waitFor(() => {
      expect(gatewayMock.openDownloadLog).toContain(
        "http://127.0.0.1/download/session/thread-history/share"
      );
    });
  });

  it("keeps idle steer errors local to the composer", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "control",
      feedbackAnchor: "composer",
      action: { type: "steerPrompt", text: "hello" }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/steer hello" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByText("/steer is only available while a turn is running.")).toBeTruthy();
    expect(screen.getByRole("region", { name: "Transcript" })).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Commands overlay" })).toBeNull();
    expect(screen.queryByRole("region", { name: "Commands" })).toBeNull();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
  });

  it("reports an active steer rejected by the selected Runtime Profile", async () => {
    gatewayMock.snapshot.activity = { running: true, activeTurnId: "turn-1", queuedTurns: 0 };
    gatewayMock.threadActionRun = (params: unknown) => ({
      kind: "steer",
      threadId: (params as { threadId: string }).threadId,
      accepted: false
    });
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "control",
      feedbackAnchor: "composer",
      action: { type: "steerPrompt", text: "hello" }
    });

    render(<App />);
    await resumeSession();

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/context/read",
        params: expect.objectContaining({ threadId: "thread-1" })
      });
      expect((screen.getByRole("button", { name: "Agent target" }) as HTMLButtonElement).disabled).toBe(false);
    });
    await screen.findByRole("button", { name: "Interrupt active turn" });
    fireEvent.change(textarea, { target: { value: "/steer hello" } });
    fireEvent.submit(textarea.closest("form") as HTMLFormElement);

    expect(await screen.findByText("The selected Runtime Profile does not support steering this turn.")).toBeTruthy();
    expect(gatewayMock.requestLog).toContainEqual({
      method: "thread/action/run",
      params: {
        scope: gatewayMock.scope,
        threadId: "thread-1",
        action: { kind: "steer", expectedTurnId: "turn-1", text: "hello" }
      }
    });
  });

  it("clears transient slash feedback after switching sessions", async () => {
    gatewayMock.sessionSummaries = [
      sessionSummary("thread-1", "First session"),
      sessionSummary("thread-2", "Second session")
    ];
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "inspect",
      feedbackAnchor: "status",
      action: { type: "showPanel", panel: "status" }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/usage" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByText("Opened Status.")).toBeTruthy();
    fireEvent.click(await screen.findByText("Second session"));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "thread-2" })
      });
    });
    await waitFor(() => {
      expect(screen.queryByText("Opened Status.")).toBeNull();
    });
  });

  it("submits unknown slash input as prompt text", async () => {
    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    await waitFor(() => {
      expect((screen.getByRole("button", { name: "Agent target" }) as HTMLButtonElement).disabled).toBe(false);
    });
    fireEvent.change(textarea, { target: { value: "/tmp/output.txt" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          input: [{ type: "text", text: "/tmp/output.txt" }]
        })
      });
    });
  });

  it("blocks unknown slash prompt passthrough when no concrete model is selected", async () => {
    gatewayMock.model = null;
    gatewayMock.modelStatus = "unconfigured";

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/tmp/output.txt" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByText("Select a provider/model before starting a conversation.")).toBeTruthy();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
  });

  it("submits dynamic slash payloads while displaying the original slash line", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "extension",
      feedbackAnchor: "composer",
      action: { type: "submitPrompt", text: "$x-daily latest", displayText: command }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/x-daily latest" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          input: [{ type: "text", text: "$x-daily latest" }]
        })
      });
    });
    expect(await screen.findByText("/x-daily latest")).toBeTruthy();
  });

  it("submits slash actions to a returned thread id", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "submit",
      feedbackAnchor: "composer",
      action: {
        type: "submitPrompt",
        text: "Start a multi-agent mission.",
        displayText: command,
        threadId: "mission-thread"
      }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/mission --team ship implement feature" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          threadId: "mission-thread",
          input: [{ type: "text", text: "Start a multi-agent mission." }]
        })
      });
    });
  });

  it("blocks dynamic slash prompt submission when no concrete model is selected", async () => {
    gatewayMock.model = null;
    gatewayMock.modelStatus = "unconfigured";
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "extension",
      feedbackAnchor: "composer",
      action: { type: "submitPrompt", text: "$x-daily latest", displayText: command }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/x-daily latest" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByText("Select a provider/model before starting a conversation.")).toBeTruthy();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
    expect(gatewayMock.optimisticLog).not.toContain("/x-daily latest");
  });

  it("submits queued slash payloads while displaying the original slash line", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "control",
      feedbackAnchor: "composer",
      action: { type: "queuePrompt", text: "hello", displayText: command }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/queue hello" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          input: [{ type: "text", text: "hello" }]
        })
      });
    });
    expect(await screen.findByText("/queue hello")).toBeTruthy();
  });

  it("shows a bounded export error instead of opening downloads without a host endpoint", async () => {
    gatewayMock.endpoint = null;
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "export",
      feedbackAnchor: "trigger",
      action: { type: "downloadSession", kind: "export", threadId: "thread-1" }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/export" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByText("Export is not available for this session.")).toBeTruthy();
    expect(gatewayMock.openDownloadLog).toEqual([]);
  });

  it("opens manual slash alias export downloads with parsed options", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "export",
      feedbackAnchor: "trigger",
      action: {
        type: "downloadSession",
        kind: "export",
        threadId: "thread-1",
        format: "json",
        include: ["last-provider-request", "last-provider-response"],
        filename: "provider-response.json"
      }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/expr" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.openDownloadLog[0]).toContain(
        "format=json&include=last-provider-request%2Clast-provider-response&filename=provider-response.json"
      );
    });
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
    expect(await screen.findByText("Export download opened.")).toBeTruthy();
  });

  it("routes session undo and redo without submitting transcript turns", async () => {
    gatewayMock.commandExecute = (command: string) => {
      if (command === "/undo") {
        return {
          accepted: true,
          command,
          known: true,
          presentationKind: "control",
          feedbackAnchor: "composer",
          message: "undone 2 messages; prompt restored",
          action: {
            type: "sessionUndo",
            threadId: "thread-1",
            prompt: "second prompt",
            revertedMessages: 2
          }
        };
      }
      return {
        accepted: true,
        command,
        known: true,
        presentationKind: "control",
        feedbackAnchor: "composer",
        message: "redone 2 messages; complete",
        action: {
          type: "sessionRedo",
          threadId: "thread-1",
          restoredMessages: 2,
          complete: true
        }
      };
    };

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement;
    const beforeUndo = gatewayMock.requestLog.length;
    fireEvent.change(textarea, { target: { value: "/undo" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(textarea.value).toBe("second prompt");
    });
    expect(await screen.findByText("undone 2 messages; prompt restored")).toBeTruthy();
    const undoMethods = gatewayMock.requestLog.slice(beforeUndo).map((entry) => entry.method);
    expect(undoMethods).toContain("thread/read");
    expect(undoMethods).toContain("thread/browser");
    expect(undoMethods).toContain("workspace/diff");
    expect(undoMethods).toContain("observability/read");
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);

    const beforeRedo = gatewayMock.requestLog.length;
    fireEvent.change(textarea, { target: { value: "/redo" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(textarea.value).toBe("");
    });
    expect(await screen.findByText("redone 2 messages; complete")).toBeTruthy();
    const redoMethods = gatewayMock.requestLog.slice(beforeRedo).map((entry) => entry.method);
    expect(redoMethods).toContain("thread/read");
    expect(redoMethods).toContain("thread/browser");
    expect(redoMethods).toContain("workspace/diff");
    expect(redoMethods).toContain("observability/read");
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
  });

  it("routes diff previews and artifact downloads from structured slash actions", async () => {
    gatewayMock.commandExecute = (command: string) => {
      if (command === "/diff") {
        return {
          accepted: true,
          command,
          known: true,
          presentationKind: "inspect",
          feedbackAnchor: "trigger",
          action: workspaceDiffAction()
        };
      }
      return {
        accepted: true,
        command,
        known: true,
        presentationKind: "export",
        feedbackAnchor: "trigger",
        action: { type: "downloadSession", kind: "export", threadId: "thread-1" }
      };
    };

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/diff" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByRole("region", { name: "Review" })).toBeTruthy();
    expect(screen.queryByLabelText("Inline preview")).toBeNull();
    expect(screen.getAllByText("src/main.rs").length).toBeGreaterThan(0);
    expect(screen.getAllByText("M↓").length).toBeGreaterThan(0);
    expect(screen.getAllByText("+1").length).toBeGreaterThan(0);
    expect(screen.getAllByText("-1").length).toBeGreaterThan(0);
    expect(screen.queryByText("diff --git a/src/main.rs b/src/main.rs")).toBeNull();

    fireEvent.change(textarea, { target: { value: "/export" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => {
      expect(gatewayMock.openDownloadLog).toContain(
        "http://127.0.0.1/download/session/thread-1/export"
      );
    });
    expect(await screen.findByText("Export download opened.")).toBeTruthy();
  });
});

function spawnedChildEntry(): TranscriptEntry {
  return {
    id: "entry-agent",
    threadId: "thread-1",
    turnId: "turn-1",
    messageSeq: 2,
    role: "assistant",
    status: "completed",
    source: "psychevo",
    blocks: [{
      id: "block-agent",
      kind: "agent",
      status: "completed",
      order: 0,
      source: "runtime",
      title: "Agent",
      body: "{\"child_thread_id\":\"child-thread\"}",
      preview: "child-thread",
      detail: null,
      artifactIds: [],
      metadata: {
        projection: "tool",
        tool_name: "spawn_agent",
        tool_call_id: "call-agent",
        result: {
          agent_name: "Planck",
          child_thread_id: "child-thread",
          parent_thread_id: "thread-1",
          task_name: "investigate"
        }
      },
      result: null,
      createdAtMs: 1_000,
      updatedAtMs: 2_000
    }],
    metadata: null,
    usage: null,
    accounting: null,
    createdAtMs: 1_000,
    updatedAtMs: 2_000
  };
}

function childCommittedEntry(
  id: string,
  messageSeq: number,
  role: "user" | "assistant",
  body: string,
  turnId: string
): TranscriptEntry {
  return {
    id,
    threadId: "child-thread",
    turnId,
    messageSeq,
    role,
    status: "completed",
    source: "runtime.message",
    blocks: [{
      id: `${id}:text`,
      kind: "text",
      status: "completed",
      order: 0,
      source: "runtime.message",
      title: null,
      body,
      preview: body,
      detail: body,
      artifactIds: [],
      metadata: null,
      result: null,
      createdAtMs: messageSeq,
      updatedAtMs: messageSeq
    }],
    metadata: null,
    usage: null,
    accounting: null,
    createdAtMs: messageSeq,
    updatedAtMs: messageSeq
  };
}

function writableChildContext(): Record<string, unknown> {
  return {
    targetId: "target:child-native",
    runtimeProfileRef: "native",
    selectionState: "bound",
    profiles: [],
    binding: {
      threadId: "child-thread",
      agentRef: "investigate",
      agentFingerprint: "agent-fingerprint",
      runtimeRef: "native",
      backendKind: "native",
      nativeKind: "native",
      sessionHandle: "child-thread",
      cwd: "/tmp/project",
      profileFingerprint: "profile-fingerprint",
      ownership: "readWrite",
      bindingRevision: 1
    },
    controls: [],
    stability: "stable",
    capabilities: [{ id: "turn.start", enabled: true, stability: "stable", unavailableReason: null }],
    compatibleTargets: [{
      targetId: "target:child-native",
      agentRef: "investigate",
      runtimeProfileRef: "native",
      agentLabel: "investigate",
      profileLabel: "Psychevo (Native)",
      label: "investigate · Psychevo (Native)",
      ready: true,
      unavailableReason: null
    }],
    inputCapabilities: [{ kind: "text", enabled: true, unavailableReason: null }],
    actions: [],
    sendability: { allowed: true, reason: null, recoveryAction: null },
    history: { owner: "psychevo", fidelity: "full", cursor: null, hint: null },
    pendingInteractions: [],
    contextRevision: "child-context-1",
    controlRevision: "child-controls-1"
  };
}
