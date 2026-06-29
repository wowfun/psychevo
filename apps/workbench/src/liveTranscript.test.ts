import { describe, expect, it } from "vitest";
import type { GatewayEvent } from "@psychevo/protocol";
import {
  appendOptimisticPrompt,
  applyLiveTranscriptEvent,
  reconcileThreadSnapshot
} from "./liveTranscript";
import {
  block,
  completedTurn,
  entry,
  eventWithEntry,
  snapshot
} from "./liveTranscript.test-support";

describe("applyLiveTranscriptEvent", () => {
  it("upserts running assistant entries and updates blocks by id", () => {
    const running = eventWithEntry("entryUpdated", entry({
      blocks: [block({ body: "hel", detail: "hel", preview: "hel", status: "running" })],
      status: "running"
    }));

    const afterRunning = applyLiveTranscriptEvent(snapshot(), running);
    expect(afterRunning.entries).toHaveLength(1);
    expect(afterRunning.entries[0]?.blocks[0]?.body).toBe("hel");
    expect(afterRunning.entries[0]?.status).toBe("running");

    const afterDelta = applyLiveTranscriptEvent(afterRunning, {
      type: "entryDelta",
      turnId: "turn-1",
      entryId: "live:turn-1:assistant",
      blockId: "live:turn-1:assistant:text",
      delta: "lo"
    });
    expect(afterDelta.entries[0]?.blocks[0]?.body).toBe("hello");
    expect(afterDelta.entries[0]?.blocks[0]?.detail).toBe("hello");
    expect(afterDelta.entries[0]?.blocks[0]?.preview).toBe("hello");
  });

  it("replaces live overlay with committed entries on turn completion", () => {
    const current = {
      ...snapshot(),
      entries: [
        entry({
          id: "live:turn-1:assistant",
          messageSeq: null,
          source: "runtime.stream",
          blocks: [block({ id: "live:turn-1:assistant:text", body: "live answer" })]
        })
      ]
    };
    const committed = entry({
      id: "message:2:assistant",
      messageSeq: 2,
      source: "runtime.message",
      blocks: [block({ id: "message:2:assistant:text", body: "committed answer" })]
    });

    const next = applyLiveTranscriptEvent(current, {
      type: "turnCompleted",
      threadId: "thread-1",
      turnId: "turn-1",
      turn: completedTurn("turn-1", "thread-1"),
      committedEntries: [committed]
    });

    expect(next.activity.running).toBe(false);
    expect(next.activity.activeTurnId).toBeNull();
    expect(next.entries.map((candidate) => candidate.id)).toEqual(["message:2:assistant"]);
    expect(next.entries[0]?.blocks[0]?.body).toBe("committed answer");
  });

  it("keeps committed assistant messages when stale assistant and tool updates arrive late", () => {
    const committed = applyLiveTranscriptEvent(snapshot(), {
      type: "turnCompleted",
      threadId: "thread-1",
      turnId: "turn-1",
      turn: completedTurn("turn-1", "thread-1"),
      committedEntries: [
        entry({
          id: "message:6",
          messageSeq: 6,
          source: "runtime.message",
          status: "completed",
          blocks: [
            block({
              id: "message:6:text",
              source: "runtime.message",
              status: "completed",
              body: "committed final answer"
            })
          ]
        })
      ]
    });

    const next = applyLiveTranscriptEvent(committed, eventWithEntry("entryUpdated", entry({
      id: "live:turn-1:assistant:stale",
      blocks: [
        block({
          id: "live:turn-1:assistant:stale:text",
          body: "duplicated final answer"
        }),
        block({
          id: "live:turn-1:tool:stale",
          kind: "shell",
          title: "exec_command",
          status: "running",
          body: "late tool output"
        })
      ]
    })));

    expect(committed.activity.activeTurnId).toBeNull();
    expect(next).toBe(committed);
    expect(next.entries.map((candidate) => candidate.id)).toEqual(["message:6"]);
    expect(next.entries[0]?.blocks.map((candidate) => candidate.id)).toEqual(["message:6:text"]);
  });

  it("keeps distinct live assistant owners even when text overlaps", () => {
    const current = {
      ...snapshot(),
      entries: [
        entry({
          id: "live:turn-1:assistant:0",
          metadata: { projection: "assistant_segment", liveOrder: 0, streamSeq: 1 },
          status: "completed",
          blocks: [
            block({
              id: "live:turn-1:assistant:0:text:0",
              status: "completed",
              body: "已创建成功。自动化标题：pevo-live-engineering-tip",
              detail: "已创建成功。自动化标题：pevo-live-engineering-tip",
              metadata: { projection: "assistant_phase" }
            })
          ]
        })
      ]
    };

    const next = applyLiveTranscriptEvent(current, eventWithEntry("entryCompleted", entry({
      id: "live:turn-1:assistant:1",
      metadata: { projection: "assistant_segment", liveOrder: 1, streamSeq: 2 },
      status: "completed",
      blocks: [
        block({
          id: "live:turn-1:assistant:1:text:0",
          status: "completed",
          body: "已创建成功。自动化标题：pevo-live-engineering-tip",
          detail: "已创建成功。自动化标题：pevo-live-engineering-tip",
          metadata: { content_array_index: 0 }
        })
      ]
    })));

    expect(next.entries.map((candidate) => candidate.id)).toEqual([
      "live:turn-1:assistant:0",
      "live:turn-1:assistant:1"
    ]);
  });

  it("settles failed turn completion without leaving tool rows running", () => {
    const current = {
      ...snapshot(),
      entries: [
        entry({
          id: "live:turn-1:tool",
          messageSeq: null,
          source: "runtime.stream",
          blocks: [
            block({
              id: "live:turn-1:tool:block",
              kind: "shell",
              title: "exec_command",
              status: "running"
            })
          ]
        })
      ]
    };

    const next = applyLiveTranscriptEvent(current, {
      type: "turnCompleted",
      threadId: "thread-1",
      turnId: "turn-1",
      turn: {
        ...completedTurn("turn-1", "thread-1"),
        status: "failed",
        outcome: "failed",
        error: { message: "model service failed" }
      },
      committedEntries: []
    });

    expect(next.activity.running).toBe(false);
    expect(next.activity.activeTurnId).toBeNull();
    expect(next.entries.map((candidate) => candidate.id)).toEqual([
      "live:turn-1:tool",
      "turn:turn-1:terminal"
    ]);
    expect(next.entries[0]?.blocks[0]?.status).toBe("failed");
    expect(next.entries[1]?.blocks[0]?.body).toBe("model service failed");
  });

  it("binds an empty snapshot to the real thread on first turn completion", () => {
    const empty = {
      ...snapshot(),
      thread: null,
      entries: [],
      activity: {
        running: false,
        activeTurnId: null,
        queuedTurns: 0
      }
    };
    const optimistic = appendOptimisticPrompt(empty, "hello", 10);
    const started = applyLiveTranscriptEvent(optimistic, {
      type: "turnStarted",
      threadId: null,
      turnId: "turn-1",
      selectedSkills: []
    });
    const withLive = applyLiveTranscriptEvent(started, eventWithEntry("entryUpdated", entry({
      threadId: "",
      blocks: [block({ body: "live answer" })]
    })));
    const committed = entry({
      id: "message:1:user",
      threadId: "thread-1",
      messageSeq: 1,
      role: "user",
      status: "completed",
      source: "runtime.message",
      blocks: [block({ id: "message:1:user:text", body: "hello", status: "completed" })]
    });

    const next = applyLiveTranscriptEvent(withLive, {
      type: "turnCompleted",
      threadId: "thread-1",
      turnId: "turn-1",
      turn: completedTurn("turn-1", "thread-1"),
      committedEntries: [committed]
    });

    expect(next.thread?.id).toBe("thread-1");
    expect(next.entries.map((candidate) => candidate.id)).toEqual(["message:1:user"]);
    expect(next.entries[0]?.blocks[0]?.body).toBe("hello");
  });

  it("treats missing committed entries on turn completion as an empty committed slice", () => {
    const current = {
      ...snapshot(),
      entries: [
        entry({
          id: "live:turn-1:assistant",
          messageSeq: null,
          source: "runtime.stream",
          blocks: [block({ id: "live:turn-1:assistant:text", body: "live answer" })]
        })
      ]
    };

    const next = applyLiveTranscriptEvent(current, {
      type: "turnCompleted",
      threadId: "thread-1",
      turnId: "turn-1",
      turn: completedTurn("turn-1", "thread-1"),
      committedEntries: []
    } as GatewayEvent);

    expect(next.activity.running).toBe(false);
    expect(next.activity.activeTurnId).toBeNull();
    expect(next.entries.map((candidate) => candidate.id)).toEqual(["live:turn-1:assistant"]);
  });

  it("removes empty live overlay on turn completion even when committed entries are missing", () => {
    const current = {
      ...snapshot(),
      entries: [
        entry({
          id: "live:turn-1:reasoning",
          messageSeq: null,
          source: "runtime.stream",
          blocks: [
            block({
              id: "live:turn-1:reasoning:block",
              kind: "reasoning",
              title: "Reasoning",
              status: "completed",
              body: null,
              detail: null,
              preview: null
            })
          ]
        }),
        entry({
          id: "live:turn-1:assistant",
          messageSeq: null,
          source: "runtime.stream",
          blocks: [block({ id: "live:turn-1:assistant:text", body: "visible live answer" })]
        })
      ]
    };

    const next = applyLiveTranscriptEvent(current, {
      type: "turnCompleted",
      threadId: "thread-1",
      turnId: "turn-1",
      turn: completedTurn("turn-1", "thread-1"),
      committedEntries: []
    } as GatewayEvent);

    expect(next.entries.map((candidate) => candidate.id)).toEqual(["live:turn-1:assistant"]);
    expect(next.entries[0]?.blocks[0]?.body).toBe("visible live answer");
  });

  it("removes empty live reasoning overlay when committed entries arrive", () => {
    const current = {
      ...snapshot(),
      entries: [
        entry({
          id: "live:turn-1:reasoning",
          messageSeq: null,
          source: "runtime.stream",
          blocks: [
            block({
              id: "live:turn-1:reasoning:block",
              kind: "reasoning",
              title: "Reasoning",
              status: "completed",
              body: null,
              detail: null,
              preview: null
            })
          ]
        })
      ]
    };
    const committed = entry({
      id: "message:2:assistant",
      messageSeq: 2,
      source: "runtime.message",
      blocks: [block({ id: "message:2:assistant:text", body: "committed answer" })]
    });

    const next = applyLiveTranscriptEvent(current, {
      type: "turnCompleted",
      threadId: "thread-1",
      turnId: "turn-1",
      turn: completedTurn("turn-1", "thread-1"),
      committedEntries: [committed]
    });

    expect(next.entries.map((candidate) => candidate.id)).toEqual(["message:2:assistant"]);
    expect(next.entries.some((candidate) => candidate.id.includes("reasoning"))).toBe(false);
  });

  it("ignores live events for a different active turn", () => {
    const current = snapshot();
    const next = applyLiveTranscriptEvent(
      current,
      eventWithEntry("entryUpdated", entry({ turnId: "turn-other" }))
    );

    expect(next).toBe(current);
  });

  it("ignores live events for a different thread", () => {
    const current = snapshot();
    const next = applyLiveTranscriptEvent(
      current,
      eventWithEntry("entryUpdated", entry({ threadId: "thread-2" }))
    );

    expect(next).toBe(current);
  });
});
