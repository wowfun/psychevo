import { afterEach, describe, expect, expectTypeOf, it, vi } from "vitest";
import type {
  ThreadHistoryDraftReadParams,
  ThreadHistoryDraftReadResult,
  WorkspaceCreateParams,
  WorkspaceCreateResult
} from "@psychevo/protocol";
import {
  GatewayClient,
  parseThreadSnapshot,
  runThreadInterrupt,
  scopeForCwd,
  type GatewayRawMessageHandler,
  type GatewayRequestParams,
  type GatewayRequestResults,
  type GatewayTransport
} from "./index";

afterEach(() => {
  vi.useRealTimers();
});

describe("generated request contracts", () => {
  it("binds corrected methods to their params and results", () => {
    expectTypeOf<GatewayRequestParams["thread/history/draft/read"]>()
      .toEqualTypeOf<ThreadHistoryDraftReadParams>();
    expectTypeOf<GatewayRequestResults["thread/history/draft/read"]>()
      .toEqualTypeOf<ThreadHistoryDraftReadResult>();
    expectTypeOf<GatewayRequestParams["workspace/create"]>()
      .toEqualTypeOf<WorkspaceCreateParams>();
    expectTypeOf<GatewayRequestResults["workspace/create"]>()
      .toEqualTypeOf<WorkspaceCreateResult>();
  });
});

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
      history: { owner: "psychevo", fidelity: "full", cursor: null, hint: null },
      entries: []
    });

    expect(parsed.entries).toEqual([]);
    expect(parsed.history).toEqual({ owner: "psychevo", fidelity: "full", cursor: null, hint: null });
    expect(parsed.activity).toEqual({ running: false, activeTurnId: null, queuedTurns: 0 });
    expect(parsed.pendingActions).toEqual([]);
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
      history: { owner: "psychevo", fidelity: "full", cursor: null, hint: null },
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
        backend: { kind: "native", sessionHandle: "thread-1", runtimeRef: "native" },
        sourceKey: "web:cwd:abc"
      },
      history: { owner: "psychevo", fidelity: "full", cursor: null, hint: null },
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
      pendingActions: []
    });

    expect(parsed.entries).toHaveLength(1);
    expect(parsed.entries[0]?.blocks[0]?.body).toBe("hello history");
  });
});

