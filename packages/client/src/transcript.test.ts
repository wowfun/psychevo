import { describe, expect, it } from "vitest";
import type { ThreadSnapshot, TranscriptBlock, TranscriptEntry } from "@psychevo/protocol";
import { appendOptimisticPrompt, applyLiveTranscriptEvent } from "./transcript";

describe("applyLiveTranscriptEvent detached drafts", () => {
  it("ignores a stale completed turn for an empty detached draft", () => {
    const current = detachedSnapshot();
    const next = applyLiveTranscriptEvent(current, {
      type: "turnCompleted",
      threadId: "thread-old",
      turnId: "turn-old",
      outcome: "normal",
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
      outcome: "normal",
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
