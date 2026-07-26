import type { GatewayClient } from "@psychevo/client";
import type { GatewayRequestScope, ThreadSnapshot } from "@psychevo/protocol";
import { describe, expect, it, vi } from "vitest";
import type { WorkbenchIntentOwnerParams } from "./workbench-intents";
import { createWorkbenchIntentOwner } from "./workbench-intents";

const scope: GatewayRequestScope = {
  cwd: "/workspace",
  source: {
    kind: "web",
    rawId: "intent-test",
    lifetime: "persistent",
    rawIdentity: null,
    visibleName: null
  }
};

describe("Workbench intent owner", () => {
  it("owns current-session deletion through durable mutation, draft reset, and both history views", async () => {
    const events: string[] = [];
    const request = vi.fn(async (method: string) => {
      events.push(`request:${method}`);
      return {};
    });
    const params = intentParams(request);
    params.currentThreadId = "thread-1";
    params.setDraftSession = vi.fn(() => events.push("draft:clear"));
    params.startNewThread = vi.fn(async () => {
      events.push("thread:new");
      return undefined;
    });
    params.refreshHistory = vi.fn(async (_client, archived) => {
      events.push(`history:${archived === true ? "archived" : "active"}`);
      return [];
    });
    const owner = createWorkbenchIntentOwner(params);

    await owner.deleteSession({
      id: "thread-1",
      cwd: "/workspace"
    } as WorkbenchIntentOwnerParams["sessions"][number]);

    expect(request).toHaveBeenCalledWith("thread/delete", { threadId: "thread-1" });
    expect(events).toEqual([
      "draft:clear",
      "request:thread/delete",
      "thread:new",
      "history:active",
      "history:archived"
    ]);
  });

  it("filters Agent mentions in the action owner while preserving completion semantics", async () => {
    const request = vi.fn(async () => ({
      items: [
        {
          id: "file",
          sigil: "@",
          label: "README.md",
          insertText: "README.md",
          kind: "file",
          target: { kind: "file", path: "README.md" }
        },
        {
          id: "agent",
          sigil: "@",
          label: "reviewer",
          insertText: "reviewer",
          kind: "agent",
          target: { kind: "agent", name: "reviewer" }
        }
      ],
      replacement: { start: 0, end: 3 }
    }));
    const params = intentParams(request);
    params.agentMentionsEnabled = false;
    const owner = createWorkbenchIntentOwner(params);

    const completion = await owner.completion("@re", 3);

    expect(completion.items.map((item) => item.id)).toEqual(["file"]);
    expect(completion.replacement).toEqual({ start: 0, end: 3 });
    expect(request).toHaveBeenCalledWith("completion/list", {
      cursor: 3,
      scope,
      text: "@re",
      threadId: "thread-1"
    });
  });

  it("routes an interaction only through the active Thread identity and refreshes its snapshot", async () => {
    const request = vi.fn(async () => ({ accepted: true }));
    const params = intentParams(request);
    const owner = createWorkbenchIntentOwner(params);

    await owner.respondClarify({
      actionId: "clarify-1",
      threadId: "thread-1",
      kind: "clarify",
      payload: {}
    }, [["yes"]], false);

    expect(request).toHaveBeenCalledWith("thread/interaction/respond", {
      interactionId: "clarify-1",
      scope,
      threadId: "thread-1",
      response: { kind: "clarify", answers: [["yes"]] }
    });
    expect(params.setCommandFeedback).toHaveBeenCalledWith({
      accepted: true,
      command: "thread/interaction/respond",
      message: "Clarify response accepted.",
      feedbackAnchor: "composer"
    });
    expect(params.refreshSnapshot).toHaveBeenCalledWith(
      expect.anything(),
      "thread-1",
      undefined,
      true
    );
  });
});

function intentParams(
  request: ReturnType<typeof vi.fn>
): WorkbenchIntentOwnerParams {
  const client = { request } as unknown as GatewayClient;
  return {
    activeScope: scope,
    agentMentionsEnabled: true,
    archivedSessions: [],
    beginExplicitViewSwitch: vi.fn(() => 1),
    clearCommandTransientUi: vi.fn(),
    client,
    currentThreadId: "thread-1",
    fallbackCwd: "/workspace",
    importScope: scope,
    initScope: scope,
    patchComposerDraft: vi.fn(),
    refreshHistory: vi.fn(async () => []),
    refreshSnapshot: vi.fn(async () => undefined),
    sessions: [],
    setAttachments: vi.fn(),
    setCommandFeedback: vi.fn(),
    setDraftSession: vi.fn(),
    setMobilePanel: vi.fn(),
    setSnapshot: vi.fn(),
    settings: undefined,
    snapshot: snapshot(),
    startNewThread: vi.fn(async () => undefined),
    steerAvailable: false,
    steerTurnId: null,
    submitThreadTurn: vi.fn(async () => undefined),
    updateMainView: vi.fn(),
    viewEpochRef: { current: 0 }
  };
}

function snapshot(): ThreadSnapshot {
  return {
    source: {
      kind: "web",
      rawId: "intent-test",
      lifetime: "persistent",
      rawIdentity: null,
      visibleName: null
    },
    scope,
    thread: {
      id: "thread-1",
      backend: {
        kind: "native",
        runtimeRef: "native",
        sessionHandle: "thread-1"
      },
      sourceKey: "source:thread-1"
    },
    history: {
      owner: "psychevo",
      fidelity: "full",
      cursor: null,
      hint: null
    },
    entries: [],
    activity: {
      running: false,
      activeTurnId: null,
      queuedTurns: 0
    },
    pendingActions: []
  };
}
