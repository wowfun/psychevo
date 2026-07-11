import { describe, expect, it } from "vitest";
import type {
  GatewayRequestScope,
  GatewayTurn,
  ThreadSnapshot,
  TranscriptBlock,
  TranscriptEntry,
  TurnResultPayload
} from "@psychevo/protocol";
import {
  acceptThreadTurn,
  applyTurnResultToThreadSnapshot,
  emptyThreadSnapshot,
  prepareThreadTurn,
  threadTurnControlsFromWorkbenchControls,
  threadTurnStartParams,
  ThreadTranscriptController
} from "./thread-controller";

describe("thread transcript controller helpers", () => {
  it("binds an accepted first turn id to optimistic prompt entries", () => {
    const scope = floatingScope();
    const prepared = prepareThreadTurn(emptyThreadSnapshot(scope), "say hi", null);
    const accepted = acceptThreadTurn(
      prepared.snapshot,
      { accepted: true, threadId: "thread-floating" },
      prepared.requestedThreadId,
      "floating turn"
    );

    expect(accepted.threadId).toBe("thread-floating");
    expect(accepted.snapshot.thread?.id).toBe("thread-floating");
    expect(accepted.snapshot.entries[0]).toMatchObject({
      role: "user",
      threadId: "thread-floating"
    });
  });

  it("applies a turn/result completion through the shared transcript reducer", () => {
    const running = {
      ...emptyThreadSnapshot(floatingScope(), "thread-floating"),
      activity: {
        activeTurnId: "turn-1",
        queuedTurns: 0,
        running: true
      }
    };

    const next = applyTurnResultToThreadSnapshot(running, turnResult({
      answer: "Hi from the provider.",
      threadId: "thread-floating",
      turnId: "turn-1"
    }));

    expect(next.activity).toEqual({
      activeTurnId: null,
      queuedTurns: 0,
      running: false
    });
    expect(next.entries).toHaveLength(1);
    expect(next.entries[0]?.blocks[0]?.body).toBe("Hi from the provider.");
  });

  it("ignores another thread's completion for the current snapshot", () => {
    const current = emptyThreadSnapshot(floatingScope(), "thread-current");
    const next = applyTurnResultToThreadSnapshot(current, turnResult({
      answer: "Wrong thread",
      threadId: "thread-other",
      turnId: "turn-other"
    }));

    expect(next).toBe(current);
  });

  it("does not synthesize a local assistant entry when Gateway omits committed entries", () => {
    const running = {
      ...emptyThreadSnapshot(floatingScope(), "thread-floating"),
      activity: {
        activeTurnId: "turn-1",
        queuedTurns: 0,
        running: true
      }
    };
    const payload = turnResult({
      answer: "Fallback text should not become a fake local message.",
      threadId: "thread-floating",
      turnId: "turn-1"
    });

    const next = applyTurnResultToThreadSnapshot(running, {
      ...payload,
      committedEntries: []
    });

    expect(next.activity.running).toBe(false);
    expect(next.entries).toEqual([]);
  });

  it("builds turn/start params with the shared Workbench turn controls", () => {
    const params = threadTurnStartParams({
      controls: {
        agentName: "planner",
        mode: "plan",
        model: "deepseek/deepseek-chat",
        permissionMode: "ask",
        reasoningEffort: "medium",
        runtimeOptions: { mode: "balanced" },
        runtimeRef: "native",
        runtimeSessionId: "runtime-session"
      },
      input: [{ type: "text", text: "say hi" }],
      mentions: [],
      scope: floatingScope(),
      text: null,
      threadId: "thread-current"
    });

    expect(params).toMatchObject({
      agentName: "planner",
      mode: "plan",
      model: "deepseek/deepseek-chat",
      permissionMode: "ask",
      reasoningEffort: "medium",
      runtimeOptions: { mode: "balanced" },
      runtimeRef: "native",
      runtimeSessionId: "runtime-session",
      threadId: "thread-current"
    });
  });

  it("maps Workbench controls into shared turn controls", () => {
    expect(threadTurnControlsFromWorkbenchControls({
      agent: "review",
      mode: "plan",
      modeOptions: ["default", "plan"],
      model: "deepseek/deepseek-chat",
      modelDetails: [],
      modelError: null,
      modelOptions: ["deepseek/deepseek-chat"],
      modelStatus: "resolved",
      permissionMode: "ask",
      permissionModeOptions: ["default", "ask"],
      recentModels: [],
      runtimeRef: "native",
      variant: "medium",
      variantOptions: ["none", "medium"]
    })).toMatchObject({
      agentName: "review",
      mode: "plan",
      model: "deepseek/deepseek-chat",
      permissionMode: "ask",
      reasoningEffort: "medium",
      runtimeOptions: {},
      runtimeRef: "native"
    });

    expect(threadTurnControlsFromWorkbenchControls({
      agent: null,
      mode: "plan",
      modeOptions: ["default", "plan"],
      model: "openai/gpt-4o",
      modelDetails: [],
      modelError: null,
      modelOptions: ["openai/gpt-4o"],
      modelStatus: "resolved",
      permissionMode: "default",
      permissionModeOptions: ["default"],
      recentModels: [],
      runtimeRef: "opencode",
      variant: "none",
      variantOptions: ["none"]
    })).toMatchObject({
      mode: null,
      reasoningEffort: null,
      runtimeOptions: { mode: "plan" },
      runtimeRef: "opencode"
    });
  });

  it("renders first-turn streaming events before turn/start resolves and preserves them after binding", () => {
    const controller = new ThreadTranscriptController(emptyThreadSnapshot(floatingScope()));
    const plan = controller.beginTurn({
      controls: { model: "deepseek/deepseek-chat", runtimeRef: "native" },
      input: [{ type: "text", text: "say hi" }],
      optimisticText: "say hi",
      scope: floatingScope()
    });

    const started = controller.applyGatewayEvent({
      selectedSkills: [],
      threadId: null,
      turnId: "turn-live",
      type: "turnStarted"
    });
    const streamed = controller.applyGatewayEvent({
      entry: entry({
        body: "Streaming before acceptance.",
        id: "assistant:live",
        messageSeq: null,
        status: "running",
        threadId: "",
        turnId: "turn-live",
        updatedAtMs: 1_100
      }),
      turnId: "turn-live",
      type: "entryUpdated"
    });

    expect(started.applied).toBe(true);
    expect(streamed.applied).toBe(true);
    expect(controller.snapshot()?.entries.some((candidate) =>
      candidate.blocks.some((block) => block.body === "Streaming before acceptance.")
    )).toBe(true);

    const accepted = controller.acceptTurnStart(
      { accepted: true, threadId: "thread-floating" },
      plan.prepared,
      "floating turn"
    );

    expect(accepted.threadId).toBe("thread-floating");
    expect(controller.snapshot()?.entries.some((candidate) =>
      candidate.threadId === "thread-floating" &&
      candidate.blocks.some((block) => block.body === "Streaming before acceptance.")
    )).toBe(true);
  });

  it("ignores stale live transcript events for another active turn", () => {
    const controller = new ThreadTranscriptController(emptyThreadSnapshot(floatingScope()));
    controller.beginTurn({
      input: [{ type: "text", text: "say hi" }],
      optimisticText: "say hi",
      scope: floatingScope()
    });
    controller.applyGatewayEvent({
      selectedSkills: [],
      threadId: null,
      turnId: "turn-live",
      type: "turnStarted"
    });

    const stale = controller.applyGatewayEvent({
      entry: entry({
        body: "Stale turn should not render.",
        id: "assistant:stale",
        messageSeq: null,
        status: "running",
        threadId: "",
        turnId: "turn-stale",
        updatedAtMs: 1_100
      }),
      turnId: "turn-stale",
      type: "entryUpdated"
    });

    expect(stale.applied).toBe(false);
    expect(controller.snapshot()?.entries.some((candidate) =>
      candidate.blocks.some((block) => block.body === "Stale turn should not render.")
    )).toBe(false);
  });
});

