import type { ThreadSnapshot, TranscriptBlock, TranscriptBlockStatus, TranscriptEntry } from "@psychevo/protocol";

import {
  LIVE_SOURCE,
  artifactIdsForBlock,
  blocksForEntry,
  entryHasVisibleTranscriptText,
  entryStatusForBlocks,
  isMessageDerivedEntry,
  liveOrder,
  mergeBlockMetadata,
  normalizedBlockText,
  recordForValue,
  sortBlocks,
  stringValue
} from "./common";

export function removeSupersededToolOverlayEntries(
  entries: TranscriptEntry[],
  authoritative: TranscriptEntry
): TranscriptEntry[] {
  const signatures = new Set(
    blocksForEntry(authoritative)
      .flatMap(toolBlockSignatures)
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
      return toolBlockSignatures(block).some((signature) => signatures.has(signature));
    });
  });
}

export function liveEntryForSnapshotReconcile(
  entry: TranscriptEntry,
  incoming: TranscriptEntry[],
  current: ThreadSnapshot,
  incomingSnapshot: ThreadSnapshot
): TranscriptEntry | null {
  if (entry.source !== LIVE_SOURCE || entry.messageSeq !== null) {
    return null;
  }
  const activeTurnId = incomingSnapshot.activity.running
    ? incomingSnapshot.activity.activeTurnId
    : null;
  if (!activeTurnId || entry.turnId !== activeTurnId) {
    return null;
  }
  if (!entryHasVisibleTranscriptText(entry)) {
    return null;
  }
  if (incoming.some((candidate) => candidate.id === entry.id)) {
    return null;
  }
  if (committedAssistantOwnerExists(incoming, entry)) {
    return null;
  }
  return removeSnapshotCoveredBlocks(entry, snapshotToolCoverage(incoming));
}

export function reconcileIncomingEntryForSnapshot(
  entries: TranscriptEntry[],
  entry: TranscriptEntry
): { entries: TranscriptEntry[]; entry: TranscriptEntry | null } {
  if (isCommittedAssistantOwner(entry)) {
    return {
      entries: removeLiveAssistantOwnerEntries(entries, entry),
      entry
    };
  }
  if (entry.source !== LIVE_SOURCE || entry.messageSeq !== null) {
    return { entries, entry };
  }
  const anchoredEntries = anchorCoveredLiveToolBlocks(entries, entry);
  if (committedAssistantOwnerExists(anchoredEntries, entry)) {
    return {
      entries: anchoredEntries.filter((candidate) => candidate.id !== entry.id),
      entry: null
    };
  }
  return {
    entries: anchoredEntries,
    entry: removeSnapshotCoveredBlocks(entry, snapshotToolCoverage(anchoredEntries))
  };
}

function isPendingOnlyToolBlock(block: TranscriptBlock): boolean {
  return isToolLikeBlock(block) &&
    !block.result &&
    (block.status === "pending" || block.status === "running");
}

function isToolLikeBlock(block: TranscriptBlock): boolean {
  return block.kind !== "text" && block.kind !== "reasoning";
}

function toolBlockSignatures(block: TranscriptBlock): string[] {
  if (!isToolLikeBlock(block)) {
    return [];
  }
  const metadata = recordForValue(block.metadata);
  const toolName = stringValue(metadata.tool_name) ?? block.title ?? block.kind;
  const signatures: string[] = [];
  const toolCallId = stringValue(metadata.tool_call_id);
  if (toolCallId) {
    signatures.push(`${toolName}:id:${toolCallId}`);
  }
  const childSessionId = stringValue(metadata.child_thread_id)
    ?? stringValue(metadata.childThreadId)
    ?? stringValue(metadata.child_session_id)
    ?? stringValue(metadata.childSessionId)
    ?? stringValue(recordForValue(metadata.result).child_thread_id)
    ?? stringValue(recordForValue(metadata.result).childThreadId)
    ?? stringValue(recordForValue(metadata.result).child_session_id)
    ?? stringValue(recordForValue(metadata.result).childSessionId)
    ?? stringValue(recordForValue(metadata.result).session_id)
    ?? stringValue(recordForValue(metadata.result).sessionId);
  if (toolName === "spawn_agent" && childSessionId) {
    signatures.push(`${toolName}:child:${childSessionId}`);
  }
  const args = metadata.args ?? metadata.arguments ?? null;
  if (toolName !== "spawn_agent" && args !== null) {
    signatures.push(`${toolName}:args:${JSON.stringify(args)}`);
  }
  return signatures;
}

