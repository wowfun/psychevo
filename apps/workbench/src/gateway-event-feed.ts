import {
  applyLiveTranscriptEvent,
  reconcileThreadSnapshot
} from "@psychevo/client";
import type { GatewayEvent, ThreadSnapshot } from "@psychevo/protocol";

export type GatewayEventFeedItem = {
  event: GatewayEvent;
  seq: number;
};

type GatewayActionThread = {
  seq: number;
  threadId: string;
  turnId: string | null;
};

type JournalRecord = GatewayEventFeedItem & {
  threadId: string | null;
};

type ActionLifecycleRecord = GatewayEventFeedItem & {
  threadId: string;
};

export type GatewayThreadEventFeed = {
  journal: GatewayEventJournal | null;
  latestSeq: number;
};

export type GatewayEventFamily = "all" | "teamLifecycle" | "turnLifecycle";

const MAX_EVENTS_PER_THREAD = 500;
const MAX_EVENTS_TOTAL = 2_000;
const MAX_ACTION_LIFECYCLES = 2_000;

export const EMPTY_GATEWAY_EVENT_FEED: GatewayThreadEventFeed = {
  journal: null,
  latestSeq: 0
};

export class GatewayEventFeedApplication {
  private readonly journal = new GatewayEventJournal();
  private readonly subscriptions = new Set<{
    family: GatewayEventFamily;
    listener: () => void;
  }>();
  private snapshot: GatewayThreadEventFeed = {
    journal: this.journal,
    latestSeq: 0
  };

  getSnapshot = (): GatewayThreadEventFeed => this.snapshot;

  subscribe(
    listener: () => void,
    family: GatewayEventFamily = "all"
  ): () => void {
    const subscription = { family, listener };
    this.subscriptions.add(subscription);
    return () => this.subscriptions.delete(subscription);
  }

  append(event: GatewayEvent): void {
    this.snapshot = {
      journal: this.journal,
      latestSeq: this.journal.append(event)
    };
    for (const subscription of this.subscriptions) {
      if (eventBelongsToFamily(event, subscription.family)) {
        subscription.listener();
      }
    }
  }
}

export class GatewayEventJournal {
  private readonly actionLifecycles = new Map<string, ActionLifecycleRecord>();
  private readonly actionThreads = new Map<string, GatewayActionThread>();
  private readonly global = new Ring<JournalRecord>(MAX_EVENTS_TOTAL);
  private readonly perThread = new Map<string, Ring<GatewayEventFeedItem>>();
  private readonly teamLifecycleSequences = new Map<string, number>();
  private latestSeq = 0;

  append(event: GatewayEvent): number {
    const seq = ++this.latestSeq;
    const threadId = gatewayEventThreadId(event) ?? this.rememberedActionThreadId(event);
    this.updateActionLifecycle(event, threadId, seq);
    this.updateActionThreads(event, threadId, seq);
    const item = { event, seq };
    const evicted = this.global.push({ ...item, threadId });
    if (evicted?.threadId) {
      const ring = this.perThread.get(evicted.threadId);
      ring?.removeOldest(evicted.seq);
      if (ring?.length === 0) {
        this.perThread.delete(evicted.threadId);
        this.teamLifecycleSequences.delete(evicted.threadId);
      }
    }
    if (threadId) {
      let ring = this.perThread.get(threadId);
      if (!ring) {
        ring = new Ring(MAX_EVENTS_PER_THREAD);
        this.perThread.set(threadId, ring);
      }
      ring.push(item);
      if (isTeamLifecycleEvent(event)) {
        this.teamLifecycleSequences.set(threadId, seq);
      }
    }
    return seq;
  }

  eventsThrough(latestSeq: number): GatewayEventFeedItem[] {
    return this.global.valuesThrough(latestSeq);
  }

  eventsForThread(threadId: string, latestSeq: number): GatewayEventFeedItem[] {
    return this.perThread.get(threadId)?.valuesThrough(latestSeq) ?? [];
  }

  actionLifecyclesForThread(
    threadId: string,
    afterSeq: number,
    latestSeq: number
  ): GatewayEventFeedItem[] {
    return [...this.actionLifecycles.values()]
      .filter((item) => (
        item.threadId === threadId
        && item.seq > afterSeq
        && item.seq <= latestSeq
      ))
      .sort((left, right) => left.seq - right.seq);
  }

