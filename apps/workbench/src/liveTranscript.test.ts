import { describe, expect, it } from "vitest";
import type { GatewayEvent, ThreadSnapshot, TranscriptBlock, TranscriptEntry } from "@psychevo/protocol";
import {
  appendOptimisticPrompt,
  applyLiveTranscriptEvent,
  reconcileThreadSnapshot
} from "./liveTranscript";

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

  it("keeps an optimistic prompt until a canonical prompt reconciles it exactly", () => {
    const current = appendOptimisticPrompt(snapshot(), "hello", 10);
    expect(current.entries[0]?.source).toBe("client.optimistic");

    const stillPending = reconcileThreadSnapshot(current, snapshot());
    expect(stillPending.entries[0]?.blocks[0]?.body).toBe("hello");

    const reconciled = reconcileThreadSnapshot(current, {
      ...snapshot(),
      entries: [
        entry({
          id: "stored-prompt",
          role: "user",
          source: "runtime.message",
          blocks: [block({ id: "stored-prompt:text", body: "hello", detail: "hello" })]
        })
      ]
    });
    expect(reconciled.entries).toHaveLength(1);
    expect(reconciled.entries[0]?.id).toBe("stored-prompt");
  });

  it("preserves visible entries from an explicit historical snapshot", () => {
    const current = { ...snapshot(), thread: null, entries: [] };
    const incoming = {
      ...snapshot(),
      activity: { running: false, activeTurnId: null, queuedTurns: 0 },
      entries: [
        entry({
          id: "message:1",
          role: "user",
          messageSeq: 1,
          source: "runtime.message",
          status: "completed",
          blocks: [
            block({
              id: "message:1:block:0",
              body: "hello history",
              detail: "hello history",
              preview: "hello history",
              source: "runtime.message",
              status: "completed"
            })
          ]
        }),
        entry({
          id: "message:2",
          messageSeq: 2,
          source: "runtime.message",
          status: "completed",
          blocks: [
            block({
              id: "message:2:block:0",
              body: "hello from assistant",
              detail: "hello from assistant",
              preview: "hello from assistant",
              source: "runtime.message",
              status: "completed"
            })
          ]
        })
      ]
    };

    const reconciled = reconcileThreadSnapshot(current, incoming);

    expect(reconciled.entries.map((candidate) => candidate.id)).toEqual(["message:1", "message:2"]);
    expect(reconciled.entries[0]?.blocks[0]?.body).toBe("hello history");
    expect(reconciled.entries[1]?.blocks[0]?.body).toBe("hello from assistant");
  });

  it("does not preserve empty live overlay during snapshot reconciliation", () => {
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
              body: null,
              detail: null,
              preview: null
            })
          ]
        })
      ]
    };
    const incoming = {
      ...snapshot(),
      entries: [
        entry({
          id: "message:2",
          messageSeq: 2,
          source: "runtime.message",
          blocks: [block({ id: "message:2:block:0", body: "committed answer" })]
        })
      ]
    };

    const reconciled = reconcileThreadSnapshot(current, incoming);

    expect(reconciled.entries.map((candidate) => candidate.id)).toEqual(["message:2"]);
  });

  it("does not preserve stale visible live overlay after the turn is inactive", () => {
    const current = {
      ...snapshot(),
      activity: { running: false, activeTurnId: null, queuedTurns: 0 },
      entries: [
        entry({
          id: "live:turn-1:assistant",
          messageSeq: null,
          source: "runtime.stream",
          blocks: [block({ id: "live:turn-1:assistant:text", body: "stale live answer" })]
        })
      ]
    };
    const incoming = {
      ...snapshot(),
      activity: { running: false, activeTurnId: null, queuedTurns: 0 },
      entries: [
        entry({
          id: "message:2",
          messageSeq: 2,
          source: "runtime.message",
          blocks: [block({ id: "message:2:block:0", body: "committed answer" })]
        })
      ]
    };

    const reconciled = reconcileThreadSnapshot(current, incoming);

    expect(reconciled.entries.map((candidate) => candidate.id)).toEqual(["message:2"]);
  });

  it("orders live entries by explicit liveOrder metadata before timestamp tie-breaks", () => {
    const current = applyLiveTranscriptEvent(
      applyLiveTranscriptEvent(
        snapshot(),
        eventWithEntry("entryUpdated", entry({
          id: "live:turn-1:tool",
          metadata: { liveOrder: 20 },
          createdAtMs: 1,
          blocks: [block({ id: "live:turn-1:tool:block", kind: "shell", body: "tool" })]
        }))
      ),
      eventWithEntry("entryUpdated", entry({
        id: "live:turn-1:assistant-text",
        metadata: { liveOrder: 10 },
        createdAtMs: 2,
        blocks: [block({ id: "live:turn-1:assistant-text:block", kind: "text", body: "assistant text" })]
      }))
    );

    expect(current.entries.map((candidate) => candidate.id)).toEqual([
      "live:turn-1:assistant-text",
      "live:turn-1:tool"
    ]);
  });

  it("keeps an optimistic prompt before same-turn live assistant and tool rows", () => {
    const idleSnapshot = {
      ...snapshot(),
      activity: { running: false, activeTurnId: null, queuedTurns: 0 }
    };
    const optimistic = appendOptimisticPrompt(idleSnapshot, "$x-daily", 10);
    const started = applyLiveTranscriptEvent(optimistic, {
      type: "turnStarted",
      threadId: "thread-1",
      turnId: "turn-1",
      selectedSkills: []
    });
    const withAssistant = applyLiveTranscriptEvent(
      started,
      eventWithEntry("entryUpdated", entry({
        id: "live:turn-1:assistant:0",
        metadata: { projection: "assistant_segment", liveOrder: 0, streamSeq: 1 },
        blocks: [block({ id: "live:turn-1:assistant:0:text", body: "好的，开始执行。" })]
      }))
    );

    expect(withAssistant.entries.map((candidate) => candidate.source)).toEqual([
      "client.optimistic",
      "runtime.stream"
    ]);
    expect(withAssistant.entries[0]?.turnId).toBe("turn-1");
    expect(withAssistant.entries[0]?.metadata).toMatchObject({ liveOrder: -1 });
    expect(withAssistant.entries[0]?.blocks[0]?.body).toBe("$x-daily");
  });

  it("binds a newly started turn thread without requiring an immediate snapshot refresh", () => {
    const idleSnapshot = {
      ...snapshot(),
      thread: null,
      activity: { running: false, activeTurnId: null, queuedTurns: 0 }
    };
    const optimistic = appendOptimisticPrompt(idleSnapshot, "hello", 10);

    const started = applyLiveTranscriptEvent(optimistic, {
      type: "turnStarted",
      threadId: "thread-2",
      turnId: "turn-2",
      selectedSkills: []
    });

    expect(started.thread).toEqual({
      id: "thread-2",
      backend: { kind: "psychevo", nativeId: "thread-2" },
      sourceKey: "web:test"
    });
    expect(started.activity.activeTurnId).toBe("turn-2");
  });

  it("does not keep active live overlay that is covered by message-derived snapshot entries", () => {
    const current = {
      ...snapshot(),
      entries: [
        entry({
          id: "live:turn-1:assistant:0",
          metadata: { projection: "assistant_segment", liveOrder: 0, streamSeq: 4 },
          blocks: [
            block({
              id: "live:turn-1:assistant:0:reasoning",
              kind: "reasoning",
              title: "Thinking",
              body: "The user wants to run the X daily skill. Let me follow the skill instructions.",
              detail: "The user wants to run the X daily skill. Let me follow the skill instructions.",
              order: 0
            }),
            block({
              id: "live:turn-1:assistant:0:text:1",
              kind: "reasoning",
              title: "Thinking",
              body: "好的，开始执行 X 日报流程。先运行抓取脚本。",
              detail: "好的，开始执行 X 日报流程。先运行抓取脚本。",
              order: 1
            }),
            block({
              id: "live:turn-1:tool:runtime-fetch",
              kind: "shell",
              title: "exec_command python fetch.py",
              status: "running",
              body: "[x-fetch] running\n",
              detail: "[x-fetch] running\n",
              order: 2,
              metadata: {
                projection: "tool",
                tool_name: "exec_command",
                tool_call_id: "runtime-fetch",
                args: { cmd: "python fetch.py" }
              }
            })
          ]
        })
      ]
    };
    const incoming = {
      ...snapshot(),
      entries: [
        entry({
          id: "message:1",
          role: "user",
          source: "runtime.message",
          messageSeq: 1,
          turnId: "message:1",
          blocks: [
            block({
              id: "message:1:block:0",
              kind: "text",
              source: "runtime.message",
              body: "$x-daily",
              detail: "$x-daily"
            })
          ]
        }),
        entry({
          id: "message:2",
          source: "runtime.message",
          messageSeq: 2,
          turnId: "message:2",
          blocks: [
            block({
              id: "message:2:block:0",
              kind: "reasoning",
              source: "runtime.message",
              title: "Thinking",
              body: "The user wants to run the X daily skill. Let me follow the skill instructions.",
              detail: "The user wants to run the X daily skill. Let me follow the skill instructions.",
              order: 0
            }),
            block({
              id: "message:2:block:1",
              kind: "text",
              source: "runtime.message",
              body: "好的，开始执行 X 日报流程。先运行抓取脚本。",
              detail: "好的，开始执行 X 日报流程。先运行抓取脚本。",
              order: 1,
              metadata: { projection: "assistant_phase" }
            }),
            block({
              id: "tool:model-fetch",
              kind: "shell",
              source: "runtime.message",
              title: "exec_command",
              status: "pending",
              order: 2,
              metadata: {
                projection: "tool",
                tool_name: "exec_command",
                tool_call_id: "model-fetch",
                args: { cmd: "python fetch.py" }
              }
            })
          ]
        })
      ]
    };

    const next = reconcileThreadSnapshot(current, incoming);

    expect(next.entries.map((candidate) => candidate.id)).toEqual(["message:1", "message:2"]);
    expect(next.entries[1]?.blocks.map((candidate) => candidate.kind)).toEqual([
      "reasoning",
      "text",
      "shell"
    ]);
    expect(next.entries[1]?.blocks[1]?.body).toBe("好的，开始执行 X 日报流程。先运行抓取脚本。");
  });

  it("drops stale pending-only tool overlays when an authoritative segment owns the same tool", () => {
    const current = applyLiveTranscriptEvent(
      snapshot(),
      eventWithEntry("entryStarted", entry({
        id: "live:turn-1:tool:pending-fetch",
        metadata: null,
        blocks: [
          block({
            id: "live:turn-1:tool:pending-fetch:block",
            kind: "shell",
            title: "exec_command python fetch.py",
            status: "pending",
            body: null,
            detail: null,
            preview: null,
            metadata: {
              projection: "tool",
              tool_name: "exec_command",
              tool_call_id: "pending-fetch",
              args: { cmd: "python fetch.py" }
            }
          })
        ]
      }))
    );
    const next = applyLiveTranscriptEvent(
      current,
      eventWithEntry("entryCompleted", entry({
        id: "live:turn-1:assistant:0",
        metadata: { projection: "assistant_segment", liveOrder: 0, streamSeq: 2, authoritativeBlocks: true },
        blocks: [
          block({
            id: "live:turn-1:assistant:0:text",
            kind: "text",
            body: "先运行 fetch.py。",
            order: 0
          }),
          block({
            id: "live:turn-1:tool:model-fetch",
            kind: "shell",
            title: "exec_command python fetch.py",
            status: "pending",
            order: 1,
            metadata: {
              projection: "tool",
              tool_name: "exec_command",
              tool_call_id: "model-fetch",
              args: { cmd: "python fetch.py" }
            }
          })
        ]
      }))
    );

    expect(next.entries.map((candidate) => candidate.id)).toEqual(["live:turn-1:assistant:0"]);
    expect(next.entries[0]?.blocks.map((candidate) => candidate.id)).toEqual([
      "live:turn-1:assistant:0:text",
      "live:turn-1:tool:model-fetch"
    ]);
  });

  it("preserves tool args when a completion update only carries result data", () => {
    const afterStart = applyLiveTranscriptEvent(
      snapshot(),
      eventWithEntry("entryStarted", entry({
        id: "live:turn-1:assistant",
        blocks: [
          block({
            id: "live:turn-1:tool:write",
            kind: "file",
            title: "write",
            metadata: {
              projection: "tool",
              tool_name: "write",
              tool_call_id: "write",
              args: { path: "feeds/report.md", content: "body" }
            }
          })
        ]
      }))
    );
    const afterEnd = applyLiveTranscriptEvent(
      afterStart,
      eventWithEntry("entryCompleted", entry({
        id: "live:turn-1:assistant",
        blocks: [
          block({
            id: "live:turn-1:tool:write",
            kind: "file",
            title: "write",
            status: "completed",
            preview: "{\"bytes_written\":4}",
            detail: "{\"bytes_written\":4}",
            metadata: {
              projection: "tool",
              tool_name: "write",
              tool_call_id: "write"
            },
            result: {
              resultMessageSeq: 2,
              status: "completed",
              content: "{\"bytes_written\":4}",
              isError: false,
              metadata: null,
              createdAtMs: 2,
              updatedAtMs: 2
            }
          })
        ]
      }))
    );

    const metadata = afterEnd.entries[0]?.blocks[0]?.metadata as Record<string, unknown>;
    expect(metadata.args).toEqual({ path: "feeds/report.md", content: "body" });
    expect(afterEnd.entries[0]?.blocks[0]?.result?.content).toBe("{\"bytes_written\":4}");
  });

  it("keeps Gateway-preserved live reasoning when an authoritative segment snapshot arrives", () => {
    const current = applyLiveTranscriptEvent(
      snapshot(),
      eventWithEntry("entryStarted", entry({
        id: "live:turn-1:assistant:0",
        metadata: { projection: "assistant_segment", liveOrder: 0, streamSeq: 1, authoritativeBlocks: false },
        createdAtMs: 10,
        blocks: [
          block({
            id: "live:turn-1:assistant:0:reasoning",
            kind: "reasoning",
            title: "Thinking",
            body: "This early stream was provisional.",
            order: 0
          }),
          block({
            id: "live:turn-1:tool:call_fetch",
            kind: "shell",
            title: "exec_command python fetch.py",
            status: "pending",
            order: 1000,
            metadata: {
              projection: "tool",
              tool_name: "exec_command",
              tool_call_id: "call_fetch",
              args: { cmd: "python fetch.py" }
            }
          })
        ]
      }))
    );

    const next = applyLiveTranscriptEvent(
      current,
      eventWithEntry("entryCompleted", entry({
        id: "live:turn-1:assistant:0",
        metadata: { projection: "assistant_segment", liveOrder: 0, streamSeq: 2, authoritativeBlocks: true },
        updatedAtMs: 20,
        blocks: [
          block({
            id: "live:turn-1:assistant:0:reasoning",
            kind: "reasoning",
            title: "Thinking",
            body: "This early stream was real runtime reasoning.",
            status: "completed",
            order: 0,
            metadata: { projection: "reasoning", origin: "run_stream_reasoning" }
          }),
          block({
            id: "live:turn-1:assistant:0:text:0",
            kind: "text",
            body: "好的，开始执行 X 日报流程。",
            detail: "好的，开始执行 X 日报流程。",
            order: 1,
            metadata: { projection: "assistant_phase" }
          }),
          block({
            id: "live:turn-1:tool:call_fetch",
            kind: "shell",
            title: "exec_command python fetch.py",
            status: "pending",
            order: 2,
            metadata: {
              projection: "tool",
              tool_name: "exec_command",
              tool_call_id: "call_fetch",
              args: { cmd: "python fetch.py" }
            }
          })
        ]
      }))
    );

    expect(next.entries).toHaveLength(1);
    expect(next.entries[0]?.createdAtMs).toBe(10);
    expect(next.entries[0]?.metadata).toMatchObject({ authoritativeBlocks: true, streamSeq: 2 });
    expect(next.entries[0]?.blocks.map((candidate) => candidate.kind)).toEqual(["reasoning", "text", "shell"]);
    expect(next.entries[0]?.blocks[0]?.body).toBe("This early stream was real runtime reasoning.");
    expect(next.entries[0]?.blocks[0]?.status).toBe("completed");
    expect(next.entries[0]?.blocks[1]?.body).toBe("好的，开始执行 X 日报流程。");
    expect(next.entries[0]?.blocks[2]?.order).toBe(2);
  });

  it("drops active live entries after snapshot reconciliation when message-derived text covers them", () => {
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
              title: "Thinking",
              status: "running",
              body: "same text"
            })
          ]
        })
      ]
    };
    const incoming = {
      ...snapshot(),
      entries: [
        entry({
          id: "message:8:reasoning",
          messageSeq: 8,
          source: "runtime.message",
          blocks: [
            block({
              id: "message:8:reasoning:block",
              kind: "reasoning",
              title: "Reasoning",
              status: "completed",
              body: "same text"
            })
          ]
        })
      ]
    };

    const next = reconcileThreadSnapshot(current, incoming);

    expect(next.entries.map((candidate) => candidate.id)).toEqual(["message:8:reasoning"]);
  });

  it("removes covered blocks from a live entry while preserving uncovered active blocks", () => {
    const current = {
      ...snapshot(),
      entries: [
        entry({
          id: "live:turn-1:assistant:0",
          metadata: { projection: "assistant_segment", liveOrder: 0, streamSeq: 8 },
          blocks: [
            block({
              id: "live:turn-1:assistant:0:text:1",
              kind: "text",
              body: "好的，开始执行 X 日报流程！",
              detail: "好的，开始执行 X 日报流程！",
              order: 1
            }),
            block({
              id: "live:turn-1:tool:call_fetch",
              kind: "shell",
              title: "exec_command python fetch.py",
              status: "running",
              body: "[x-fetch] still running\n",
              detail: "[x-fetch] still running\n",
              order: 2,
              metadata: {
                projection: "tool",
                tool_name: "exec_command",
                tool_call_id: "call_fetch",
                args: { cmd: "python fetch.py" }
              }
            }),
            block({
              id: "live:turn-1:assistant:0:reasoning:tail",
              kind: "reasoning",
              title: "Thinking",
              status: "running",
              body: "Now I am analyzing the latest output.",
              detail: "Now I am analyzing the latest output.",
              order: 3
            })
          ]
        })
      ]
    };
    const incoming = {
      ...snapshot(),
      entries: [
        entry({
          id: "message:6",
          source: "runtime.message",
          messageSeq: 6,
          turnId: "message:6",
          status: "completed",
          blocks: [
            block({
              id: "message:6:block:1",
              kind: "text",
              source: "runtime.message",
              body: "好的，开始执行 X 日报流程！",
              detail: "好的，开始执行 X 日报流程！",
              status: "completed",
              order: 1
            }),
            block({
              id: "message:6:block:2",
              kind: "shell",
              source: "runtime.message",
              title: "exec_command",
              status: "running",
              order: 2,
              metadata: {
                projection: "tool",
                tool_name: "exec_command",
                tool_call_id: "call_fetch",
                args: { cmd: "python fetch.py" }
              }
            })
          ]
        })
      ]
    };

    const next = reconcileThreadSnapshot(current, incoming);

    expect(next.entries.map((candidate) => candidate.id)).toEqual([
      "message:6",
      "live:turn-1:assistant:0"
    ]);
    expect(next.entries[1]?.blocks.map((candidate) => candidate.id)).toEqual([
      "live:turn-1:assistant:0:reasoning:tail"
    ]);
    expect(next.entries[1]?.blocks[0]?.body).toBe("Now I am analyzing the latest output.");
  });

  it("removes stale running agent and wait overlays covered by committed tool ids", () => {
    const current = {
      ...snapshot(),
      entries: [
        entry({
          id: "live:turn-1:assistant:0",
          metadata: { projection: "assistant_segment", liveOrder: 0, streamSeq: 8 },
          blocks: [
            block({
              id: "live:turn-1:tool:call_agent_translate",
              kind: "agent",
              title: "Agent",
              status: "running",
              order: 1,
              metadata: {
                projection: "tool",
                tool_name: "Agent",
                tool_call_id: "call_agent_translate",
                args: {
                  agent_type: "translate",
                  prompt: "Translate the following message to Chinese: hello"
                }
              }
            }),
            block({
              id: "live:turn-1:tool:call_wait_agent",
              kind: "toolCall",
              title: "wait_agent",
              status: "running",
              order: 2,
              metadata: {
                projection: "tool",
                tool_name: "wait_agent",
                tool_call_id: "call_wait_agent"
              }
            })
          ]
        })
      ]
    };
    const incoming = {
      ...snapshot(),
      activity: { running: false, activeTurnId: null, queuedTurns: 0 },
      entries: [
        entry({
          id: "message:6",
          source: "runtime.message",
          messageSeq: 6,
          turnId: "message:6",
          status: "completed",
          blocks: [
            block({
              id: "message:6:block:1",
              kind: "agent",
              source: "runtime.message",
              title: "Agent",
              status: "completed",
              order: 1,
              metadata: {
                projection: "tool",
                tool_name: "Agent",
                tool_call_id: "call_agent_translate",
                args: {
                  agent_type: "translate",
                  prompt: "Translate the following message to Chinese: hello"
                },
                result: {
                  agent_name: "translate",
                  child_session_id: "child-thread",
                  status: "completed",
                  summary: "你好"
                }
              }
            }),
            block({
              id: "message:6:block:2",
              kind: "toolCall",
              source: "runtime.message",
              title: "wait_agent",
              status: "completed",
              order: 2,
              metadata: {
                projection: "tool",
                tool_name: "wait_agent",
                tool_call_id: "call_wait_agent",
                result: {
                  message: "both agents completed",
                  timed_out: false
                }
              }
            })
          ]
        })
      ]
    };

    const next = reconcileThreadSnapshot(current, incoming);

    expect(next.entries.map((candidate) => candidate.id)).toEqual(["message:6"]);
    expect(next.entries[0]?.blocks.map((candidate) => candidate.status)).toEqual([
      "completed",
      "completed"
    ]);
  });

  it("anchors incoming covered live tool updates to the message-derived block", () => {
    const current = {
      ...snapshot(),
      entries: [
        entry({
          id: "message:6",
          source: "runtime.message",
          messageSeq: 6,
          turnId: "message:6",
          status: "completed",
          blocks: [
            block({
              id: "message:6:block:1",
              kind: "text",
              source: "runtime.message",
              body: "好的，开始执行 X 日报流程！",
              detail: "好的，开始执行 X 日报流程！",
              status: "completed",
              order: 1
            }),
            block({
              id: "message:6:block:2",
              kind: "shell",
              source: "runtime.message",
              title: "exec_command python fetch.py",
              status: "pending",
              order: 2,
              metadata: {
                projection: "tool",
                tool_name: "exec_command",
                tool_call_id: "call_fetch",
                args: { cmd: "python fetch.py" }
              }
            })
          ]
        })
      ]
    };

    const next = applyLiveTranscriptEvent(
      current,
      eventWithEntry("entryUpdated", entry({
        id: "live:turn-1:assistant:0",
        metadata: { projection: "assistant_segment", liveOrder: 0, streamSeq: 9 },
        blocks: [
          block({
            id: "live:turn-1:assistant:0:text:1",
            kind: "text",
            body: "好的，开始执行 X 日报流程！",
            detail: "好的，开始执行 X 日报流程！",
            order: 1
          }),
          block({
            id: "live:turn-1:tool:call_fetch",
            kind: "shell",
            title: "exec_command python fetch.py",
            status: "running",
            body: "[x-fetch] running\n",
            detail: "[x-fetch] running\n",
            order: 2,
            metadata: {
              projection: "tool",
              tool_name: "exec_command",
              tool_call_id: "call_fetch",
              args: { cmd: "python fetch.py" }
            }
          })
        ]
      }))
    );

    expect(next.entries.map((candidate) => candidate.id)).toEqual(["message:6"]);
    expect(next.entries[0]?.blocks.map((candidate) => candidate.id)).toEqual([
      "message:6:block:1",
      "message:6:block:2"
    ]);
    expect(next.entries[0]?.blocks[0]?.body).toBe("好的，开始执行 X 日报流程！");
    expect(next.entries[0]?.blocks[1]?.status).toBe("running");
    expect(next.entries[0]?.blocks[1]?.body).toBe("[x-fetch] running\n");
  });
});

