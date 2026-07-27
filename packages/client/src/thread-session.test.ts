import type {
  GatewayEvent,
  GatewayMethod,
  GatewayRequestResults,
  GatewayRequestScope,
  RpcNotification,
  ThreadContextReadResult,
  ThreadSnapshot
} from "@psychevo/protocol";
import { describe, expect, it, vi } from "vitest";

import {
  GatewayClientError,
  type GatewayRequestArguments,
  type GatewayConnectionSnapshot
} from "./index";
import {
  ThreadSession,
  type ThreadSessionClient
} from "./thread-session";
import { emptyThreadSnapshot } from "./thread-controller";

describe("ThreadSession", () => {
  it("publishes one immutable composite view for a context-only update", () => {
    const session = new ThreadSession({
      context: readyContext(),
      snapshot: emptyThreadSnapshot(scope())
    });
    const before = session.getView();
    const listener = vi.fn();
    session.subscribe(listener);
    const context = { ...readyContext(), contextRevision: "context-2" };

    session.setContext(context);

    expect(listener).toHaveBeenCalledOnce();
    expect(session.getView()).not.toBe(before);
    expect(session.getView()).toEqual({
      context,
      liveEntries: [],
      threadSnapshot: before.threadSnapshot
    });
  });

  it("publishes a control receipt once to snapshot and context consumers", async () => {
    const client = new FakeThreadSessionClient();
    const session = readySession(client);
    const context = { ...readyContext(), controlRevision: "control-2" };
    client.respond("thread/control/set", {
      bindingRevision: 0,
      changed: true,
      context,
      contextRevision: "context-1",
      control: {
        capabilityRevision: "capability-1",
        applyScope: "session",
        channelSafe: true,
        choices: [],
        effectiveSource: "runtimeObserved",
        effectiveValue: "high",
        enabled: true,
        id: "reasoning",
        isDefault: false,
        label: "Reasoning",
        mutability: "selectable",
        required: false,
        stability: "stable",
        surfaceRole: "reasoning",
        unavailableReason: null
      },
      controlRevision: "control-2",
      status: "observed"
    });
    const listener = vi.fn();
    session.subscribe(listener);

    await session.setControl({
      control: {
        capabilityRevision: "capability-1",
        applyScope: "session",
        channelSafe: true,
        choices: [],
        effectiveSource: "runtimeObserved",
        effectiveValue: "medium",
        enabled: true,
        id: "reasoning",
        isDefault: false,
        label: "Reasoning",
        mutability: "selectable",
        required: false,
        stability: "stable",
        surfaceRole: "reasoning",
        unavailableReason: null
      },
      scope: scope(),
      targetId: "agent-1",
      threadId: null,
      value: "high"
    });

    expect(listener).toHaveBeenCalledOnce();
    expect(session.getView().context?.controlRevision).toBe("control-2");
  });

  it("does not apply a control receipt after the Session view changes", async () => {
    const client = new FakeThreadSessionClient();
    const session = readySession(client);
    const pending = deferred<GatewayRequestResults["thread/control/set"]>();
    client.handle("thread/control/set", () => pending.promise);
    const mutation = session.setControl({
      control: {
        capabilityRevision: "capability-1",
        applyScope: "session",
        channelSafe: true,
        choices: [],
        effectiveSource: "runtimeObserved",
        effectiveValue: "medium",
        enabled: true,
        id: "reasoning",
        isDefault: false,
        label: "Reasoning",
        mutability: "selectable",
        required: false,
        stability: "stable",
        surfaceRole: "reasoning",
        unavailableReason: null
      },
      scope: scope(),
      targetId: "agent-1",
      threadId: null,
      value: "high"
    });
    const replacement = { ...readyContext(), controlRevision: "replacement-control" };
    session.reset(emptyThreadSnapshot(scope()), replacement);
    const listener = vi.fn();
    session.subscribe(listener);

    pending.resolve({
      bindingRevision: 0,
      changed: true,
      context: { ...readyContext(), controlRevision: "stale-control" },
      contextRevision: "context-1",
      control: {
        capabilityRevision: "capability-1",
        applyScope: "session",
        channelSafe: true,
        choices: [],
        effectiveSource: "runtimeObserved",
        effectiveValue: "high",
        enabled: true,
        id: "reasoning",
        isDefault: false,
        label: "Reasoning",
        mutability: "selectable",
        required: false,
        stability: "stable",
        surfaceRole: "reasoning",
        unavailableReason: null
      },
      controlRevision: "stale-control",
      status: "observed"
    });
    await mutation;

    expect(session.getContext()).toBe(replacement);
    expect(listener).not.toHaveBeenCalled();
  });

  it("prepends one strictly older history page and advances its cursor", async () => {
    const client = new FakeThreadSessionClient();
    const latest = entryEvent("entryUpdated", "latest").entry;
    const older = {
      ...entryEvent("entryUpdated", "older").entry,
      id: "entry-older",
      messageSeq: 1
    };
    const session = readySession(client, {
      ...runningSnapshot(),
      activity: { running: false, activeTurnId: null, queuedTurns: 0 },
      entries: [latest],
      history: { owner: "psychevo", fidelity: "full", cursor: "message:2", hint: null }
    });
    client.respond("thread/history/read", {
      threadId: "thread-1",
      entries: [older],
      history: { owner: "psychevo", fidelity: "full", cursor: null, hint: null },
      nextCursor: null
    });

    await session.loadOlder();

    expect(session.getSnapshot()?.entries.map((entry) => entry.id))
      .toEqual(["entry-older", latest.id]);
    expect(session.getSnapshot()?.history.cursor).toBeNull();
  });

  it("drops an older history page after the Session view changes", async () => {
    const client = new FakeThreadSessionClient();
    const pending = deferred<GatewayRequestResults["thread/history/read"]>();
    client.handle("thread/history/read", () => pending.promise);
    const session = readySession(client, {
      ...runningSnapshot(),
      history: { owner: "psychevo", fidelity: "full", cursor: "message:2", hint: null }
    });

    const load = session.loadOlder();
    const replacement = emptyThreadSnapshot(scope());
    session.reset(replacement, readyContext());
    pending.resolve({
      threadId: "thread-1",
      entries: [entryEvent("entryUpdated", "stale").entry],
      history: { owner: "psychevo", fidelity: "full", cursor: null, hint: null },
      nextCursor: null
    });
    await load;

    expect(session.getSnapshot()).toBe(replacement);
  });

  it("owns optimistic Send and Gateway acceptance", async () => {
    const client = new FakeThreadSessionClient();
    const session = readySession(client);
    client.respond("turn/start", {
      accepted: true,
      thread: gatewayThread("thread-1"),
      threadId: "thread-1",
      turnId: "turn-1"
    });

    const outcome = await session.send(sendInput());

    expect(outcome).toMatchObject({
      status: "accepted",
      threadId: "thread-1",
      turnId: "turn-1"
    });
    expect(session.getSnapshot()?.entries[0]?.blocks[0]?.body).toBe("hello");
  });

  it("reconciles unknown delivery from the authoritative receipt without replay", async () => {
    const client = new FakeThreadSessionClient();
    const session = readySession(client);
    client.fail("turn/start", new GatewayClientError(
      "request_timeout",
      "unknown",
      "timed out"
    ));
    client.handle("thread/resume", (params) => {
      const optimistic = session.getSnapshot()!;
      const turnStart = client.requests.find((request) => request.method === "turn/start")!;
      const clientTurnId = (turnStart.params as { clientTurnId: string }).clientTurnId;
      return {
        ...optimistic,
        thread: gatewayThread("thread-1"),
        turnStartReceipts: [{
          clientTurnId,
          createdAtMs: 1,
          turnId: "turn-1"
        }]
      };
    });

    const outcome = await session.send(sendInput());

    expect(outcome).toMatchObject({
      status: "reconciled",
      accepted: true,
      threadId: "thread-1"
    });
    expect(client.requests.filter((request) => request.method === "turn/start")).toHaveLength(1);
    expect(client.requests.filter((request) => request.method === "thread/resume")).toHaveLength(1);
  });

  it("keeps unknown delivery pending until a connected generation can recover it", async () => {
    const client = new FakeThreadSessionClient();
    const session = readySession(client);
    client.fail("turn/start", new GatewayClientError(
      "disconnected",
      "unknown",
      "disconnected"
    ));
    client.setConnection("reconnecting");
    const send = session.send(sendInput());
    await Promise.resolve();
    expect(client.requests.filter((request) => request.method === "thread/resume")).toHaveLength(0);

    client.handle("thread/resume", () => ({
      ...session.getSnapshot()!,
      turnStartReceipts: []
    }));
    client.setConnection("connected");

    await expect(send).resolves.toMatchObject({
      status: "reconciled",
      accepted: false
    });
  });

  it("reactivates the same owner after lifecycle cleanup", async () => {
    const client = new FakeThreadSessionClient();
    const session = readySession(client);
    session.dispose();
    expect(client.notificationSubscriberCount()).toBe(0);
    expect(client.connectionSubscriberCount()).toBe(0);
    session.attachClient(client);
    expect(client.notificationSubscriberCount()).toBe(1);
    expect(client.connectionSubscriberCount()).toBe(1);
    client.respond("turn/start", {
      accepted: true,
      thread: gatewayThread("thread-1"),
      threadId: "thread-1",
      turnId: "turn-1"
    });

    await expect(session.send(sendInput())).resolves.toMatchObject({
      status: "accepted",
      detached: false,
      threadId: "thread-1",
      turnId: "turn-1"
    });
  });

  it("reports accepted delivery as detached when the view changes before the receipt", async () => {
    const client = new FakeThreadSessionClient();
    const session = readySession(client);
    const receipt = deferred<{
      accepted: true;
      thread: ReturnType<typeof gatewayThread>;
      threadId: string;
      turnId: string;
    }>();
    client.handle("turn/start", () => receipt.promise);

    const send = session.send(sendInput());
    session.reset(emptyThreadSnapshot(scope()), readyContext());
    receipt.resolve({
      accepted: true,
      thread: gatewayThread("thread-accepted"),
      threadId: "thread-accepted",
      turnId: "turn-accepted"
    });

    await expect(send).resolves.toMatchObject({
      status: "accepted",
      detached: true,
      threadId: "thread-accepted",
      turnId: "turn-accepted"
    });
    expect(session.getSnapshot()?.thread).toBeNull();
    expect(session.getSnapshot()?.entries).toHaveLength(0);
  });

  it("settles unknown recovery when the owning view is invalidated", async () => {
    const client = new FakeThreadSessionClient();
    const session = readySession(client);
    client.fail("turn/start", new GatewayClientError(
      "disconnected",
      "unknown",
      "disconnected"
    ));
    client.setConnection("reconnecting");

    const send = session.send(sendInput());
    await Promise.resolve();
    session.reset(emptyThreadSnapshot(scope()), readyContext());

    await expect(send).resolves.toMatchObject({
      status: "cancelled",
      delivery: "unknown",
      reason: "view_changed"
    });
  });

  it("applies first assistant output immediately and flushes later output before terminal", async () => {
    vi.useFakeTimers();
    const client = new FakeThreadSessionClient();
    const session = readySession(client, runningSnapshot());

    client.notify(gatewayNotification(entryEvent("entryUpdated", "first")));
    expect(session.getSnapshot()?.entries.at(-1)?.blocks[0]?.body).toBe("first");

    client.notify(gatewayNotification(entryEvent("entryUpdated", "second")));
    client.notify(gatewayNotification({
      type: "turnCompleted",
      committedEntries: [],
      threadId: "thread-1",
      turnId: "turn-1",
      turn: {
        id: "turn-1",
        threadId: "thread-1",
        status: "completed",
        outcome: "ok",
        error: null,
        startedAtMs: 1,
        completedAtMs: 2
      }
    }));
    await vi.runAllTimersAsync();

    expect(session.getSnapshot()?.activity.running).toBe(false);
    expect(session.getSnapshot()?.entries.at(-1)?.blocks[0]?.body).toBe("second");
    vi.useRealTimers();
  });

  it("rejects unrelated Thread events before they enter the pacing queue", () => {
    vi.useFakeTimers();
    const client = new FakeThreadSessionClient();
    const session = readySession(client, runningSnapshot());
    const unrelated = entryEvent("entryUpdated", "other thread");
    unrelated.turnId = "turn-other";
    unrelated.entry = {
      ...unrelated.entry,
      threadId: "thread-other",
      turnId: "turn-other"
    };

    client.notify(gatewayNotification(unrelated));

    expect(vi.getTimerCount()).toBe(0);
    expect(session.getView().liveEntries).toHaveLength(0);
    vi.useRealTimers();
  });
});