function removeLiveAssistantOwnerEntries(
  entries: TranscriptEntry[],
  committed: TranscriptEntry
): TranscriptEntry[] {
  return entries.filter((entry) => !sameAssistantOwner(entry, committed));
}

function committedAssistantOwnerExists(
  entries: TranscriptEntry[],
  liveEntry: TranscriptEntry
): boolean {
  return entries.some((entry) => isCommittedAssistantOwner(entry) && sameAssistantOwner(liveEntry, entry));
}

function sameAssistantOwner(left: TranscriptEntry, right: TranscriptEntry): boolean {
  const leftOrder = liveOrder(left);
  const rightOrder = liveOrder(right);
  return left.role === "assistant" &&
    right.role === "assistant" &&
    Boolean(left.turnId) &&
    left.turnId === right.turnId &&
    leftOrder !== null &&
    leftOrder === rightOrder;
}

function isCommittedAssistantOwner(entry: TranscriptEntry): boolean {
  return isMessageDerivedEntry(entry) && entry.role === "assistant" && liveOrder(entry) !== null;
}

function anchorCoveredLiveToolBlocks(
  entries: TranscriptEntry[],
  liveEntry: TranscriptEntry
): TranscriptEntry[] {
  const liveTools = new Map<string, TranscriptBlock>();
  for (const block of blocksForEntry(liveEntry)) {
    for (const signature of toolBlockSignatures(block)) {
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
      const liveBlock = toolBlockSignatures(block)
        .map((signature) => liveTools.get(signature))
        .find((candidate): candidate is TranscriptBlock => Boolean(candidate));
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
    status: monotonicBlockStatus(current.status, liveBlock.status),
    title: liveBlock.title ?? current.title,
    body: liveBlock.body ?? current.body,
    preview: liveBlock.preview ?? current.preview,
    detail: liveBlock.detail ?? current.detail,
    artifactIds: liveArtifactIds.length > 0 ? liveArtifactIds : currentArtifactIds,
    metadata: mergeBlockMetadata(current, liveBlock),
    result: liveBlock.result ?? current.result,
    updatedAtMs: Math.max(current.updatedAtMs, liveBlock.updatedAtMs)
  };
}

function monotonicBlockStatus(
  current: TranscriptBlockStatus,
  next: TranscriptBlockStatus
): TranscriptBlockStatus {
  return blockStatusRank(next) < blockStatusRank(current) ? current : next;
}

function blockStatusRank(status: TranscriptBlockStatus): number {
  switch (status) {
    case "pending":
      return 0;
    case "running":
    case "needsInput":
      return 1;
    case "completed":
    case "failed":
    case "cancelled":
      return 2;
    case "info":
      return 3;
  }
}

function removeSnapshotCoveredBlocks(
  entry: TranscriptEntry,
  coverage: { tools: Set<string> }
): TranscriptEntry | null {
  if (coverage.tools.size === 0) {
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

function snapshotToolCoverage(entries: TranscriptEntry[]): { tools: Set<string> } {
  const tools = new Set<string>();
  for (const entry of entries) {
    if (!isMessageDerivedEntry(entry)) {
      continue;
    }
    for (const block of blocksForEntry(entry)) {
      if (recordForValue(block.metadata).hidden === true) {
        continue;
      }
      const signatures = toolBlockSignatures(block);
      if (signatures.length > 0) {
        for (const signature of signatures) {
          tools.add(signature);
        }
        continue;
      }
    }
  }
  return { tools };
}

function blockVisibleForCoverage(block: TranscriptBlock): boolean {
  if (recordForValue(block.metadata).hidden === true) {
    return false;
  }
  return Boolean(toolBlockSignatures(block).length > 0 || normalizedBlockText(block));
}

function blockCoveredBySnapshot(
  block: TranscriptBlock,
  coverage: { tools: Set<string> }
): boolean {
  const signatures = toolBlockSignatures(block);
  return signatures.length > 0 && signatures.some((signature) => coverage.tools.has(signature));
}
