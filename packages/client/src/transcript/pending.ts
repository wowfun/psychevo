import type {
  GatewayEvent,
  PendingClarifyView,
  PendingPermissionView
} from "@psychevo/protocol";

type PermissionRequestedEvent = Extract<GatewayEvent, { type: "permissionRequested" }>;
type ClarifyRequestedEvent = Extract<GatewayEvent, { type: "clarifyRequested" }>;

export function pendingPermissionFromEvent(event: PermissionRequestedEvent): PendingPermissionView {
  const request: PendingPermissionView = {
    requestId: event.requestId,
    toolName: event.toolName,
    summary: event.summary,
    reason: event.reason,
    allowAlways: event.allowAlways,
    timeoutSecs: event.timeoutSecs
  };
  if (event.matchedRule) {
    request.matchedRule = event.matchedRule;
  }
  if (event.suggestedRule) {
    request.suggestedRule = event.suggestedRule;
  }
  assignPendingContext(request, event);
  return request;
}

export function pendingClarifyFromEvent(event: ClarifyRequestedEvent): PendingClarifyView {
  const request: PendingClarifyView = {
    requestId: event.requestId,
    raw: event.raw
  };
  assignPendingContext(request, event);
  return request;
}

function assignPendingContext(
  request: PendingPermissionView | PendingClarifyView,
  event: PermissionRequestedEvent | ClarifyRequestedEvent
) {
  if (event.threadId) {
    request.threadId = event.threadId;
  }
  if (event.turnId) {
    request.turnId = event.turnId;
  }
  if (event.activityId) {
    request.activityId = event.activityId;
  }
  if (event.sourceKey) {
    request.sourceKey = event.sourceKey;
  }
  if (event.ownerId) {
    request.ownerId = event.ownerId;
  }
  if (event.leaseExpiresAtMs !== undefined) {
    request.leaseExpiresAtMs = event.leaseExpiresAtMs;
  }
}

export function upsertPendingInteraction<T extends { requestId: string }>(
  requests: T[],
  request: T
): T[] {
  const next = requests.filter((candidate) => candidate.requestId !== request.requestId);
  next.push(request);
  return next;
}

export function removePendingInteractionsForTurn<T extends { turnId?: string }>(
  requests: T[],
  turnId: string
): T[] {
  return requests.filter((request) => request.turnId !== turnId);
}