type RequestRecord = { method: string; params: unknown };

class FakeThreadSessionClient implements ThreadSessionClient {
  readonly requests: RequestRecord[] = [];
  private connection: GatewayConnectionSnapshot = {
    state: "connected",
    generation: 1,
    attempt: 1,
    nextRetryAtMs: null,
    lastError: null
  };
  private readonly handlers = new Set<(notification: RpcNotification) => void>();
  private readonly connectionHandlers =
    new Set<(snapshot: GatewayConnectionSnapshot) => void>();
  private readonly responders = new Map<string, (params: unknown) => unknown>();

  connectionSnapshot(): GatewayConnectionSnapshot {
    return { ...this.connection };
  }

  request<M extends GatewayMethod>(
    method: M,
    ...arguments_: GatewayRequestArguments<M>
  ): Promise<GatewayRequestResults[M]> {
    const [params] = arguments_;
    this.requests.push({ method, params });
    const responder = this.responders.get(method);
    if (!responder) return Promise.reject(new Error(`No response for ${method}`));
    try {
      return Promise.resolve(responder(params)) as Promise<GatewayRequestResults[M]>;
    } catch (error) {
      return Promise.reject(error);
    }
  }

  subscribe(handler: (notification: RpcNotification) => void): () => void {
    this.handlers.add(handler);
    return () => this.handlers.delete(handler);
  }

