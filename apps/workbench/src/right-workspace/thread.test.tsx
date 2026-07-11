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
      title: "Agent",
      onSubmitThreadTurn: vi.fn(async () => {})
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
      title: "Agent",
      onSubmitThreadTurn: vi.fn(async () => {})
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
        onSubmitThreadTurn={vi.fn(async () => {})}
      />
    );

    fireEvent.click(await screen.findByRole("button", { name: "Open file reports/result.html" }));
    expect(onOpen).toHaveBeenCalledWith("reports/result.html");
  });

  it("uses persisted binding ownership instead of the runtime child event flag", async () => {
    const client = threadClientWithOwnership("readOnly");
    const feed = appendGatewayEventFeed(EMPTY_GATEWAY_EVENT_FEED, {
      type: "runtimeChildChanged",
      runtimeRef: "opencode",
      parentThreadId: "parent-thread",
      threadId: "child-thread",
      dedupKey: "opaque-child",
      status: "running",
      readOnly: false
    });

    render(
      <ThreadPanel
        client={client}
        disabled={false}
        gatewayEventFeed={feed}
        kind="agentSession"
        parentThreadId="parent-thread"
        scope={null}
        threadId="child-thread"
        title="Runtime child"
        onSubmitThreadTurn={vi.fn(async () => {})}
      />
    );

    const panel = screen.getByRole("region", { name: "Runtime child" });
    expect(await within(panel).findByText(/^Read-only runtime child/)).toBeTruthy();
    expect(within(panel).queryByRole("button", { name: "Send message" })).toBeNull();
    expect(within(panel).queryByRole("button", { name: "Interrupt active turn" })).toBeNull();
    expect(client.request).toHaveBeenCalledWith("runtime/context/read", {
      threadId: "child-thread",
      runtimeRef: null,
      scope: null
    });
  });

  it("queues OpenCode managed-member input when turn.steer is not advertised", async () => {
    const client = threadClientWithOwnership("readWrite");
    const onSubmitThreadTurn = vi.fn(async () => {});
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
        onSubmitThreadTurn={onSubmitThreadTurn}
      />
    );

    const composer = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(composer, { target: { value: "queue after the active turn" } });
    expect(screen.queryByRole("button", { name: "Steer" })).toBeNull();
    fireEvent.submit(composer.closest("form") as HTMLFormElement);
    await waitFor(() => {
      expect(onSubmitThreadTurn).toHaveBeenCalledWith(
        "child-thread",
        "queue after the active turn",
        []
      );
    });
    expect(client.request).not.toHaveBeenCalledWith("turn/steer", expect.anything());
  });

  it.each([
    ["full", "Read-only runtime child · Full history"],
    ["summary", "Read-only runtime child · Summary history; only a condensed record is available."],
    ["partial", "Read-only runtime child · Partial history; some messages or detail may be missing."]
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
        onSubmitThreadTurn={vi.fn(async () => {})}
      />
    );

    const historyNotice = await screen.findByRole("note");
    expect(historyNotice.textContent).toBe(notice);
    expect(historyNotice.getAttribute("data-history-fidelity")).toBe(fidelity);
    expect(client.request).toHaveBeenCalledWith("runtime/session/read", {
      runtimeRef: "opencode",
      sessionHandle: "native-child",
      scope: null
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
          backend: { kind: "peerAgent", sessionHandle: "acp-native", runtimeRef: "opencode" }
        }
      : null,
    entries: [],
    activity: {
      ...value.activity,
      activeTurnId: "parent-turn",
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
    turnId: "parent-turn",
    updatedAtMs: 1,
    usage: null
  } satisfies TranscriptEntry);
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

function threadReadClient(result: Promise<ThreadSnapshot>): GatewayClient & { request: ReturnType<typeof vi.fn> } {
  return {
    request: vi.fn(async (method: string) => {
      if (method === "thread/read") {
        return result;
      }
      throw new Error(`unexpected request: ${method}`);
    })
  } as unknown as GatewayClient & { request: ReturnType<typeof vi.fn> };
}

function threadClientWithOwnership(
  ownership: "readOnly" | "readWrite",
  fidelity: "full" | "summary" | "partial" = "full"
): GatewayClient & { request: ReturnType<typeof vi.fn> } {
  return {
    request: vi.fn(async (method: string) => {
      if (method === "thread/read") {
        return childSnapshot();
      }
      if (method === "runtime/context/read") {
        return {
          runtimeRef: "opencode",
          selectionState: "bound",
          profiles: [],
          binding: {
            threadId: "child-thread",
            runtimeRef: "opencode",
            backendKind: "runtime",
            nativeKind: "opencode",
            sessionHandle: "native-child",
            cwd: "/workspace",
            profileFingerprint: "fingerprint",
            ownership,
            bindingRevision: 2
          },
          controls: [],
          activeSession: null
        };
      }
      if (method === "runtime/session/read") {
        return {
          runtimeRef: "opencode",
          sessionHandle: "native-child",
          supported: true,
          changed: false,
          session: {
            sessionHandle: "native-child",
            threadId: "child-thread",
            fidelity,
            ownership,
            actions: ["read"]
          },
          message: null
        };
      }
      throw new Error(`unexpected request: ${method}`);
    })
  } as unknown as GatewayClient & { request: ReturnType<typeof vi.fn> };
}
