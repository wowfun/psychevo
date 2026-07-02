import { describe, expect, it } from "vitest";
import { ThreadSnapshotSchema } from "./index";

describe("ThreadSnapshotSchema", () => {
  it("parses the Gateway web snapshot shape", () => {
    const parsed = ThreadSnapshotSchema.parse({
      source: {
        kind: "web",
        rawId: "cwd:abc",
        lifetime: "persistent",
        rawIdentity: null,
        visibleName: "psychevo"
      },
      scope: {
        cwd: "/tmp/project",
        source: {
          kind: "web",
          rawId: "cwd:abc",
          lifetime: "persistent",
          rawIdentity: null,
          visibleName: "psychevo"
        }
      },
      thread: {
        id: "s1",
        backend: { kind: "psychevo", nativeId: "s1" },
        sourceKey: "web:cwd:abc"
      },
      entries: [
        {
          id: "message:1:user",
          threadId: "s1",
          turnId: "message:1",
          messageSeq: 1,
          role: "user",
          status: "completed",
          source: "runtime.message",
          blocks: [
            {
              id: "message:1:user:text",
              kind: "text",
              status: "completed",
              order: 0,
              source: "runtime.message",
              title: null,
              body: "hello",
              preview: "hello",
              detail: "hello",
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
        },
        {
          id: "message:2:assistant",
          threadId: "s1",
          turnId: "message:2",
          messageSeq: 2,
          role: "assistant",
          status: "completed",
          source: "runtime.message",
          blocks: [
            {
              id: "message:2:assistant:text",
              kind: "text",
              status: "completed",
              order: 0,
              source: "runtime.message",
              title: null,
              body: "hi",
              preview: "hi",
              detail: "hi",
              artifactIds: [],
              metadata: null,
              result: null,
              createdAtMs: 2,
              updatedAtMs: 2
            }
          ],
          metadata: null,
          usage: null,
          accounting: null,
          createdAtMs: 2,
          updatedAtMs: 2
        }
      ],
      activity: { running: false, activeTurnId: null, queuedTurns: 0 },
      pendingActions: []
    });

    expect(parsed.thread?.id).toBe("s1");
    expect(parsed.entries).toHaveLength(2);
  });
});