  teamLifecycleRevision(threadId: string, latestSeq: number): number {
    return Math.min(this.teamLifecycleSequences.get(threadId) ?? 0, latestSeq);
  }

  private rememberedActionThreadId(event: GatewayEvent): string | null {
    switch (event.type) {
      case "actionRequested":
      case "actionUpdated":
        return this.actionThreads.get(event.action.actionId)?.threadId ?? null;
      case "actionResolved":
      case "actionCancelled":
        return this.actionThreads.get(event.actionId)?.threadId ?? null;
      default:
        return null;
    }
  }

  private updateActionThreads(
    event: GatewayEvent,
    threadId: string | null,
    seq: number
  ): void {
    if (event.type === "actionResolved" || event.type === "actionCancelled") {
      this.actionThreads.delete(event.actionId);
      return;
    }
    if (event.type === "turnCompleted") {
      for (const [actionId, action] of this.actionThreads) {
        if (actionSettledByTurn(action, event, threadId)) {
          this.actionThreads.delete(actionId);
        }
      }
      return;
    }
    if ((event.type !== "actionRequested" && event.type !== "actionUpdated") || !threadId) {
      return;
    }
    this.actionThreads.delete(event.action.actionId);
    this.actionThreads.set(event.action.actionId, {
      seq,
      threadId,
      turnId: event.action.turnId ?? null
    });
    while (this.actionThreads.size > MAX_EVENTS_TOTAL) {
      const oldest = this.actionThreads.keys().next().value;
      if (typeof oldest !== "string") break;
      this.actionThreads.delete(oldest);
    }
  }

  private updateActionLifecycle(
    event: GatewayEvent,
    threadId: string | null,
    seq: number
  ): void {
    if (event.type === "turnCompleted") {
      for (const [actionId, action] of this.actionThreads) {
        if (!actionSettledByTurn(action, event, threadId)) continue;
        this.rememberActionLifecycle(actionId, {
          event,
          seq,
          threadId: action.threadId
        });
      }
      return;
    }
    if (!threadId) return;
    const actionId = event.type === "actionRequested" || event.type === "actionUpdated"
      ? event.action.actionId
      : event.type === "actionResolved" || event.type === "actionCancelled"
        ? event.actionId
        : null;
    if (!actionId) return;
    this.rememberActionLifecycle(actionId, { event, seq, threadId });
  }

  private rememberActionLifecycle(actionId: string, record: ActionLifecycleRecord): void {
    this.actionLifecycles.delete(actionId);
    this.actionLifecycles.set(actionId, record);
    while (this.actionLifecycles.size > MAX_ACTION_LIFECYCLES) {
      const oldest = this.actionLifecycles.keys().next().value;
      if (typeof oldest !== "string") break;
      this.actionLifecycles.delete(oldest);
    }
  }
}

function actionSettledByTurn(
  action: GatewayActionThread,
  event: Extract<GatewayEvent, { type: "turnCompleted" }>,
  threadId: string | null
): boolean {
  const sameThread = threadId === null || action.threadId === threadId;
  const sameTurn = action.turnId === event.turnId
    || (threadId !== null && action.turnId === null);
  return sameThread && sameTurn;
}

export function appendGatewayEventFeed(
  current: GatewayThreadEventFeed,
  event: GatewayEvent
): GatewayThreadEventFeed {
  const journal = current.journal ?? new GatewayEventJournal();
  return {
    journal,
    latestSeq: journal.append(event)
  };
}

export function gatewayEventsForThread(
  feed: GatewayThreadEventFeed,
  threadId: string | null
): GatewayEventFeedItem[] {
  return threadId && feed.journal
    ? feed.journal.eventsForThread(threadId, feed.latestSeq)
    : [];
}