function snapshot(): ThreadSnapshot {
  return {
    source: {
      kind: "web",
      rawId: "test",
      lifetime: "persistent",
      rawIdentity: null,
      visibleName: null
    },
    scope: {
      workdir: "/tmp/project",
      source: {
        kind: "web",
        rawId: "test",
        lifetime: "persistent",
        rawIdentity: null,
        visibleName: null
      }
    },
    thread: {
      id: "thread-1",
      backend: {
        kind: "psychevo",
        nativeId: "thread-1"
      },
      sourceKey: "source-1"
    },
    entries: [],
    activity: {
      running: true,
      activeTurnId: "turn-1",
      queuedTurns: 0
    },
    pendingPermissions: [],
    pendingClarifies: []
  };
}

function eventWithEntry(
  type: "entryStarted" | "entryUpdated" | "entryCompleted",
  nextEntry: TranscriptEntry
): GatewayEvent {
  return {
    type,
    turnId: nextEntry.turnId ?? "turn-1",
    entry: nextEntry
  };
}

function completedTurn(id: string, threadId: string | null) {
  return {
    id,
    threadId,
    status: "completed" as const,
    outcome: "normal",
    error: null,
    startedAtMs: 1,
    completedAtMs: 2
  };
}

function entry(overrides: Partial<TranscriptEntry> = {}): TranscriptEntry {
  return {
    id: "live:turn-1:assistant",
    threadId: "thread-1",
    turnId: "turn-1",
    messageSeq: null,
    role: "assistant",
    status: "running",
    source: "runtime.stream",
    blocks: [block()],
    metadata: null,
    usage: null,
    accounting: null,
    createdAtMs: 1,
    updatedAtMs: 1,
    ...overrides
  };
}

function block(overrides: Partial<TranscriptBlock> = {}): TranscriptBlock {
  return {
    id: "live:turn-1:assistant:text",
    kind: "text",
    status: "running",
    order: 0,
    source: "runtime.stream",
    title: null,
    body: null,
    preview: null,
    detail: null,
    artifactIds: [],
    metadata: null,
    result: null,
    createdAtMs: 1,
    updatedAtMs: 1,
    ...overrides
  };
}
