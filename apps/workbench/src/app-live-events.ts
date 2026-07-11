import { useRef, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import {
  applyGatewayEventToThreadSnapshot,
  type GatewayClient
} from "@psychevo/client";
import type {
  GatewayEvent,
  GatewayRequestScope,
  ThreadSnapshot
} from "@psychevo/protocol";
import { applyTurnCompletionQueueBarrier } from "./liveTranscript";
import { appendGatewayEventFeed, type GatewayThreadEventFeed } from "./gateway-event-feed";
import { normalizeSnapshot } from "./session-utils";

export const LIVE_EVENT_REFRESH_SETTLE_MS = 650;

type RefreshSnapshot = (
  nextClient?: GatewayClient | null,
  threadId?: string,
  scope?: GatewayRequestScope,
  readOnly?: boolean,
  expectedEpoch?: number | null,
  allowDetachedAdoption?: boolean
) => Promise<void>;

type GatewayLiveEventsParams = {
  refreshSnapshot: RefreshSnapshot;
  selectedThreadIdRef: MutableRefObject<string | null>;
  setLatestGatewayEvent: Dispatch<SetStateAction<GatewayThreadEventFeed>>;
  setSnapshot: Dispatch<SetStateAction<ThreadSnapshot>>;
  viewEpochRef: MutableRefObject<number>;
};

function pacedGatewayEvent(event: GatewayEvent): boolean {
  return event.type === "entryStarted" ||
    event.type === "entryUpdated" ||
    event.type === "entryCompleted";
}

export function applyWorkbenchGatewayEventSnapshot(
  current: ThreadSnapshot,
  event: GatewayEvent
): ThreadSnapshot {
  return normalizeSnapshot(applyGatewayEventToThreadSnapshot(current, event));
}

export function useGatewayLiveEvents(params: GatewayLiveEventsParams) {
  const gatewayEventQueueRef = useRef<GatewayEvent[]>([]);
  const gatewayEventRafRef = useRef<number | null>(null);

  function scheduleGatewayEventFlush() {
    if (gatewayEventRafRef.current !== null) {
      return;
    }
    gatewayEventRafRef.current = window.requestAnimationFrame(() => {
      gatewayEventRafRef.current = null;
      const event = gatewayEventQueueRef.current.shift();
      if (event) {
        params.setSnapshot((current) => {
          const next = applyWorkbenchGatewayEventSnapshot(current, event);
          params.selectedThreadIdRef.current = next.thread?.id ?? null;
          return next;
        });
      }
      if (gatewayEventQueueRef.current.length > 0) {
        scheduleGatewayEventFlush();
      }
    });
  }

  function publishGatewayEvent(event: GatewayEvent) {
    params.setLatestGatewayEvent((current) => appendGatewayEventFeed(current, event));
  }

  function applyGatewayEvent(event: GatewayEvent) {
    publishGatewayEvent(event);
    if (event.type === "turnCompleted") {
      gatewayEventQueueRef.current = applyTurnCompletionQueueBarrier(gatewayEventQueueRef.current, event);
      params.setSnapshot((current) => {
        const next = applyWorkbenchGatewayEventSnapshot(current, event);
        params.selectedThreadIdRef.current = next.thread?.id ?? null;
        return next;
      });
      return;
    }
    if (!pacedGatewayEvent(event)) {
      params.setSnapshot((current) => {
        const next = applyWorkbenchGatewayEventSnapshot(current, event);
        params.selectedThreadIdRef.current = next.thread?.id ?? null;
        return next;
      });
      return;
    }
    gatewayEventQueueRef.current.push(event);
    scheduleGatewayEventFlush();
  }

  function scheduleSnapshotRefreshAfterLiveSettle(
    nextClient: GatewayClient,
    threadId: string | null,
    epoch = params.viewEpochRef.current
  ) {
    window.setTimeout(() => {
      if (threadId) {
        void params.refreshSnapshot(nextClient, threadId, undefined, true, epoch);
      } else {
        void params.refreshSnapshot(nextClient);
      }
    }, LIVE_EVENT_REFRESH_SETTLE_MS);
  }

  return {
    applyGatewayEvent,
    gatewayEventQueueRef,
    gatewayEventRafRef,
    scheduleSnapshotRefreshAfterLiveSettle
  };
}
