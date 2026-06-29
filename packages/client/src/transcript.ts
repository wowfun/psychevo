import type {
  GatewayEvent,
  ThreadSnapshot,
  TranscriptBlock,
  TranscriptEntry
} from "@psychevo/protocol";

import { enrichCommittedAgentTargetsFromLive } from "./transcript/agent-targets";
import {
  OPTIMISTIC_LIVE_ORDER,
  OPTIMISTIC_SOURCE,
  artifactIdsForBlock,
  blocksForEntry,
  blocksForValue,
  compactText,
  entryHasVisibleTranscriptText,
  entryStatusForBlocks,
  hashText,
  isAuthoritativeLiveBlockSnapshot,
  isEmptyLiveOverlayForTurn,
  isHiddenTranscriptEntry,
  isLiveOverlayForTurn,
  mergeBlockMetadata,
  mergeMetadata,
  normalizedEntryText,
  removeEmptyLiveOverlayForTurn,
  sortBlocks,
  sortTranscriptEntries,
  threadForTurn
} from "./transcript/common";
import {
  pendingClarifyFromEvent,
  pendingPermissionFromEvent,
  removePendingInteractionsForTurn,
  upsertPendingInteraction
} from "./transcript/pending";
import {
  liveEntryForSnapshotReconcile,
  reconcileIncomingEntryForSnapshot,
  removeSupersededToolOverlayEntries
} from "./transcript/reconciliation";

type LiveTranscriptObservationEvent = Extract<
  GatewayEvent,
  { type: "entryStarted" | "entryUpdated" | "entryCompleted" | "entryDelta" }
>;

export function applyLiveTranscriptEvent(
  snapshot: ThreadSnapshot,
  event: GatewayEvent
): ThreadSnapshot {
  if (!eventAppliesToSnapshot(snapshot, event)) {
    return snapshot;
  }

  switch (event.type) {
    case "entryStarted":
    case "entryUpdated":
    case "entryCompleted": {
      const nextEntry = entryForSnapshot(snapshot, event.entry);
      if (isHiddenTranscriptEntry(nextEntry)) {
        return {
          ...snapshot,
          entries: snapshot.entries.filter((entry) => entry.id !== nextEntry.id)
        };
      }
      const reconciled = reconcileIncomingEntryForSnapshot(snapshot.entries, nextEntry);
      const entries = reconciled.entry
        ? upsertEntry(reconciled.entries, reconciled.entry)
        : reconciled.entries.filter((entry) => entry.id !== nextEntry.id);
      return {
        ...snapshot,
        entries: sortTranscriptEntries(entries)
      };
    }
    case "entryDelta":
      if (!event.entryId) {
        return snapshot;
      }
      return {
        ...snapshot,
        entries: sortTranscriptEntries(applyEntryDelta(snapshot.entries, event.entryId, event.blockId, event.delta))
      };
    case "turnStarted":
      return {
        ...snapshot,
        thread: threadForTurn(snapshot, event.threadId),
        entries: sortTranscriptEntries(bindOptimisticPromptsToTurn(snapshot.entries, event.turnId)),
        activity: {
          ...snapshot.activity,
          activeTurnId: event.turnId,
          running: true
        }
      };
    case "turnCompleted": {
      const committedEntries = Array.isArray(event.committedEntries) ? event.committedEntries : [];
      const terminal = event.turn;
      const terminalStatus = terminal.status;
      const terminalThreadId = event.threadId ?? terminal.threadId ?? null;
      const failedTerminal = terminalStatus === "failed" || terminalStatus === "interrupted";
      const entries = failedTerminal
        ? ensureTerminalDiagnosticEntry(
            finalizePendingEntriesForTurn(
              mergeTerminalCommittedEntries(snapshot, event.turnId, committedEntries),
              event.turnId,
              terminalStatus === "interrupted" ? "cancelled" : "failed"
            ),
            snapshot,
            event
          )
        : committedEntries.length > 0
          ? mergeCommittedEntries(snapshot, event.turnId, committedEntries)
          : removeEmptyLiveOverlayForTurn(snapshot.entries, event.turnId);
      return {
        ...snapshot,
        thread: threadForTurn(snapshot, terminalThreadId),
        entries,
        pendingPermissions: removePendingInteractionsForTurn(
          snapshot.pendingPermissions,
          event.turnId
        ),
        pendingClarifies: removePendingInteractionsForTurn(
          snapshot.pendingClarifies,
          event.turnId
        ),
        activity: snapshot.activity.activeTurnId === event.turnId
          ? {
              ...snapshot.activity,
              activeTurnId: null,
              running: false
            }
          : snapshot.activity
      };
    }
    case "permissionRequested":
      return {
        ...snapshot,
        pendingPermissions: upsertPendingInteraction(
          snapshot.pendingPermissions,
          pendingPermissionFromEvent(event)
        )
      };
    case "permissionResolved":
      return {
        ...snapshot,
        pendingPermissions: snapshot.pendingPermissions.filter((request) =>
          request.requestId !== event.requestId
        )
      };
    case "clarifyRequested":
      return {
        ...snapshot,
        pendingClarifies: upsertPendingInteraction(
          snapshot.pendingClarifies,
          pendingClarifyFromEvent(event)
        )
      };
    case "clarifyResolved":
      return {
        ...snapshot,
        pendingClarifies: snapshot.pendingClarifies.filter((request) =>
          request.requestId !== event.requestId
        )
      };
    case "activityChanged":
      return {
        ...snapshot,
        activity: event.activity
      };
    default:
      return snapshot;
  }
}

