import { describe, expect, it } from "vitest";
import type { ThreadSnapshot, TranscriptBlock, TranscriptEntry } from "@psychevo/protocol";
import { appendOptimisticPrompt, applyLiveTranscriptEvent, reconcileThreadSnapshot } from "./transcript";

describe("applyLiveTranscriptEvent detached drafts", () => {
  it("ignores a stale completed turn for an empty detached draft", () => {
    const current = detachedSnapshot();
    const next = applyLiveTranscriptEvent(current, {
      type: "turnCompleted",
      threadId: "thread-old",
      turnId: "turn-old",
      turn: completedTurn("turn-old", "thread-old"),
      committedEntries: [
        entry({
          id: "message:1:user",
          threadId: "thread-old",
          turnId: "turn-old",
          messageSeq: 1,
          role: "user",
          source: "runtime.message",
          blocks: [block({ id: "message:1:user:text", body: "old prompt" })]
        })
      ]
    });

    expect(next).toBe(current);
    expect(next.thread).toBeNull();
    expect(next.entries).toEqual([]);
  });

  it("ignores a stale threaded entry for an empty detached draft", () => {
    const current = detachedSnapshot();
    const next = applyLiveTranscriptEvent(current, {
      type: "entryUpdated",
      turnId: "turn-old",
      entry: entry({ threadId: "thread-old", turnId: "turn-old" })
    });

    expect(next).toBe(current);
    expect(next.thread).toBeNull();
    expect(next.entries).toEqual([]);
  });

  it("still binds an optimistic first prompt to its resolved thread", () => {
    const optimistic = appendOptimisticPrompt(detachedSnapshot(), "hello", 10);
    const started = applyLiveTranscriptEvent(optimistic, {
      type: "turnStarted",
      threadId: null,
      turnId: "turn-new",
      selectedSkills: []
    });
    const next = applyLiveTranscriptEvent(started, {
      type: "turnCompleted",
      threadId: "thread-new",
      turnId: "turn-new",
      turn: completedTurn("turn-new", "thread-new"),
      committedEntries: [
        entry({
          id: "message:1:user",
          threadId: "thread-new",
          turnId: "turn-new",
          messageSeq: 1,
          role: "user",
          source: "runtime.message",
          blocks: [block({ id: "message:1:user:text", body: "hello" })]
        })
      ]
    });

    expect(next.thread?.id).toBe("thread-new");
    expect(next.entries.map((candidate) => candidate.id)).toEqual(["message:1:user"]);
  });

  it("settles a failed terminal turn and keeps the diagnostic visible", () => {
    const current = {
      ...threadSnapshot(),
      entries: [
        entry({
          id: "live:turn-1:tool",
          blocks: [block({
            id: "live:turn-1:tool:block",
            kind: "shell",
            status: "running",
            title: "exec_command"
          })]
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
        error: { message: "model service failed" },
        completedAtMs: 20
      },
      committedEntries: []
    });

    expect(next.activity.running).toBe(false);
    expect(next.entries.map((candidate) => candidate.id)).toEqual([
      "live:turn-1:tool",
      "turn:turn-1:terminal"
    ]);
    expect(next.entries[0]?.status).toBe("failed");
    expect(next.entries[0]?.blocks[0]?.status).toBe("failed");
    expect(next.entries[1]?.blocks[0]?.body).toBe("model service failed");
  });

  it("ignores late live entries after a turn has committed durable messages", () => {
    const committed = applyLiveTranscriptEvent(threadSnapshot(), {
      type: "turnCompleted",
      threadId: "thread-1",
      turnId: "turn-1",
      turn: completedTurn("turn-1", "thread-1"),
      committedEntries: [
        entry({
          id: "message:2",
          threadId: "thread-1",
          turnId: "turn-1",
          messageSeq: 2,
          role: "assistant",
          status: "completed",
          source: "runtime.message",
          blocks: [
            block({
              id: "message:2:text",
              source: "runtime.message",
              status: "completed",
              body: "committed answer"
            })
          ]
        })
      ]
    });

    const next = applyLiveTranscriptEvent(committed, {
      type: "entryUpdated",
      turnId: "turn-1",
      entry: entry({
        id: "live:turn-1:assistant:stale",
        threadId: "thread-1",
        turnId: "turn-1",
        blocks: [
          block({
            id: "live:turn-1:assistant:stale:text",
            body: "stale streamed answer"
          })
        ]
      })
    });

    expect(committed.activity.activeTurnId).toBeNull();
    expect(next).toBe(committed);
    expect(next.entries.map((candidate) => candidate.id)).toEqual(["message:2"]);
    expect(next.entries[0]?.blocks[0]?.body).toBe("committed answer");
  });

  it("routes child-thread live entries away from the parent snapshot", () => {
    const childEvent = {
      type: "entryUpdated" as const,
      turnId: "turn-child",
      entry: entry({
        id: "live:turn-child:assistant:0",
        threadId: "child-thread",
        turnId: "turn-child",
        blocks: [
          block({
            id: "live:turn-child:assistant:0:reasoning",
            kind: "reasoning",
            title: "Thinking",
            body: "child work"
          })
        ]
      })
    };

    const parent = threadSnapshot();
    expect(applyLiveTranscriptEvent(parent, childEvent)).toBe(parent);

    const child = {
      ...threadSnapshot(),
      thread: {
        id: "child-thread",
        backend: {
          kind: "psychevo" as const,
          nativeId: "child-thread"
        },
        sourceKey: "web:test"
      },
      activity: {
        running: true,
        activeTurnId: "turn-child",
        queuedTurns: 0
      }
    };
    const next = applyLiveTranscriptEvent(child, childEvent);

    expect(next.entries.map((candidate) => candidate.id)).toEqual([
      "live:turn-child:assistant:0"
    ]);
    expect(next.entries[0]?.threadId).toBe("child-thread");
    expect(next.entries[0]?.blocks[0]?.body).toBe("child work");
  });

  it("keeps a live Agent child target when committed entries replace the overlay", () => {
    const current = {
      ...threadSnapshot(),
      entries: [
        entry({
          id: "live:turn-1:agent",
          source: "runtime.stream",
          messageSeq: null,
          blocks: [
            block({
              id: "live:turn-1:agent:block",
              kind: "agent",
              title: "Agent",
              status: "running",
              metadata: {
                tool_name: "spawn_agent",
                tool_call_id: "call-agent",
                result: {
                  agent_name: "translate",
                  child_thread_id: "child-thread",
                  parent_thread_id: "thread-1",
                  task_name: "zh_to_en"
                }
              }
            })
          ]
        })
      ]
    };

    const next = applyLiveTranscriptEvent(current, {
      type: "turnCompleted",
      threadId: "thread-1",
      turnId: "turn-1",
      turn: completedTurn("turn-1", "thread-1"),
      committedEntries: [
        entry({
          id: "message:2",
          source: "runtime.message",
          messageSeq: 2,
          status: "completed",
          blocks: [
            block({
              id: "message:2:agent",
              kind: "agent",
              source: "runtime.message",
              status: "completed",
              title: "Agent",
              metadata: {
                tool_name: "spawn_agent",
                tool_call_id: "call-agent",
                args: {
                  agent_type: "translate",
                  task_name: "zh_to_en",
                  message: "Translate the greeting to English."
                },
                result: {
                  agent_name: "translate",
                  task_name: "zh_to_en"
                }
              },
              result: {
                resultMessageSeq: 3,
                status: "completed",
                content: "{\"agent_name\":\"translate\",\"task_name\":\"zh_to_en\"}",
                isError: false,
                metadata: {
                  result: {
                    agent_name: "translate",
                    task_name: "zh_to_en"
                  }
                },
                createdAtMs: 3,
                updatedAtMs: 3
              }
            })
          ]
        })
      ]
    });

    expect(next.entries.map((candidate) => candidate.id)).toEqual(["message:2"]);
    const agentBlock = next.entries[0]?.blocks[0];
    expect((agentBlock?.metadata as Record<string, unknown>)?.["child_thread_id"]).toBe("child-thread");
    expect(((agentBlock?.metadata as Record<string, unknown>)?.["result"] as Record<string, unknown>)?.["child_thread_id"]).toBe("child-thread");
    expect(((agentBlock?.result?.metadata as Record<string, unknown>)?.["result"] as Record<string, unknown>)?.["child_thread_id"]).toBe("child-thread");
  });

  it("keeps distinct live assistant owners even when text overlaps", () => {
    const current = {
      ...threadSnapshot(),
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

    const next = applyLiveTranscriptEvent(current, {
      type: "entryCompleted",
      turnId: "turn-1",
      entry: entry({
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
      })
    });

    expect(next.entries.map((candidate) => candidate.id)).toEqual([
      "live:turn-1:assistant:0",
      "live:turn-1:assistant:1"
    ]);
    expect(next.entries[1]?.blocks[0]?.body).toBe("已创建成功。自动化标题：pevo-live-engineering-tip");
  });

  it("drops late live assistant text owned by an active committed message snapshot", () => {
    const current = {
      ...threadSnapshot(),
      entries: [
        entry({
          id: "message:4",
          threadId: "thread-1",
          turnId: "turn-1",
          messageSeq: 4,
          source: "runtime.message",
          status: "completed",
          metadata: { liveOrder: 1 },
          blocks: [
            block({
              id: "message:4:block:1",
              source: "runtime.message",
              status: "completed",
              body: "已创建成功。自动化标题：**pevo-live-engineering-tip**",
              detail: "已创建成功。自动化标题：**pevo-live-engineering-tip**"
            })
          ]
        })
      ]
    };

    const next = applyLiveTranscriptEvent(current, {
      type: "entryCompleted",
      turnId: "turn-1",
      entry: entry({
        id: "live:turn-1:assistant:1",
        metadata: { projection: "assistant_segment", liveOrder: 1, streamSeq: 2 },
        status: "completed",
        blocks: [
          block({
            id: "live:turn-1:assistant:1:text:0",
            status: "completed",
            body: "已创建成功。自动化标题：pevo-live-engineering-tip",
            detail: "已创建成功。自动化标题：pevo-live-engineering-tip"
          })
        ]
      })
    });

    expect(next.entries.map((candidate) => candidate.id)).toEqual(["message:4"]);
  });

  it("preserves a live Agent child target when a later live frame omits it", () => {
    const current = {
      ...threadSnapshot(),
      entries: [
        entry({
          id: "live:turn-1:assistant:0",
          source: "runtime.stream",
          messageSeq: null,
          blocks: [
            block({
              id: "live:turn-1:tool:call-agent",
              kind: "agent",
              title: "Agent",
              status: "running",
              metadata: {
                projection: "tool",
                tool_name: "spawn_agent",
                tool_call_id: "call-agent",
                result: {
                  agent_name: "translate",
                  task_name: "zh_to_en",
                  child_thread_id: "child-thread"
                }
              }
            })
          ]
        })
      ]
    };

    const next = applyLiveTranscriptEvent(current, {
      type: "entryUpdated",
      turnId: "turn-1",
      entry: entry({
        id: "live:turn-1:assistant:0",
        source: "runtime.stream",
        messageSeq: null,
        blocks: [
          block({
            id: "live:turn-1:tool:call-agent",
            kind: "agent",
            title: "Agent",
            status: "pending",
            metadata: {
              projection: "tool",
              tool_name: "spawn_agent",
              tool_call_id: "call-agent",
              args: {
                agent_type: "translate",
                task_name: "zh_to_en",
                message: "Translate the greeting to English."
              }
            }
          })
        ]
      })
    });

    const agentBlock = next.entries[0]?.blocks[0];
    expect(((agentBlock?.metadata as Record<string, unknown>)?.["result"] as Record<string, unknown>)?.["child_thread_id"]).toBe("child-thread");
  });

  it("upserts and resolves pending permission requests from live events", () => {
    const requested = applyLiveTranscriptEvent(detachedSnapshot(), {
      type: "permissionRequested",
      requestId: "permission-draft",
      toolName: "exec_command",
      summary: "inline Python could not be reduced",
      reason: "requires approval",
      matchedRule: "exec:python3 -c",
      suggestedRule: null,
      allowAlways: true,
      timeoutSecs: 300,
      turnId: "turn-draft",
      activityId: "activity-draft",
      sourceKey: "web:draft"
    });

    expect(requested.pendingPermissions).toEqual([
      {
        requestId: "permission-draft",
        toolName: "exec_command",
        summary: "inline Python could not be reduced",
        reason: "requires approval",
        matchedRule: "exec:python3 -c",
        allowAlways: true,
        timeoutSecs: 300,
        turnId: "turn-draft",
        activityId: "activity-draft",
        sourceKey: "web:draft"
      }
    ]);

    const resolved = applyLiveTranscriptEvent(requested, {
      type: "permissionResolved",
      requestId: "permission-draft",
      decision: "allowAlways"
    });

    expect(resolved.pendingPermissions).toEqual([]);
  });

  it("upserts pending clarify requests and clears same-turn pending requests on completion", () => {
    const draftRunning = {
      ...detachedSnapshot(),
      activity: {
        running: true,
        activeTurnId: "turn-draft",
        queuedTurns: 0
      }
    };
    const withClarify = applyLiveTranscriptEvent(draftRunning, {
      type: "clarifyRequested",
      requestId: "clarify-draft",
      raw: { questions: [{ question: "Which path?", options: [{ label: "A" }, { label: "B" }] }] },
      turnId: "turn-draft",
      activityId: "activity-draft",
      sourceKey: "web:draft"
    });
    const withBoth = applyLiveTranscriptEvent(withClarify, {
      type: "permissionRequested",
      requestId: "permission-draft",
      toolName: "exec_command",
      summary: "needs approval",
      reason: "requires approval",
      matchedRule: null,
      suggestedRule: null,
      allowAlways: false,
      timeoutSecs: 300,
      turnId: "turn-draft",
      activityId: "activity-draft",
      sourceKey: "web:draft"
    });

    expect(withBoth.pendingClarifies[0]).toMatchObject({
      requestId: "clarify-draft",
      activityId: "activity-draft",
      sourceKey: "web:draft"
    });
    expect(withBoth.pendingPermissions[0]?.requestId).toBe("permission-draft");

    const completed = applyLiveTranscriptEvent(withBoth, {
      type: "turnCompleted",
      threadId: "thread-draft",
      turnId: "turn-draft",
      turn: completedTurn("turn-draft", "thread-draft"),
      committedEntries: []
    });

    expect(completed.pendingClarifies).toEqual([]);
    expect(completed.pendingPermissions).toEqual([]);
  });

  it("does not treat spawn_agent args as a live overlay identity", () => {
    const current = {
      ...threadSnapshot(),
      entries: [
        entry({
          id: "live:turn-1:assistant:0",
          source: "runtime.stream",
          messageSeq: null,
          blocks: [
            block({
              id: "live:turn-1:tool:spawn_agent@0:0:0",
              kind: "agent",
              title: "Agent",
              status: "pending",
              metadata: {
                projection: "tool",
                tool_name: "spawn_agent",
                args: {
                  agent_type: "translate",
                  task_name: "zh_to_en",
                  message: "Translate the greeting to English."
                }
              }
            })
          ]
        })
      ]
    };
    const incoming = {
      ...threadSnapshot(),
      entries: [
        entry({
          id: "message:2",
          source: "runtime.message",
          messageSeq: 2,
          status: "completed",
          blocks: [
            block({
              id: "message:2:agent",
              kind: "agent",
              source: "runtime.message",
              status: "completed",
              title: "Agent",
              metadata: {
                projection: "tool",
                tool_name: "spawn_agent",
                args: {
                  agent_type: "translate",
                  task_name: "zh_to_en",
                  message: "Translate the greeting to English."
                }
              }
            })
          ]
        })
      ]
    };

    const next = reconcileThreadSnapshot(current, incoming);

    expect(next.entries.map((candidate) => candidate.id)).toEqual([
      "message:2",
      "live:turn-1:assistant:0"
    ]);
  });
});

describe("reconcileThreadSnapshot", () => {
  it("adopts a switched-back running snapshot with live tool output and start time", () => {
    const current = {
      ...threadSnapshot(),
      thread: {
        id: "thread-2",
        backend: {
          kind: "psychevo" as const,
          nativeId: "thread-2"
        },
        sourceKey: "web:test"
      },
      entries: []
    };
    const incoming = {
      ...threadSnapshot(),
      activity: {
        running: true,
        activeTurnId: "turn-1",
        queuedTurns: 0,
        startedAtMs: 1_000
      },
      entries: [
        entry({
          id: "message:2",
          threadId: "thread-1",
          turnId: "message:2",
          messageSeq: 2,
          source: "runtime.message",
          status: "running",
          blocks: [
            block({
              id: "message:2:tool",
              kind: "shell",
              title: "exec_command python fetch.py",
              status: "running",
              body: "{\"session_id\":7,\"exit_code\":null,\"output\":\"first\\nsecond\\npoll\\n\"}",
              detail: "{\"session_id\":7,\"exit_code\":null,\"output\":\"first\\nsecond\\npoll\\n\"}",
              metadata: {
                projection: "tool",
                tool_name: "exec_command",
                tool_call_id: "call_exec",
                result: {
                  session_id: 7,
                  exit_code: null,
                  output: "first\nsecond\npoll\n"
                }
              }
            })
          ]
        })
      ]
    };

    const next = reconcileThreadSnapshot(current, incoming);

    expect(next.activity.startedAtMs).toBe(1_000);
    expect(next.entries).toHaveLength(1);
    expect(next.entries[0]?.blocks[0]?.status).toBe("running");
    expect(next.entries[0]?.blocks[0]?.metadata).toMatchObject({
      result: { output: "first\nsecond\npoll\n" }
    });
  });

  it("keeps older live overlays before newer durable messages by timeline", () => {
    const current = {
      ...threadSnapshot(),
      entries: [
        entry({
          id: "live:turn-1:assistant:13",
          messageSeq: null,
          createdAtMs: 1300,
          updatedAtMs: 1300,
          metadata: { projection: "assistant_segment", liveOrder: 0, streamSeq: 13 },
          blocks: [
            block({
              id: "live:turn-1:assistant:13:reasoning",
              kind: "reasoning",
              body: "Now let me write the report file.",
              createdAtMs: 1300,
              updatedAtMs: 1300
            })
          ]
        })
      ]
    };
    const incoming = {
      ...threadSnapshot(),
      entries: [
        messageEntry(2, "The user wants me to run the x-daily skill.", 200),
        messageEntry(4, "The command is still running.", 400),
        messageEntry(15, "Now I have all the data I need.", 1500)
      ]
    };

    const next = reconcileThreadSnapshot(current, incoming);

    expect(next.entries.map((candidate) => candidate.id)).toEqual([
      "message:2",
      "message:4",
      "live:turn-1:assistant:13",
      "message:15"
    ]);
  });

  it("does not keep live final text when an incoming snapshot marks the turn inactive", () => {
    const current = {
      ...threadSnapshot(),
      entries: [
        entry({
          id: "live:turn-1:assistant:final",
          source: "runtime.stream",
          messageSeq: null,
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
      ...threadSnapshot(),
      activity: {
        running: false,
        activeTurnId: null,
        queuedTurns: 0
      },
      entries: [
        entry({
          id: "message:6",
          threadId: "thread-1",
          turnId: "message:6",
          messageSeq: 6,
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

    const next = reconcileThreadSnapshot(current, incoming);

    expect(next.activity.activeTurnId).toBeNull();
    expect(next.entries.map((candidate) => candidate.id)).toEqual(["message:6"]);
    expect(next.entries[0]?.source).toBe("runtime.message");
  });

  it("drops live assistant text when an active snapshot has the same committed owner identity", () => {
    const current = {
      ...threadSnapshot(),
      entries: [
        entry({
          id: "live:turn-1:assistant:0",
          source: "runtime.stream",
          messageSeq: null,
          status: "completed",
          metadata: { projection: "assistant_segment", liveOrder: 0, streamSeq: 1 },
          blocks: [
            block({
              id: "live:turn-1:assistant:0:text",
              source: "runtime.stream",
              status: "completed",
              body: "stale live text that does not overlap",
              detail: "stale live text that does not overlap"
            })
          ]
        })
      ]
    };
    const incoming = {
      ...threadSnapshot(),
      activity: {
        running: true,
        activeTurnId: "turn-1",
        queuedTurns: 0
      },
      entries: [
        entry({
          id: "message:6",
          threadId: "thread-1",
          turnId: "turn-1",
          messageSeq: 6,
          source: "runtime.message",
          status: "completed",
          metadata: { liveOrder: 0 },
          blocks: [
            block({
              id: "message:6:block:0",
              source: "runtime.message",
              status: "completed",
              body: "committed final answer",
              detail: "committed final answer"
            })
          ]
        })
      ]
    };

    const next = reconcileThreadSnapshot(current, incoming);

    expect(next.entries.map((candidate) => candidate.id)).toEqual(["message:6"]);
    expect(next.entries[0]?.source).toBe("runtime.message");
  });

  it("drops side-inherited parent context from thread snapshots and live entries", () => {
    const incoming = {
      ...threadSnapshot(),
      entries: [
        entry({
          id: "message:1",
          threadId: "thread-1",
          turnId: "message:1",
          messageSeq: 1,
          role: "user",
          status: "completed",
          source: "runtime.message",
          metadata: { side_inherited: { hidden: true, parent_session_id: "parent-thread" } },
          blocks: [block({ id: "message:1:text", status: "completed", body: "parent history" })]
        }),
        entry({
          id: "message:2",
          threadId: "thread-1",
          turnId: "message:2",
          messageSeq: 2,
          role: "user",
          status: "completed",
          source: "runtime.message",
          blocks: [block({ id: "message:2:text", status: "completed", body: "side prompt" })]
        })
      ]
    };

    const reconciled = reconcileThreadSnapshot(threadSnapshot(), incoming);
    const next = applyLiveTranscriptEvent(reconciled, {
      type: "entryUpdated",
      turnId: "turn-1",
      entry: entry({
        id: "live:parent-context",
        threadId: "thread-1",
        turnId: "turn-1",
        metadata: { side_inherited: { hidden: true, parent_session_id: "parent-thread" } },
        blocks: [block({ id: "live:parent-context:text", body: "late parent history" })]
      })
    });

    expect(reconciled.entries.map((candidate) => candidate.id)).toEqual(["message:2"]);
    expect(next.entries.map((candidate) => candidate.id)).toEqual(["message:2"]);
  });
});

function detachedSnapshot(): ThreadSnapshot {
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
    thread: null,
    entries: [],
    activity: {
      running: false,
      activeTurnId: null,
      queuedTurns: 0
    },
    pendingPermissions: [],
    pendingClarifies: []
  };
}

function threadSnapshot(): ThreadSnapshot {
  return {
    ...detachedSnapshot(),
    thread: {
      id: "thread-1",
      backend: {
        kind: "psychevo",
        nativeId: "thread-1"
      },
      sourceKey: "web:test"
    },
    activity: {
      running: true,
      activeTurnId: "turn-1",
      queuedTurns: 0
    }
  };
}

function messageEntry(messageSeq: number, text: string, createdAtMs: number): TranscriptEntry {
  return entry({
    id: `message:${messageSeq}`,
    threadId: "thread-1",
    turnId: `message:${messageSeq}`,
    messageSeq,
    source: "runtime.message",
    status: "completed",
    createdAtMs,
    updatedAtMs: createdAtMs,
    blocks: [
      block({
        id: `message:${messageSeq}:reasoning:0`,
        kind: "reasoning",
        source: "runtime.message",
        status: "completed",
        body: text,
        createdAtMs,
        updatedAtMs: createdAtMs
      })
    ]
  });
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
    threadId: "",
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
