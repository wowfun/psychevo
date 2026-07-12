// @vitest-environment jsdom

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { GatewayClient } from "@psychevo/client";
import type { GatewayEvent, ThreadSnapshot, TranscriptEntry } from "@psychevo/protocol";
import { eventWithEntry, snapshot } from "../liveTranscript.test-support";
import { setupComponentFallbackTests, transcriptBlock } from "../componentFallbacks.test-support";
import {
  EMPTY_GATEWAY_EVENT_FEED,
  appendGatewayEventFeed
} from "../gateway-event-feed";
import { ThreadPanel } from "./thread";

setupComponentFallbackTests();

describe("ThreadPanel live event feed", () => {
  it("replays every child event received while its snapshot loads and continues streaming", async () => {
    const pending = deferred<ThreadSnapshot>();
    const client = {
      request: vi.fn(async (method: string) => {
        if (method === "thread/read") {
          return pending.promise;
        }
        if (method === "thread/history/read") {
          return historyReadResult(await pending.promise);
        }
        if (method === "thread/context/read") {
          return threadContext("readOnly", "full");
        }
        throw new Error(`unexpected request: ${method}`);
      })
    } as unknown as GatewayClient;
    let feed = EMPTY_GATEWAY_EVENT_FEED;
    const props = {
      client,
      disabled: false,
      gatewayEventFeed: feed,
      kind: "agentSession" as const,
      parentThreadId: "parent-thread",
      scope: null,
      threadId: "child-thread",
      title: "Agent"
    };
    const view = render(<ThreadPanel {...props} />);

    feed = appendGatewayEventFeed(feed, childEntryEvent("child-reasoning", "Thinking before open", "reasoning"));
    feed = appendGatewayEventFeed(feed, childEntryEvent("child-answer", "First child chunk", "text"));
    feed = appendGatewayEventFeed(feed, childEntryEvent("parent-answer", "Must stay in parent", "text", "parent-thread"));
    view.rerender(<ThreadPanel {...props} gatewayEventFeed={feed} />);

    await act(async () => {
      pending.resolve(childSnapshot());
      await pending.promise;
    });

    expect(await screen.findByText("Thinking before open")).toBeTruthy();
    expect(screen.getByText("First child chunk")).toBeTruthy();
    expect(screen.queryByText("Must stay in parent")).toBeNull();

    feed = appendGatewayEventFeed(feed, childEntryEvent("child-answer", "First child chunk and future text", "text"));
    view.rerender(<ThreadPanel {...props} gatewayEventFeed={feed} />);

    await waitFor(() => expect(screen.getByText("First child chunk and future text")).toBeTruthy());
    expect(screen.queryByText("First child chunk")).toBeNull();
  });

  it("keeps the newest thread snapshot when an older client read resolves last", async () => {
    const olderRead = deferred<ThreadSnapshot>();
    const newerRead = deferred<ThreadSnapshot>();
    const olderClient = threadReadClient(olderRead.promise);
    const newerClient = threadReadClient(newerRead.promise);
    const props = {
      client: olderClient,
      disabled: false,
      gatewayEventFeed: EMPTY_GATEWAY_EVENT_FEED,
      kind: "agentSession" as const,
      parentThreadId: "parent-thread",
      scope: null,
      threadId: "child-thread",
      title: "Agent"
    };
    const view = render(<ThreadPanel {...props} />);

    await waitFor(() => expect(olderClient.request).toHaveBeenCalledWith("thread/read", {
      threadId: "child-thread"
    }));
    view.rerender(<ThreadPanel {...props} client={newerClient} />);
    await waitFor(() => expect(newerClient.request).toHaveBeenCalledWith("thread/read", {
      threadId: "child-thread"
    }));

    await act(async () => {
      newerRead.resolve(childSnapshotWithText("Newest snapshot"));
      await newerRead.promise;
    });
    expect(await screen.findByText("Newest snapshot")).toBeTruthy();

    await act(async () => {
      olderRead.resolve(childSnapshotWithText("Stale snapshot"));
      await olderRead.promise;
    });
    expect(screen.queryByText("Stale snapshot")).toBeNull();
    expect(screen.getByText("Newest snapshot")).toBeTruthy();
  });

  it("opens workspace file paths from a child assistant transcript", async () => {
    const onOpen = vi.fn();
    render(
      <ThreadPanel
        client={threadReadClient(Promise.resolve(childSnapshotWithText("Open reports/result.html now")))}
        disabled={false}
        gatewayEventFeed={EMPTY_GATEWAY_EVENT_FEED}
        kind="agentSession"
        parentThreadId="parent-thread"
        scope={null}
        threadId="child-thread"
        title="Agent"
        workspaceFileLinks={{
          entries: [
            { depth: 1, kind: "file", name: "result.html", path: "reports/result.html" }
          ],
          onOpen,
          root: "/workspace/project"
        }}
      />
    );

    fireEvent.click(await screen.findByRole("button", { name: "Open file reports/result.html" }));
    expect(onOpen).toHaveBeenCalledWith("reports/result.html");
  });

  it("uses persisted binding ownership for a public child thread", async () => {
    const client = threadClientWithOwnership("readOnly");

    render(
      <ThreadPanel
        client={client}
        disabled={false}
        gatewayEventFeed={EMPTY_GATEWAY_EVENT_FEED}
        kind="agentSession"
        parentThreadId="parent-thread"
        scope={null}
        threadId="child-thread"
        title="Runtime child"
      />
    );

    const panel = screen.getByRole("region", { name: "Runtime child" });
    expect(await within(panel).findByText(/^Read-only runtime child/)).toBeTruthy();
    expect(within(panel).queryByRole("button", { name: "Send message" })).toBeNull();
    expect(within(panel).queryByRole("button", { name: "Interrupt active turn" })).toBeNull();
    expect(client.request).toHaveBeenCalledWith("thread/context/read", {
      threadId: "child-thread",
      target: null,
      scope: childSnapshot().scope
    });
  });

  it("queues OpenCode managed-member input when turn.steer is not advertised", async () => {
    const client = threadClientWithOwnership("readWrite");
    render(
      <ThreadPanel
        client={client}
        disabled={false}
        gatewayEventFeed={EMPTY_GATEWAY_EVENT_FEED}
        kind="agentSession"
        parentThreadId="parent-thread"
        scope={null}
        threadId="child-thread"
        title="Writable OpenCode member"
      />
    );

    const composer = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(composer, { target: { value: "queue after the active turn" } });
    expect(screen.queryByRole("button", { name: "Steer" })).toBeNull();
    fireEvent.submit(composer.closest("form") as HTMLFormElement);
    await waitFor(() => {
      expect(client.request).toHaveBeenCalledWith("turn/start", expect.objectContaining({
        input: [{ type: "text", text: "queue after the active turn" }],
        threadId: "child-thread"
      }));
    });
    expect(client.request).not.toHaveBeenCalledWith("thread/action/run", expect.anything());
  });

  it("keeps one prompt owner while a writable child follow-up commits before turn acceptance", async () => {
    const turnStart = deferred<Record<string, unknown>>();
    const initial = childHistorySnapshot();
    let current = initial;
    const client = {
      request: vi.fn(async (method: string) => {
        if (method === "thread/read") return current;
        if (method === "thread/history/read") return historyReadResult(current);
        if (method === "thread/context/read") return threadContext("readWrite", "full");
        if (method === "turn/start") return turnStart.promise;
        throw new Error(`unexpected request: ${method}`);
      })
    } as unknown as GatewayClient & { request: ReturnType<typeof vi.fn> };
    let feed = EMPTY_GATEWAY_EVENT_FEED;
    const props = {
      client,
      disabled: false,
      gatewayEventFeed: feed,
      kind: "agentSession" as const,
      parentThreadId: "parent-thread",
      scope: null,
      threadId: "child-thread",
      title: "Writable child"
    };
    const view = render(<ThreadPanel {...props} />);
    const panel = screen.getByRole("region", { name: "Writable child" });

    expect(await within(panel).findByText("Main Agent instruction")).toBeTruthy();
    const composer = within(panel).getByPlaceholderText("Ask Psychevo...");
    fireEvent.change(composer, { target: { value: "你有哪些工具" } });
    fireEvent.submit(composer.closest("form") as HTMLFormElement);

    await waitFor(() => expect(client.request).toHaveBeenCalledWith(
      "turn/start",
      expect.objectContaining({
        input: [{ type: "text", text: "你有哪些工具" }],
        threadId: "child-thread"
      })
    ));
    expect(within(panel).getAllByText("Main Agent instruction")).toHaveLength(1);
    expect(within(panel).getAllByText("你有哪些工具")).toHaveLength(1);

    feed = appendGatewayEventFeed(feed, {
      type: "turnStarted",
      threadId: "child-thread",
      turnId: "turn-follow-up",
      selectedSkills: []
    });
    feed = appendGatewayEventFeed(feed, eventWithEntry("entryUpdated", committedChildEntry(
      "message:3",
      3,
      "user",
      "你有哪些工具",
      "turn-follow-up"
    )));
    feed = appendGatewayEventFeed(feed, eventWithEntry("entryUpdated", {
      ...committedChildEntry(
        "live:turn-follow-up:assistant:0",
        null,
        "assistant",
        "Second answer streaming",
        "turn-follow-up"
      ),
      source: "runtime.stream",
      status: "running",
      blocks: [transcriptBlock({
        id: "live:turn-follow-up:assistant:0:text:0",
        body: "Second answer streaming",
        detail: "Second answer streaming",
        preview: "Second answer streaming",
        source: "runtime.stream",
        status: "running"
      })]
    }));
    const committedUser = committedChildEntry(
      "message:3",
      3,
      "user",
      "你有哪些工具",
      "turn-follow-up"
    );
    const committedAssistant = committedChildEntry(
      "message:4",
      4,
      "assistant",
      "Second answer complete",
      "turn-follow-up"
    );
    feed = appendGatewayEventFeed(feed, {
      type: "turnCompleted",
      threadId: "child-thread",
      turnId: "turn-follow-up",
      turn: {
        id: "turn-follow-up",
        threadId: "child-thread",
        status: "completed",
        outcome: "normal",
        error: null,
        startedAtMs: 10,
        completedAtMs: 20
      },
      committedEntries: [committedUser, committedAssistant]
    });
    current = {
      ...initial,
      entries: [
        ...initial.entries,
        committedUser,
        committedAssistant
      ],
      activity: { activeTurnId: null, queuedTurns: 0, running: false }
    };
    view.rerender(<ThreadPanel {...props} gatewayEventFeed={feed} />);

    await waitFor(() => expect(within(panel).getByText("Second answer complete")).toBeTruthy());
    expect(within(panel).getAllByText("Main Agent instruction")).toHaveLength(1);
    expect(within(panel).getAllByText("你有哪些工具")).toHaveLength(1);
    expect(panel.querySelectorAll('[data-entry-id="message:1"]')).toHaveLength(1);
    expect(panel.querySelectorAll('[data-entry-id="message:3"]')).toHaveLength(1);
    expect(panel.querySelectorAll('[data-entry-id^="optimistic:"]')).toHaveLength(0);

    await act(async () => {
      turnStart.resolve({
        accepted: true,
        threadId: "child-thread",
        turnId: "turn-follow-up",
        thread: initial.thread
      });
      await turnStart.promise;
    });
    expect(within(panel).getAllByText("你有哪些工具")).toHaveLength(1);
    expect(panel.querySelectorAll('[data-entry-id^="optimistic:"]')).toHaveLength(0);
  });

  it("rolls back the visible child prompt when turn/start is rejected", async () => {
    const turnStart = deferred<Record<string, unknown>>();
    const initial = childHistorySnapshot();
    const client = {
      request: vi.fn(async (method: string) => {
        if (method === "thread/read") return initial;
        if (method === "thread/history/read") return historyReadResult(initial);
        if (method === "thread/context/read") return threadContext("readWrite", "full");
        if (method === "turn/start") return turnStart.promise;
        throw new Error(`unexpected request: ${method}`);
      })
    } as unknown as GatewayClient;
    render(
      <ThreadPanel
        client={client}
        disabled={false}
        gatewayEventFeed={EMPTY_GATEWAY_EVENT_FEED}
        kind="agentSession"
        parentThreadId="parent-thread"
        scope={null}
        threadId="child-thread"
        title="Rejected child"
      />
    );
    const panel = screen.getByRole("region", { name: "Rejected child" });
    const composer = await within(panel).findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(composer, { target: { value: "rejected follow-up" } });
    fireEvent.submit(composer.closest("form") as HTMLFormElement);

    await waitFor(() => expect(within(panel).getAllByText("rejected follow-up")).toHaveLength(1));
    await act(async () => {
      turnStart.reject(new Error("turn rejected"));
      try {
        await turnStart.promise;
      } catch {
        // ThreadPanel owns the rejection and rolls back its optimistic prompt.
      }
    });

    expect(await within(panel).findByText("turn rejected")).toBeTruthy();
    expect(within(panel).queryByText("rejected follow-up")).toBeNull();
    expect(within(panel).getAllByText("Main Agent instruction")).toHaveLength(1);
  });

  it("refreshes authoritative child history and idle activity after an empty terminal slice", async () => {
    const initial: ThreadSnapshot = {
      ...childSnapshot(),
      entries: [
        committedChildEntry("message:1", 1, "user", "first question", "turn-1"),
        {
          ...committedChildEntry(
            "live:turn-1:assistant:0",
            null,
            "assistant",
            "first answer streaming",
            "turn-1"
          ),
          source: "runtime.stream",
          status: "running",
          metadata: { liveOrder: 0, projection: "assistant_segment", streamSeq: 1 }
        }
      ],
      activity: {
        activeTurnId: "stale-local-turn",
        queuedTurns: 0,
        running: true,
        startedAtMs: 100
      }
    };
    const authoritative: ThreadSnapshot = {
      ...childSnapshot(),
      entries: [
        committedChildEntry("message:1", 1, "user", "first question", "turn-1"),
        committedChildEntry("message:2", 2, "assistant", "first answer committed", "turn-1")
      ],
      activity: { activeTurnId: null, queuedTurns: 0, running: false }
    };
    let current = initial;
    const client = {
      request: vi.fn(async (method: string) => {
        if (method === "thread/read") return current;
        if (method === "thread/history/read") return historyReadResult(current);
        if (method === "thread/context/read") return threadContext("readWrite", "full");
        if (method === "turn/start") {
          return {
            accepted: true,
            threadId: "child-thread",
            turnId: "turn-2",
            thread: authoritative.thread
          };
        }
        throw new Error(`unexpected request: ${method}`);
      })
    } as unknown as GatewayClient & { request: ReturnType<typeof vi.fn> };
    let feed = EMPTY_GATEWAY_EVENT_FEED;
    const props = {
      client,
      disabled: false,
      gatewayEventFeed: feed,
      kind: "agentSession" as const,
      parentThreadId: "parent-thread",
      scope: null,
      threadId: "child-thread",
      title: "Settling child"
    };
    const view = render(<ThreadPanel {...props} />);
    const panel = screen.getByRole("region", { name: "Settling child" });

    expect(await within(panel).findByText("first answer streaming")).toBeTruthy();
    current = authoritative;
    feed = appendGatewayEventFeed(feed, {
      type: "turnCompleted",
      threadId: "child-thread",
      turnId: "turn-1",
      turn: {
        id: "turn-1",
        threadId: "child-thread",
        status: "completed",
        outcome: "normal",
        error: null,
        startedAtMs: 100,
        completedAtMs: 200
      },
      committedEntries: []
    });
    view.rerender(<ThreadPanel {...props} gatewayEventFeed={feed} />);

    await waitFor(() => expect(client.request.mock.calls.filter(([method]) => (
      method === "thread/read"
    ))).toHaveLength(2));
    expect(await within(panel).findByText("first answer committed")).toBeTruthy();
    expect(within(panel).queryByText("first answer streaming")).toBeNull();
    expect(panel.querySelectorAll('[data-entry-id="message:2"]')).toHaveLength(1);
    expect(within(panel).queryByRole("button", { name: "Interrupt active turn" })).toBeNull();

    const composer = within(panel).getByPlaceholderText("Ask Psychevo...");
    fireEvent.change(composer, { target: { value: "second question" } });
    fireEvent.submit(composer.closest("form") as HTMLFormElement);
    await waitFor(() => expect(within(panel).getByText("second question")).toBeTruthy());
    const orderedEntryIds = Array.from(panel.querySelectorAll("[data-entry-id]"), (node) => (
      node.getAttribute("data-entry-id") ?? ""
    ));
    expect(orderedEntryIds.slice(0, 2)).toEqual(["message:1", "message:2"]);
    expect(orderedEntryIds[2]?.startsWith("optimistic:")).toBe(true);
  });

  it("keeps the retained child transcript visible when terminal refresh fails", async () => {
    const initial: ThreadSnapshot = {
      ...childSnapshot(),
      entries: [
        committedChildEntry("message:1", 1, "user", "first question", "turn-1"),
        {
          ...committedChildEntry(
            "live:turn-1:assistant:0",
            null,
            "assistant",
            "retained first answer",
            "turn-1"
          ),
          source: "runtime.stream",
          metadata: { liveOrder: 0, projection: "assistant_segment", streamSeq: 1 }
        }
      ],
      activity: {
        activeTurnId: "turn-1",
        queuedTurns: 0,
        running: true,
        startedAtMs: 100
      }
    };
    let reads = 0;
    const client = {
      request: vi.fn(async (method: string) => {
        if (method === "thread/read") {
          reads += 1;
          if (reads > 1) throw new Error("authoritative refresh failed");
          return initial;
        }
        if (method === "thread/history/read") return historyReadResult(initial);
        if (method === "thread/context/read") return threadContext("readWrite", "full");
        throw new Error(`unexpected request: ${method}`);
      })
    } as unknown as GatewayClient;
    let feed = EMPTY_GATEWAY_EVENT_FEED;
    const props = {
      client,
      disabled: false,
      gatewayEventFeed: feed,
      kind: "agentSession" as const,
      parentThreadId: "parent-thread",
      scope: null,
      threadId: "child-thread",
      title: "Refresh failure child"
    };
    const view = render(<ThreadPanel {...props} />);
    const panel = screen.getByRole("region", { name: "Refresh failure child" });
    expect(await within(panel).findByText("retained first answer")).toBeTruthy();

    feed = appendGatewayEventFeed(feed, {
      type: "turnCompleted",
      threadId: "child-thread",
      turnId: "turn-1",
      turn: {
        id: "turn-1",
        threadId: "child-thread",
        status: "completed",
        outcome: "normal",
        error: null,
        startedAtMs: 100,
        completedAtMs: 200
      },
      committedEntries: []
    });
    view.rerender(<ThreadPanel {...props} gatewayEventFeed={feed} />);

    expect(await within(panel).findByText("authoritative refresh failed")).toBeTruthy();
    expect(within(panel).getByText("retained first answer")).toBeTruthy();
  });

  it.each([
    ["full", "Read-only runtime child · Full history"],
    ["summary", "Read-only runtime child · Summary history; only a condensed record is available."],
    ["partial", "Read-only runtime child · Partial history; some messages or detail may be missing."],
    ["unavailable", "Read-only runtime child · History unavailable; earlier messages could not be restored."]
  ] as const)("keeps %s runtime-child history fidelity visible after lazy read", async (fidelity, notice) => {
    const client = threadClientWithOwnership("readOnly", fidelity);
    render(
      <ThreadPanel
        client={client}
        disabled={false}
        gatewayEventFeed={EMPTY_GATEWAY_EVENT_FEED}
        historyFidelity={fidelity === "summary" ? "partial" : fidelity}
        kind="agentSession"
        parentThreadId="parent-thread"
        scope={null}
        threadId="child-thread"
        title={`${fidelity} runtime child`}
      />
    );

    const historyNotice = await screen.findByRole("note");
    expect(historyNotice.textContent).toBe(notice);
    expect(historyNotice.getAttribute("data-history-fidelity")).toBe(fidelity);
    expect(client.request).toHaveBeenCalledWith("thread/context/read", {
      threadId: "child-thread",
      target: null,
      scope: childSnapshot().scope
    });
    expect(client.request).toHaveBeenCalledWith("thread/read", {
      threadId: "child-thread"
    });
  });
});