  subscribeConnectionState(
    handler: (snapshot: GatewayConnectionSnapshot) => void
  ): () => void {
    this.connectionHandlers.add(handler);
    handler(this.connectionSnapshot());
    return () => this.connectionHandlers.delete(handler);
  }

  respond(method: string, result: unknown): void {
    this.handle(method, () => result);
  }

  fail(method: string, error: Error): void {
    this.handle(method, () => {
      throw error;
    });
  }

  handle(method: string, responder: (params: any) => unknown): void {
    this.responders.set(method, responder);
  }

  notify(notification: RpcNotification): void {
    for (const handler of this.handlers) handler(notification);
  }

  notificationSubscriberCount(): number {
    return this.handlers.size;
  }

  connectionSubscriberCount(): number {
    return this.connectionHandlers.size;
  }

  setConnection(state: GatewayConnectionSnapshot["state"]): void {
    this.connection = {
      ...this.connection,
      state,
      generation: state === "connected" ? this.connection.generation + 1 : this.connection.generation
    };
    for (const handler of this.connectionHandlers) handler(this.connectionSnapshot());
  }
}

function readySession(
  client: FakeThreadSessionClient,
  snapshot: ThreadSnapshot = emptyThreadSnapshot(scope())
): ThreadSession {
  return new ThreadSession({
    client,
    context: readyContext(),
    snapshot
  });
}

