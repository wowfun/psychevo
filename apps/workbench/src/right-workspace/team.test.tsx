// @vitest-environment jsdom

import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { GatewayClient } from "@psychevo/client";
import type { GatewayEvent, TeamStatusResult } from "@psychevo/protocol";
import {
  EMPTY_GATEWAY_EVENT_FEED,
  appendGatewayEventFeed
} from "../gateway-event-feed";
import { TeamPanel } from "./team";

afterEach(cleanup);

describe("TeamPanel refresh scheduling", () => {
  it("ignores transcript churn and refreshes for an agent lifecycle event", async () => {
    const client = teamClient(async () => emptyTeamStatus());
    let feed = EMPTY_GATEWAY_EVENT_FEED;
    const props = panelProps(client, feed);
    const view = render(<TeamPanel {...props} />);

    await waitFor(() => expect(client.request).toHaveBeenCalledTimes(1));
    for (let index = 0; index < 100; index += 1) {
      feed = appendGatewayEventFeed(feed, entryEvent("text", `text-${index}`));
    }
    view.rerender(<TeamPanel {...props} latestGatewayEvent={feed} />);
    await act(async () => Promise.resolve());
    expect(client.request).toHaveBeenCalledTimes(1);

    feed = appendGatewayEventFeed(feed, entryEvent("agent", "agent-start"));
    view.rerender(<TeamPanel {...props} latestGatewayEvent={feed} />);
    await waitFor(() => expect(client.request).toHaveBeenCalledTimes(2));
  });

  it("coalesces concurrent lifecycle refreshes into one trailing request", async () => {
    const second = deferred<TeamStatusResult>();
    const client = teamClient(vi.fn()
      .mockResolvedValueOnce(emptyTeamStatus())
      .mockReturnValueOnce(second.promise)
      .mockResolvedValueOnce(emptyTeamStatus()));
    let feed = EMPTY_GATEWAY_EVENT_FEED;
    const props = panelProps(client, feed);
    const view = render(<TeamPanel {...props} />);
    await waitFor(() => expect(client.request).toHaveBeenCalledTimes(1));

    feed = appendGatewayEventFeed(feed, turnEvent("turnStarted"));
    view.rerender(<TeamPanel {...props} latestGatewayEvent={feed} />);
    await waitFor(() => expect(client.request).toHaveBeenCalledTimes(2));

    feed = appendGatewayEventFeed(feed, turnEvent("turnCompleted"));
    view.rerender(<TeamPanel {...props} latestGatewayEvent={feed} />);
    expect(client.request).toHaveBeenCalledTimes(2);

    await act(async () => {
      second.resolve(emptyTeamStatus());
      await second.promise;
    });
    await waitFor(() => expect(client.request).toHaveBeenCalledTimes(3));
  });
});

function panelProps(client: GatewayClient, latestGatewayEvent: typeof EMPTY_GATEWAY_EVENT_FEED) {
  return {
    client,
    disabled: false,
    latestGatewayEvent,
    nativeActivities: [],
    scope: null,
    threadId: "thread-1",
    onOpenAgentSession: vi.fn()
  };
}

function teamClient(request: (...args: unknown[]) => unknown): GatewayClient {
  return { request: vi.fn(request) } as unknown as GatewayClient;
}

function emptyTeamStatus(): TeamStatusResult {
  return {
    agents: [],
    control: {
      spawningPaused: false,
      maxSpawnDepthCap: 4,
      concurrencyCap: 4
    }
  };
}

function entryEvent(kind: "agent" | "text", id: string): GatewayEvent {
  return {
    type: "entryUpdated",
    turnId: "turn-1",
    entry: {
      id,
      threadId: "thread-1",
      turnId: "turn-1",
      role: "assistant",
      status: "running",
      source: "runtime.stream",
      createdAtMs: 1,
      updatedAtMs: 1,
      blocks: [{
        id: `${id}-block`,
        kind,
        status: "running",
        order: 0,
        source: "runtime",
        title: null,
        body: id,
        artifactIds: [],
        metadata: null,
        result: null,
        createdAtMs: 1,
        updatedAtMs: 1
      }]
    }
  };
}

function turnEvent(type: "turnStarted" | "turnCompleted"): GatewayEvent {
  if (type === "turnStarted") {
    return {
      type,
      threadId: "thread-1",
      turnId: "turn-1",
      selectedSkills: []
    };
  }
  return {
    type,
    threadId: "thread-1",
    turnId: "turn-1",
    turn: {
      id: "turn-1",
      threadId: "thread-1",
      status: "completed",
      startedAtMs: 1,
      completedAtMs: 2,
      error: null
    },
    committedEntries: []
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
