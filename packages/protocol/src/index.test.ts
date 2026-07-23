import { describe, expect, it } from "vitest";
import {
  ClientRequestSchema,
  GatewayEventSchema,
  ThreadSnapshotSchema,
  gatewaySchemas
} from "./index";

describe("ClientRequestSchema", () => {
  it("validates every corrected generated request signature", () => {
    expect(ClientRequestSchema.safeParse({
      method: "thread/history/draft/read",
      params: {
        scope: {
          cwd: "/tmp/project",
          source: { kind: "web", rawId: "thread-test" }
        },
        threadId: "thread-1",
        messageId: "message:7"
      }
    }).success).toBe(true);
    expect(ClientRequestSchema.safeParse({
      method: "workspace/create",
      params: { name: "research", parent: "/tmp/workspaces" }
    }).success).toBe(true);
  });

  it("requires a client correlation id for turn/start", () => {
    const params = {
      scope: {
        cwd: "/tmp/project",
        source: { kind: "web", rawId: "thread-test" }
      },
      input: [{ type: "text", text: "hello" }]
    };
    expect(ClientRequestSchema.safeParse({
      method: "turn/start",
      params: { ...params, clientTurnId: "client-turn-1" }
    }).success).toBe(true);
    expect(ClientRequestSchema.safeParse({
      method: "turn/start",
      params
    }).success).toBe(false);
  });
});

describe("ThreadApplication hard-cut schemas", () => {
  it("projects an optional diagnostic reason on runtime capability facts", () => {
    expect(gatewaySchemas.RuntimeCapabilityView.required).toEqual([
      "enabled",
      "id",
      "stability"
    ]);
    expect(gatewaySchemas.RuntimeCapabilityView.properties.unavailableReason).toEqual({
      default: null,
      type: ["string", "null"]
    });
  });

  it("exports semantic history owners without the retired runtime owner", () => {
    expect(gatewaySchemas.ThreadHistoryOwnerView.enum).toEqual([
      "psychevo",
      "agent",
      "process"
    ]);
  });

  it("rejects producerless runtime state and child events", () => {
    expect(GatewayEventSchema.safeParse({
      type: "runtimeStateChanged",
      runtimeRef: "acp:codex",
      threadId: "thread-1",
      state: "ready",
      detail: null,
      processEpoch: 2,
      instanceEpoch: 1
    }).success).toBe(false);
    expect(GatewayEventSchema.safeParse({
      type: "runtimeChildChanged",
      runtimeRef: "acp:codex",
      parentThreadId: "thread-1",
      threadId: "thread-child",
      dedupKey: "child-1",
      status: "running",
      readOnly: true
    }).success).toBe(false);
  });
});

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
        backend: { kind: "native", sessionHandle: "s1" },
        sourceKey: "web:cwd:abc"
      },
      history: { owner: "psychevo", fidelity: "full", cursor: null, hint: null },
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
      turnStartReceipts: [{ clientTurnId: "client-turn-1", turnId: "turn-1" }],
      pendingActions: []
    });

    expect(parsed.thread?.id).toBe("s1");
    expect(parsed.history).toEqual({ owner: "psychevo", fidelity: "full", cursor: null, hint: null });
    expect(parsed.entries).toHaveLength(2);
    expect(parsed.turnStartReceipts).toEqual([
      { clientTurnId: "client-turn-1", turnId: "turn-1" }
    ]);
  });
});
