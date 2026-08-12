// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { ThreadSession, emptyThreadSnapshot } from "@psychevo/client";
import { describe, expect, it, vi } from "vitest";
import type { GatewayEvent } from "@psychevo/protocol";
import {
  GatewayEventFeedApplication,
  gatewayEventsForThread
} from "./gateway-event-feed";
import { useGatewayLiveEvents } from "./app-live-events";

describe("useGatewayLiveEvents", () => {
  it("keeps the transport event feed separate from ThreadSession reduction", () => {
    const session = new ThreadSession({
      snapshot: emptyThreadSnapshot(scope(), "thread-shared")
    });
    const gatewayEvents = new GatewayEventFeedApplication();
    const { result } = renderHook(() => useGatewayLiveEvents({
      gatewayEvents,
      selectedThreadIdRef: { current: "thread-shared" },
      threadSession: session
    }));
    const event: GatewayEvent = {
      displayTitle: "Updated",
      threadId: "thread-shared",
      title: "Updated",
      type: "titleChanged"
    };

    act(() => result.current.applyGatewayEvent(event));

    expect(
      gatewayEventsForThread(gatewayEvents.getSnapshot(), "thread-shared")[0]?.event
    ).toEqual(event);
    expect(session.getSnapshot()?.thread?.id).toBe("thread-shared");
  });

  it("projects ThreadSession identity into the selected-thread ref", () => {
    const session = new ThreadSession({
      snapshot: emptyThreadSnapshot(scope(), "thread-a")
    });
    const selectedThreadIdRef = { current: "thread-a" as string | null };
    renderHook(() => useGatewayLiveEvents({
      gatewayEvents: new GatewayEventFeedApplication(),
      selectedThreadIdRef,
      threadSession: session
    }));

    act(() => session.reset(emptyThreadSnapshot(scope(), "thread-b")));

    expect(selectedThreadIdRef.current).toBe("thread-b");
  });

  it("projects identity without materializing the committed transcript and live overlay", () => {
    const session = new ThreadSession({
      snapshot: emptyThreadSnapshot(scope(), "thread-a")
    });
    const selectedThreadIdRef = { current: "thread-a" as string | null };
    renderHook(() => useGatewayLiveEvents({
      gatewayEvents: new GatewayEventFeedApplication(),
      selectedThreadIdRef,
      threadSession: session
    }));
    vi.spyOn(session, "getSnapshot").mockImplementation(() => {
      throw new Error("identity projection must not materialize the transcript");
    });

    act(() => session.reset(emptyThreadSnapshot(scope(), "thread-b")));

    expect(selectedThreadIdRef.current).toBe("thread-b");
  });

  it("projects an accepted first-Turn identity instead of the committed snapshot identity", () => {
    const session = new ThreadSession({
      snapshot: emptyThreadSnapshot(scope(), null)
    });
    const selectedThreadIdRef = { current: null as string | null };
    renderHook(() => useGatewayLiveEvents({
      gatewayEvents: new GatewayEventFeedApplication(),
      selectedThreadIdRef,
      threadSession: session
    }));
    vi.spyOn(session, "getActiveThreadId").mockReturnValue("thread-accepted");

    act(() => session.reset(emptyThreadSnapshot(scope(), "thread-committed")));

    expect(selectedThreadIdRef.current).toBe("thread-accepted");
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
