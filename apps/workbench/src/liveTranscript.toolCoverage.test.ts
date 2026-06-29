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

  it("drops active live entries after snapshot reconciliation when message-derived owner matches", () => {
    const current = {
      ...snapshot(),
      entries: [
        entry({
          id: "live:turn-1:reasoning",
          messageSeq: null,
          source: "runtime.stream",
          metadata: { liveOrder: 0 },
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
          turnId: "turn-1",
          metadata: { liveOrder: 0 },
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

  it("removes covered tool blocks without text coverage when owner identity is missing", () => {
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
      "live:turn-1:assistant:0:text:1",
      "live:turn-1:assistant:0:reasoning:tail"
    ]);
    expect(next.entries[1]?.blocks[0]?.body).toBe("好的，开始执行 X 日报流程！");
    expect(next.entries[1]?.blocks[1]?.body).toBe("Now I am analyzing the latest output.");
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
                tool_name: "spawn_agent",
                tool_call_id: "call_agent_translate",
                args: {
                  agent_type: "translate",
                  task_name: "translate_to_chinese",
                  message: "Translate the following message to Chinese: hello"
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
          turnId: "turn-1",
          metadata: { liveOrder: 0 },
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
                tool_name: "spawn_agent",
                tool_call_id: "call_agent_translate",
                args: {
                  agent_type: "translate",
                  task_name: "translate_to_chinese",
                  message: "Translate the following message to Chinese: hello"
                },
                result: {
                  agent_name: "translate",
                  child_thread_id: "child-thread",
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
          turnId: "turn-1",
          metadata: { liveOrder: 0 },
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