export function appendOptimisticPrompt(
  snapshot: ThreadSnapshot,
  text: string,
  now = Date.now()
): ThreadSnapshot {
  const body = text.trim();
  if (!body) {
    return snapshot;
  }
  const id = `optimistic:${now}:${hashText(body)}`;
  const block: TranscriptBlock = {
    id: `${id}:text`,
    kind: "text",
    status: "completed",
    order: 0,
    source: OPTIMISTIC_SOURCE,
    title: null,
    body,
    preview: compactText(body, 240),
    detail: body,
    artifactIds: [],
    metadata: null,
    result: null,
    createdAtMs: now,
    updatedAtMs: now
  };
  const entry: TranscriptEntry = {
    id,
    threadId: snapshot.thread?.id ?? "",
    turnId: snapshot.activity.activeTurnId,
    messageSeq: null,
    role: "user",
    status: "completed",
    source: OPTIMISTIC_SOURCE,
    blocks: [block],
    metadata: { projection: "optimistic_prompt", liveOrder: OPTIMISTIC_LIVE_ORDER },
    usage: null,
    accounting: null,
    createdAtMs: now,
    updatedAtMs: now
  };
  return {
    ...snapshot,
    entries: [...snapshot.entries, entry]
  };
}

export function reconcileThreadSnapshot(
  current: ThreadSnapshot,
  incoming: ThreadSnapshot
): ThreadSnapshot {
  if ((current.thread?.id ?? null) !== (incoming.thread?.id ?? null)) {
    return normalizeSnapshotEntries(incoming);
  }

  const entries = incoming.entries.filter((entry) => !isHiddenTranscriptEntry(entry));
  for (const entry of current.entries) {
    if (isHiddenTranscriptEntry(entry)) {
      continue;
    }
    if (isUnreconciledOptimisticPrompt(entry, entries)) {
      entries.push(entry);
      continue;
    }
    const liveEntry = liveEntryForSnapshotReconcile(entry, entries, current, incoming);
    if (liveEntry) {
      entries.push(liveEntry);
    }
  }

  return {
    ...incoming,
    entries: sortTranscriptEntries(entries)
  };
}