export function reconcileThreadSnapshotAfterGatewayBarrier(
  current: ThreadSnapshot,
  incoming: ThreadSnapshot,
  feed: GatewayThreadEventFeed,
  threadId: string,
  barrierSeq: number
): ThreadSnapshot {
  let next = reconcileThreadSnapshot(current, incoming);
  const retainedTurnCompletions = gatewayEventsForThread(feed, threadId)
    .filter(({ event, seq }) => seq > barrierSeq && event.type === "turnCompleted");
  const retainedActionLifecycles = feed.journal?.actionLifecyclesForThread(
    threadId,
    barrierSeq,
    feed.latestSeq
  ) ?? [];
  const lifecycle = [...retainedTurnCompletions, ...retainedActionLifecycles]
    .sort((left, right) => left.seq - right.seq);
  for (const { event } of lifecycle) {
    if (!changesPendingActions(event)) {
      continue;
    }
    const reduced = applyLiveTranscriptEvent(next, event);
    if (reduced.pendingActions !== next.pendingActions) {
      next = { ...next, pendingActions: reduced.pendingActions };
    }
  }
  return next;
}

export function teamLifecycleRevision(
  feed: GatewayThreadEventFeed,
  threadId: string | null
): number {
  return threadId && feed.journal
    ? feed.journal.teamLifecycleRevision(threadId, feed.latestSeq)
    : 0;
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
      event.type === "turnStarted"
      || event.type === "turnQueued"
      || event.type === "turnCompleted"
    ));
  if (!lifecycle) {
    return snapshotActiveTurnId;
  }
  return lifecycle.type === "turnStarted" ? lifecycle.turnId : null;
}

function gatewayEventThreadId(event: GatewayEvent): string | null {
  switch (event.type) {
    case "turnStarted":
    case "turnQueued":
      return event.threadId || null;
    case "turnCompleted":
      return event.threadId
        || event.turn.threadId
        || event.committedEntries.find((entry) => entry.threadId)?.threadId
        || null;
    case "entryStarted":
    case "entryUpdated":
    case "entryCompleted":
      return event.entry.threadId || null;
    case "entryBlockTextDelta":
      return event.threadId || null;
    case "activityChanged":
    case "titleChanged":
      return event.threadId || null;
    case "actionRequested":
    case "actionUpdated":
      return event.action.threadId || null;
    default:
      return null;
  }
}

function isTeamLifecycleEvent(event: GatewayEvent): boolean {
  switch (event.type) {
    case "turnStarted":
    case "turnCompleted":
    case "activityChanged":
      return true;
    case "entryStarted":
    case "entryUpdated":
    case "entryCompleted":
      return event.entry.blocks.some((block) => block.kind === "agent");
    default:
      return false;
  }
}

function changesPendingActions(event: GatewayEvent): boolean {
  return event.type === "actionRequested"
    || event.type === "actionUpdated"
    || event.type === "actionResolved"
    || event.type === "actionCancelled"
    || event.type === "turnCompleted";
}

function eventBelongsToFamily(event: GatewayEvent, family: GatewayEventFamily): boolean {
  if (family === "all") return true;
  if (family === "teamLifecycle") return isTeamLifecycleEvent(event);
  return event.type === "turnStarted"
    || event.type === "turnQueued"
    || event.type === "turnCompleted";
}

class Ring<T extends { seq: number }> {
  private readonly buffer: Array<T | undefined>;
  private head = 0;
  private size = 0;

  constructor(private readonly capacity: number) {
    this.buffer = new Array(capacity);
  }

  get length(): number {
    return this.size;
  }

  push(value: T): T | null {
    if (this.size < this.capacity) {
      this.buffer[(this.head + this.size) % this.capacity] = value;
      this.size += 1;
      return null;
    }
    const evicted = this.buffer[this.head] ?? null;
    this.buffer[this.head] = value;
    this.head = (this.head + 1) % this.capacity;
    return evicted;
  }

  removeOldest(seq: number): void {
    const oldest = this.buffer[this.head];
    if (!oldest || oldest.seq !== seq) return;
    this.buffer[this.head] = undefined;
    this.head = (this.head + 1) % this.capacity;
    this.size -= 1;
  }

  valuesThrough(latestSeq: number): T[] {
    const values: T[] = [];
    for (let index = 0; index < this.size; index += 1) {
      const value = this.buffer[(this.head + index) % this.capacity];
      if (value && value.seq <= latestSeq) values.push(value);
    }
    return values;
  }
}
