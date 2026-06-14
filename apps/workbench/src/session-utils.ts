import { scopeForWorkdir } from "@psychevo/client";
import type { GatewayRequestScope, SessionSummary, ThreadSnapshot } from "@psychevo/protocol";

export function startupDraftScope(launchScope: GatewayRequestScope, sessions: SessionSummary[]): GatewayRequestScope {
  if (launchScope.workdir?.trim()) {
    return launchScope;
  }
  const recentWorkdir = sessions.find((session) => session.workdir?.trim())?.workdir;
  return scopeForWorkdir(recentWorkdir?.trim() || window.location.pathname);
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
  return { running: false, activeTurnId: null, queuedTurns: 0 };
}

export function normalizeActivity(activity: Partial<ThreadSnapshot["activity"]> | null | undefined): ThreadSnapshot["activity"] {
  return {
    running: activity?.running === true,
    activeTurnId: typeof activity?.activeTurnId === "string" ? activity.activeTurnId : null,
    queuedTurns: Number.isFinite(activity?.queuedTurns) ? Number(activity?.queuedTurns) : 0
  };
}

export function normalizeSnapshot(snapshot: ThreadSnapshot): ThreadSnapshot {
  return {
    ...snapshot,
    entries: Array.isArray(snapshot.entries) ? snapshot.entries : [],
    activity: normalizeActivity(snapshot.activity),
    pendingPermissions: Array.isArray(snapshot.pendingPermissions) ? snapshot.pendingPermissions : [],
    pendingClarifies: Array.isArray(snapshot.pendingClarifies) ? snapshot.pendingClarifies : []
  };
}

export function normalizeSessionSummary(session: SessionSummary): SessionSummary {
  return {
    ...session,
    activity: normalizeActivity(session.activity)
  };
}
