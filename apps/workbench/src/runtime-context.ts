import type {
  BackendConfigTarget,
  RuntimeBindingView,
  RuntimeProfileView,
  RuntimeReadinessStageView,
  RuntimeReadinessStatusView,
  RuntimeBindingOwnershipView,
  RunnableTargetView,
  ThreadActionDescriptorView,
  ThreadActionKind,
  ThreadContextReadResult,
  ThreadControlDescriptorView,
  ThreadHistoryOwnerView
} from "@psychevo/protocol";

const READINESS_STATES = new Set<RuntimeReadinessStatusView>([
  "unchecked",
  "ready",
  "missing",
  "needsAuth",
  "unsupported",
  "error"
]);

const SESSION_OWNERSHIP = new Set<RuntimeBindingOwnershipView>([
  "readWrite",
  "readOnly",
  "active"
]);

const THREAD_ACTION_KINDS = new Set<ThreadActionKind>([
  "interrupt",
  "steer",
  "compact",
  "fork",
  "forkBefore",
  "revertConversation",
  "unrevertConversation"
]);

export function parseThreadContext(value: unknown): ThreadContextReadResult {
  const record = objectValue(value);
  const profiles = arrayValue(record.profiles).map(parseRuntimeProfile).filter((profile) => profile.id);
  const runtimeProfileRef = stringValue(record.runtimeProfileRef)
    || "";
  const targetId = stringValue(record.targetId).trim();
  if (!targetId) {
    throw new Error("Thread Context is missing its canonical targetId.");
  }
  const sendability = objectValue(record.sendability);
  const history = objectValue(record.history);
  return {
    targetId,
    runtimeProfileRef,
    selectionState: stringValue(record.selectionState) || "default",
    profiles,
    binding: record.binding == null ? null : parseRuntimeBinding(record.binding),
    controls: arrayValue(record.controls).map(parseThreadControl).filter((control) => control.id),
    stability: parseRuntimeStability(record.stability),
    capabilities: parseRuntimeCapabilities(record.capabilities),
    compatibleTargets: arrayValue(record.compatibleTargets).map((target) => {
      const item = objectValue(target);
      return {
        targetId: stringValue(item.targetId),
        agentRef: nullableString(item.agentRef),
        runtimeProfileRef: stringValue(item.runtimeProfileRef),
        agentLabel: stringValue(item.agentLabel),
        profileLabel: stringValue(item.profileLabel),
        label: stringValue(item.label),
        ready: item.ready === true,
        unavailableReason: nullableString(item.unavailableReason)
      };
    }).filter((target) => target.targetId && target.runtimeProfileRef),
    inputCapabilities: arrayValue(record.inputCapabilities).map((capability) => {
      const item = objectValue(capability);
      return {
        kind: stringValue(item.kind),
        enabled: item.enabled === true,
        unavailableReason: nullableString(item.unavailableReason)
      };
    }).filter((capability) => capability.kind),
    actions: arrayValue(record.actions).flatMap((action): ThreadActionDescriptorView[] => {
      const item = objectValue(action);
      const id = threadActionKind(item.id);
      if (!id) return [];
      return [{
        id,
        label: stringValue(item.label) || stringValue(item.id),
        enabled: item.enabled === true,
        stability: parseRuntimeStability(item.stability) ?? "unavailable",
        channelSafe: item.channelSafe === true,
        unavailableReason: nullableString(item.unavailableReason)
      }];
    }),
    sendability: {
      allowed: sendability.allowed === true,
      reason: nullableString(sendability.reason),
      recoveryAction: nullableString(sendability.recoveryAction)
    },
    history: {
      owner: threadHistoryOwner(history.owner),
      fidelity: history.fidelity === "full"
        || history.fidelity === "summary"
        || history.fidelity === "partial"
        ? history.fidelity
        : "unavailable",
      cursor: nullableString(history.cursor),
      hint: nullableString(history.hint)
    },
    pendingInteractions: arrayValue(record.pendingInteractions) as ThreadContextReadResult["pendingInteractions"],
    contextRevision: stringValue(record.contextRevision),
    controlRevision: stringValue(record.controlRevision)
  };
}

function threadActionKind(value: unknown): ThreadActionKind | null {
  const action = stringValue(value) as ThreadActionKind;
  return THREAD_ACTION_KINDS.has(action) ? action : null;
}

function threadHistoryOwner(value: unknown): ThreadHistoryOwnerView {
  return value === "agent" || value === "process" ? value : "psychevo";
}

