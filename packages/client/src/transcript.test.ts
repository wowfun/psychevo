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

  it("replaces an accepted optimistic prompt when completion arrives without a start event", () => {
    const accepted = {
      ...threadSnapshot(),
      activity: {
        running: false,
        activeTurnId: null,
        queuedTurns: 0
      }
    };
    const optimistic = appendOptimisticPrompt(accepted, "hello", 10);
    const next = applyLiveTranscriptEvent(optimistic, {
      type: "turnCompleted",
      threadId: "thread-1",
      turnId: "turn-detached",
      turn: completedTurn("turn-detached", "thread-1"),
      committedEntries: [
        entry({
          id: "message:1:user",
          threadId: "thread-1",
          turnId: "turn-detached",
          messageSeq: 1,
          role: "user",
          source: "runtime.message",
          blocks: [block({ id: "message:1:user:text", body: "hello" })]
        })
      ]
    });

    expect(next.entries.map((candidate) => candidate.id)).toEqual(["message:1:user"]);
  });

  it("replaces a bound optimistic prompt as soon as its committed user entry arrives", () => {
    const history = {
      ...threadSnapshot(),
      entries: [
        entry({
          id: "message:1",
          threadId: "thread-1",
          turnId: "turn-initial",
          messageSeq: 1,
          role: "user",
          source: "runtime.message",
          status: "completed",
          blocks: [block({
            id: "message:1:text",
            body: "Main Agent instruction",
            detail: "Main Agent instruction",
            source: "runtime.message",
            status: "completed"
          })]
        }),
        entry({
          id: "message:2",
          threadId: "thread-1",
          turnId: "turn-initial",
          messageSeq: 2,
          role: "assistant",
          source: "runtime.message",
          status: "completed",
          blocks: [block({
            id: "message:2:text",
            body: "Initial answer",
            detail: "Initial answer",
            source: "runtime.message",
            status: "completed"
          })]
        })
      ],
      activity: {
        running: false,
        activeTurnId: null,
        queuedTurns: 0
      }
    };
    const optimistic = appendOptimisticPrompt(history, "你有哪些工具", 10);
    const started = applyLiveTranscriptEvent(optimistic, {
      type: "turnStarted",
      threadId: "thread-1",
      turnId: "turn-follow-up",
      selectedSkills: []
    });
    const next = applyLiveTranscriptEvent(started, {
      type: "entryUpdated",
      turnId: "turn-follow-up",
      entry: entry({
        id: "message:3",
        threadId: "thread-1",
        turnId: "turn-follow-up",
        messageSeq: 3,
        role: "user",
        source: "runtime.message",
        status: "completed",
        blocks: [block({
          id: "message:3:text",
          body: "你有哪些工具",
          detail: "你有哪些工具",
          source: "runtime.message",
          status: "completed"
        })]
      })
    });

    expect(next.entries.map((candidate) => candidate.id)).toEqual([
      "message:1",
      "message:2",
      "message:3"
    ]);
    expect(next.entries.filter((candidate) => candidate.role === "user"))
      .toHaveLength(2);
    expect(next.entries[0]?.blocks[0]?.body).toBe("Main Agent instruction");
  });

  it("keeps identical committed user text from separate turns", () => {
    const firstTurn = {
      ...threadSnapshot(),
      entries: [entry({
        id: "message:1",
        threadId: "thread-1",
        turnId: "turn-1",
        messageSeq: 1,
        role: "user",
        source: "runtime.message",
        status: "completed",
        blocks: [block({
          id: "message:1:text",
          body: "repeat this",
          source: "runtime.message",
          status: "completed"
        })]
      })],
      activity: { running: false, activeTurnId: null, queuedTurns: 0 }
    };
    const optimistic = appendOptimisticPrompt(firstTurn, "repeat this", 20);
    const started = applyLiveTranscriptEvent(optimistic, {
      type: "turnStarted",
      threadId: "thread-1",
      turnId: "turn-2",
      selectedSkills: []
    });
    const next = applyLiveTranscriptEvent(started, {
      type: "entryUpdated",
      turnId: "turn-2",
      entry: entry({
        id: "message:2",
        threadId: "thread-1",
        turnId: "turn-2",
        messageSeq: 2,
        role: "user",
        source: "runtime.message",
        status: "completed",
        blocks: [block({
          id: "message:2:text",
          body: "repeat this",
          source: "runtime.message",
          status: "completed"
        })]
      })
    });

    expect(next.entries.map((candidate) => candidate.id)).toEqual(["message:1", "message:2"]);
  });

  it("replaces one matching detached optimistic prompt on failed completion", () => {
    const accepted = {
      ...threadSnapshot(),
      activity: {
        running: false,
        activeTurnId: null,
        queuedTurns: 0
      }
    };
    const once = appendOptimisticPrompt(accepted, "hello", 10);
    const optimistic = appendOptimisticPrompt(once, "hello", 11);
    const next = applyLiveTranscriptEvent(optimistic, {
      type: "turnCompleted",
      threadId: "thread-1",
      turnId: "turn-agent",
      turn: {
        ...completedTurn("turn-agent", "thread-1"),
        status: "failed",
        outcome: "failed",
        error: {
          message: "agent runtime failed",
          code: null,
          stage: null,
          retryClass: null,
          delivery: "unknown",
          recoveryAction: null,
          diagnosticRef: null
        }
      },
      committedEntries: [
        entry({
          id: "message:1:user",
          threadId: "thread-1",
          turnId: "turn-agent",
          messageSeq: 1,
          role: "user",
          source: "runtime.message",
          blocks: [block({ id: "message:1:user:text", body: "hello" })]
        })
      ]
    });

    const users = next.entries.filter((candidate) => candidate.role === "user");
    expect(users).toHaveLength(2);
    expect(users.filter((candidate) => candidate.id === "message:1:user")).toHaveLength(1);
    expect(users.filter((candidate) => candidate.source === "client.optimistic")).toHaveLength(1);
    expect(users.find((candidate) => candidate.source === "client.optimistic")?.createdAtMs).toBe(10);
    expect(next.entries.some((candidate) => candidate.id === "turn:turn-agent:terminal")).toBe(true);
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
        error: {
          message: "model service failed",
          code: null,
          stage: null,
          retryClass: null,
          delivery: "unknown",
          recoveryAction: null,
          diagnosticRef: null
        },
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
          kind: "native" as const,
          sessionHandle: "child-thread"
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
      type: "actionRequested",
      action: {
        actionId: "permission-draft",
        kind: "permission",
        title: "exec_command",
        summary: "inline Python could not be reduced",
        payload: {
          toolName: "exec_command",
          summary: "inline Python could not be reduced",
          reason: "requires approval",
          matchedRule: "exec:python3 -c",
          suggestedRule: null,
          allowAlways: true,
          timeoutSecs: 300
        },
        turnId: "turn-draft",
        activityId: "activity-draft",
        sourceKey: "web:draft"
      }
    });

    expect(requested.pendingActions).toEqual([
      {
        actionId: "permission-draft",
        kind: "permission",
        title: "exec_command",
        summary: "inline Python could not be reduced",
        payload: {
          toolName: "exec_command",
          summary: "inline Python could not be reduced",
          reason: "requires approval",
          matchedRule: "exec:python3 -c",
          suggestedRule: null,
          allowAlways: true,
          timeoutSecs: 300
        },
        turnId: "turn-draft",
        activityId: "activity-draft",
        sourceKey: "web:draft"
      }
    ]);

    const resolved = applyLiveTranscriptEvent(requested, {
      type: "actionResolved",
      actionId: "permission-draft",
      kind: "permission",
      outcome: "accepted",
      payload: { decision: "allowAlways" }
    });

    expect(resolved.pendingActions).toEqual([]);
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
      type: "actionRequested",
      action: {
        actionId: "clarify-draft",
        kind: "clarify",
        title: "Clarify",
        summary: "Which path?",
        payload: {
          raw: { questions: [{ question: "Which path?", options: [{ label: "A" }, { label: "B" }] }] }
        },
        turnId: "turn-draft",
        activityId: "activity-draft",
        sourceKey: "web:draft"
      }
    });
    const withBoth = applyLiveTranscriptEvent(withClarify, {
      type: "actionRequested",
      action: {
        actionId: "permission-draft",
        kind: "permission",
        title: "exec_command",
        summary: "needs approval",
        payload: {
          toolName: "exec_command",
          summary: "needs approval",
          reason: "requires approval",
          matchedRule: null,
          suggestedRule: null,
          allowAlways: false,
          timeoutSecs: 300
        },
        turnId: "turn-draft",
        activityId: "activity-draft",
        sourceKey: "web:draft"
      }
    });

    expect(withBoth.pendingActions.find((action) => action.kind === "clarify")).toMatchObject({
      actionId: "clarify-draft",
      activityId: "activity-draft",
      sourceKey: "web:draft"
    });
    expect(withBoth.pendingActions.find((action) => action.kind === "permission")?.actionId).toBe("permission-draft");

    const completed = applyLiveTranscriptEvent(withBoth, {
      type: "turnCompleted",
      threadId: "thread-draft",
      turnId: "turn-draft",
      turn: completedTurn("turn-draft", "thread-draft"),
      committedEntries: []
    });

    expect(completed.pendingActions).toEqual([]);
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
          kind: "native" as const,
          sessionHandle: "thread-2",
          runtimeRef: "native"
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

  it("keeps a retained earlier-turn live answer before a later detached optimistic prompt", () => {
    const incoming = {
      ...threadSnapshot(),
      entries: [
        entry({
          id: "message:1",
          threadId: "thread-1",
          turnId: "turn-1",
          messageSeq: 1,
          role: "user",
          source: "runtime.message",
          createdAtMs: 100,
          updatedAtMs: 100,
          blocks: [block({ id: "message:1:text", body: "first question" })]
        }),
        entry({
          id: "live:turn-1:assistant:0",
          threadId: "thread-1",
          turnId: "turn-1",
          messageSeq: null,
          role: "assistant",
          source: "runtime.stream",
          createdAtMs: 200,
          updatedAtMs: 200,
          metadata: { liveOrder: 0, projection: "assistant_segment" },
          blocks: [block({ id: "live:turn-1:assistant:0:text", body: "first answer" })]
        }),
        entry({
          id: "optimistic:300:second",
          threadId: "thread-1",
          turnId: null,
          messageSeq: null,
          role: "user",
          source: "client.optimistic",
          createdAtMs: 300,
          updatedAtMs: 300,
          metadata: { liveOrder: -1, projection: "optimistic_prompt" },
          blocks: [block({ id: "optimistic:300:second:text", body: "second question" })]
        })
      ]
    };

    const next = reconcileThreadSnapshot(threadSnapshot(), incoming);

    expect(next.entries.map((candidate) => candidate.id)).toEqual([
      "message:1",
      "live:turn-1:assistant:0",
      "optimistic:300:second"
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

  it("self-reconciles stale live assistant entries already present in an incoming snapshot", () => {
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
        }),
        entry({
          id: "live:turn-1:assistant:0",
          threadId: "thread-1",
          turnId: "turn-1",
          messageSeq: null,
          source: "runtime.stream",
          status: "completed",
          metadata: { projection: "assistant_segment", liveOrder: 0, streamSeq: 13 },
          blocks: [
            block({
              id: "live:turn-1:assistant:0:text",
              source: "runtime.stream",
              status: "completed",
              body: "stale live final text",
              detail: "stale live final text"
            })
          ]
        })
      ]
    };

    const next = reconcileThreadSnapshot(threadSnapshot(), incoming);

    expect(next.entries.map((candidate) => candidate.id)).toEqual(["message:6"]);
    expect(next.entries[0]?.source).toBe("runtime.message");
  });

  it("self-reconciles stale pending tool overlays already present in an incoming snapshot", () => {
    const incoming = {
      ...threadSnapshot(),
      activity: {
        running: true,
        activeTurnId: "turn-1",
        queuedTurns: 0
      },
      entries: [
        entry({
          id: "message:8",
          threadId: "thread-1",
          turnId: "turn-1",
          messageSeq: 8,
          source: "runtime.message",
          status: "completed",
          blocks: [
            block({
              id: "message:8:tool:0",
              kind: "shell",
              source: "runtime.message",
              status: "completed",
              title: "exec_command python fetch.py",
              body: "done\n",
              detail: "done\n",
              metadata: {
                projection: "tool",
                tool_name: "exec_command",
                tool_call_id: "call_exec",
                args: { cmd: "python fetch.py" },
                result: { exit_code: 0, output: "done\n" }
              }
            })
          ]
        }),
        entry({
          id: "live:turn-1:assistant:0",
          threadId: "thread-1",
          turnId: "turn-1",
          messageSeq: null,
          source: "runtime.stream",
          status: "running",
          blocks: [
            block({
              id: "live:turn-1:tool:call_exec",
              kind: "shell",
              source: "runtime.stream",
              status: "pending",
              title: "exec_command python fetch.py",
              metadata: {
                projection: "tool",
                tool_name: "exec_command",
                tool_call_id: "call_exec",
                args: { cmd: "python fetch.py" }
              }
            })
          ]
        })
      ]
    };

    const next = reconcileThreadSnapshot(threadSnapshot(), incoming);

    expect(next.entries.map((candidate) => candidate.id)).toEqual(["message:8"]);
    expect(next.entries[0]?.blocks[0]?.status).toBe("completed");
  });

  it("replays a normalized ledger when incoming snapshots contain committed rows and stale live overlays", () => {
    const current = {
      ...threadSnapshot(),
      entries: [
        entry({
          id: "live:turn-1:assistant:current",
          threadId: "thread-1",
          turnId: "turn-1",
          messageSeq: null,
          source: "runtime.stream",
          blocks: [
            block({
              id: "live:turn-1:tool:call_exec:current",
              kind: "shell",
              source: "runtime.stream",
              status: "pending",
              title: "exec_command python fetch.py",
              metadata: {
                projection: "tool",
                tool_name: "exec_command",
                tool_call_id: "call_exec",
                args: { cmd: "python fetch.py" }
              }
            })
          ]
        })
      ]
    };
    const incoming = {
      ...threadSnapshot(),
      entries: [
        completedExecEntry(),
        entry({
          id: "live:turn-1:assistant:stale",
          threadId: "thread-1",
          turnId: "turn-1",
          messageSeq: null,
          source: "runtime.stream",
          status: "running",
          blocks: [
            block({
              id: "live:turn-1:tool:call_exec:stale",
              kind: "shell",
              source: "runtime.stream",
              status: "running",
              title: "exec_command python fetch.py",
              metadata: {
                projection: "tool",
                tool_name: "exec_command",
                tool_call_id: "call_exec",
                args: { cmd: "python fetch.py" }
              }
            })
          ]
        })
      ]
    };
    const before = runtimeLedger(current);

    const next = reconcileThreadSnapshot(current, incoming);
    const after = runtimeLedger(next);

    assertRuntimeLedgerIdentity(after, "snapshot stale overlay replay");
    assertRuntimeLedgerMonotonic(before, after, "snapshot stale overlay replay");
    expect(after).toMatchObject([
      {
        entryId: "message:8",
        blockId: "message:8:tool:0",
        source: "runtime.message",
        toolCallId: "call_exec",
        status: "completed",
        hasResult: true,
        activeElapsedOwner: false
      }
    ]);
    expect(after.some((row) => row.source === "runtime.stream" && row.toolCallId === "call_exec")).toBe(false);
  });

  it("drops current live pending overlays covered by a completed tool snapshot", () => {
    const current = {
      ...threadSnapshot(),
      entries: [
        entry({
          id: "live:turn-1:assistant:0",
          threadId: "thread-1",
          turnId: "turn-1",
          source: "runtime.stream",
          blocks: [
            block({
              id: "live:turn-1:tool:call_exec",
              kind: "shell",
              source: "runtime.stream",
              status: "pending",
              title: "exec_command python fetch.py",
              metadata: {
                projection: "tool",
                tool_name: "exec_command",
                tool_call_id: "call_exec",
                args: { cmd: "python fetch.py" }
              }
            })
          ]
        })
      ]
    };
    const incoming = {
      ...threadSnapshot(),
      entries: [completedExecEntry()]
    };
    const before = runtimeLedger(current);

    const next = reconcileThreadSnapshot(current, incoming);
    const after = runtimeLedger(next);

    assertRuntimeLedgerIdentity(after, "completed snapshot covers live pending");
    assertRuntimeLedgerMonotonic(before, after, "completed snapshot covers live pending");
    expect(after.map((row) => `${row.source}:${row.toolCallId}:${row.status}`)).toEqual([
      "runtime.message:call_exec:completed"
    ]);
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
      cwd: "/tmp/project",
      source: {
        kind: "web",
        rawId: "test",
        lifetime: "persistent",
        rawIdentity: null,
        visibleName: null
      }
    },
    thread: null,
    history: {
      owner: "psychevo",
      fidelity: "full",
      cursor: null,
      hint: null
    },
    entries: [],
    activity: {
      running: false,
      activeTurnId: null,
      queuedTurns: 0
    },
    pendingActions: []
  };
}

