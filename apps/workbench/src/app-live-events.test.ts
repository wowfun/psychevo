// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { ThreadSession, emptyThreadSnapshot } from "@psychevo/client";
import { describe, expect, it, vi } from "vitest";
import type { GatewayEvent } from "@psychevo/protocol";
import { EMPTY_GATEWAY_EVENT_FEED } from "./gateway-event-feed";
import { useGatewayLiveEvents } from "./app-live-events";

describe("useGatewayLiveEvents", () => {
  it("keeps the transport event feed separate from ThreadSession reduction", () => {
    const session = new ThreadSession({
      snapshot: emptyThreadSnapshot(scope(), "thread-shared")
    });
    const setLatestGatewayEvent = vi.fn();
    const { result } = renderHook(() => useGatewayLiveEvents({
      selectedThreadIdRef: { current: "thread-shared" },
      setLatestGatewayEvent,
      threadSession: session
    }));
    const event: GatewayEvent = {
      displayTitle: "Updated",
      threadId: "thread-shared",
      title: "Updated",
      type: "titleChanged"
    };

    act(() => result.current.applyGatewayEvent(event));

    expect(setLatestGatewayEvent).toHaveBeenCalledOnce();
    const update = setLatestGatewayEvent.mock.calls[0]?.[0] as (
      current: typeof EMPTY_GATEWAY_EVENT_FEED
    ) => typeof EMPTY_GATEWAY_EVENT_FEED;
    expect(update(EMPTY_GATEWAY_EVENT_FEED).byThread["thread-shared"]?.[0]?.event).toEqual(event);
    expect(session.getSnapshot()?.thread?.id).toBe("thread-shared");
  });

  it("projects ThreadSession identity into the selected-thread ref", () => {
    const session = new ThreadSession({
      snapshot: emptyThreadSnapshot(scope(), "thread-a")
    });
    const selectedThreadIdRef = { current: "thread-a" as string | null };
    renderHook(() => useGatewayLiveEvents({
      selectedThreadIdRef,
      setLatestGatewayEvent: vi.fn(),
      threadSession: session
    }));

    act(() => session.reset(emptyThreadSnapshot(scope(), "thread-b")));

    expect(selectedThreadIdRef.current).toBe("thread-b");
  });
});

function scope() {
  return {
    cwd: "/repo",
    source: {
      kind: "web" as const,
      lifetime: "persistent" as const,
      rawId: "cwd:/repo",
      rawIdentity: null,
      visibleName: null
    }
  };
}
