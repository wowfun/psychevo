import type { GatewayEvent, ThreadSnapshot, TranscriptBlock, TranscriptEntry } from "@psychevo/protocol";

export function snapshot(): ThreadSnapshot {
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
    pendingActions: []
  };
}

export function eventWithEntry(
  type: "entryStarted" | "entryUpdated" | "entryCompleted",
  nextEntry: TranscriptEntry
): GatewayEvent {
  return {
    type,
    turnId: nextEntry.turnId ?? "turn-1",
    entry: nextEntry
  };
}

export function completedTurn(id: string, threadId: string | null) {
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

export function entry(overrides: Partial<TranscriptEntry> = {}): TranscriptEntry {
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

export function block(overrides: Partial<TranscriptBlock> = {}): TranscriptBlock {
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
