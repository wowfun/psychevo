// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { GatewayClient, type GatewayRawMessageHandler, type GatewayTransport } from "@psychevo/client";
import { FloatingApp, type FloatingActivation, type FloatingRuntime } from "./FloatingApp";

beforeEach(() => {
  Object.defineProperty(HTMLElement.prototype, "scrollTo", {
    configurable: true,
    value: vi.fn()
  });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  delete (HTMLElement.prototype as Partial<HTMLElement>).scrollTo;
});

describe("FloatingApp Gateway flow", () => {
  it("uses turn/start threadId for the first floating submit", async () => {
    const transport = new TestGatewayTransport();
    render(<FloatingApp runtime={floatingRuntime(transport)} />);

    const ask = await toolbarButton("Ask");
    await waitFor(() => expect((ask as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(ask);

    await screen.findByText("Floating answer ready.");

    expect(transport.methods()).toContain("turn/start");
    expect(transport.methods()).not.toContain("thread/start");
    expect(transport.turnStartParams()[0]).toMatchObject({
      scope: {
        source: {
          kind: "floating",
          lifetime: "process"
        }
      },
      threadId: null
    });
    expect(screen.queryByText("Gateway did not create a floating thread.")).toBeNull();
    expect(screen.queryByText("Gateway bridge is not connected")).toBeNull();
  });

  it("passes shared turn controls into the floating turn/start request", async () => {
    const transport = new TestGatewayTransport();
    const turnControls = vi.fn().mockResolvedValue({
      agentName: "review",
      mode: "plan",
      model: "deepseek/deepseek-chat",
      permissionMode: "ask",
      reasoningEffort: "medium",
      runtimeOptions: {},
      runtimeRef: "native",
      runtimeSessionId: "runtime-session"
    });
    render(<FloatingApp runtime={floatingRuntime(transport, { turnControls })} />);

    const ask = await toolbarButton("Ask");
    await waitFor(() => expect((ask as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(ask);

    await screen.findByText("Floating answer ready.");

    expect(turnControls).toHaveBeenCalledWith(expect.objectContaining({
      threadId: null
    }));
    expect(transport.turnStartParams()[0]).toMatchObject({
      agentName: "review",
      mode: "plan",
      model: "deepseek/deepseek-chat",
      permissionMode: "ask",
      reasoningEffort: "medium",
      runtimeRef: "native",
      runtimeSessionId: "runtime-session"
    });
    expect(screen.queryByText("Working")).toBeNull();
  });

  it("renders logo-only branding without the old pevo label", async () => {
    const transport = new TestGatewayTransport();
    const { container } = render(<FloatingApp runtime={floatingRuntime(transport)} />);

    await toolbarButton("Ask");

    expect(container.querySelector(".pevo-floating-logo")).toBeTruthy();
    expect(screen.queryByText("pevo")).toBeNull();
  });

  it("requests native drag from blank toolbar space without stealing action button clicks", async () => {
    const transport = new TestGatewayTransport();
    const startWindowDrag = vi.fn().mockResolvedValue(undefined);
    const { container } = render(<FloatingApp runtime={floatingRuntime(transport, { startWindowDrag })} />);

    const ask = await toolbarButton("Ask");
    fireEvent.pointerDown(ask, {
      button: 0
    });
    expect(startWindowDrag).not.toHaveBeenCalled();

    const toolbar = container.querySelector(".pevo-floating-capsuleToolbar");
    expect(toolbar).toBeTruthy();
    fireEvent.pointerDown(toolbar!, {
      button: 0
    });

    expect(startWindowDrag).toHaveBeenCalledOnce();
  });

  it("fits the native floating window to capsule content", async () => {
    installResizeObserverStub();
    const transport = new TestGatewayTransport();
    const fitWindowToContent = vi.fn().mockResolvedValue(undefined);
    render(<FloatingApp runtime={floatingRuntime(transport, { fitWindowToContent })} />);

    await toolbarButton("Ask");

    await waitFor(() => expect(fitWindowToContent).toHaveBeenCalled());
    expect(fitWindowToContent).toHaveBeenLastCalledWith({
      height: 48,
      width: window.innerWidth
    });
  });

	  it("renders answers with the shared Transcript markdown rules", async () => {
	    const transport = new TestGatewayTransport("Floating **answer** ready.\n\n```ts\nconst value = 1;\n```");
	    const { container } = render(<FloatingApp runtime={floatingRuntime(transport)} />);

    const ask = await toolbarButton("Ask");
    await waitFor(() => expect((ask as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(ask);

	    await screen.findByText("const value = 1;");
	    expect(container.querySelector(".pevo-transcript")).toBeTruthy();
	    expect(container.querySelector(".pevo-markdown strong")?.textContent).toBe("answer");
	    expect(container.querySelector(".pevo-floating-messageRow")).toBeNull();
	    expect(screen.queryByText("Copy answer")).toBeNull();
	    expect(screen.getAllByRole("button", { name: "Copy message" }).length).toBeGreaterThan(0);
	  });

	  it("closes by hiding the native floating window without leaving a logo fallback", async () => {
	    const transport = new TestGatewayTransport();
	    const closeFloatingWindow = vi.fn().mockResolvedValue(undefined);
	    const { container } = render(<FloatingApp runtime={floatingRuntime(transport, { closeFloatingWindow })} />);

	    await toolbarButton("Ask");
	    fireEvent.click(closeButton(container));

	    await waitFor(() => expect(closeFloatingWindow).toHaveBeenCalledOnce());
	    expect(container.querySelector(".pevo-floating-capsule")).toBeNull();
	    expect(container.querySelector(".pevo-floating-parkedButton")).toBeNull();
	  });

	  it("keeps Park as the recoverable logo state", async () => {
	    const transport = new TestGatewayTransport();
	    const closeFloatingWindow = vi.fn().mockResolvedValue(undefined);
	    const { container } = render(<FloatingApp runtime={floatingRuntime(transport, { closeFloatingWindow })} />);

	    await toolbarButton("Ask");
	    fireEvent.click(parkButton(container));

	    const parked = container.querySelector(".pevo-floating-parkedButton");
	    expect(parked).toBeTruthy();
	    expect(parked?.getAttribute("aria-label")).toBe("Restore Floating");
	    expect(closeFloatingWindow).not.toHaveBeenCalled();
	  });

  it("opens the accepted thread in Workbench without closing Floating", async () => {
    const transport = new TestGatewayTransport();
    const openThreadInWorkbench = vi.fn().mockResolvedValue(undefined);
    render(<FloatingApp runtime={floatingRuntime(transport, { openThreadInWorkbench })} />);

    const ask = await toolbarButton("Ask");
    await waitFor(() => expect((ask as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(ask);
    await screen.findByText("Floating answer ready.");

    fireEvent.click(screen.getByRole("button", { name: "Open in main window" }));

    expect(openThreadInWorkbench).toHaveBeenCalledWith("thread-floating");
    expect(screen.getByText("Floating answer ready.")).toBeTruthy();
  });

  it("renders first-submit live transcript events that arrive before turn/start resolves", async () => {
    const transport = new PreResponseLiveGatewayTransport();
    render(<FloatingApp runtime={floatingRuntime(transport)} />);

    const ask = await toolbarButton("Ask");
    await waitFor(() => expect((ask as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(ask);

    await screen.findByText("Streaming before acceptance.");
    expect(screen.queryByText("Working")).toBeNull();

    transport.resolveTurnStart();

    await waitFor(() => expect(transport.turnStartResolved()).toBe(true));
    expect(screen.getByText("Streaming before acceptance.")).toBeTruthy();

    transport.emitStaleEntry();
    expect(screen.queryByText("Stale turn should not render.")).toBeNull();
  });
});

async function toolbarButton(name: string): Promise<HTMLButtonElement> {
  const toolbar = await screen.findByRole("toolbar", { name: "Floating actions" });
  return within(toolbar).getByRole("button", { name }) as HTMLButtonElement;
}

function closeButton(container: HTMLElement): HTMLButtonElement {
  const button = container.querySelector<HTMLButtonElement>('button[title="Close"]');
  expect(button).toBeTruthy();
  return button!;
}

function parkButton(container: HTMLElement): HTMLButtonElement {
  const button = container.querySelector<HTMLButtonElement>('button[title="Park"]');
  expect(button).toBeTruthy();
  return button!;
}

function installResizeObserverStub(): void {
  class TestResizeObserver implements ResizeObserver {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  }
  vi.stubGlobal("ResizeObserver", TestResizeObserver);
}

function floatingRuntime(
  transport: GatewayTransport,
  overrides: Partial<FloatingRuntime> = {}
): FloatingRuntime {
  return {
    async captureSelection() {
      return activation("rescan");
    },
    async connectGateway() {
      const client = new GatewayClient(transport);
      await client.connect();
      return client;
    },
    async initialActivation() {
      return activation("initial");
    },
    locale: "English",
    ...overrides
  };
}

function activation(label: string): FloatingActivation {
  return {
    activationId: `activation-${label}`,
    anchor: { height: 20, width: 220, x: 100, y: 80 },
    attachments: [{
      bounds: { height: 20, width: 220, x: 100, y: 80 },
      id: `selection-${label}`,
      kind: "textSelection",
      name: "Selected text",
      preview: "selected context",
      sourceApp: "Test",
      text: "selected context",
      visibleToModel: true
    }],
    cwd: "/repo"
  };
}

class TestGatewayTransport implements GatewayTransport {
  private connected = false;
  private readonly disconnectHandlers = new Set<(message: string) => void>();
  private readonly messageHandlers = new Set<GatewayRawMessageHandler>();
  private readonly requests: Array<{ id: string; method: string; params?: Record<string, unknown> }> = [];
  private turnCount = 0;

  constructor(private readonly finalAnswer = "Floating answer ready.") {}

  async connect(): Promise<void> {
    this.connected = true;
  }

  close(): void {
    this.connected = false;
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
    const request = JSON.parse(data) as { id: string; method: string; params?: Record<string, unknown> };
    this.requests.push(request);
    if (request.method === "thread/start") {
      this.respond(request.id, {
        activity: { activeTurnId: null, queuedTurns: 0, running: false },
        entries: [],
        pendingActions: [],
        scope: request.params?.scope,
        source: (request.params?.scope as { source?: unknown } | undefined)?.source ?? null,
        thread: null
      });
      return;
    }
    if (request.method === "turn/start") {
      const turnId = `turn-${++this.turnCount}`;
      const completedAtMs = Date.now();
      this.respond(request.id, { accepted: true, threadId: "thread-floating" });
      window.setTimeout(() => {
        this.emit({
          jsonrpc: "2.0",
          method: "turn/result",
          params: {
            committedEntries: [transcriptEntry("assistant", `assistant:${turnId}`, this.finalAnswer, turnId, completedAtMs)],
            result: { finalAnswer: this.finalAnswer },
            thread: {
              backend: { kind: "psychevo", sessionHandle: "thread-floating" },
              id: "thread-floating",
              sourceKey: null
            },
            turn: {
              completedAtMs,
              error: null,
              id: turnId,
              outcome: "completed",
              startedAtMs: completedAtMs - 1,
              status: "completed",
              threadId: "thread-floating"
            }
          }
        });
      }, 0);
      return;
    }
    this.respond(request.id, {});
  }

  methods(): string[] {
    return this.requests.map((request) => request.method);
  }

  turnStartParams(): Array<Record<string, unknown> | undefined> {
    return this.requests
      .filter((request) => request.method === "turn/start")
      .map((request) => request.params);
  }

  private respond(id: string, result: unknown): void {
    window.setTimeout(() => {
      this.emit({ id, jsonrpc: "2.0", result });
    }, 0);
  }

  private emit(message: unknown): void {
    const raw = JSON.stringify(message);
    for (const handler of this.messageHandlers) {
      handler(raw);
    }
  }
}

class PreResponseLiveGatewayTransport implements GatewayTransport {
  private connected = false;
  private readonly disconnectHandlers = new Set<(message: string) => void>();
  private readonly messageHandlers = new Set<GatewayRawMessageHandler>();
  private pendingRequestId: string | null = null;
  private resolved = false;

  async connect(): Promise<void> {
    this.connected = true;
  }

  close(): void {
    this.connected = false;
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
    const request = JSON.parse(data) as { id: string; method: string };
    if (request.method !== "turn/start") {
      this.respond(request.id, {});
      return;
    }

    this.pendingRequestId = request.id;
    window.setTimeout(() => {
      this.emit({
        jsonrpc: "2.0",
        method: "gateway/event",
        params: {
          selectedSkills: [],
          threadId: null,
          turnId: "turn-live",
          type: "turnStarted"
        }
      });
      this.emit({
        jsonrpc: "2.0",
        method: "gateway/event",
        params: {
          entry: transcriptEntry(
            "assistant",
            "assistant:live",
            "Streaming before acceptance.",
            "turn-live"
          ),
          turnId: "turn-live",
          type: "entryUpdated"
        }
      });
    }, 0);
  }

  resolveTurnStart(): void {
    if (!this.pendingRequestId) {
      throw new Error("turn/start was not requested");
    }
    this.resolved = true;
    this.respond(this.pendingRequestId, { accepted: true, threadId: "thread-floating" });
  }

  turnStartResolved(): boolean {
    return this.resolved;
  }

  emitStaleEntry(): void {
    this.emit({
      jsonrpc: "2.0",
      method: "gateway/event",
      params: {
        entry: transcriptEntry(
          "assistant",
          "assistant:stale",
          "Stale turn should not render.",
          "turn-stale"
        ),
        turnId: "turn-stale",
        type: "entryUpdated"
      }
    });
  }

  private respond(id: string, result: unknown): void {
    window.setTimeout(() => {
      this.emit({ id, jsonrpc: "2.0", result });
    }, 0);
  }

  private emit(message: unknown): void {
    const raw = JSON.stringify(message);
    for (const handler of this.messageHandlers) {
      handler(raw);
    }
  }
}

function transcriptEntry(
  role: "assistant" | "user",
  id: string,
  body: string,
  turnId = "turn",
  updatedAtMs = Date.now()
) {
  return {
    accounting: null,
    blocks: [{
      artifactIds: [],
      body,
      createdAtMs: updatedAtMs,
      detail: body,
      id: `${id}:text`,
      kind: "text" as const,
      metadata: null,
      order: 0,
      preview: body.slice(0, 240),
      result: null,
      source: "test",
      status: "completed" as const,
      title: null,
      updatedAtMs
    }],
    createdAtMs: updatedAtMs,
    id,
    messageSeq: null,
    metadata: null,
    role,
    source: "test",
    status: "completed" as const,
    threadId: "thread-floating",
    turnId,
    updatedAtMs,
    usage: null
  };
}
