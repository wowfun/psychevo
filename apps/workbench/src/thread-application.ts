import type { GatewayClient } from "@psychevo/client";
import type {
  GatewayRequestScope,
  ThreadActionKind,
  ThreadActionDescriptorView,
  ThreadContextReadResult,
  ThreadHistoryView,
  ThreadSnapshot,
  TranscriptEntry
} from "@psychevo/protocol";

export type WorkbenchThreadActionKind = ThreadActionKind;

export type ThreadApplicationTarget = {
  scope: GatewayRequestScope;
  threadId: string;
};

export type ProjectedThreadHistory = {
  entries: TranscriptEntry[];
  history: ThreadHistoryView;
};

type ThreadHistoryPage = ProjectedThreadHistory & {
  threadId: string;
  nextCursor: string | null;
};

export function threadApplicationTarget(
  scope: GatewayRequestScope | null | undefined,
  threadId: string | null | undefined
): ThreadApplicationTarget | null {
  return scope && threadId ? { scope, threadId } : null;
}

export function snapshotThreadApplicationTarget(
  snapshot: ThreadSnapshot,
  requestedThreadId?: string | null
): ThreadApplicationTarget | null {
  const threadId = snapshot.thread?.id ?? null;
  if (!threadId || (requestedThreadId && requestedThreadId !== threadId)) {
    return null;
  }
  return { scope: snapshot.scope, threadId };
}

export function threadActionDescriptor(
  context: ThreadContextReadResult | null | undefined,
  kind: WorkbenchThreadActionKind
): ThreadActionDescriptorView | null {
  return context?.actions.find((action) => action.id === kind) ?? null;
}

export function enabledThreadAction(
  context: ThreadContextReadResult | null | undefined,
  kind: WorkbenchThreadActionKind
): ThreadActionDescriptorView | null {
  const descriptor = threadActionDescriptor(context, kind);
  return descriptor?.enabled ? descriptor : null;
}

export async function readProjectedThreadHistory(
  client: GatewayClient,
  scope: GatewayRequestScope,
  threadId: string
): Promise<ProjectedThreadHistory> {
  const entries: TranscriptEntry[] = [];
  const cursors = new Set<string>();
  let cursor: string | null = null;
  let history: ThreadHistoryView | null = null;

  do {
    const page: ThreadHistoryPage = await client.request("thread/history/read", {
      scope,
      threadId,
      cursor,
      limit: 200
    });
    if (page.threadId !== threadId) {
      throw new Error(`Thread history response changed identity from ${threadId} to ${page.threadId}.`);
    }
    entries.push(...page.entries);
    history = page.history;
    cursor = page.nextCursor;
    if (cursor) {
      if (cursors.has(cursor)) {
        throw new Error(`Thread history repeated cursor ${cursor}.`);
      }
      cursors.add(cursor);
    }
  } while (cursor);

  if (!history) {
    throw new Error(`Thread history for ${threadId} returned no page.`);
  }
  return { entries, history };
}

export async function hydrateThreadSnapshotHistory(
  client: GatewayClient,
  snapshot: ThreadSnapshot
): Promise<ThreadSnapshot> {
  const target = snapshotThreadApplicationTarget(snapshot);
  if (!target) {
    return snapshot;
  }
  const projected = await readProjectedThreadHistory(client, target.scope, target.threadId);
  return {
    ...snapshot,
    entries: projected.entries,
    history: projected.history
  };
}