function childSnapshot(): ThreadSnapshot {
  const value = snapshot();
  return {
    ...value,
    thread: value.thread
      ? {
          ...value.thread,
          id: "child-thread",
          backend: { kind: "acp", sessionHandle: "acp-native", runtimeRef: "opencode" }
        }
      : null,
    entries: [],
    activity: {
      ...value.activity,
      activeTurnId: "child-turn",
      running: true
    }
  };
}

function childSnapshotWithText(body: string): ThreadSnapshot {
  const value = childSnapshot();
  const event = childEntryEvent(`snapshot-${body}`, body, "text");
  if (!("entry" in event)) {
    throw new Error("expected transcript entry event");
  }
  return {
    ...value,
    entries: [event.entry]
  };
}

function childHistorySnapshot(): ThreadSnapshot {
  return {
    ...childSnapshot(),
    entries: [
      committedChildEntry("message:1", 1, "user", "Main Agent instruction", "turn-initial"),
      committedChildEntry("message:2", 2, "assistant", "Initial answer", "turn-initial")
    ],
    activity: {
      activeTurnId: null,
      queuedTurns: 0,
      running: false
    }
  };
}

function committedChildEntry(
  id: string,
  messageSeq: number | null,
  role: "user" | "assistant",
  body: string,
  turnId: string
): TranscriptEntry {
  return {
    accounting: null,
    blocks: [transcriptBlock({
      id: `${id}:text`,
      body,
      detail: body,
      preview: body,
      source: "runtime.message",
      status: "completed"
    })],
    createdAtMs: messageSeq ?? 10,
    id,
    messageSeq,
    metadata: messageSeq === null ? { liveOrder: 0, streamSeq: 1 } : null,
    role,
    source: "runtime.message",
    status: "completed",
    threadId: "child-thread",
    turnId,
    updatedAtMs: messageSeq ?? 10,
    usage: null
  };
}