export function parseRuntimeProfile(value: unknown): RuntimeProfileView {
  const record = objectValue(value);
  const runtimeValue = stringValue(record.runtime);
  const runtime = runtimeValue === "acp" || runtimeValue === "native" ? runtimeValue : "";
  return {
    id: stringValue(record.id),
    runtime,
    enabled: record.enabled !== false,
    label: stringValue(record.label) || stringValue(record.id),
    generated: record.generated === true,
    configured: record.configured === true,
    backendRef: nullableString(record.backendRef),
    provenance: stringValue(record.provenance) || defaultRuntimeProvenance(runtime),
    profileRevision: unsignedDecimalString(record.profileRevision),
    capabilityRevision: unsignedDecimalString(record.capabilityRevision),
    stability: parseRuntimeStability(record.stability),
    capabilities: parseRuntimeCapabilities(record.capabilities),
    defaultModel: nullableString(record.defaultModel),
    defaultMode: nullableString(record.defaultMode),
    defaultAgent: nullableString(record.defaultAgent),
    approvalMode: nullableString(record.approvalMode),
    sandbox: nullableString(record.sandbox),
    workspaceRoots: stringArray(record.workspaceRoots),
    optionKeys: stringArray(record.optionKeys),
    sourceTargets: stringArray(record.sourceTargets).filter(isBackendConfigTarget),
    health: parseRuntimeHealth(record.health),
    readinessStages: arrayValue(record.readinessStages).map(parseReadinessStage).filter((stage) => stage.id),
    diagnostics: arrayValue(record.diagnostics).map((diagnostic) => {
      const item = objectValue(diagnostic);
      return { kind: stringValue(item.kind), message: stringValue(item.message) };
    }).filter((diagnostic) => diagnostic.message)
  };
}

export function runtimeProfileDisplayLabel(profile: RuntimeProfileView): string {
  const label = profile.label.trim() || profile.id;
  if (profile.runtime !== "acp") return label;
  const base = label.replace(/\s*\(ACP\)\s*$/i, "").trim()
    || profile.id.replace(/^acp:/i, "").trim();
  return `${base} (ACP)`;
}

export function runtimeProfileProvenance(profile: RuntimeProfileView): string {
  return profile.provenance.trim() || defaultRuntimeProvenance(profile.runtime);
}

export function runtimeProfileCapsuleLabel(profile: RuntimeProfileView): string {
  return `${runtimeProfileDisplayLabel(profile)} · ${runtimeProfileProvenance(profile)}`;
}

export function runtimeProfileSourceLabel(profile: RuntimeProfileView): string {
  if (profile.sourceTargets.length > 0) return profile.sourceTargets.map(capitalize).join(" + ");
  return profile.generated ? "Generated" : "Configured";
}

export function runtimeProfileUnavailableReason(profile: RuntimeProfileView): string | null {
  if (!profile.enabled) return "This Runtime Profile is disabled.";
  switch (profile.health.status) {
    case "missing":
      return `${runtimeProfileDisplayLabel(profile)} is missing on this device. Open Runtime Profiles to repair it.`;
    case "needsAuth":
      return `${runtimeProfileDisplayLabel(profile)} needs authentication. Open Runtime Profiles to repair it.`;
    case "unsupported":
    case "error":
      return profile.health.summary || `${runtimeProfileDisplayLabel(profile)} is not ready.`;
    default:
      return null;
  }
}

export function runnableTargetUnavailableReason(target: RunnableTargetView | null): string | null {
  if (!target) return "Select an Agent target before starting a turn.";
  return target.ready
    ? null
    : target.unavailableReason ?? `${target.label || target.targetId} is not ready.`;
}

export function runtimeControlValueLabel(
  control: ThreadControlDescriptorView,
  value: unknown = control.effectiveValue
): string {
  const choice = control.choices.find((candidate) => valuesEqual(candidate.value, value));
  if (choice) return choice.label;
  if (value == null || value === "") return "Unavailable";
  return typeof value === "string" ? value : JSON.stringify(value);
}

export function runtimeControlSelections(
  controls: ThreadControlDescriptorView[],
  values: Record<string, unknown>
): Record<string, unknown> {
  return Object.fromEntries(controls.flatMap((control) => {
    if (
      control.mutability !== "selectable"
      || !control.enabled
      || !runtimeControlDependencyMatches(control, controls, values)
      || !Object.prototype.hasOwnProperty.call(values, control.id)
    ) return [];
    const value = values[control.id];
    return value == null ? [] : [[control.id, value]];
  }));
}

export function runtimeControlDependencyMatches(
  control: ThreadControlDescriptorView,
  controls: ThreadControlDescriptorView[],
  values: Record<string, unknown>
): boolean {
  const dependency = control.dependsOn;
  if (!dependency) return true;
  if (Object.prototype.hasOwnProperty.call(values, dependency.controlId)) {
    return valuesEqual(values[dependency.controlId], dependency.value);
  }
  const source = controls.find((candidate) => candidate.id === dependency.controlId);
  return source ? valuesEqual(source.effectiveValue, dependency.value) : false;
}

export function formatRuntimeCheckedAt(checkedAtMs: number | null): string {
  if (checkedAtMs == null || !Number.isFinite(checkedAtMs)) return "Never checked";
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short"
  }).format(new Date(checkedAtMs));
}

