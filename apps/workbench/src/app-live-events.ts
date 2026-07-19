import { useRef, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import { type ThreadController } from "@psychevo/client";
import type { GatewayEvent } from "@psychevo/protocol";
import { applyTurnCompletionQueueBarrier } from "./liveTranscript";
import { appendGatewayEventFeed, type GatewayThreadEventFeed } from "./gateway-event-feed";

type GatewayLiveEventsParams = {
  selectedThreadIdRef: MutableRefObject<string | null>;
  setLatestGatewayEvent: Dispatch<SetStateAction<GatewayThreadEventFeed>>;
  threadController: ThreadController;
};

function pacedGatewayEvent(event: GatewayEvent): boolean {
  return event.type === "entryStarted" ||
    event.type === "entryUpdated" ||
    event.type === "entryCompleted";
}

function enqueuePacedGatewayEvent(queue: GatewayEvent[], event: GatewayEvent): void {
  if (event.type === "entryUpdated") {
    const existing = queue.findIndex((candidate) => (
      candidate.type === "entryUpdated"
      && candidate.turnId === event.turnId
      && candidate.entry.id === event.entry.id
    ));
    if (existing >= 0) {
      queue[existing] = event;
      return;
    }
  }
  queue.push(event);
}

function recordJourneyDiagnostic(id: string, data: Record<string, unknown>): void {
  if (typeof window === "undefined" || typeof CustomEvent === "undefined") return;
  if (!(window as Window & { __psychevoJourneyDiagnosticsEnabled?: boolean })
    .__psychevoJourneyDiagnosticsEnabled) return;
  window.dispatchEvent(new CustomEvent("psychevo:journey-diagnostic", {
    detail: { data, id }
  }));
}

function journeyEventTurnId(event: GatewayEvent): string | null {
  return "turnId" in event && typeof event.turnId === "string" ? event.turnId : null;
}

function hasNonEmptyAssistantText(event: GatewayEvent): boolean {
  if (
    event.type !== "entryStarted"
    && event.type !== "entryUpdated"
    && event.type !== "entryCompleted"
  ) {
    return false;
  }
  return event.entry.role === "assistant" && event.entry.blocks.some((block) => (
    block.kind === "text"
    && [block.body, block.preview, block.detail].some((value) => (
      typeof value === "string" && Boolean(value.trim())
    ))
  ));
}

export function useGatewayLiveEvents(params: GatewayLiveEventsParams) {
  const gatewayEventQueueRef = useRef<GatewayEvent[]>([]);
  const gatewayEventRafRef = useRef<number | null>(null);
  const firstNonEmptyAssistantAppliedRef = useRef(new Set<string>());

  function recordApplication(
    event: GatewayEvent,
    application: ReturnType<ThreadController["applyGatewayEvent"]>
  ) {
    if (application.applied) {
      params.selectedThreadIdRef.current = application.snapshot?.thread?.id ?? null;
    }
    const turnId = journeyEventTurnId(event);
    if (
      application.applied
      && turnId
      && hasNonEmptyAssistantText(event)
      && !firstNonEmptyAssistantAppliedRef.current.has(turnId)
    ) {
      firstNonEmptyAssistantAppliedRef.current.add(turnId);
      recordJourneyDiagnostic("controller_first_nonempty_assistant_applied", {
        eventType: event.type,
        turnId
      });
    }
    if (event.type === "turnCompleted") {
      recordJourneyDiagnostic("turn_completed_applied", {
        applied: application.applied,
        queueDepth: gatewayEventQueueRef.current.length,
        turnId: event.turnId
      });
    }
  }

  function reduceGatewayEvent(event: GatewayEvent) {
    recordApplication(event, params.threadController.applyGatewayEvent(event));
  }

  function reduceGatewayEvents(events: GatewayEvent[]) {
    const applications = params.threadController.applyGatewayEvents(events);
    events.forEach((event, index) => recordApplication(event, applications[index]!));
  }

  function scheduleGatewayEventFlush() {
    if (gatewayEventRafRef.current !== null) {
      return;
    }
    gatewayEventRafRef.current = window.requestAnimationFrame(() => {
      gatewayEventRafRef.current = null;
      const batch = gatewayEventQueueRef.current.splice(0);
      reduceGatewayEvents(batch);
      for (const event of batch) {
        recordJourneyDiagnostic("frontend_queue_applied", {
          eventType: event.type,
          queueDepth: gatewayEventQueueRef.current.length,
          turnId: journeyEventTurnId(event)
        });
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
      if (gatewayEventQueueRef.current.length === 0 && gatewayEventRafRef.current !== null) {
        window.cancelAnimationFrame(gatewayEventRafRef.current);
        gatewayEventRafRef.current = null;
      }
      reduceGatewayEvent(event);
      return;
    }
    if (!pacedGatewayEvent(event)) {
      reduceGatewayEvent(event);
      return;
    }
    const turnId = journeyEventTurnId(event);
    if (
      turnId
      && hasNonEmptyAssistantText(event)
      && !firstNonEmptyAssistantAppliedRef.current.has(turnId)
    ) {
      reduceGatewayEvent(event);
      return;
    }
    enqueuePacedGatewayEvent(gatewayEventQueueRef.current, event);
    recordJourneyDiagnostic("frontend_queue_enqueued", {
      eventType: event.type,
      queueDepth: gatewayEventQueueRef.current.length,
      turnId: journeyEventTurnId(event)
    });
    scheduleGatewayEventFlush();
  }

  return {
    applyGatewayEvent,
    gatewayEventQueueRef,
    gatewayEventRafRef
  };
}
