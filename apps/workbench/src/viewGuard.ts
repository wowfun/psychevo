import type { HistoryDraftSession } from "@psychevo/components";
import type { ThreadSnapshot } from "@psychevo/protocol";

export type PendingDetachedShell = {
  epoch: number;
  token: number;
};

export function shouldApplyReadOnlySnapshot(
  current: ThreadSnapshot,
  threadId: string,
  currentEpoch: number,
  expectedEpoch: number | null | undefined,
  allowDetachedAdoption = false
): boolean {
  if (expectedEpoch != null && expectedEpoch !== currentEpoch) {
    return false;
  }
  if (current.thread?.id === threadId) {
    return true;
  }
  if (current.thread) {
    return false;
  }
  return allowDetachedAdoption;
}

export function shouldAdoptDetachedShellResult(
  current: ThreadSnapshot,
  threadId: string | null,
  currentEpoch: number,
  pending: PendingDetachedShell | null
): boolean {
  return Boolean(
    threadId &&
    pending &&
    pending.epoch === currentEpoch &&
    shouldApplyReadOnlySnapshot(current, threadId, currentEpoch, pending.epoch, true)
  );
}

export function createHistoryDraftSession(
  epoch: number,
  workdir: string,
  now = Date.now()
): HistoryDraftSession {
  return {
    id: `draft:${epoch}`,
    title: "New session",
    createdAtMs: now,
    workdir
  };
}

export function visibleHistoryDraftSession(
  draftSession: HistoryDraftSession | null,
  archived: boolean
): HistoryDraftSession | null {
  return archived ? null : draftSession;
}
