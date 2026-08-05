import { afterEach, describe, expect, expectTypeOf, it, vi } from "vitest";
import type {
  ThreadHistoryDraftReadParams,
  ThreadHistoryDraftReadResult,
  WorkspaceCreateParams,
  WorkspaceCreateResult
} from "@psychevo/protocol";
import {
  GatewayClient,
  GatewayClientError,
  gatewayScopeKey,
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

  it("keeps required fields required while allowing default-only params to be omitted", () => {
    const client = null as unknown as GatewayClient;
    if (false) {
      void client.request("thread/list");
      // @ts-expect-error thread/read requires threadId.
      void client.request("thread/read", {});
      // @ts-expect-error turn/start requires scope and clientTurnId.
      void client.request("turn/start", { input: [] });
      // @ts-expect-error thread/action/run requires scope, threadId, and action.
      void client.request("thread/action/run", {});
    }
    expect(true).toBe(true);
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

describe("gatewayScopeKey", () => {
  it("uses only canonical source identity fields", () => {
    const base = scopeForCwd("/tmp/project");
    const decorated = {
      ...base,
      source: {
        ...base.source,
        rawIdentity: { volatile: "transport-owned" },
        visibleName: "Renamed workspace"
      }
    };

    expect(gatewayScopeKey(base)).toBe(gatewayScopeKey(decorated));
    expect(gatewayScopeKey(null)).toBe("");
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
      turnStartReceipts: [],
      entries: []
    });

    expect(parsed.entries).toEqual([]);
    expect(parsed.history).toEqual({ owner: "psychevo", fidelity: "full", cursor: null, hint: null });
    expect(parsed.activity).toEqual({ running: false, activeTurnId: null, queuedTurns: 0 });
    expect(parsed.pendingActions).toEqual([]);
  });

  it("requires durable turn-start receipts in every snapshot", () => {
    expect(() => parseThreadSnapshot({
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
      activity: { running: false, activeTurnId: null, queuedTurns: 0 },
      pendingActions: []
    })).toThrow(/turnStartReceipts/);
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
      turnStartReceipts: [],
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
      turnStartReceipts: [],
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

  it("preserves JSON-RPC error code and data as an acknowledged server failure", async () => {
    const transport = new FakeGatewayTransport();
    const client = new GatewayClient(transport);
    await client.connect();

    const pending = client.request("thread/list", {});
    transport.emit(JSON.stringify({
      jsonrpc: "2.0",
      id: "1",
      error: {
        code: -32042,
        message: "Thread scope is stale",
        data: { currentRevision: "revision-2" }
      }
    }));

    const error = await pending.catch((failure: unknown) => failure);
    expect(error).toBeInstanceOf(GatewayClientError);
    expect(error).toMatchObject({
      code: "server_error",
      data: { currentRevision: "revision-2" },
      delivery: "acknowledged",
      kind: "server",
      message: "Thread scope is stale",
      rpcCode: -32042
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

  it("can connect again after close interrupts an in-flight connection", async () => {
    const transport = new DeferredGatewayTransport();
    const client = new GatewayClient(transport);

    const first = client.connect();
    const firstRejection = expect(first).rejects.toMatchObject({
      code: "connect_failed",
      delivery: "not_sent"
    });
    client.close();
    await firstRejection;
    expect(client.connectionSnapshot().state).toBe("closed");

    const second = client.connect();
    expect(transport.connectCalls).toBe(2);
    transport.resolveLatestConnect();
    await expect(second).resolves.toBeUndefined();
    expect(client.connectionSnapshot()).toMatchObject({
      state: "connected",
      generation: 1
    });
  });

  it("isolates an immediate connection subscriber failure", () => {
    const client = new GatewayClient(new FakeGatewayTransport());
    const diagnostics: Array<{ kind: string; message: string }> = [];
    client.subscribeDiagnostics((diagnostic) => diagnostics.push(diagnostic));

    expect(() => client.subscribeConnectionState(() => {
      throw new Error(`immediate:${"x".repeat(1_100)}`);
    })).not.toThrow();

    expect(diagnostics).toHaveLength(1);
    expect(diagnostics[0]?.kind).toBe("connection_handler");
    expect(diagnostics[0]?.message.length).toBe(1_000);
  });

  it("continues connection state delivery when an update subscriber fails", async () => {
    const transport = new FakeGatewayTransport();
    const client = new GatewayClient(transport);
    const states: string[] = [];
    const diagnostics: string[] = [];
    client.subscribeDiagnostics((diagnostic) => diagnostics.push(diagnostic.kind));
    client.subscribeConnectionState((snapshot) => {
      if (snapshot.state !== "idle") {
        throw new Error("broken connection observer");
      }
    });
    client.subscribeConnectionState((snapshot) => states.push(snapshot.state));

    await expect(client.connect()).resolves.toBeUndefined();

    expect(client.connectionSnapshot().state).toBe("connected");
    expect(states).toEqual(["idle", "connecting", "connected"]);
    expect(diagnostics).toEqual(["connection_handler", "connection_handler"]);
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

  it.each([
    "thread/read",
    "turn/start",
    "thread/action/run"
  ] as const)("rejects a semantically invalid %s result", async (method) => {
    const transport = new FakeGatewayTransport();
    const client = new GatewayClient(transport);
    await client.connect();

    const pending = method === "thread/read"
      ? client.request(method, { threadId: "thread-1" })
      : method === "turn/start"
        ? client.request(method, {
            scope: scopeForCwd("/tmp/project"),
            clientTurnId: "client-turn-1",
            input: []
          })
        : client.request(method, {
            scope: scopeForCwd("/tmp/project"),
            threadId: "thread-1",
            action: { kind: "interrupt" }
          });
    transport.emit(JSON.stringify({
      jsonrpc: "2.0",
      id: "1",
      result: {}
    }));

    await expect(pending).rejects.toMatchObject({
      code: "protocol_fault",
      delivery: "unknown"
    });
    client.close();
  });

  it("enforces an object boundary for explicitly opaque results", async () => {
    const transport = new FakeGatewayTransport();
    const client = new GatewayClient(transport);
    await client.connect();

    const pending = client.request("plugin/list", {});
    transport.emit(JSON.stringify({
      jsonrpc: "2.0",
      id: "1",
      result: []
    }));

    await expect(pending).rejects.toMatchObject({
      code: "protocol_fault",
      delivery: "unknown"
    });
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

class DeferredGatewayTransport implements GatewayTransport {
  connectCalls = 0;
  private latestResolve: (() => void) | null = null;

  connect(): Promise<void> {
    this.connectCalls += 1;
    return new Promise((resolve) => {
      this.latestResolve = resolve;
    });
  }

  close(): void {}

  onDisconnect(_handler: (message: string) => void): () => void {
    return () => undefined;
  }

  onMessage(_handler: GatewayRawMessageHandler): () => void {
    return () => undefined;
  }

  send(_data: string): void {}

  resolveLatestConnect(): void {
    this.latestResolve?.();
    this.latestResolve = null;
  }
}
