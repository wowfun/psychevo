// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { ThreadController } from "@psychevo/client";
import { describe, expect, it, vi } from "vitest";
import type {
  GatewayEvent,
  GatewayRequestScope,
  GatewayTurn,
  ThreadSnapshot,
  TranscriptBlock,
  TranscriptEntry
} from "@psychevo/protocol";
import { useGatewayLiveEvents } from "./app-live-events";

describe("useGatewayLiveEvents", () => {
  it("settles the selected thread when a floating-origin completion is broadcast", () => {
    const controller = new ThreadController(runningSnapshot());
    const apply = vi.spyOn(controller, "applyGatewayEvent");
    const { result } = renderLiveEvents(controller);
    const event: GatewayEvent = {
      committedEntries: [
        entry({
          body: "Hi from Floating.",
          id: "message:2",
          threadId: "thread-shared",
          turnId: "turn-floating"
        })
      ],
      threadId: "thread-shared",
      turn: completedTurn("turn-floating", "thread-shared"),
      turnId: "turn-floating",
      type: "turnCompleted"
    };

    act(() => result.current.applyGatewayEvent(event));

    const next = controller.snapshot()!;
    expect(apply).toHaveBeenCalledWith(event);
    expect(next.activity.running).toBe(false);
    expect(next.activity.activeTurnId).toBeNull();
    expect(next.entries).toHaveLength(1);
    expect(next.entries[0]?.blocks[0]?.body).toBe("Hi from Floating.");
  });

  it("does not apply another thread's broadcast completion", () => {
    const current = runningSnapshot();
    const controller = new ThreadController(current);
    const { result } = renderLiveEvents(controller);
    act(() => result.current.applyGatewayEvent({
      committedEntries: [
        entry({
          body: "Wrong thread.",
          id: "message:other",
          threadId: "thread-other",
          turnId: "turn-other"
        })
      ],
      threadId: "thread-other",
      turn: completedTurn("turn-other", "thread-other"),
      turnId: "turn-other",
      type: "turnCompleted"
    }));

    const next = controller.snapshot()!;
    expect(next.activity.running).toBe(true);
    expect(next.entries).toEqual([]);
    expect(next.thread?.id).toBe("thread-shared");
  });
});

function renderLiveEvents(controller: ThreadController) {
  return renderHook(() => useGatewayLiveEvents({
    refreshSnapshot: async () => {},
    selectedThreadIdRef: { current: controller.snapshot()?.thread?.id ?? null },
    setLatestGatewayEvent: vi.fn(),
    threadController: controller,
    viewEpochRef: { current: 0 }
  }));
}

function runningSnapshot(): ThreadSnapshot {
  return {
    activity: {
      activeTurnId: "turn-floating",
      queuedTurns: 0,
      running: true
    },
    history: { owner: "psychevo", fidelity: "full", cursor: null, hint: null },
    entries: [],
    pendingActions: [],
    scope: scope(),
    source: {
      kind: "web",
      lifetime: "persistent",
      rawId: "cwd:/repo",
      rawIdentity: null,
      visibleName: null
    },
    thread: {
      backend: { kind: "native", sessionHandle: "thread-shared", runtimeRef: "native" },
      id: "thread-shared",
      sourceKey: null
    }
  };
}

function scope(): GatewayRequestScope {
  return {
    cwd: "/repo",
    source: {
      kind: "web",
      lifetime: "persistent",
      rawId: "cwd:/repo",
      rawIdentity: null,
      visibleName: null
    }
  };
}

function completedTurn(id: string, threadId: string): GatewayTurn {
  return {
    completedAtMs: 1_000,
    error: null,
    id,
    outcome: "completed",
    startedAtMs: 900,
    status: "completed",
    threadId
  };
}

function entry({
  body,
  id,
  threadId,
  turnId
}: {
  body: string;
  id: string;
  threadId: string;
  turnId: string;
}): TranscriptEntry {
  return {
    accounting: null,
    blocks: [block({ body, id: `${id}:text` })],
    createdAtMs: 1_000,
    id,
    messageSeq: 2,
    metadata: null,
    role: "assistant",
    source: "runtime.message",
    status: "completed",
    threadId,
    turnId,
    updatedAtMs: 1_000,
    usage: null
  };
}

function block({
  body,
  id
}: {
  body: string;
  id: string;
}): TranscriptBlock {
  return {
    artifactIds: [],
    body,
    createdAtMs: 1_000,
    detail: body,
    id,
    kind: "text",
    metadata: null,
    order: 0,
    preview: body,
    result: null,
    source: "runtime.message",
    status: "completed",
    title: null,
    updatedAtMs: 1_000
  };
}