function threadSnapshot(): ThreadSnapshot {
  return {
    ...detachedSnapshot(),
    thread: {
      id: "thread-1",
      backend: {
        kind: "native",
        sessionHandle: "thread-1",
        runtimeRef: "native"
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

type RuntimeLedgerRow = {
  turnId: string | null;
  entryId: string;
  blockId: string;
  source: string | null;
  toolName: string | null;
  toolCallId: string | null;
  status: TranscriptBlock["status"];
  order: number;
  title: string | null;
  hasResult: boolean;
  activeElapsedOwner: boolean;
};

function runtimeLedger(snapshot: ThreadSnapshot): RuntimeLedgerRow[] {
  return snapshot.entries.flatMap((candidate) => candidate.blocks.map((candidateBlock) => {
    const metadata = record(candidateBlock.metadata);
    const result = metadata.result ?? candidateBlock.result ?? null;
    return {
      turnId: candidate.turnId,
      entryId: candidate.id,
      blockId: candidateBlock.id,
      source: candidateBlock.source || candidate.source,
      toolName: stringValue(metadata.tool_name),
      toolCallId: stringValue(metadata.tool_call_id),
      status: candidateBlock.status,
      order: candidateBlock.order,
      title: candidateBlock.title,
      hasResult: result !== null && result !== undefined,
      activeElapsedOwner: candidateBlock.status === "running" &&
        candidateBlock.kind !== "text" &&
        candidateBlock.kind !== "reasoning" &&
        (candidateBlock.source || candidate.source) === "runtime.stream"
    };
  }));
}

function assertRuntimeLedgerIdentity(rows: RuntimeLedgerRow[], checkpoint: string) {
  const blockIds = new Set<string>();
  const liveToolIds = new Set<string>();
  for (const row of rows) {
    const blockKey = `${row.turnId ?? ""}:${row.blockId}`;
    expect(blockIds.has(blockKey), `${checkpoint}: duplicate block identity ${blockKey}`).toBe(false);
    blockIds.add(blockKey);
    if (!row.toolName || !row.toolCallId) {
      continue;
    }
    expect(row.toolCallId, `${checkpoint}: tool_call_id fell back to bare tool name`).not.toBe(row.toolName);
    if (row.source === "runtime.stream") {
      const toolKey = `${row.turnId ?? ""}:${row.toolName}:${row.toolCallId}`;
      expect(liveToolIds.has(toolKey), `${checkpoint}: duplicate live tool identity ${toolKey}`).toBe(false);
      liveToolIds.add(toolKey);
    }
  }
}

function assertRuntimeLedgerMonotonic(
  before: RuntimeLedgerRow[],
  after: RuntimeLedgerRow[],
  checkpoint: string
) {
  for (const row of after) {
    if (!row.toolCallId) {
      continue;
    }
    const prior = before.find((candidate) => candidate.toolCallId === row.toolCallId);
    if (!prior) {
      continue;
    }
    expect(
      statusRank(row.status),
      `${checkpoint}: status downgraded from ${prior.status} to ${row.status}`
    ).toBeGreaterThanOrEqual(statusRank(prior.status));
    if (prior.hasResult) {
      expect(row.hasResult, `${checkpoint}: result fact disappeared`).toBe(true);
    }
    if (statusRank(row.status) >= statusRank("completed")) {
      expect(row.activeElapsedOwner, `${checkpoint}: terminal row kept active elapsed ownership`).toBe(false);
    }
  }
}

function statusRank(status: TranscriptBlock["status"]): number {
  switch (status) {
    case "pending":
      return 0;
    case "running":
    case "needsInput":
      return 1;
    case "completed":
    case "failed":
    case "cancelled":
      return 2;
    case "info":
      return 3;
  }
}

function completedExecEntry(): TranscriptEntry {
  return entry({
    id: "message:8",
    threadId: "thread-1",
    turnId: "turn-1",
    messageSeq: 8,
    source: "runtime.message",
    status: "completed",
    blocks: [
      block({
        id: "message:8:tool:0",
        kind: "shell",
        source: "runtime.message",
        status: "completed",
        title: "exec_command python fetch.py",
        body: "done\n",
        detail: "done\n",
        metadata: {
          projection: "tool",
          tool_name: "exec_command",
          tool_call_id: "call_exec",
          args: { cmd: "python fetch.py" },
          result: { exit_code: 0, output: "done\n" }
        }
      })
    ]
  });
}

function record(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function stringValue(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
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