function sendInput() {
  return {
    controls: {
      targetId: "agent-1",
      expectedContextRevision: "context-1",
      expectedControlRevision: "control-1"
    },
    input: [{ type: "text" as const, text: "hello" }],
    optimisticText: "hello",
    scope: scope(),
    threadId: null
  };
}

function scope(): GatewayRequestScope {
  return {
    cwd: "/workspace",
    source: {
      kind: "web",
      rawId: "workspace",
      lifetime: "persistent",
      rawIdentity: null,
      visibleName: "workspace"
    }
  };
}

function readyContext(): ThreadContextReadResult {
  return {
    binding: null,
    actions: [],
    capabilities: [],
    compatibleTargets: [{
      agentRef: "agent-1",
      agentLabel: "Agent",
      label: "Agent",
      profileLabel: "Native",
      ready: true,
      runtimeProfileRef: "native",
      targetId: "agent-1",
      unavailableReason: null
    }],
    contextRevision: "context-1",
    controlRevision: "control-1",
    controls: [],
    history: { cursor: null, fidelity: "full", hint: null, owner: "psychevo" },
    inputCapabilities: [{
      enabled: true,
      kind: "text",
      unavailableReason: null
    }],
    pendingInteractions: [],
    profiles: [],
    runtimeProfileRef: "native",
    selectedTargetId: "agent-1",
    selectionState: "prospective",
    sendability: { allowed: true, reason: null, recoveryAction: null },
    stability: null,
    suggestedTargetId: "agent-1",
  };
}

function gatewayThread(id: string) {
  return {
    backend: {
      kind: "native" as const,
      runtimeRef: "native",
      sessionHandle: id
    },
    id,
    sourceKey: "web:workspace"
  };
}

function runningSnapshot(): ThreadSnapshot {
  return {
    ...emptyThreadSnapshot(scope(), "thread-1"),
    activity: {
      activeTurnId: "turn-1",
      queuedTurns: 0,
      running: true
    },
    thread: gatewayThread("thread-1")
  };
}

function entryEvent(
  type: "entryUpdated",
  text: string
): Extract<GatewayEvent, { type: "entryUpdated" }> {
  return {
    type,
    turnId: "turn-1",
    entry: {
      blocks: [{
        artifactIds: [],
        body: text,
        createdAtMs: 1,
        detail: text,
        id: "block-1",
        kind: "text",
        metadata: null,
        order: 0,
        preview: text,
        result: null,
        source: "runtime",
        status: "running",
        title: null,
        updatedAtMs: 1
      }],
      createdAtMs: 1,
      id: "entry-1",
      metadata: null,
      role: "assistant",
      source: "runtime",
      status: "running",
      threadId: "thread-1",
      turnId: "turn-1",
      updatedAtMs: 1,
      usage: null,
      accounting: null,
      messageSeq: null
    }
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function gatewayNotification(event: GatewayEvent): RpcNotification {
  return {
    jsonrpc: "2.0",
    method: "gateway/event",
    params: event
  };
}
