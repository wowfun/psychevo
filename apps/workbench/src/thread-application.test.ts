import type { GatewayClient } from "@psychevo/client";
import type { ThreadContextReadResult, ThreadSnapshot, TranscriptEntry } from "@psychevo/protocol";
import { describe, expect, it, vi } from "vitest";
import {
  enabledThreadAction,
  hydrateThreadSnapshotHistory,
  readProjectedThreadHistory,
  snapshotThreadApplicationTarget
} from "./thread-application";

const scope = {
  cwd: "/tmp/project",
  source: {
    kind: "web",
    rawId: null,
    lifetime: "persistent" as const,
    rawIdentity: null,
    visibleName: null
  }
};

describe("Workbench Thread Application boundary", () => {
  it("reads opaque history pages and replaces bootstrap entries", async () => {
    const first = entry("entry-1", "first");
    const second = entry("entry-2", "second");
    const request = vi.fn(async (_method: string, params: { cursor?: string | null }) => (
      params.cursor
        ? {
            threadId: "thread-1",
            history: { owner: "psychevo", fidelity: "full", cursor: null, hint: null },
            entries: [second],
            nextCursor: null
          }
        : {
            threadId: "thread-1",
            history: { owner: "psychevo", fidelity: "full", cursor: "entry-1", hint: null },
            entries: [first],
            nextCursor: "entry-1"
          }
    ));
    const client = { request } as unknown as GatewayClient;
    const hydrated = await hydrateThreadSnapshotHistory(client, snapshot([entry("bootstrap", "stale")]));

    expect(hydrated.entries.map((item) => item.id)).toEqual(["entry-1", "entry-2"]);
    expect(request).toHaveBeenNthCalledWith(1, "thread/history/read", {
      scope,
      threadId: "thread-1",
      cursor: null,
      limit: 200
    });
    expect(request).toHaveBeenNthCalledWith(2, "thread/history/read", {
      scope,
      threadId: "thread-1",
      cursor: "entry-1",
      limit: 200
    });
  });

  it("rejects a repeated opaque history cursor", async () => {
    const client = {
      request: vi.fn(async () => ({
        threadId: "thread-1",
        history: { owner: "psychevo", fidelity: "full", cursor: "loop", hint: null },
        entries: [entry("entry-1", "first")],
        nextCursor: "loop"
      }))
    } as unknown as GatewayClient;

    await expect(readProjectedThreadHistory(client, scope, "thread-1"))
      .rejects.toThrow("repeated cursor loop");
  });

  it("requires the active snapshot identity and an enabled action descriptor", () => {
    const current = snapshot([]);
    expect(snapshotThreadApplicationTarget(current, "another-thread")).toBeNull();
    expect(snapshotThreadApplicationTarget(current)).toEqual({ scope, threadId: "thread-1" });
    expect(enabledThreadAction(context(false), "steer")).toBeNull();
    expect(enabledThreadAction(context(true), "steer")?.id).toBe("steer");
  });
});

function snapshot(entries: TranscriptEntry[]): ThreadSnapshot {
  return {
    source: {
      kind: "web",
      rawId: "workbench",
      lifetime: "persistent",
      rawIdentity: null,
      visibleName: null
    },
    scope,
    thread: {
      id: "thread-1",
      backend: { kind: "native", runtimeRef: "native", sessionHandle: "thread-1" },
      sourceKey: "source-thread-1"
    },
    history: { owner: "psychevo", fidelity: "full", cursor: null, hint: null },
    entries,
    activity: { running: false, activeTurnId: null, queuedTurns: 0 },
    pendingActions: []
  };
}

function context(steerEnabled: boolean): ThreadContextReadResult {
  return {
    selectedTargetId: "target:native",
    suggestedTargetId: null,
    runtimeProfileRef: "native",
    selectionState: "bound",
    profiles: [],
    binding: null,
    controls: [],
    stability: "stable",
    capabilities: [],
    compatibleTargets: [],
    inputCapabilities: [],
    actions: [{
      id: "steer",
      label: "Steer",
      enabled: steerEnabled,
      stability: "stable",
      channelSafe: true,
      unavailableReason: steerEnabled ? null : "No running turn."
    }],
    sendability: { allowed: true, reason: null, recoveryAction: null },
    history: { owner: "psychevo", fidelity: "full", cursor: null, hint: null },
    pendingInteractions: [],
    contextRevision: "1",
    controlRevision: "1"
  };
}

function entry(id: string, body: string): TranscriptEntry {
  return {
    id,
    threadId: "thread-1",
    turnId: "turn-1",
    messageSeq: 1,
    role: "assistant",
    status: "completed",
    source: "test",
    blocks: [{
      id: `${id}:text`,
      kind: "text",
      status: "completed",
      order: 0,
      source: "test",
      title: null,
      body,
      preview: body,
      detail: body,
      artifactIds: [],
      metadata: null,
      result: null,
      createdAtMs: 1,
      updatedAtMs: 1
    }],
    metadata: null,
    usage: null,
    accounting: null,
    createdAtMs: 1,
    updatedAtMs: 1
  };
}
