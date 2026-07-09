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

  it("does not preserve live final text when snapshot refresh observes an inactive turn", () => {
    const current = {
      ...snapshot(),
      entries: [
        entry({
          id: "live:turn-1:assistant:final",
          messageSeq: null,
          source: "runtime.stream",
          status: "completed",
          blocks: [
            block({
              id: "live:turn-1:assistant:final:text",
              source: "runtime.stream",
              status: "completed",
              body: "✅ 已创建每 5 分钟 一次的喝水提醒！💧\n\n到时候会自动提醒你：\"💧 该喝水啦！\"\n\n想暂停或取消提醒随时告诉我。",
              detail: "✅ 已创建每 5 分钟 一次的喝水提醒！💧\n\n到时候会自动提醒你：\"💧 该喝水啦！\"\n\n想暂停或取消提醒随时告诉我。"
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
          messageSeq: 6,
          turnId: "message:6",
          source: "runtime.message",
          status: "completed",
          blocks: [
            block({
              id: "message:6:reasoning:0",
              kind: "reasoning",
              source: "runtime.message",
              status: "completed",
              body: "Done.",
              detail: "Done.",
              order: 0
            }),
            block({
              id: "message:6:block:1",
              source: "runtime.message",
              status: "completed",
              body: "✅ 已创建每 **5 分钟** 一次的喝水提醒！💧\n\n到时候会自动提醒你：**\"💧 该喝水啦！\"**\n\n想暂停或取消提醒随时告诉我。",
              detail: "✅ 已创建每 **5 分钟** 一次的喝水提醒！💧\n\n到时候会自动提醒你：**\"💧 该喝水啦！\"**\n\n想暂停或取消提醒随时告诉我。",
              order: 1
            })
          ]
        })
      ]
    };

    const reconciled = reconcileThreadSnapshot(current, incoming);

    expect(reconciled.activity.activeTurnId).toBeNull();
    expect(reconciled.entries.map((candidate) => candidate.id)).toEqual(["message:6"]);
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

  it("ignores older same-entry live updates after a newer snapshot", () => {
    const current = applyLiveTranscriptEvent(
      snapshot(),
      eventWithEntry("entryUpdated", entry({
        id: "live:turn-1:assistant:0",
        metadata: { projection: "assistant_segment", liveOrder: 0, streamSeq: 2, authoritativeBlocks: true },
        blocks: [
          block({
            id: "live:turn-1:assistant:0:text:0",
            kind: "text",
            body: "current answer",
            detail: "current answer",
            order: 0
          }),
          block({
            id: "live:turn-1:tool:call_wait_agent",
            kind: "toolCall",
            title: "wait_agent",
            status: "pending",
            order: 1,
            metadata: {
              projection: "tool",
              tool_name: "wait_agent",
              tool_call_id: "call_wait_agent"
            }
          })
        ]
      }))
    );

    const next = applyLiveTranscriptEvent(
      current,
      eventWithEntry("entryUpdated", entry({
        id: "live:turn-1:assistant:0",
        metadata: { projection: "assistant_segment", liveOrder: 0, streamSeq: 1 },
        blocks: [
          block({
            id: "live:turn-1:assistant:0:text:0",
            kind: "text",
            body: "stale answer",
            detail: "stale answer",
            order: 0
          })
        ]
      }))
    );

    expect(next.entries).toHaveLength(1);
    expect(next.entries[0]?.metadata).toMatchObject({ streamSeq: 2 });
    expect(next.entries[0]?.blocks.map((candidate) => candidate.id)).toEqual([
      "live:turn-1:assistant:0:text:0",
      "live:turn-1:tool:call_wait_agent"
    ]);
    expect(next.entries[0]?.blocks[0]?.body).toBe("current answer");
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
      backend: { kind: "psychevo", nativeId: "thread-2", runtimeRef: "native" },
      sourceKey: "web:test"
    });
    expect(started.activity.activeTurnId).toBe("turn-2");
  });

  it("does not keep active live overlay owned by same message-derived snapshot segment", () => {
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
          turnId: "turn-1",
          metadata: { liveOrder: 0 },
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
});
