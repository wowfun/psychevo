import { describe, expect, it } from "vitest";
import type {
  GatewayRequestScope,
  GatewayThread,
  GatewayTurn,
  ThreadContextReadResult,
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
  threadTurnStartParams,
  ThreadController
} from "./thread-controller";

describe("thread transcript controller helpers", () => {
  it("binds an accepted first turn id to optimistic prompt entries", () => {
    const scope = floatingScope();
    const prepared = prepareThreadTurn(emptyThreadSnapshot(scope), "say hi", null);
    const accepted = acceptThreadTurn(
      prepared.snapshot,
      turnStartResult("thread-floating"),
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

  it("rejects conflicting turn/start thread identities", () => {
    const scope = floatingScope();
    const prepared = prepareThreadTurn(emptyThreadSnapshot(scope), "say hi", null);
    const result = {
      ...turnStartResult("thread-materialized"),
      thread: {
        ...turnStartResult("thread-authoritative").thread
      }
    };

    expect(() => acceptThreadTurn(
      prepared.snapshot,
      result,
      prepared.requestedThreadId,
      "floating turn"
    )).toThrow("conflicting thread identities");
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
    const context = threadContext("native", "planner");
    const params = threadTurnStartParams({
      controls: {
        targetId: context.targetId,
        turnOverrides: {
          mode: "plan",
          model: "deepseek/deepseek-chat",
          permissionMode: "ask",
          reasoning: "medium"
        },
        expectedContextRevision: "context-1",
        expectedControlRevision: "controls-1"
      },
      context,
      input: [{ type: "text", text: "say hi" }],
      mentions: [],
      scope: floatingScope(),
      threadId: "thread-current"
    });

    expect(params).toMatchObject({
      target: { agentRef: "planner", runtimeProfileRef: "native" },
      turnOverrides: {
        mode: "plan",
        model: "deepseek/deepseek-chat",
        permissionMode: "ask",
        reasoning: "medium"
      },
      expectedContextRevision: "context-1",
      expectedControlRevision: "controls-1",
      threadId: "thread-current"
    });
  });

  it("renders first-turn streaming events before turn/start resolves and preserves them after binding", () => {
    const controller = new ThreadController(emptyThreadSnapshot(floatingScope()));
    controller.setContext(threadContext("native", null));
    const plan = controller.beginTurn({
      controls: {
        targetId: "target:default:native",
        turnOverrides: { model: "deepseek/deepseek-chat" }
      },
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
      turnStartResult("thread-floating"),
      plan.prepared,
      "floating turn"
    );

    expect(accepted.threadId).toBe("thread-floating");
    expect(controller.snapshot()?.entries.some((candidate) =>
      candidate.threadId === "thread-floating" &&
      candidate.blocks.some((block) => block.body === "Streaming before acceptance.")
    )).toBe(true);
  });

  it("preserves the authoritative ACP identity on an accepted first turn", () => {
    const controller = new ThreadController(emptyThreadSnapshot(floatingScope()));
    controller.setContext(threadContext("codex", "codex"));
    const plan = controller.beginTurn({
      controls: { targetId: "target:codex:codex", turnOverrides: {} },
      input: [{ type: "text", text: "inspect" }],
      optimisticText: "inspect",
      scope: floatingScope()
    });

    const accepted = controller.acceptTurnStart(
      turnStartResult("thread-acp", {
        kind: "acp",
        runtimeRef: "codex",
        sessionHandle: null
      }),
      plan.prepared
    );

    expect(accepted.thread.backend).toEqual({
      kind: "acp",
      runtimeRef: "codex",
      sessionHandle: null
    });
    expect(controller.snapshot()?.thread?.backend.kind).toBe("acp");
  });

  it("atomically replaces every control descriptor from a control receipt", () => {
    const controller = new ThreadController(emptyThreadSnapshot(floatingScope(), "thread-acp"));
    const context = threadContext("codex-fixture", "codex-fixture");
    const control = (id: string, revision: string): ThreadContextReadResult["controls"][number] => ({
      id,
      label: id,
      surfaceRole: id === "model" ? "model" : "reasoning",
      mutability: "selectable",
      enabled: true,
      required: false,
      unavailableReason: null,
      effectiveValue: id === "model" ? "fixture/default" : "medium",
      effectiveSource: "runtimeObserved",
      isDefault: false,
      choices: [],
      dependsOn: null,
      applyScope: "session",
      stability: "stable",
      channelSafe: true,
      capabilityRevision: revision
    });
    context.controls = [control("model", "old-revision"), control("effort", "old-revision")];
    controller.setContext(context);
    const controls = [control("model", "new-revision"), control("effort", "new-revision")];

    controller.applyControlReceipt({
      changed: true,
      status: "observed",
      control: controls[0]!,
      context: {
        ...context,
        controls,
        contextRevision: "context-new",
        controlRevision: "controls-new"
      },
      bindingRevision: 0,
      contextRevision: "context-new",
      controlRevision: "controls-new"
    });

    const effort = controller.context()?.controls.find((candidate) => candidate.id === "effort");
    expect(effort?.capabilityRevision).toBe("new-revision");
    expect(controller.controlSetParams(
      context.targetId,
      effort!,
      "high",
      floatingScope(),
      "thread-acp"
    )).toMatchObject({
      targetId: context.targetId,
      expectedCapabilityRevision: "new-revision",
      expectedContextRevision: "context-new",
      expectedControlRevision: "controls-new"
    });
  });

  it("correlates a config failure after turn/start acceptance before any gateway event", () => {
    const controller = new ThreadController(emptyThreadSnapshot(floatingScope(), "thread-current"));
    controller.setContext(threadContext("native", null));
    const plan = controller.beginTurn({
      controls: { targetId: "target:default:native", turnOverrides: {} },
      input: [{ type: "text", text: "fail during configuration" }],
      optimisticText: "fail during configuration",
      scope: floatingScope(),
      threadId: "thread-current"
    });

    controller.acceptTurnStart(
      turnStartResult("thread-current", undefined, "turn-config-failure"),
      plan.prepared
    );

    expect(controller.turnId()).toBe("turn-config-failure");
    expect(controller.applyTurnError(
      turnError("thread-current", "turn-config-failure")
    ).applied).toBe(true);
    expect(controller.turnId()).toBeNull();
  });

  it("rolls back an optimistic prompt when turn/start rejects before acceptance", () => {
    const initial = emptyThreadSnapshot(floatingScope());
    const controller = new ThreadController(initial);
    controller.setContext(threadContext("native", null));
    const plan = controller.beginTurn({
      controls: { targetId: "target:default:native", turnOverrides: {} },
      input: [{ type: "text", text: "rejected before acceptance" }],
      optimisticText: "rejected before acceptance",
      scope: floatingScope()
    });

    expect(controller.snapshot()?.entries).toHaveLength(1);
    controller.rejectTurnStart(plan.prepared);

    expect(controller.snapshot()).toBe(initial);
    expect(controller.threadId()).toBeNull();
    expect(controller.turnId()).toBeNull();
    expect(() => controller.beginTurn({
      controls: { targetId: "target:default:native", turnOverrides: {} },
      input: [{ type: "text", text: "retry" }],
      optimisticText: "retry",
      scope: floatingScope()
    })).not.toThrow();
  });

  it("admits only one turn/start while Gateway acceptance is pending", () => {
    const controller = new ThreadController(emptyThreadSnapshot(floatingScope()));
    controller.setContext(threadContext("native", null));
    controller.beginTurn({
      controls: { targetId: "target:default:native", turnOverrides: {} },
      input: [{ type: "text", text: "first" }],
      optimisticText: "first",
      scope: floatingScope()
    });

    expect(() => controller.beginTurn({
      controls: { targetId: "target:default:native", turnOverrides: {} },
      input: [{ type: "text", text: "second" }],
      optimisticText: "second",
      scope: floatingScope()
    })).toThrow("awaiting Gateway acceptance");
    expect(controller.snapshot()?.entries).toHaveLength(1);
  });

  it("does not resurrect a turn settled before the turn/start response", () => {
    const controller = new ThreadController(emptyThreadSnapshot(floatingScope()));
    controller.setContext(threadContext("native", null));
    const plan = controller.beginTurn({
      controls: { targetId: "target:default:native", turnOverrides: {} },
      input: [{ type: "text", text: "fail before response" }],
      optimisticText: "fail before response",
      scope: floatingScope()
    });

    expect(controller.applyTurnError(
      turnError("thread-created", "turn-before-response")
    ).applied).toBe(true);
    controller.acceptTurnStart(
      turnStartResult("thread-created", undefined, "turn-before-response"),
      plan.prepared
    );

    expect(controller.threadId()).toBe("thread-created");
    expect(controller.turnId()).toBeNull();
  });

  it("does not resurrect a gateway completion received before turn/start acceptance", () => {
    const controller = new ThreadController(emptyThreadSnapshot(floatingScope()));
    controller.setContext(threadContext("native", null));
    const plan = controller.beginTurn({
      controls: { targetId: "target:default:native", turnOverrides: {} },
      input: [{ type: "text", text: "complete before response" }],
      optimisticText: "complete before response",
      scope: floatingScope()
    });

    expect(controller.applyGatewayEvent({
      selectedSkills: [],
      threadId: null,
      turnId: "turn-before-response",
      type: "turnStarted"
    }).applied).toBe(true);

    expect(controller.applyTurnResult(turnResult({
      answer: "Already complete.",
      threadId: "thread-created",
      turnId: "turn-before-response"
    })).applied).toBe(true);
    controller.reset(controller.snapshot());
    controller.acceptTurnStart(
      turnStartResult("thread-created", undefined, "turn-before-response"),
      plan.prepared
    );

    expect(controller.threadId()).toBe("thread-created");
    expect(controller.turnId()).toBeNull();
    expect(controller.snapshot()?.activity.running).toBe(false);
  });

  it("ignores stale live transcript events for another active turn", () => {
    const controller = new ThreadController(emptyThreadSnapshot(floatingScope()));
    controller.setContext(threadContext("native", null));
    controller.beginTurn({
      controls: { targetId: "target:default:native", turnOverrides: {} },
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

  it("does not let stale or foreign terminals settle the active Thread turn", () => {
    const controller = new ThreadController(emptyThreadSnapshot(floatingScope(), "thread-current"));
    controller.setContext(threadContext("native", null));
    controller.beginTurn({
      controls: { targetId: "target:default:native", turnOverrides: {} },
      input: [{ type: "text", text: "active" }],
      optimisticText: "active",
      scope: floatingScope(),
      threadId: "thread-current"
    });
    controller.applyGatewayEvent({
      selectedSkills: [],
      threadId: "thread-current",
      turnId: "turn-active",
      type: "turnStarted"
    });

    expect(controller.applyTurnResult(turnResult({
      answer: "stale",
      threadId: "thread-current",
      turnId: "turn-stale"
    })).applied).toBe(false);
    expect(controller.applyTurnResult(turnResult({
      answer: "foreign",
      threadId: "thread-foreign",
      turnId: "turn-active"
    })).applied).toBe(false);
    expect(controller.applyTurnError(turnError("thread-current", "turn-stale")).applied).toBe(false);
    expect(controller.applyTurnError(turnError("thread-foreign", "turn-active")).applied).toBe(false);
    expect(controller.turnId()).toBe("turn-active");
    expect(controller.snapshot()?.activity.running).toBe(true);

    expect(controller.applyTurnError(turnError("thread-current", "turn-active")).applied).toBe(true);
    expect(controller.turnId()).toBeNull();
    expect(controller.snapshot()?.activity.running).toBe(false);
  });

  it("ignores a paced live update that flushes after its terminal result", () => {
    const controller = new ThreadController(emptyThreadSnapshot(floatingScope(), "thread-current"));
    controller.setContext(threadContext("native", null));
    controller.beginTurn({
      controls: { targetId: "target:default:native", turnOverrides: {} },
      input: [{ type: "text", text: "active" }],
      optimisticText: "active",
      scope: floatingScope(),
      threadId: "thread-current"
    });
    controller.applyGatewayEvent({
      selectedSkills: [],
      threadId: "thread-current",
      turnId: "turn-active",
      type: "turnStarted"
    });
    expect(controller.applyTurnResult(turnResult({
      answer: "complete",
      threadId: "thread-current",
      turnId: "turn-active"
    })).applied).toBe(true);

    expect(controller.applyGatewayEvent({
      entry: entry({
        body: "Late stale update.",
        id: "assistant:late",
        messageSeq: null,
        status: "running",
        threadId: "thread-current",
        turnId: "turn-active",
        updatedAtMs: 1_200
      }),
      turnId: "turn-active",
      type: "entryUpdated"
    }).applied).toBe(false);
    expect(controller.snapshot()?.entries.some((candidate) => candidate.id === "assistant:late")).toBe(false);
  });

  it("fails closed before optimistic mutation when Thread Context is absent", () => {
    const controller = new ThreadController(emptyThreadSnapshot(floatingScope()));

    expect(controller.sendability()).toEqual({
      allowed: false,
      reason: "Thread Context is required before starting a turn."
    });
    expect(() => controller.beginTurn({
      controls: { targetId: "target:canonical-agent:opaque-profile", turnOverrides: {} },
      input: [{ type: "text", text: "must not render" }],
      optimisticText: "must not render",
      scope: floatingScope()
    })).toThrow("Thread Context is required");
    expect(controller.snapshot()?.entries).toEqual([]);
  });

  it("admits only the exact Gateway target and never accepts a caller path as agentRef", () => {
    const controller = new ThreadController(emptyThreadSnapshot(floatingScope()));
    controller.setContext(threadContext("opaque-profile", "canonical-agent"));

    expect(controller.admitTurn({
      controls: { targetId: "target:canonical-agent:opaque-profile", turnOverrides: {} },
      input: [{ type: "text", text: "accepted" }],
      mentions: []
    })).toEqual({ allowed: true, reason: null });
    expect(controller.admitTurn({
      controls: { targetId: "/workspace/agents/canonical-agent.md", turnOverrides: {} },
      input: [{ type: "text", text: "rejected" }],
      mentions: []
    })).toEqual({
      allowed: false,
      reason: "The selected Agent target does not match the current Thread Context."
    });
  });

  it("forwards an opaque fourth catalog target without interpreting its id", () => {
    const context = threadContext("native", null);
    context.compatibleTargets.push({
      targetId: "target:7f4a26e91d5c0bb4",
      agentRef: "arbitrary-reviewer",
      runtimeProfileRef: "acp:arbitrary",
      agentLabel: "Arbitrary Reviewer",
      profileLabel: "Arbitrary ACP",
      label: "Arbitrary Reviewer · Arbitrary ACP",
      ready: true,
      unavailableReason: null
    });
    context.targetId = "target:7f4a26e91d5c0bb4";
    context.runtimeProfileRef = "acp:arbitrary";
    const controller = new ThreadController(emptyThreadSnapshot(floatingScope()));
    controller.setContext(context);

    const controls = controller.turnControls(context.targetId, {});
    expect(controls).toEqual(expect.objectContaining({
      targetId: "target:7f4a26e91d5c0bb4"
    }));
    expect(controls).not.toHaveProperty("agentRef");
    expect(controls).not.toHaveProperty("runtimeProfileRef");
    expect(threadTurnStartParams({
      controls,
      context,
      input: [{ type: "text", text: "review" }],
      mentions: [],
      scope: floatingScope(),
      threadId: null
    }).target).toEqual({
      agentRef: "arbitrary-reviewer",
      runtimeProfileRef: "acp:arbitrary"
    });
  });

  it("admits every input part and Agent mention through target-scoped descriptors", () => {
    const context = threadContext("opaque-profile", "canonical-agent");
    context.inputCapabilities = context.inputCapabilities.map((capability) => (
      capability.kind === "image"
        ? { ...capability, enabled: false, unavailableReason: "Images are unavailable for this target." }
        : capability
    ));
    const controller = new ThreadController(emptyThreadSnapshot(floatingScope()));
    controller.setContext(context);
    const controls = { targetId: "target:canonical-agent:opaque-profile", turnOverrides: {} };

    expect(controller.admitTurn({
      controls,
      input: [{ type: "image", input: { kind: "url", url: "data:image/png;base64,AA==" } }],
      mentions: []
    })).toEqual({ allowed: false, reason: "Images are unavailable for this target." });
    expect(controller.admitTurn({
      controls,
      input: [{ type: "context", label: "Evidence", text: "source", visibleToModel: true }],
      mentions: [{
        visibleText: "@reviewer",
        range: { start: 0, end: 9 },
        target: { kind: "agent", name: "reviewer", source: null, entrypoints: [], backendRef: null }
      }]
    })).toEqual({ allowed: true, reason: null });

    context.inputCapabilities = context.inputCapabilities.map((capability) => (
      capability.kind === "agentMention"
        ? { ...capability, enabled: false, unavailableReason: "Agent delegation is unavailable." }
        : capability
    ));
    controller.setContext(context);
    expect(controller.admitTurn({
      controls,
      input: [{ type: "text", text: "@reviewer" }],
      mentions: [{
        visibleText: "@reviewer",
        range: { start: 0, end: 9 },
        target: { kind: "agent", name: "reviewer", source: null, entrypoints: [], backendRef: null }
      }]
    })).toEqual({ allowed: false, reason: "Agent delegation is unavailable." });
  });
});

function threadContext(runtimeProfileRef: string, agentRef: string | null): ThreadContextReadResult {
  const targetId = `target:${agentRef ?? "default"}:${runtimeProfileRef}`;
  return {
    targetId,
    runtimeProfileRef,
    selectionState: "draft",
    profiles: [],
    binding: null,
    controls: [],
    stability: "stable",
    capabilities: [],
    compatibleTargets: [{
      targetId,
      agentRef,
      runtimeProfileRef,
      agentLabel: agentRef ?? "Default Agent",
      profileLabel: "Opaque Profile",
      label: `${agentRef ?? "Default Agent"} · Opaque Profile`,
      ready: true,
      unavailableReason: null
    }],
    inputCapabilities: ["text", "image", "resource", "resourceLink", "embeddedContext", "agentMention"].map((kind) => ({
      kind,
      enabled: true,
      unavailableReason: null
    })),
    actions: [],
    sendability: { allowed: true, reason: null, recoveryAction: null },
    history: { owner: "agent", fidelity: "full", cursor: null, hint: null },
    pendingInteractions: [],
    contextRevision: "context-opaque",
    controlRevision: "controls-opaque"
  };
}

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

function turnError(threadId: string, turnId: string) {
  return {
    error: {
      message: "Turn failed.",
      code: "test_failure",
      stage: "delivery",
      retryClass: "never",
      delivery: "notDelivered" as const,
      recoveryAction: null,
      diagnosticRef: null
    },
    threadId,
    turnId
  };
}

function turnStartResult(
  threadId: string,
  backend: GatewayThread["backend"] = {
    kind: "native",
    runtimeRef: "native",
    sessionHandle: null
  },
  turnId = "turn-started"
) {
  return {
    accepted: true,
    threadId,
    turnId,
    thread: {
      backend,
      id: threadId,
      sourceKey: null
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
      backend: { kind: "native", sessionHandle: threadId, runtimeRef: "native" },
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
