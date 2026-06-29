import { describe, expect, it } from "vitest";
import { parseThreadSnapshot, scopeForCwd } from "./index";

describe("scopeForCwd", () => {
  it("creates a persistent web source scope", () => {
    expect(scopeForCwd("/tmp/project")).toEqual({
      cwd: "/tmp/project",
      source: {
        kind: "web",
        rawId: null,
        lifetime: "persistent",
        rawIdentity: null,
        visibleName: null
      }
    });
  });
});

describe("parseThreadSnapshot", () => {
  it("rejects snapshots without transcript entries", () => {
    expect(() => parseThreadSnapshot({
      source: {
        kind: "web",
        rawId: "cwd:abc",
        lifetime: "persistent",
        rawIdentity: null,
        visibleName: "psychevo"
      },
      thread: null
    })).toThrow(/entries/);
  });

  it("defaults idle snapshot fields before strict validation", () => {
    const parsed = parseThreadSnapshot({
      source: {
        kind: "web",
        rawId: "cwd:abc",
        lifetime: "persistent",
        rawIdentity: null,
        visibleName: "psychevo"
      },
      thread: null,
      entries: []
    });

    expect(parsed.entries).toEqual([]);
    expect(parsed.activity).toEqual({ running: false, activeTurnId: null, queuedTurns: 0 });
    expect(parsed.pendingPermissions).toEqual([]);
    expect(parsed.pendingClarifies).toEqual([]);
  });

  it("preserves optional activity fields when applying defaults", () => {
    const parsed = parseThreadSnapshot({
      source: {
        kind: "web",
        rawId: "cwd:abc",
        lifetime: "persistent",
        rawIdentity: null,
        visibleName: "psychevo"
      },
      thread: null,
      entries: [],
      activity: {
        running: true,
        activeTurnId: "turn-1",
        queuedTurns: 0,
        startedAtMs: 1_000,
        updatedAtMs: 2_000,
        ownerId: "gateway:owner",
        ownerSurface: "web",
        leaseExpiresAtMs: 30_000,
        takeoverState: "requested"
      }
    });

    expect(parsed.activity).toEqual({
      running: true,
      activeTurnId: "turn-1",
      queuedTurns: 0,
      startedAtMs: 1_000,
      updatedAtMs: 2_000,
      ownerId: "gateway:owner",
      ownerSurface: "web",
      leaseExpiresAtMs: 30_000,
      takeoverState: "requested"
    });
  });

  it("preserves message-derived entries in a history snapshot", () => {
    const parsed = parseThreadSnapshot({
      source: {
        kind: "web",
        rawId: "cwd:abc",
        lifetime: "persistent",
        rawIdentity: null,
        visibleName: "psychevo"
      },
      thread: {
        id: "thread-1",
        backend: { kind: "psychevo", nativeId: "thread-1" },
        sourceKey: "web:cwd:abc"
      },
      entries: [
        {
          id: "message:1",
          threadId: "thread-1",
          turnId: "message:1",
          messageSeq: 1,
          role: "user",
          status: "completed",
          source: "runtime.message",
          blocks: [
            {
              id: "message:1:block:0",
              kind: "text",
              status: "completed",
              order: 0,
              source: "runtime.message",
              title: null,
              body: "hello history",
              preview: "hello history",
              detail: "hello history",
              artifactIds: [],
              metadata: null,
              result: null,
              createdAtMs: 1,
              updatedAtMs: 1
            }
          ],
          metadata: null,
          usage: null,
          accounting: null,
          createdAtMs: 1,
          updatedAtMs: 1
        }
      ],
      activity: { running: false, activeTurnId: null, queuedTurns: 0 },
      pendingPermissions: [],
      pendingClarifies: []
    });

    expect(parsed.entries).toHaveLength(1);
    expect(parsed.entries[0]?.blocks[0]?.body).toBe("hello history");
  });
});
