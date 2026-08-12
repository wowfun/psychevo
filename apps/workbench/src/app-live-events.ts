import { useEffect, type MutableRefObject } from "react";
import type { ThreadSession } from "@psychevo/client";
import type { GatewayEvent } from "@psychevo/protocol";
import type { GatewayEventFeedApplication } from "./gateway-event-feed";

type GatewayLiveEventsParams = {
  gatewayEvents: GatewayEventFeedApplication;
  selectedThreadIdRef: MutableRefObject<string | null>;
  threadSession: ThreadSession;
};

export function useGatewayLiveEvents(params: GatewayLiveEventsParams) {
  useEffect(() => params.threadSession.subscribe(() => {
    params.selectedThreadIdRef.current = params.threadSession.getActiveThreadId();
  }), [params.threadSession, params.selectedThreadIdRef]);

  function applyGatewayEvent(event: GatewayEvent) {
    params.gatewayEvents.append(event);
  }

  return { applyGatewayEvent };
}
