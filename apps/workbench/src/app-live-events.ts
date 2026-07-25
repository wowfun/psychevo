import { useEffect, useRef, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import type { ThreadSession } from "@psychevo/client";
import type { GatewayEvent } from "@psychevo/protocol";
import { appendGatewayEventFeed, type GatewayThreadEventFeed } from "./gateway-event-feed";

type GatewayLiveEventsParams = {
  selectedThreadIdRef: MutableRefObject<string | null>;
  setLatestGatewayEvent: Dispatch<SetStateAction<GatewayThreadEventFeed>>;
  threadSession: ThreadSession;
};

export function useGatewayLiveEvents(params: GatewayLiveEventsParams) {
  // Kept as empty compatibility refs until the connection effect no longer
  // owns transport cleanup. Event pacing and reduction live in ThreadSession.
  const gatewayEventQueueRef = useRef<GatewayEvent[]>([]);
  const gatewayEventRafRef = useRef<number | null>(null);

  useEffect(() => params.threadSession.subscribe(() => {
    params.selectedThreadIdRef.current =
      params.threadSession.getSnapshot()?.thread?.id ?? null;
  }), [params.threadSession, params.selectedThreadIdRef]);

  function applyGatewayEvent(event: GatewayEvent) {
    params.setLatestGatewayEvent((current) => appendGatewayEventFeed(current, event));
  }

  return {
    applyGatewayEvent,
    gatewayEventQueueRef,
    gatewayEventRafRef
  };
}
