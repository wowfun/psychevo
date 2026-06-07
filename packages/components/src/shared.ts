import type { GatewayActivity } from "@psychevo/protocol";

const IDLE_ACTIVITY: GatewayActivity = {
  running: false,
  activeTurnId: null,
  queuedTurns: 0
};

export function asRecord(value: unknown): Record<string, unknown> {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  if (typeof value === "string" && value.trim().startsWith("{")) {
    try {
      const parsed = JSON.parse(value) as unknown;
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        return parsed as Record<string, unknown>;
      }
    } catch {
      return {};
    }
  }
  return {};
}

export function stringValue(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

export function compactJson(value: unknown): string {
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

export function prettyJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

export function compactText(text: string, max: number): string {
  const normalized = text.replace(/\s+/g, " ").trim();
  if (normalized.length <= max) {
    return normalized;
  }
  return `${normalized.slice(0, Math.max(0, max - 3))}...`;
}

export function normalizeActivity(activity?: Partial<GatewayActivity> | null): GatewayActivity {
  const queuedTurns = activity?.queuedTurns;
  return {
    running: activity?.running === true,
    activeTurnId: typeof activity?.activeTurnId === "string" ? activity.activeTurnId : null,
    queuedTurns: Number.isFinite(queuedTurns) ? Number(queuedTurns) : IDLE_ACTIVITY.queuedTurns
  };
}
