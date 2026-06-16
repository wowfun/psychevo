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
                tool_name: "Agent",
                tool_call_id: "call-agent",
                result: {
                  agent_name: "translate",
                  child_session_id: "child-thread",
                  parent_session_id: "thread-1",
                  task_name: "zh-to-en"
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
                tool_name: "Agent",
                tool_call_id: "call-agent",
                args: {
                  agent_type: "translate",
                  task_name: "zh-to-en"
                },
                result: {
                  agent_name: "translate",
                  task_name: "zh-to-en"
                }
              },
              result: {
                resultMessageSeq: 3,
                status: "completed",
                content: "{\"agent_name\":\"translate\",\"task_name\":\"zh-to-en\"}",
                isError: false,
                metadata: {
                  result: {
                    agent_name: "translate",
                    task_name: "zh-to-en"
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
    expect((agentBlock?.metadata as Record<string, unknown>)?.["child_session_id"]).toBe("child-thread");
    expect(((agentBlock?.metadata as Record<string, unknown>)?.["result"] as Record<string, unknown>)?.["child_session_id"]).toBe("child-thread");
    expect(((agentBlock?.result?.metadata as Record<string, unknown>)?.["result"] as Record<string, unknown>)?.["child_session_id"]).toBe("child-thread");
  });
});

describe("reconcileThreadSnapshot", () => {
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
