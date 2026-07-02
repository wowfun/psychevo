import { scopeForCwd } from "@psychevo/client";
import type { GatewayRequestScope, SessionSummary, ThreadSnapshot } from "@psychevo/protocol";

export function startupDraftScope(launchScope: GatewayRequestScope, sessions: SessionSummary[]): GatewayRequestScope {
  if (launchScope.cwd.trim()) {
    return launchScope;
  }
  const recentCwd = sessions.find((session) => session.cwd.trim())?.cwd ?? "";
  return scopeForCwd(recentCwd.trim() || window.location.pathname);
}

export function shortSessionId(id: string): string {
  return id.length > 12 ? `${id.slice(0, 8)}...${id.slice(-4)}` : id;
}

export function multilineList(value: string): string[] {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

export function idleActivity(): ThreadSnapshot["activity"] {
  return {
    running: false,
    activeTurnId: null,
    queuedTurns: 0
  };
}

export function normalizeActivity(activity: Partial<ThreadSnapshot["activity"]> | null | undefined): ThreadSnapshot["activity"] {
  const normalized: ThreadSnapshot["activity"] = {
    running: activity?.running === true,
    activeTurnId: typeof activity?.activeTurnId === "string" ? activity.activeTurnId : null,
    queuedTurns: Number.isFinite(activity?.queuedTurns) ? Number(activity?.queuedTurns) : 0
  };
  if (Number.isFinite(activity?.startedAtMs)) {
    normalized.startedAtMs = Number(activity?.startedAtMs);
  }
  if (Number.isFinite(activity?.updatedAtMs)) {
    normalized.updatedAtMs = Number(activity?.updatedAtMs);
  }
  if (typeof activity?.ownerId === "string") {
    normalized.ownerId = activity.ownerId;
  }
  if (typeof activity?.ownerSurface === "string") {
    normalized.ownerSurface = activity.ownerSurface;
  }
  if (Number.isFinite(activity?.leaseExpiresAtMs)) {
    normalized.leaseExpiresAtMs = Number(activity?.leaseExpiresAtMs);
  }
  if (typeof activity?.takeoverState === "string") {
    normalized.takeoverState = activity.takeoverState;
  }
  return normalized;
}

export function normalizeSnapshot(snapshot: ThreadSnapshot): ThreadSnapshot {
  return {
    ...snapshot,
    entries: Array.isArray(snapshot.entries) ? snapshot.entries : [],
    activity: normalizeActivity(snapshot.activity),
    pendingActions: Array.isArray(snapshot.pendingActions) ? snapshot.pendingActions : []
  };
}

export function normalizeSessionSummary(session: SessionSummary): SessionSummary {
  return {
    ...session,
    activity: normalizeActivity(session.activity)
  };
}