function floatingScope(): GatewayRequestScope {
  return {
    cwd: "/repo",
    source: {
      kind: "floating",
      lifetime: "process",
      rawId: "activation:test",
      rawIdentity: null,
      visibleName: "Floating"
    }
  };
}

function turnResult({
  answer,
  threadId,
  turnId
}: {
  answer: string;
  threadId: string;
  turnId: string;
}): TurnResultPayload {
  const completedAtMs = 1_000;
  return {
    committedEntries: [entry({
      body: answer,
      id: `message:${turnId}:assistant`,
      threadId,
      turnId,
      updatedAtMs: completedAtMs
    })],
    result: {
      finalAnswer: answer,
      model: "mock-model",
      outcome: "completed",
      provider: "mock-provider",
      sessionId: threadId,
      toolFailures: 0
    },
    thread: {
      backend: { kind: "psychevo", sessionHandle: threadId, runtimeRef: "native" },
      id: threadId,
      sourceKey: null
    },
    turn: completedTurn(turnId, threadId, completedAtMs)
  };
}

function completedTurn(id: string, threadId: string, completedAtMs: number): GatewayTurn {
  return {
    completedAtMs,
    error: null,
    id,
    outcome: "completed",
    startedAtMs: completedAtMs - 100,
    status: "completed",
    threadId
  };
}

function entry({
  body,
  id,
  messageSeq = 2,
  status = "completed",
  threadId,
  turnId,
  updatedAtMs
}: {
  body: string;
  id: string;
  messageSeq?: number | null;
  status?: "cancelled" | "completed" | "failed" | "running";
  threadId: string;
  turnId: string;
  updatedAtMs: number;
}): TranscriptEntry {
  return {
    accounting: null,
    blocks: [block({ body, id: `${id}:text`, updatedAtMs })],
    createdAtMs: updatedAtMs,
    id,
    messageSeq,
    metadata: null,
    role: "assistant",
    source: "runtime.message",
    status,
    threadId,
    turnId,
    updatedAtMs,
    usage: null
  };
}

function block({
  body,
  id,
  updatedAtMs
}: {
  body: string;
  id: string;
  updatedAtMs: number;
}): TranscriptBlock {
  return {
    artifactIds: [],
    body,
    createdAtMs: updatedAtMs,
    detail: body,
    id,
    kind: "text",
    metadata: null,
    order: 0,
    preview: body.slice(0, 240),
    result: null,
    source: "runtime.message",
    status: "completed",
    title: null,
    updatedAtMs
  };
}