function eventAppliesToSnapshot(snapshot: ThreadSnapshot, event: GatewayEvent): boolean {
  const currentThreadId = snapshot.thread?.id ?? null;
  const eventThreadId = eventThreadIdForEvent(event);
  if ("threadId" in event && event.threadId && currentThreadId && event.threadId !== currentThreadId) {
    return false;
  }
  if ("entry" in event && event.entry.threadId && currentThreadId && event.entry.threadId !== currentThreadId) {
    return false;
  }
  if (!currentThreadId && eventThreadId && !detachedSnapshotCanAcceptThreadedEvent(snapshot, event)) {
    return false;
  }
  if (
    "entry" in event &&
    !event.entry.threadId &&
    currentThreadId &&
    event.turnId !== snapshot.activity.activeTurnId
  ) {
    return false;
  }
  if (isLiveTranscriptObservation(event)) {
    const activeTurnId = snapshot.activity.activeTurnId;
    if (!activeTurnId || event.turnId !== activeTurnId) {
      return false;
    }
  }
  if (
    "turnId" in event &&
    event.type !== "turnCompleted" &&
    snapshot.activity.activeTurnId &&
    event.turnId !== snapshot.activity.activeTurnId
  ) {
    return false;
  }
  return true;
}

function isLiveTranscriptObservation(event: GatewayEvent): event is LiveTranscriptObservationEvent {
  return event.type === "entryStarted" ||
    event.type === "entryUpdated" ||
    event.type === "entryCompleted" ||
    event.type === "entryDelta";
}

function eventThreadIdForEvent(event: GatewayEvent): string | null {
  switch (event.type) {
    case "turnStarted":
    case "turnQueued":
      return event.threadId || null;
    case "turnCompleted":
      return event.threadId ||
        event.turn.threadId ||
        (Array.isArray(event.committedEntries)
          ? event.committedEntries.find((entry) => entry.threadId)?.threadId
          : null) ||
        null;
    case "entryStarted":
    case "entryUpdated":
    case "entryCompleted":
      return event.entry.threadId || null;
    case "activityChanged":
      return event.threadId || null;
    case "permissionRequested":
    case "clarifyRequested":
      return event.threadId || null;
    case "titleChanged":
      return event.threadId || null;
    default:
      return null;
  }
}

function detachedSnapshotCanAcceptThreadedEvent(snapshot: ThreadSnapshot, event: GatewayEvent): boolean {
  const activeTurnId = snapshot.activity.activeTurnId;
  if ("turnId" in event && activeTurnId && event.turnId === activeTurnId) {
    return true;
  }
  return event.type === "turnStarted" && hasUnboundOptimisticPrompt(snapshot);
}

function hasUnboundOptimisticPrompt(snapshot: ThreadSnapshot): boolean {
  return snapshot.entries.some((entry) =>
    entry.source === OPTIMISTIC_SOURCE &&
    entry.role === "user" &&
    entry.messageSeq === null &&
    !entry.turnId
  );
}

function entryForSnapshot(snapshot: ThreadSnapshot, entry: TranscriptEntry): TranscriptEntry {
  const now = Date.now();
  const existing = snapshot.entries.find((candidate) => candidate.id === entry.id);
  return {
    ...entry,
    threadId: entry.threadId ||
      (entry.turnId && entry.turnId === snapshot.activity.activeTurnId ? snapshot.thread?.id ?? "" : ""),
    blocks: sortBlocks(blocksForEntry(entry)),
    createdAtMs: entry.createdAtMs || existing?.createdAtMs || now,
    updatedAtMs: entry.updatedAtMs || now
  };
}

function mergeCommittedEntries(
  snapshot: ThreadSnapshot,
  turnId: string,
  committedEntries: TranscriptEntry[]
): TranscriptEntry[] {
  const enrichedCommittedEntries = committedEntries.map((entry) =>
    enrichCommittedAgentTargetsFromLive(entry, snapshot.entries, turnId)
  );
  let entries = snapshot.entries.filter((entry) => !isLiveOverlayForTurn(entry, turnId));
  for (const entry of enrichedCommittedEntries) {
    if (isHiddenTranscriptEntry(entry)) {
      continue;
    }
    entries = upsertEntry(entries, entryForSnapshot(snapshot, entry));
  }
  return sortTranscriptEntries(entries);
}

