import {
  sideInheritedMetadataHidden,
  type GatewayThread,
  type ThreadSnapshot,
  type TranscriptBlock,
  type TranscriptEntry
} from "@psychevo/protocol";

export const OPTIMISTIC_SOURCE = "client.optimistic";
export const LIVE_SOURCE = "runtime.stream";
export const OPTIMISTIC_LIVE_ORDER = -1;

export function isLiveOverlayForTurn(entry: TranscriptEntry, turnId: string): boolean {
  return entry.turnId === turnId && (entry.source === LIVE_SOURCE || entry.messageSeq === null);
}

export function removeEmptyLiveOverlayForTurn(entries: TranscriptEntry[], turnId: string): TranscriptEntry[] {
  return entries.filter((entry) => !isLiveOverlayForTurn(entry, turnId) || entryHasVisibleTranscriptText(entry));
}

export function isEmptyLiveOverlayForTurn(entry: TranscriptEntry, turnId: string): boolean {
  return isLiveOverlayForTurn(entry, turnId) && !entryHasVisibleTranscriptText(entry);
}

export function sortTranscriptEntries(entries: TranscriptEntry[]): TranscriptEntry[] {
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

export function sortBlocks(blocks: TranscriptBlock[]): TranscriptBlock[] {
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

export function normalizedEntryText(entry: TranscriptEntry): string {
  return blocksForEntry(entry)
    .filter((block) => block.kind === "text")
    .map((block) => block.detail ?? block.body ?? "")
    .join("\n")
    .trim()
    .replace(/\s+/g, " ");
}

export function entryHasVisibleTranscriptText(entry: TranscriptEntry): boolean {
  if (isHiddenTranscriptEntry(entry)) {
    return false;
  }
  return blocksForEntry(entry).some((block) => {
    if (metadataHidden(block.metadata)) {
      return false;
    }
    if (blockText(block).trim().length > 0) {
      return true;
    }
    return block.kind !== "text" &&
      block.kind !== "reasoning" &&
      typeof block.title === "string" &&
      block.title.trim().length > 0;
  });
}

export function isHiddenTranscriptEntry(entry: TranscriptEntry): boolean {
  return metadataHidden(entry.metadata);
}

export function metadataHidden(metadata: unknown): boolean {
  return recordForValue(metadata).hidden === true || sideInheritedMetadataHidden(metadata);
}

export function blockText(block: TranscriptBlock): string {
  return block.body ?? block.detail ?? block.preview ?? "";
}

export function blocksForEntry(entry: TranscriptEntry): TranscriptBlock[] {
  return Array.isArray(entry.blocks) ? entry.blocks : [];
}

export function blocksForValue(value: unknown): TranscriptBlock[] {
  return Array.isArray(value) ? value : [];
}

export function artifactIdsForBlock(block: TranscriptBlock): string[] {
  return Array.isArray(block.artifactIds) ? block.artifactIds : [];
}

export function mergeMetadata(left: unknown, right: unknown): unknown {
  if (isRecord(left) && isRecord(right)) {
    return { ...left, ...right };
  }
  return right ?? left ?? null;
}

export function mergeBlockMetadata(current: TranscriptBlock, next: TranscriptBlock): unknown {
  if (isSpawnAgentBlock(current) || isSpawnAgentBlock(next)) {
    return mergeAgentMetadata(current.metadata, next.metadata);
  }
  return mergeMetadata(current.metadata, next.metadata);
}

export function isSpawnAgentBlock(block: TranscriptBlock): boolean {
  const metadata = recordForValue(block.metadata);
  return stringValue(metadata.tool_name) === "spawn_agent";
}

export function mergeAgentMetadata(left: unknown, right: unknown): unknown {
  if (!isRecord(left) || !isRecord(right)) {
    return right ?? left ?? null;
  }
  const merged: Record<string, unknown> = { ...right };
  for (const key of [
    "projection",
    "tool_name",
    "tool_call_id",
    "parent_thread_id",
    "parent_session_id",
    "child_thread_id",
    "child_session_id",
    "session_id",
    "agent_id",
    "agent_name",
    "agent_type",
    "agent_path",
    "task_name",
    "message",
    "task",
    "prompt",
    "args",
    "arguments"
  ]) {
    copyFieldIfMissing(merged, left, key);
  }
  const leftResult = recordForValue(left.result);
  if (Object.keys(leftResult).length > 0) {
    const result = { ...recordForValue(merged.result) };
    for (const key of [
      "parent_thread_id",
      "parent_session_id",
      "child_thread_id",
      "child_session_id",
      "session_id",
      "agent_id",
      "agent_name",
      "agent_type",
      "agent_path",
      "task_name",
      "message",
      "task",
      "prompt"
    ]) {
      copyFieldIfMissing(result, leftResult, key);
    }
    merged.result = result;
  }
  return merged;
}

export function copyFieldIfMissing(
  target: Record<string, unknown>,
  source: Record<string, unknown>,
  key: string
) {
  if (target[key] !== undefined && target[key] !== null) {
    return;
  }
  const value = source[key];
  if (value !== undefined && value !== null) {
    target[key] = value;
  }
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function recordForValue(value: unknown): Record<string, unknown> {
  return isRecord(value) ? value : {};
}

export function stringValue(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

export function threadForTurn(snapshot: ThreadSnapshot, threadId: string | null): GatewayThread | null {
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
      nativeId: threadId,
      runtimeRef: "native"
    },
    sourceKey: sourceKeyForSnapshot(snapshot)
  };
}

export function sourceKeyForSnapshot(snapshot: ThreadSnapshot): string | null {
  const source = snapshot.source;
  const kind = typeof source.kind === "string" && source.kind.trim() ? source.kind : null;
  const rawId = typeof source.rawId === "string" && source.rawId.trim() ? source.rawId : null;
  return kind && rawId ? `${kind}:${rawId}` : snapshot.thread?.sourceKey ?? null;
}

export function entryStatusForBlocks(
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

export function normalizedBlockText(block: TranscriptBlock): string {
  return blockText(block).trim().replace(/\s+/g, " ");
}

export function isMessageDerivedEntry(entry: TranscriptEntry): boolean {
  return entry.source === "runtime.message" && entry.messageSeq !== null;
}

export function isAuthoritativeLiveBlockSnapshot(entry: TranscriptEntry): boolean {
  return entry.source === LIVE_SOURCE && recordForValue(entry.metadata).authoritativeBlocks === true;
}

export function liveOrder(entry: TranscriptEntry): number | null {
  const metadata = recordForValue(entry.metadata);
  const value = metadata.liveOrder ?? metadata.live_order;
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

export function compareTimelineMs(left: TranscriptEntry, right: TranscriptEntry): number {
  const leftTime = timelineMs(left);
  const rightTime = timelineMs(right);
  return leftTime !== null && rightTime !== null && leftTime !== rightTime ? leftTime - rightTime : 0;
}

export function timelineMs(entry: TranscriptEntry): number | null {
  const value = entry.createdAtMs || entry.updatedAtMs;
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? value : null;
}

export function streamSeq(entry: TranscriptEntry): number | null {
  const metadata = recordForValue(entry.metadata);
  const value = metadata.streamSeq ?? metadata.stream_seq;
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

export function compactText(text: string, max: number): string {
  const compacted = text.replace(/\s+/g, " ").trim();
  if (compacted.length <= max) {
    return compacted;
  }
  return `${compacted.slice(0, Math.max(0, max - 1))}…`;
}

export function hashText(text: string): string {
  let hash = 0;
  for (let index = 0; index < text.length; index += 1) {
    hash = ((hash << 5) - hash + text.charCodeAt(index)) | 0;
  }
  return Math.abs(hash).toString(36);
}
