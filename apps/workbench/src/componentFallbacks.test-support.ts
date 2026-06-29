import { cleanup } from "@testing-library/react";
import { afterEach, beforeAll, vi } from "vitest";
import type { SessionSummary, TranscriptBlock, TranscriptEntry } from "@psychevo/protocol";

export const noop = vi.fn();

export function setupComponentFallbackTests() {
  beforeAll(() => {
    Object.defineProperty(HTMLElement.prototype, "scrollTo", {
      configurable: true,
      value: vi.fn()
    });
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });
}

export function transcriptEntry(overrides: Partial<TranscriptEntry> = {}): TranscriptEntry {
  return {
    id: "entry-1",
    threadId: "thread-1",
    turnId: "turn-1",
    messageSeq: 1,
    role: "assistant",
    status: "completed",
    source: "runtime.message",
    blocks: [],
    metadata: null,
    usage: null,
    accounting: null,
    createdAtMs: 1,
    updatedAtMs: 1,
    ...overrides
  };
}

export function transcriptBlock(overrides: Partial<TranscriptBlock> = {}): TranscriptBlock {
  return {
    id: "block-1",
    kind: "text",
    status: "completed",
    order: 0,
    source: "runtime.message",
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

export function sessionSummary(overrides: Partial<SessionSummary> = {}): SessionSummary {
  const summary: SessionSummary = {
    id: "thread-1",
    cwd: "/tmp/project",
    project: { cwd: "/tmp/project", label: "project", displayPath: "/tmp/project" },
    model: null,
    provider: null,
    startedAtMs: 1,
    updatedAtMs: null,
    endedAtMs: null,
    endReason: null,
    archivedAtMs: null,
    messageCount: 1,
    toolCallCount: 0,
    visibleEntryCount: 1,
    activity: {
      running: false,
      activeTurnId: null,
      queuedTurns: 0
    },
    title: "Session",
    displayTitle: "Session",
    preview: null,
    ...overrides
  };
  if (overrides.displayTitle === undefined && overrides.title !== undefined) {
    summary.displayTitle = overrides.title;
  }
  return summary;
}