function childEntryEvent(
  id: string,
  body: string,
  kind: "reasoning" | "text",
  threadId = "child-thread"
): GatewayEvent {
  return eventWithEntry("entryUpdated", {
    accounting: null,
    blocks: [transcriptBlock({
      id: `${id}:block`,
      body,
      detail: body,
      kind,
      preview: body,
      source: "runtime.stream",
      status: "running"
    })],
    createdAtMs: 1,
    id,
    messageSeq: null,
    metadata: null,
    role: "assistant",
    source: "runtime.stream",
    status: "running",
    threadId,
    turnId: "child-turn",
    updatedAtMs: 1,
    usage: null
  } satisfies TranscriptEntry);
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((next, fail) => {
    resolve = next;
    reject = fail;
  });
  return { promise, reject, resolve };
}

function threadReadClient(result: Promise<ThreadSnapshot>): GatewayClient & { request: ReturnType<typeof vi.fn> } {
  return {
    request: vi.fn(async (method: string) => {
      if (method === "thread/read") {
        return result;
      }
      if (method === "thread/history/read") {
        return historyReadResult(await result);
      }
      if (method === "thread/context/read") {
        return threadContext("readOnly", "full");
      }
      throw new Error(`unexpected request: ${method}`);
    })
  } as unknown as GatewayClient & { request: ReturnType<typeof vi.fn> };
}