describe("GatewayClient transport", () => {
  it("can use a non-browser raw-message transport", async () => {
    const transport = new FakeGatewayTransport();
    const client = new GatewayClient(transport);

    await client.connect();
    const pending = client.request("thread/list", {});

    expect(JSON.parse(transport.sent[0]!)).toMatchObject({
      jsonrpc: "2.0",
      method: "thread/list",
      params: {}
    });

    transport.emit(JSON.stringify({
      jsonrpc: "2.0",
      id: "1",
      result: {
        sessions: []
      }
    }));

    await expect(pending).resolves.toEqual({ sessions: [] });
  });

  it("rejects pending requests when the transport disconnects", async () => {
    const transport = new FakeGatewayTransport();
    const client = new GatewayClient(transport);

    await client.connect();
    const pending = client.request("thread/list", {});
    transport.disconnect("bridge closed");

    await expect(pending).rejects.toMatchObject({
      code: "disconnected",
      delivery: "unknown",
      message: "bridge closed"
    });
  });

  it("shares concurrent connect work and publishes a transport generation", async () => {
    const transport = new FakeGatewayTransport();
    const client = new GatewayClient(transport);
    const states: string[] = [];
    client.subscribeConnectionState((snapshot) => {
      states.push(`${snapshot.state}:${snapshot.generation}`);
    });

    await Promise.all([client.connect(), client.connect()]);

    expect(transport.connectCalls).toBe(1);
    expect(client.connectionSnapshot()).toMatchObject({
      state: "connected",
      generation: 1
    });
    expect(states).toContain("connecting:0");
    expect(states).toContain("connected:1");
  });

  it("reconnects after a successful generation with capped-policy first delay", async () => {
    vi.useFakeTimers();
    const transport = new FakeGatewayTransport();
    const client = new GatewayClient(transport);
    await client.connect();

    transport.disconnect("bridge closed");
    expect(client.connectionSnapshot()).toMatchObject({
      state: "reconnecting",
      attempt: 1
    });
    expect(transport.connectCalls).toBe(1);

    await vi.advanceTimersByTimeAsync(249);
    expect(transport.connectCalls).toBe(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(transport.connectCalls).toBe(2);
    expect(client.connectionSnapshot()).toMatchObject({
      state: "connected",
      generation: 2
    });
    client.close();
  });

  it("classifies request timeout and abort after send as unknown delivery", async () => {
    vi.useFakeTimers();
    const transport = new FakeGatewayTransport();
    const client = new GatewayClient(transport);
    await client.connect();

    const timedOut = client.request("thread/list", {}, { timeoutMs: 50 });
    const timeoutExpectation = expect(timedOut).rejects.toMatchObject({
      code: "request_timeout",
      delivery: "unknown"
    });
    await vi.advanceTimersByTimeAsync(50);
    await timeoutExpectation;

    const abort = new AbortController();
    const aborted = client.request("thread/list", {}, { signal: abort.signal, timeoutMs: 0 });
    const abortExpectation = expect(aborted).rejects.toMatchObject({
      code: "request_aborted",
      delivery: "unknown"
    });
    abort.abort();
    await abortExpectation;
    expect(transport.sent).toHaveLength(2);
    client.close();
  });

  it("rejects a known disconnected request as not sent", async () => {
    const client = new GatewayClient(new FakeGatewayTransport());
    await expect(client.request("thread/list", {})).rejects.toMatchObject({
      code: "not_connected",
      delivery: "not_sent"
    });
  });

  it("turns malformed frames into a protocol fault and isolates handler failures", async () => {
    vi.useFakeTimers();
    const transport = new FakeGatewayTransport();
    const client = new GatewayClient(transport);
    const diagnostics: string[] = [];
    const observed: string[] = [];
    client.subscribeDiagnostics((diagnostic) => diagnostics.push(diagnostic.kind));
    client.subscribe(() => {
      throw new Error("broken observer");
    });
    client.subscribe((notification) => observed.push(notification.method));
    await client.connect();

    transport.emit(JSON.stringify({ jsonrpc: "2.0", method: "custom/event", params: null }));
    expect(observed).toEqual(["custom/event"]);
    expect(diagnostics).toContain("notification_handler");

    transport.emit("{not-json");
    expect(diagnostics).toContain("protocol");
    expect(client.connectionSnapshot().state).toBe("reconnecting");
    client.close();
  });

  it("sends the sealed Thread Application action, interaction, and history methods", async () => {
    const transport = new FakeGatewayTransport();
    const client = new GatewayClient(transport);
    const scope = scopeForCwd("/tmp/project");
    await client.connect();

    const action = runThreadInterrupt(client, { scope, threadId: "thread-1" });
    expect(JSON.parse(transport.sent.at(-1)!)).toMatchObject({
      method: "thread/action/run",
      params: { action: { kind: "interrupt" }, threadId: "thread-1" }
    });
    transport.emit(JSON.stringify({
      jsonrpc: "2.0",
      id: "1",
      result: { kind: "interrupt", threadId: "thread-1", interrupted: true, cleared: 0 }
    }));
    await expect(action).resolves.toMatchObject({ kind: "interrupt", interrupted: true });

    const interaction = client.request("thread/interaction/respond", {
      interactionId: "permission-1",
      response: { kind: "permission", decision: "allowOnce" },
      scope,
      threadId: "thread-1"
    });
    expect(JSON.parse(transport.sent.at(-1)!)).toMatchObject({
      method: "thread/interaction/respond",
      params: {
        interactionId: "permission-1",
        response: { kind: "permission", decision: "allowOnce" }
      }
    });
    transport.emit(JSON.stringify({
      jsonrpc: "2.0",
      id: "2",
      result: { accepted: true, interactionId: "permission-1", outcome: "accepted" }
    }));
    await expect(interaction).resolves.toMatchObject({ accepted: true, outcome: "accepted" });

    const history = client.request("thread/history/read", {
      cursor: null,
      limit: 20,
      scope,
      threadId: "thread-1"
    });
    expect(JSON.parse(transport.sent.at(-1)!)).toMatchObject({
      method: "thread/history/read",
      params: { cursor: null, limit: 20, threadId: "thread-1" }
    });
    transport.emit(JSON.stringify({
      jsonrpc: "2.0",
      id: "3",
      result: {
        threadId: "thread-1",
        history: { owner: "psychevo", fidelity: "full", cursor: null, hint: null },
        entries: [],
        nextCursor: null
      }
    }));
    await expect(history).resolves.toMatchObject({ entries: [], nextCursor: null });
  });
});

class FakeGatewayTransport implements GatewayTransport {
  readonly sent: string[] = [];
  connectCalls = 0;
  private connected = false;
  private readonly disconnectHandlers = new Set<(message: string) => void>();
  private readonly messageHandlers = new Set<GatewayRawMessageHandler>();

  async connect(): Promise<void> {
    this.connectCalls += 1;
    this.connected = true;
  }

  close(): void {
    this.connected = false;
    this.disconnect("closed");
  }

  onDisconnect(handler: (message: string) => void): () => void {
    this.disconnectHandlers.add(handler);
    return () => this.disconnectHandlers.delete(handler);
  }

  onMessage(handler: GatewayRawMessageHandler): () => void {
    this.messageHandlers.add(handler);
    return () => this.messageHandlers.delete(handler);
  }

  send(data: string): void {
    if (!this.connected) {
      throw new Error("not connected");
    }
    this.sent.push(data);
  }

  emit(data: string): void {
    for (const handler of this.messageHandlers) {
      handler(data);
    }
  }

  disconnect(message: string): void {
    this.connected = false;
    for (const handler of this.disconnectHandlers) {
      handler(message);
    }
  }
}
