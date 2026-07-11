import { describe, expect, it } from "vitest";
import type { ThreadSnapshot } from "@psychevo/protocol";
import {
  createHistoryDraftSession,
  shouldAdoptDetachedShellResult,
  shouldApplyReadOnlySnapshot,
  visibleHistoryDraftSession
} from "./viewGuard";

describe("view snapshot guards", () => {
  it("does not let stale read-only refresh adopt an empty detached draft", () => {
    expect(shouldApplyReadOnlySnapshot(detachedSnapshot(), "thread-old", 2, 1)).toBe(false);
    expect(shouldApplyReadOnlySnapshot(detachedSnapshot(), "thread-old", 2, 2)).toBe(false);
  });

  it("allows read-only refresh for the visible thread", () => {
    expect(shouldApplyReadOnlySnapshot(threadSnapshot("thread-1"), "thread-1", 2, 2)).toBe(true);
  });

  it("only adopts a detached shell result for the same view epoch", () => {
    const current = detachedSnapshot();

    expect(shouldAdoptDetachedShellResult(current, "thread-shell", 3, { epoch: 3, token: 1 })).toBe(true);
    expect(shouldAdoptDetachedShellResult(current, "thread-shell", 4, { epoch: 3, token: 1 })).toBe(false);
    expect(shouldAdoptDetachedShellResult(current, null, 3, { epoch: 3, token: 1 })).toBe(false);
  });

  it("creates a local new-session draft and hides it in archived history", () => {
    const draft = createHistoryDraftSession(4, "/tmp/project", 100);

    expect(draft).toEqual({ id: "draft:4", title: "New session", createdAtMs: 100, cwd: "/tmp/project" });
    expect(visibleHistoryDraftSession(draft, false)).toBe(draft);
    expect(visibleHistoryDraftSession(draft, true)).toBeNull();
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
    entries: [],
    activity: {
      running: false,
      activeTurnId: null,
      queuedTurns: 0
    },
    pendingActions: []
  };
}

function threadSnapshot(threadId: string): ThreadSnapshot {
  return {
    ...detachedSnapshot(),
    thread: {
      id: threadId,
      backend: {
        kind: "psychevo",
        sessionHandle: threadId,
        runtimeRef: "native"
      },
      sourceKey: "web:test"
    }
  };
}
