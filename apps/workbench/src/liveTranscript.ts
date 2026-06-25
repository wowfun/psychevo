import type { GatewayEvent } from "@psychevo/protocol";

export {
  appendOptimisticPrompt,
  applyLiveTranscriptEvent,
  reconcileThreadSnapshot
} from "@psychevo/client";

type TurnCompletedEvent = Extract<GatewayEvent, { type: "turnCompleted" }>;

export function applyTurnCompletionQueueBarrier(
  queue: GatewayEvent[],
  completion: TurnCompletedEvent
): GatewayEvent[] {
  return queue.filter((event) => !("turnId" in event && event.turnId === completion.turnId));
}