function mergeTerminalCommittedEntries(
  snapshot: ThreadSnapshot,
  turnId: string,
  committedEntries: TranscriptEntry[]
): TranscriptEntry[] {
  let entries = [...snapshot.entries];
  for (const entry of committedEntries.map((candidate) =>
    enrichCommittedAgentTargetsFromLive(candidate, snapshot.entries, turnId)
  )) {
    if (isHiddenTranscriptEntry(entry)) {
      continue;
    }
    entries = upsertEntry(entries, entryForSnapshot(snapshot, entry));
  }
  return entries.filter((entry) => !isEmptyLiveOverlayForTurn(entry, turnId));
}

function finalizePendingEntriesForTurn(
  entries: TranscriptEntry[],
  turnId: string,
  status: "failed" | "cancelled"
): TranscriptEntry[] {
  return sortTranscriptEntries(entries.map((entry) => {
    if (entry.turnId !== turnId) {
      return entry;
    }
    let changed = false;
    const blocks = blocksForEntry(entry).map((block) => {
      if (block.status !== "pending" && block.status !== "running") {
        return block;
      }
      changed = true;
      return {
        ...block,
        status,
        updatedAtMs: Date.now()
      };
    });
    if (!changed) {
      return entry;
    }
    return {
      ...entry,
      blocks,
      status: entryStatusForBlocks(blocks, status),
      updatedAtMs: Date.now()
    };
  }));
}

function ensureTerminalDiagnosticEntry(
  entries: TranscriptEntry[],
  snapshot: ThreadSnapshot,
  event: Extract<GatewayEvent, { type: "turnCompleted" }>
): TranscriptEntry[] {
  const status = event.turn.status;
  if (status !== "failed" && status !== "interrupted") {
    return sortTranscriptEntries(entries);
  }
  const id = `turn:${event.turnId}:terminal`;
  if (entries.some((entry) => entry.id === id)) {
    return sortTranscriptEntries(entries);
  }
  const blockStatus = status === "interrupted" ? "cancelled" : "failed";
  const message = event.turn.error?.message?.trim()
    || (status === "interrupted" ? "The turn was interrupted." : "The turn failed.");
  const now = event.turn.completedAtMs ?? Date.now();
  const threadId = event.threadId ?? event.turn.threadId ?? snapshot.thread?.id ?? "";
  const entry: TranscriptEntry = {
    id,
    threadId,
    turnId: event.turnId,
    messageSeq: null,
    role: "diagnostic",
    status: blockStatus,
    source: "gateway.turn",
    blocks: [{
      id: `${id}:block`,
      kind: "status",
      status: blockStatus,
      order: 0,
      source: "gateway.turn",
      title: status === "interrupted" ? "Turn interrupted" : "Turn failed",
      body: message,
      preview: compactText(message, 240),
      detail: message,
      artifactIds: [],
      metadata: {
        projection: "turn_terminal",
        status,
        outcome: event.turn.outcome ?? null
      },
      result: null,
      createdAtMs: now,
      updatedAtMs: now
    }],
    metadata: {
      projection: "turn_terminal",
      status,
      outcome: event.turn.outcome ?? null
    },
    usage: null,
    accounting: null,
    createdAtMs: now,
    updatedAtMs: now
  };
  return sortTranscriptEntries([...entries, entry]);
}

function bindOptimisticPromptsToTurn(entries: TranscriptEntry[], turnId: string): TranscriptEntry[] {
  return entries.map((entry) => {
    if (entry.source !== OPTIMISTIC_SOURCE || entry.role !== "user" || entry.messageSeq !== null) {
      return entry;
    }
    return {
      ...entry,
      turnId: entry.turnId ?? turnId,
      metadata: mergeMetadata(entry.metadata, {
        projection: "optimistic_prompt",
        liveOrder: OPTIMISTIC_LIVE_ORDER
      })
    };
  });
}