function threadClientWithOwnership(
  ownership: "readOnly" | "readWrite",
  fidelity: "full" | "summary" | "partial" | "unavailable" = "full"
): GatewayClient & { request: ReturnType<typeof vi.fn> } {
  return {
    request: vi.fn(async (method: string) => {
      if (method === "thread/read") {
        return {
          ...childSnapshot(),
          history: { owner: "agent", fidelity, cursor: null, hint: null }
        };
      }
      if (method === "thread/history/read") {
        return historyReadResult({
          ...childSnapshot(),
          history: { owner: "agent", fidelity, cursor: null, hint: null }
        });
      }
      if (method === "thread/context/read") {
        return threadContext(ownership, fidelity);
      }
      if (method === "turn/start") {
        return {
          accepted: true,
          threadId: "child-thread",
          turnId: "queued-turn",
          thread: childSnapshot().thread
        };
      }
      throw new Error(`unexpected request: ${method}`);
    })
  } as unknown as GatewayClient & { request: ReturnType<typeof vi.fn> };
}

function historyReadResult(value: ThreadSnapshot) {
  return {
    threadId: "child-thread",
    history: value.history,
    entries: value.entries,
    nextCursor: null
  };
}

function threadContext(
  ownership: "readOnly" | "readWrite",
  fidelity: "full" | "summary" | "partial" | "unavailable"
) {
  return {
    targetId: "target:opencode-bound",
    runtimeProfileRef: "opencode",
    selectionState: "bound",
    profiles: [],
    binding: {
      threadId: "child-thread",
      agentRef: null,
      agentFingerprint: "agent-fingerprint",
      runtimeRef: "opencode",
      backendKind: "acp",
      nativeKind: null,
      sessionHandle: "native-child",
      cwd: "/workspace",
      profileFingerprint: "fingerprint",
      ownership,
      bindingRevision: 2
    },
    controls: [],
    stability: "stable",
    capabilities: [],
    compatibleTargets: [{
      targetId: "target:opencode-bound",
      agentRef: null,
      runtimeProfileRef: "opencode",
      agentLabel: "OpenCode",
      profileLabel: "OpenCode (ACP)",
      label: "OpenCode · OpenCode (ACP)",
      ready: true,
      unavailableReason: null
    }],
    inputCapabilities: [{ kind: "text", enabled: true, unavailableReason: null }],
    actions: [],
    sendability: { allowed: ownership === "readWrite", reason: null, recoveryAction: null },
    history: { owner: "agent", fidelity, cursor: null, hint: null },
    pendingInteractions: [],
    contextRevision: "1",
    controlRevision: "1"
  };
}
