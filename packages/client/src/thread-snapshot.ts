import {
  ThreadSnapshotSchema,
  type GatewayRequestScope,
  type ThreadSnapshot
} from "@psychevo/protocol";

export function parseThreadSnapshot(value: unknown): ThreadSnapshot {
  return ThreadSnapshotSchema.parse(withThreadSnapshotDefaults(value));
}

function withThreadSnapshotDefaults(value: unknown): unknown {
  const record = asRecord(value);
  if (!record) {
    return value;
  }
  return {
    ...record,
    scope: record.scope ?? defaultScopeFromSource(record.source),
    thread: Object.prototype.hasOwnProperty.call(record, "thread") ? record.thread : null,
    activity: withActivityDefaults(record.activity),
    pendingActions: Array.isArray(record.pendingActions) ? record.pendingActions : []
  };
}

function defaultScopeFromSource(value: unknown): GatewayRequestScope {
  const source = asRecord(value);
  return {
    cwd: "",
    source: {
      kind: typeof source?.kind === "string" ? source.kind : "web",
      rawId: typeof source?.rawId === "string" ? source.rawId : null,
      lifetime: source?.lifetime === "invocation"
        || source?.lifetime === "process"
        || source?.lifetime === "persistent"
        ? source.lifetime
        : "persistent",
      rawIdentity: source?.rawIdentity ?? null,
      visibleName: typeof source?.visibleName === "string" ? source.visibleName : null
    }
  };
}

function withActivityDefaults(value: unknown): Record<string, unknown> {
  const activity = asRecord(value) ?? {};
  return {
    ...activity,
    running: activity.running === true,
    activeTurnId: typeof activity.activeTurnId === "string" ? activity.activeTurnId : null,
    queuedTurns: Number.isFinite(activity.queuedTurns) ? activity.queuedTurns : 0
  };
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}