function upsertEntry(entries: TranscriptEntry[], next: TranscriptEntry): TranscriptEntry[] {
  if (isHiddenTranscriptEntry(next)) {
    return entries.filter((entry) => entry.id !== next.id);
  }
  const currentEntries = isAuthoritativeLiveBlockSnapshot(next)
    ? removeSupersededToolOverlayEntries(entries, next)
    : entries;
  const existing = currentEntries.findIndex((entry) => entry.id === next.id);
  if (existing >= 0) {
    return currentEntries.map((entry, index) => (index === existing ? mergeEntry(entry, next) : entry));
  }
  return [...currentEntries, next];
}

function mergeEntry(current: TranscriptEntry, next: TranscriptEntry): TranscriptEntry {
  const replaceBlocks = isAuthoritativeLiveBlockSnapshot(next);
  return {
    ...current,
    ...next,
    messageSeq: next.messageSeq ?? current.messageSeq,
    createdAtMs: current.createdAtMs || next.createdAtMs,
    blocks: replaceBlocks ? sortBlocks(blocksForEntry(next)) : mergeBlocks(current.blocks, next.blocks),
    metadata: mergeMetadata(current.metadata, next.metadata)
  };
}

function mergeBlocks(current: TranscriptBlock[], next: TranscriptBlock[]): TranscriptBlock[] {
  let blocks = [...blocksForValue(current)];
  for (const block of blocksForValue(next)) {
    const existing = blocks.findIndex((candidate) => candidate.id === block.id);
    if (existing >= 0) {
      blocks = blocks.map((candidate, index) => (
        index === existing ? mergeBlock(candidate, block) : candidate
      ));
    } else {
      blocks.push(block);
    }
  }
  return sortBlocks(blocks);
}

function mergeBlock(current: TranscriptBlock, next: TranscriptBlock): TranscriptBlock {
  const currentArtifactIds = artifactIdsForBlock(current);
  const nextArtifactIds = artifactIdsForBlock(next);
  return {
    ...current,
    ...next,
    order: current.order || next.order,
    createdAtMs: current.createdAtMs || next.createdAtMs,
    artifactIds: nextArtifactIds.length > 0 ? nextArtifactIds : currentArtifactIds,
    metadata: mergeBlockMetadata(current, next),
    result: next.result ?? current.result
  };
}

function applyEntryDelta(
  entries: TranscriptEntry[],
  entryId: string,
  blockId: string | null,
  delta: string
): TranscriptEntry[] {
  return entries.map((entry) => {
    if (entry.id !== entryId) {
      return entry;
    }
    const blocks = blocksForEntry(entry).map((block, index) => {
      if ((blockId && block.id !== blockId) || (!blockId && index !== 0)) {
        return block;
      }
      const body = `${block.body ?? ""}${delta}`;
      return {
        ...block,
        body,
        detail: `${block.detail ?? ""}${delta}`,
        preview: compactText(body, 240),
        updatedAtMs: Date.now()
      };
    });
    return {
      ...entry,
      blocks,
      updatedAtMs: Date.now()
    };
  });
}

function isUnreconciledOptimisticPrompt(entry: TranscriptEntry, incoming: TranscriptEntry[]): boolean {
  if (entry.source !== OPTIMISTIC_SOURCE || entry.role !== "user") {
    return false;
  }
  const text = normalizedEntryText(entry);
  if (!text) {
    return false;
  }
  return !incoming.some((candidate) => candidate.role === "user" && normalizedEntryText(candidate) === text);
}

function normalizeSnapshotEntries(snapshot: ThreadSnapshot): ThreadSnapshot {
  return {
    ...snapshot,
    entries: sortTranscriptEntries(snapshot.entries.filter((entry) => !isHiddenTranscriptEntry(entry)))
  };
}
