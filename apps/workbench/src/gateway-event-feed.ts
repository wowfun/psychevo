import type { GatewayEvent } from "@psychevo/protocol";

export type GatewayEventFeedItem = {
  event: GatewayEvent;
  seq: number;
};

type GatewayActionThread = {
  seq: number;
  threadId: string;
  turnId: string | null;
};

export type GatewayThreadEventFeed = {
  actionThreads: Record<string, GatewayActionThread>;
  byThread: Record<string, GatewayEventFeedItem[]>;
  latestSeq: number;
};

const MAX_EVENTS_PER_THREAD = 500;
const MAX_EVENTS_TOTAL = 2_000;

export const EMPTY_GATEWAY_EVENT_FEED: GatewayThreadEventFeed = {
  actionThreads: {},
  byThread: {},
  latestSeq: 0
};

export function appendGatewayEventFeed(
  current: GatewayThreadEventFeed,
  event: GatewayEvent
): GatewayThreadEventFeed {
  const seq = current.latestSeq + 1;
  const threadId = gatewayEventThreadId(event) ?? rememberedActionThreadId(current, event);
  const actionThreads = updateActionThreads(current.actionThreads, event, threadId, seq);
  if (!threadId) {
    return { ...current, actionThreads, latestSeq: seq };
  }
  const records = [
    ...(current.byThread[threadId] ?? []),
    { event, seq }
  ].slice(-MAX_EVENTS_PER_THREAD);
  const byThread = pruneOldestEvents({
    ...current.byThread,
    [threadId]: records
  });
  return { actionThreads, byThread, latestSeq: seq };
}

export function gatewayEventsForThread(
  feed: GatewayThreadEventFeed,
  threadId: string | null
): GatewayEventFeedItem[] {
  return threadId ? feed.byThread[threadId] ?? [] : [];
}

export function confirmedSteerTurnId(
  feed: GatewayThreadEventFeed,
  threadId: string | null,
  snapshotActiveTurnId: string | null
): string | null {
  if (!threadId) {
    return null;
  }
  const lifecycle = [...gatewayEventsForThread(feed, threadId)]
    .reverse()
    .map(({ event }) => event)
    .find((event) => (
      event.type === "turnStarted" ||
      event.type === "turnQueued" ||
      event.type === "turnCompleted"
    ));
  if (!lifecycle) {
    return snapshotActiveTurnId;
  }
  return lifecycle.type === "turnStarted" ? lifecycle.turnId : null;
}

export function gatewayEventThreadId(event: GatewayEvent): string | null {
  switch (event.type) {
    case "turnStarted":
    case "turnQueued":
      return event.threadId || null;
    case "turnCompleted":
      return event.threadId ||
        event.turn.threadId ||
        event.committedEntries.find((entry) => entry.threadId)?.threadId ||
        null;
    case "entryStarted":
    case "entryUpdated":
    case "entryCompleted":
      return event.entry.threadId || null;
    case "activityChanged":
    case "titleChanged":
      return event.threadId || null;
    case "runtimeChildChanged":
      return event.threadId || null;
    case "actionRequested":
    case "actionUpdated":
      return event.action.threadId || null;
    default:
      return null;
  }
}

function rememberedActionThreadId(
  feed: GatewayThreadEventFeed,
  event: GatewayEvent
): string | null {
  switch (event.type) {
    case "actionRequested":
    case "actionUpdated":
      return feed.actionThreads[event.action.actionId]?.threadId ?? null;
    case "actionResolved":
    case "actionCancelled":
      return feed.actionThreads[event.actionId]?.threadId ?? null;
    default:
      return null;
  }
}

function updateActionThreads(
  current: Record<string, GatewayActionThread>,
  event: GatewayEvent,
  threadId: string | null,
  seq: number
): Record<string, GatewayActionThread> {
  if (event.type === "actionResolved" || event.type === "actionCancelled") {
    if (!current[event.actionId]) {
      return current;
    }
    const next = { ...current };
    delete next[event.actionId];
    return next;
  }
  if (event.type === "turnCompleted") {
    const remaining = Object.entries(current).filter(([, action]) => {
      const sameThread = threadId === null || action.threadId === threadId;
      const sameTurn = action.turnId === event.turnId || (threadId !== null && action.turnId === null);
      return !(sameThread && sameTurn);
    });
    return remaining.length === Object.keys(current).length
      ? current
      : Object.fromEntries(remaining);
  }
  if ((event.type !== "actionRequested" && event.type !== "actionUpdated") || !threadId) {
    return current;
  }
  const next = { ...current };
  delete next[event.action.actionId];
  next[event.action.actionId] = {
    seq,
    threadId,
    turnId: event.action.turnId ?? null
  };
  const actions = Object.entries(next);
  if (actions.length <= MAX_EVENTS_TOTAL) {
    return next;
  }
  return Object.fromEntries(
    actions
      .sort(([, left], [, right]) => left.seq - right.seq)
      .slice(-MAX_EVENTS_TOTAL)
  );
}

function pruneOldestEvents(
  byThread: Record<string, GatewayEventFeedItem[]>
): Record<string, GatewayEventFeedItem[]> {
  const allRecords = Object.entries(byThread)
    .flatMap(([threadId, records]) => records.map((record) => ({ record, threadId })))
    .sort((left, right) => left.record.seq - right.record.seq);
  const removeCount = Math.max(0, allRecords.length - MAX_EVENTS_TOTAL);
  if (removeCount === 0) {
    return byThread;
  }
  const removed = new Set(allRecords.slice(0, removeCount).map(({ record }) => record.seq));
  return Object.fromEntries(
    Object.entries(byThread)
      .map(([threadId, records]) => [
        threadId,
        records.filter((record) => !removed.has(record.seq))
      ] as const)
      .filter(([, records]) => records.length > 0)
  );
}
