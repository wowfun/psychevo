import type { GatewayEvent, PendingActionView } from "@psychevo/protocol";

type ActionRequestedEvent = Extract<GatewayEvent, { type: "actionRequested" | "actionUpdated" }>;

export function pendingActionFromEvent(event: ActionRequestedEvent): PendingActionView {
  return event.action;
}

export function upsertPendingInteraction<T extends { actionId: string }>(
  requests: T[],
  request: T
): T[] {
  const next = requests.filter((candidate) => candidate.actionId !== request.actionId);
  next.push(request);
  return next;
}

export function removePendingInteractionsForTurn<T extends { turnId?: string }>(
  requests: T[],
  turnId: string
): T[] {
  return requests.filter((request) => request.turnId !== turnId);
}