function parseThreadControl(value: unknown): ThreadControlDescriptorView {
  const record = objectValue(value);
  const surfaceRole = stringValue(record.surfaceRole);
  const effectiveSource = stringValue(record.effectiveSource);
  const dependency = objectValue(record.dependsOn);
  const dependencyControlId = stringValue(dependency.controlId);
  return {
    id: stringValue(record.id),
    label: stringValue(record.label) || stringValue(record.id),
    surfaceRole: surfaceRole === "mode" || surfaceRole === "model" || surfaceRole === "reasoning"
      ? surfaceRole
      : "advanced",
    mutability: record.mutability === "selectable" ? "selectable" : "readOnly",
    enabled: record.enabled === true,
    required: record.required === true,
    unavailableReason: nullableString(record.unavailableReason),
    effectiveValue: record.effectiveValue ?? null,
    effectiveSource: parseControlEffectiveSource(effectiveSource),
    isDefault: record.isDefault === true,
    choices: arrayValue(record.choices).map((choice) => {
      const item = objectValue(choice);
      return {
        value: item.value,
        label: stringValue(item.label) || String(item.value ?? ""),
        description: nullableString(item.description)
      };
    }),
    dependsOn: dependencyControlId
      ? { controlId: dependencyControlId, value: dependency.value }
      : null,
    applyScope: record.applyScope === "session" ? "session" : "turnDraft",
    stability: parseRuntimeStability(record.stability) ?? "unavailable",
    channelSafe: record.channelSafe === true,
    capabilityRevision: stringValue(record.capabilityRevision)
  };
}

function parseControlEffectiveSource(value: string): ThreadControlDescriptorView["effectiveSource"] {
  switch (value) {
    case "profileDefault":
    case "sourceDraft":
    case "threadPreference":
    case "turnOverride":
    case "runtimeObserved":
      return value;
    default:
      return "runtimeDefault";
  }
}

function parseRuntimeHealth(value: unknown): RuntimeProfileView["health"] {
  const record = objectValue(value);
  return {
    status: stringValue(record.status) || "unchecked",
    summary: stringValue(record.summary) || "Not checked",
    commandPath: nullableString(record.commandPath),
    checkedAtMs: nullableNumber(record.checkedAtMs)
  };
}

function parseReadinessStage(value: unknown): RuntimeReadinessStageView {
  const record = objectValue(value);
  const status = stringValue(record.status) as RuntimeReadinessStatusView;
  return {
    id: stringValue(record.id),
    status: READINESS_STATES.has(status) ? status : "unchecked",
    summary: stringValue(record.summary),
    observedAtMs: nullableNumber(record.observedAtMs)
  };
}

function parseRuntimeStability(value: unknown): RuntimeProfileView["stability"] {
  return value === "stable" || value === "experimental" || value === "unavailable" ? value : null;
}

function parseRuntimeCapabilities(value: unknown): RuntimeProfileView["capabilities"] {
  return arrayValue(value).map((capability) => {
    const record = objectValue(capability);
    return {
      id: stringValue(record.id),
      enabled: record.enabled === true,
      stability: parseRuntimeStability(record.stability) ?? "unavailable",
      unavailableReason: nullableString(record.unavailableReason)
    };
  }).filter((capability) => capability.id);
}

function parseRuntimeBinding(value: unknown): RuntimeBindingView {
  const record = objectValue(value);
  const ownership = stringValue(record.ownership) as RuntimeBindingOwnershipView;
  return {
    threadId: stringValue(record.threadId),
    agentRef: nullableString(record.agentRef),
    agentFingerprint: stringValue(record.agentFingerprint),
    runtimeRef: stringValue(record.runtimeRef),
    backendKind: stringValue(record.backendKind),
    nativeKind: nullableString(record.nativeKind),
    sessionHandle: nullableString(record.sessionHandle),
    cwd: stringValue(record.cwd),
    profileFingerprint: stringValue(record.profileFingerprint),
    ownership: SESSION_OWNERSHIP.has(ownership) ? ownership : "readOnly",
    bindingRevision: nonNegativeNumber(record.bindingRevision)
  };
}

function defaultRuntimeProvenance(runtime: string): string {
  return runtime === "native" ? "Built in" : "ACP backend";
}

function isBackendConfigTarget(value: string): value is BackendConfigTarget {
  return value === "project" || value === "profile";
}

function valuesEqual(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true;
  try {
    return JSON.stringify(left) === JSON.stringify(right);
  } catch {
    return false;
  }
}

function objectValue(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function arrayValue(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function stringValue(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function nullableString(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

function nullableNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function nonNegativeNumber(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : 0;
}

function unsignedDecimalString(value: unknown): string {
  if (typeof value === "string" && /^\d+$/.test(value)) return value;
  if (typeof value === "number" && Number.isSafeInteger(value) && value >= 0) return String(value);
  return "0";
}

function stringArray(value: unknown): string[] {
  return arrayValue(value).map(stringValue).filter(Boolean);
}

function capitalize(value: string): string {
  return value ? `${value[0]?.toUpperCase()}${value.slice(1)}` : value;
}
