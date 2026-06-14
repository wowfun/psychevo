import type { GatewayEvent, GatewayThread, ThreadSnapshot, TranscriptBlock, TranscriptEntry } from "@psychevo/protocol";

const OPTIMISTIC_SOURCE = "client.optimistic";
const LIVE_SOURCE = "runtime.stream";
const OPTIMISTIC_LIVE_ORDER = -1;

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
      const entries = committedEntries.length > 0
        ? mergeCommittedEntries(snapshot, event.turnId, committedEntries)
        : removeEmptyLiveOverlayForTurn(snapshot.entries, event.turnId);
      return {
        ...snapshot,
        thread: threadForTurn(snapshot, event.threadId),
        entries,
        activity: snapshot.activity.activeTurnId === event.turnId
          ? {
              ...snapshot.activity,
              activeTurnId: null,
              running: false
            }
          : snapshot.activity
      };
    }
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

  const entries = [...incoming.entries];
  for (const entry of current.entries) {
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

function eventThreadIdForEvent(event: GatewayEvent): string | null {
  switch (event.type) {
    case "turnStarted":
    case "turnQueued":
      return event.threadId || null;
    case "turnCompleted":
      return event.threadId ||
        (Array.isArray(event.committedEntries)
          ? event.committedEntries.find((entry) => entry.threadId)?.threadId
          : null) ||
        null;
    case "entryStarted":
    case "entryUpdated":
    case "entryCompleted":
      return event.entry.threadId || null;
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
  let entries = snapshot.entries.filter((entry) => !isLiveOverlayForTurn(entry, turnId));
  for (const entry of committedEntries) {
    entries = upsertEntry(entries, entryForSnapshot(snapshot, entry));
  }
  return sortTranscriptEntries(entries);
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

function isLiveOverlayForTurn(entry: TranscriptEntry, turnId: string): boolean {
  return entry.turnId === turnId && (entry.source === LIVE_SOURCE || entry.messageSeq === null);
}

function removeEmptyLiveOverlayForTurn(entries: TranscriptEntry[], turnId: string): TranscriptEntry[] {
  return entries.filter((entry) => !isLiveOverlayForTurn(entry, turnId) || entryHasVisibleTranscriptText(entry));
}

function upsertEntry(entries: TranscriptEntry[], next: TranscriptEntry): TranscriptEntry[] {
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
    metadata: mergeMetadata(current.metadata, next.metadata),
    result: next.result ?? current.result
  };
}

function removeSupersededToolOverlayEntries(
  entries: TranscriptEntry[],
  authoritative: TranscriptEntry
): TranscriptEntry[] {
  const signatures = new Set(
    blocksForEntry(authoritative)
      .map(toolBlockSignature)
      .filter((signature): signature is string => Boolean(signature))
  );
  if (signatures.size === 0) {
    return entries;
  }
  return entries.filter((entry) => {
    if (
      entry.id === authoritative.id ||
      entry.source !== LIVE_SOURCE ||
      entry.turnId !== authoritative.turnId ||
      entry.messageSeq !== null
    ) {
      return true;
    }
    const blocks = blocksForEntry(entry);
    if (blocks.length === 0 || !blocks.every(isPendingOnlyToolBlock)) {
      return true;
    }
    return !blocks.some((block) => {
      const signature = toolBlockSignature(block);
      return signature !== null && signatures.has(signature);
    });
  });
}

function isPendingOnlyToolBlock(block: TranscriptBlock): boolean {
  return isToolLikeBlock(block) &&
    !block.result &&
    (block.status === "pending" || block.status === "running");
}

function isToolLikeBlock(block: TranscriptBlock): boolean {
  return block.kind !== "text" && block.kind !== "reasoning";
}

function toolBlockSignature(block: TranscriptBlock): string | null {
  if (!isToolLikeBlock(block)) {
    return null;
  }
  const metadata = recordForValue(block.metadata);
  const toolName = stringValue(metadata.tool_name) ?? block.title ?? block.kind;
  const args = metadata.args ?? metadata.arguments ?? null;
  if (args === null) {
    return null;
  }
  return `${toolName}:${JSON.stringify(args)}`;
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

function liveEntryForSnapshotReconcile(
  entry: TranscriptEntry,
  incoming: TranscriptEntry[],
  current: ThreadSnapshot,
  incomingSnapshot: ThreadSnapshot
): TranscriptEntry | null {
  if (entry.source !== LIVE_SOURCE || entry.messageSeq !== null) {
    return null;
  }
  const activeTurnId = incomingSnapshot.activity.activeTurnId ?? current.activity.activeTurnId;
  if (!activeTurnId || entry.turnId !== activeTurnId) {
    return null;
  }
  if (!entryHasVisibleTranscriptText(entry)) {
    return null;
  }
  if (incoming.some((candidate) => candidate.id === entry.id)) {
    return null;
  }
  return removeSnapshotCoveredBlocks(entry, snapshotCoverage(incoming));
}

function normalizeSnapshotEntries(snapshot: ThreadSnapshot): ThreadSnapshot {
  return {
    ...snapshot,
    entries: sortTranscriptEntries(snapshot.entries)
  };
}

function sortTranscriptEntries(entries: TranscriptEntry[]): TranscriptEntry[] {
  return [...entries]
    .map((entry) => ({ ...entry, blocks: sortBlocks(blocksForEntry(entry)) }))
    .sort((left, right) => {
      if (left.messageSeq !== null && right.messageSeq !== null && left.messageSeq !== right.messageSeq) {
        return left.messageSeq - right.messageSeq;
      }
      if (left.messageSeq !== right.messageSeq) {
        const timelineComparison = compareTimelineMs(left, right);
        if (timelineComparison !== 0) {
          return timelineComparison;
        }
        return left.messageSeq !== null ? -1 : 1;
      }
      const leftLiveOrder = liveOrder(left);
      const rightLiveOrder = liveOrder(right);
      if (leftLiveOrder !== null && rightLiveOrder !== null && leftLiveOrder !== rightLiveOrder) {
        return leftLiveOrder - rightLiveOrder;
      }
      if (leftLiveOrder !== null && rightLiveOrder === null) {
        return -1;
      }
      if (leftLiveOrder === null && rightLiveOrder !== null) {
        return 1;
      }
      const leftStreamSeq = streamSeq(left);
      const rightStreamSeq = streamSeq(right);
      if (leftStreamSeq !== null && rightStreamSeq !== null && leftStreamSeq !== rightStreamSeq) {
        return leftStreamSeq - rightStreamSeq;
      }
      if (left.createdAtMs !== right.createdAtMs) {
        return left.createdAtMs - right.createdAtMs;
      }
      return left.id.localeCompare(right.id);
    });
}

function sortBlocks(blocks: TranscriptBlock[]): TranscriptBlock[] {
  return [...blocks].sort((left, right) => {
    if (left.order !== right.order) {
      return left.order - right.order;
    }
    if (left.createdAtMs !== right.createdAtMs) {
      return left.createdAtMs - right.createdAtMs;
    }
    return left.id.localeCompare(right.id);
  });
}

function normalizedEntryText(entry: TranscriptEntry): string {
  return blocksForEntry(entry)
    .filter((block) => block.kind === "text")
    .map((block) => block.detail ?? block.body ?? "")
    .join("\n")
    .trim()
    .replace(/\s+/g, " ");
}

function entryHasVisibleTranscriptText(entry: TranscriptEntry): boolean {
  return blocksForEntry(entry).some((block) => {
    if (recordForValue(block.metadata).hidden === true) {
      return false;
    }
    return blockText(block).trim().length > 0;
  });
}

function blockText(block: TranscriptBlock): string {
  return block.body ?? block.detail ?? block.preview ?? "";
}

function blocksForEntry(entry: TranscriptEntry): TranscriptBlock[] {
  return Array.isArray(entry.blocks) ? entry.blocks : [];
}

function blocksForValue(value: unknown): TranscriptBlock[] {
  return Array.isArray(value) ? value : [];
}

function artifactIdsForBlock(block: TranscriptBlock): string[] {
  return Array.isArray(block.artifactIds) ? block.artifactIds : [];
}

function mergeMetadata(left: unknown, right: unknown): unknown {
  if (isRecord(left) && isRecord(right)) {
    return { ...left, ...right };
  }
  return right ?? left ?? null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function recordForValue(value: unknown): Record<string, unknown> {
  return isRecord(value) ? value : {};
}

function stringValue(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

function threadForTurn(snapshot: ThreadSnapshot, threadId: string | null): GatewayThread | null {
  if (!threadId) {
    return snapshot.thread;
  }
  if (snapshot.thread?.id === threadId) {
    return snapshot.thread;
  }
  return {
    id: threadId,
    backend: {
      kind: "psychevo",
      nativeId: threadId
    },
    sourceKey: sourceKeyForSnapshot(snapshot)
  };
}

function sourceKeyForSnapshot(snapshot: ThreadSnapshot): string | null {
  const source = snapshot.source;
  const kind = typeof source.kind === "string" && source.kind.trim() ? source.kind : null;
  const rawId = typeof source.rawId === "string" && source.rawId.trim() ? source.rawId : null;
  return kind && rawId ? `${kind}:${rawId}` : snapshot.thread?.sourceKey ?? null;
}

function reconcileIncomingEntryForSnapshot(
  entries: TranscriptEntry[],
  entry: TranscriptEntry
): { entries: TranscriptEntry[]; entry: TranscriptEntry | null } {
  if (entry.source !== LIVE_SOURCE || entry.messageSeq !== null) {
    return { entries, entry };
  }
  const anchoredEntries = anchorCoveredLiveToolBlocks(entries, entry);
  return {
    entries: anchoredEntries,
    entry: removeSnapshotCoveredBlocks(entry, snapshotCoverage(anchoredEntries))
  };
}

function anchorCoveredLiveToolBlocks(
  entries: TranscriptEntry[],
  liveEntry: TranscriptEntry
): TranscriptEntry[] {
  const liveTools = new Map<string, TranscriptBlock>();
  for (const block of blocksForEntry(liveEntry)) {
    const signature = toolBlockSignature(block);
    if (signature) {
      liveTools.set(signature, block);
    }
  }
  if (liveTools.size === 0) {
    return entries;
  }
  return entries.map((entry) => {
    if (!isMessageDerivedEntry(entry)) {
      return entry;
    }
    let changed = false;
    const blocks = blocksForEntry(entry).map((block) => {
      const signature = toolBlockSignature(block);
      const liveBlock = signature ? liveTools.get(signature) : undefined;
      if (!liveBlock) {
        return block;
      }
      changed = true;
      return mergeLiveToolBlockIntoMessageBlock(block, liveBlock);
    });
    if (!changed) {
      return entry;
    }
    return {
      ...entry,
      blocks: sortBlocks(blocks),
      status: entryStatusForBlocks(blocks, entry.status),
      updatedAtMs: Math.max(entry.updatedAtMs, liveEntry.updatedAtMs)
    };
  });
}

function mergeLiveToolBlockIntoMessageBlock(
  current: TranscriptBlock,
  liveBlock: TranscriptBlock
): TranscriptBlock {
  const currentArtifactIds = artifactIdsForBlock(current);
  const liveArtifactIds = artifactIdsForBlock(liveBlock);
  return {
    ...current,
    status: liveBlock.status,
    title: liveBlock.title ?? current.title,
    body: liveBlock.body ?? current.body,
    preview: liveBlock.preview ?? current.preview,
    detail: liveBlock.detail ?? current.detail,
    artifactIds: liveArtifactIds.length > 0 ? liveArtifactIds : currentArtifactIds,
    metadata: mergeMetadata(current.metadata, liveBlock.metadata),
    result: liveBlock.result ?? current.result,
    updatedAtMs: Math.max(current.updatedAtMs, liveBlock.updatedAtMs)
  };
}

function removeSnapshotCoveredBlocks(
  entry: TranscriptEntry,
  coverage: { texts: string[]; tools: Set<string> }
): TranscriptEntry | null {
  if (coverage.texts.length === 0 && coverage.tools.size === 0) {
    return entry;
  }
  const blocks = blocksForEntry(entry).filter((block) => (
    !blockVisibleForCoverage(block) || !blockCoveredBySnapshot(block, coverage)
  ));
  const next: TranscriptEntry = {
    ...entry,
    blocks: sortBlocks(blocks),
    status: entryStatusForBlocks(blocks, entry.status)
  };
  if (!entryHasVisibleTranscriptText(next)) {
    return null;
  }
  return next;
}

function snapshotCoverage(entries: TranscriptEntry[]): { texts: string[]; tools: Set<string> } {
  const texts: string[] = [];
  const tools = new Set<string>();
  for (const entry of entries) {
    if (!isMessageDerivedEntry(entry)) {
      continue;
    }
    for (const block of blocksForEntry(entry)) {
      if (recordForValue(block.metadata).hidden === true) {
        continue;
      }
      const signature = toolBlockSignature(block);
      if (signature) {
        tools.add(signature);
        continue;
      }
      const text = normalizedBlockText(block);
      if (text) {
        texts.push(text);
      }
    }
  }
  return { texts, tools };
}

function isMessageDerivedEntry(entry: TranscriptEntry): boolean {
  return entry.source === "runtime.message" && entry.messageSeq !== null;
}

function blockVisibleForCoverage(block: TranscriptBlock): boolean {
  if (recordForValue(block.metadata).hidden === true) {
    return false;
  }
  return Boolean(toolBlockSignature(block) || normalizedBlockText(block));
}

function blockCoveredBySnapshot(
  block: TranscriptBlock,
  coverage: { texts: string[]; tools: Set<string> }
): boolean {
  const signature = toolBlockSignature(block);
  if (signature) {
    return coverage.tools.has(signature);
  }
  const text = normalizedBlockText(block);
  return Boolean(text && coverage.texts.some((candidate) => textOverlaps(candidate, text)));
}

function entryStatusForBlocks(
  blocks: TranscriptBlock[],
  fallback: TranscriptEntry["status"]
): TranscriptEntry["status"] {
  const statuses = blocks.map((block) => block.status);
  for (const status of ["failed", "cancelled", "needsInput", "running", "pending", "info"] as const) {
    if (statuses.includes(status)) {
      return status;
    }
  }
  return statuses.length > 0 ? "completed" : fallback;
}

function normalizedBlockText(block: TranscriptBlock): string {
  return blockText(block).trim().replace(/\s+/g, " ");
}

function textOverlaps(left: string, right: string): boolean {
  if (!left || !right) {
    return false;
  }
  if (left.length < 16 || right.length < 16) {
    return left === right;
  }
  return left.includes(right) || right.includes(left);
}

function isAuthoritativeLiveBlockSnapshot(entry: TranscriptEntry): boolean {
  return entry.source === LIVE_SOURCE && recordForValue(entry.metadata).authoritativeBlocks === true;
}

function liveOrder(entry: TranscriptEntry): number | null {
  const metadata = recordForValue(entry.metadata);
  const value = metadata.liveOrder ?? metadata.live_order;
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function compareTimelineMs(left: TranscriptEntry, right: TranscriptEntry): number {
  const leftTime = timelineMs(left);
  const rightTime = timelineMs(right);
  return leftTime !== null && rightTime !== null && leftTime !== rightTime ? leftTime - rightTime : 0;
}

function timelineMs(entry: TranscriptEntry): number | null {
  const value = entry.createdAtMs || entry.updatedAtMs;
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? value : null;
}

function streamSeq(entry: TranscriptEntry): number | null {
  const metadata = recordForValue(entry.metadata);
  const value = metadata.streamSeq ?? metadata.stream_seq;
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function compactText(text: string, max: number): string {
  const compacted = text.replace(/\s+/g, " ").trim();
  if (compacted.length <= max) {
    return compacted;
  }
  return `${compacted.slice(0, Math.max(0, max - 1))}…`;
}

function hashText(text: string): string {
  let hash = 0;
  for (let index = 0; index < text.length; index += 1) {
    hash = ((hash << 5) - hash + text.charCodeAt(index)) | 0;
  }
  return Math.abs(hash).toString(36);
}
