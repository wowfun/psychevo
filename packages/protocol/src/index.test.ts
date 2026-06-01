import { describe, expect, it } from "vitest";
import { ThreadSnapshotSchema } from "./index";

describe("ThreadSnapshotSchema", () => {
  it("parses the Gateway web snapshot shape", () => {
    const parsed = ThreadSnapshotSchema.parse({
      source: {
        kind: "web",
        rawId: "workdir:abc",
        lifetime: "persistent",
        rawIdentity: null,
        visibleName: "psychevo"
      },
      thread: {
        id: "s1",
        backend: { kind: "psychevo", nativeId: "s1" },
        sourceKey: "web:workdir:abc"
      },
      items: [
        {
          id: "message:1:prompt",
          threadId: "s1",
          turnId: "message:1",
          sequence: 1,
          kind: "prompt",
          status: "completed",
          source: "runtime.message",
          title: null,
          body: "hello",
          preview: "hello",
          detail: "hello",
          artifactIds: [],
          metadata: null,
          createdAtMs: 1,
          updatedAtMs: 1
        },
        {
          id: "message:2:assistant",
          threadId: "s1",
          turnId: "message:2",
          sequence: 2,
          kind: "assistant",
          status: "completed",
          source: "runtime.message",
          title: null,
          body: "hi",
          preview: "hi",
          detail: "hi",
          artifactIds: [],
          metadata: null,
          createdAtMs: 2,
          updatedAtMs: 2
        }
      ],
      activity: { running: false, activeTurnId: null, queuedTurns: 0 },
      pendingPermissions: [],
      pendingClarifies: []
    });

    expect(parsed.thread?.id).toBe("s1");
    expect(parsed.items).toHaveLength(2);
  });
});
