import type {
  BackendConfigTarget,
  RuntimeAccountRateLimitsView,
  RuntimeBindingView,
  RuntimeContextReadResult,
  RuntimeControlDescriptorView,
  RuntimeCreditsSnapshotView,
  RuntimeGoalStatusView,
  RuntimeGoalView,
  RuntimeHistoryFidelityView,
  RuntimeProfileView,
  RuntimeRateLimitReachedTypeView,
  RuntimeRateLimitSnapshotView,
  RuntimeRateLimitWindowView,
  RuntimeReadinessStageView,
  RuntimeReadinessStatusView,
  RuntimeSessionOwnershipView,
  RuntimeSessionView,
  RuntimeSpendControlLimitSnapshotView
} from "@psychevo/protocol";
import type { AgentContribution, RightWorkspaceTab, WorkbenchAgent } from "./types";

const READINESS_STATES = new Set<RuntimeReadinessStatusView>([
  "unchecked",
  "ready",
  "missing",
  "needsAuth",
  "unsupported",
  "error"
]);

const SESSION_OWNERSHIP = new Set<RuntimeSessionOwnershipView>([
  "readWrite",
  "readOnly",
  "active"
]);

const HISTORY_FIDELITY = new Set<RuntimeHistoryFidelityView>([
  "full",
  "summary",
  "partial"
]);

const GOAL_STATUSES = new Set<RuntimeGoalStatusView>([
  "active",
  "paused",
  "blocked",
  "usage_limited",
  "budget_limited",
  "complete"
]);

const RATE_LIMIT_REACHED_TYPES = new Set<RuntimeRateLimitReachedTypeView>([
  "rate_limit_reached",
  "workspace_owner_credits_depleted",
  "workspace_member_credits_depleted",
  "workspace_owner_usage_limit_reached",
  "workspace_member_usage_limit_reached"
]);

export function parseRuntimeContext(value: unknown): RuntimeContextReadResult {
  const record = objectValue(value);
  const profiles = arrayValue(record.profiles).map(parseRuntimeProfile).filter((profile) => profile.id);
  const fallbackRuntimeRef = profiles.find((profile) => profile.runtime === "native")?.id
    ?? profiles[0]?.id
    ?? "native";
  return {
    runtimeRef: stringValue(record.runtimeRef) || fallbackRuntimeRef,
    selectionState: stringValue(record.selectionState) || "default",
    profiles,
    binding: record.binding == null ? null : parseRuntimeBinding(record.binding),
    controls: arrayValue(record.controls).map(parseRuntimeControl).filter((control) => control.id),
    stability: parseRuntimeStability(record.stability),
    capabilities: parseRuntimeCapabilities(record.capabilities),
    activeSession: record.activeSession == null ? null : parseRuntimeSession(record.activeSession),
    children: arrayValue(record.children).map(parseRuntimeSession).filter((session) => session.threadId),
    goal: parseRuntimeGoal(record.goal),
    accountRateLimits: parseRuntimeAccountRateLimits(record.accountRateLimits)
  };
}

export function parseRuntimeProfile(value: unknown): RuntimeProfileView {
  const record = objectValue(value);
  const runtime = stringValue(record.runtime) || "native";
  return {
    id: stringValue(record.id),
    runtime,
    enabled: record.enabled !== false,
    label: stringValue(record.label) || stringValue(record.id),
    generated: record.generated === true,
    configured: record.configured === true,
    command: nullableString(record.command),
    args: stringArray(record.args),
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
    envKeys: stringArray(record.envKeys),
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

export function registerRuntimeContextChildTabs(
  current: RightWorkspaceTab[],
  context: RuntimeContextReadResult
): RightWorkspaceTab[] {
  return context.children.reduce((tabs, child) => {
    if (!child.threadId || child.parentThreadId !== context.binding?.threadId) {
      return tabs;
    }
    const existing = tabs.find((tab) => tab.kind === "agentSession" && tab.threadId === child.threadId);
    const next: RightWorkspaceTab = {
      id: existing?.id ?? `runtime-child:${encodeURIComponent(child.threadId)}`,
      kind: "agentSession",
      title: child.title?.trim() || `${runtimeRefShortLabel(context.runtimeRef)} child`,
      threadId: child.threadId,
      parentThreadId: child.parentThreadId,
      runtimeRef: context.runtimeRef,
      runtimeStatus: child.status ?? null,
      runtimeReadOnly: child.ownership === "readOnly",
      historyFidelity: child.fidelity,
      pendingPrompt: null,
      path: null,
      diff: null,
      file: null,
      preview: null,
      message: null
    };
    return existing
      ? tabs.map((tab) => tab.id === existing.id ? { ...tab, ...next } : tab)
      : [...tabs, next];
  }, current);
}

export function runtimeSessionHistoryFidelity(value: unknown): RuntimeHistoryFidelityView | null {
  const session = objectValue(objectValue(value).session);
  const fidelity = stringValue(session.fidelity) as RuntimeHistoryFidelityView;
  return HISTORY_FIDELITY.has(fidelity) ? fidelity : null;
}

export function runtimeProfileDisplayLabel(profile: RuntimeProfileView): string {
  const label = profile.label.trim() || profile.id;
  if (profile.runtime !== "acp") {
    return label;
  }
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
  if (profile.sourceTargets.length > 0) {
    return profile.sourceTargets.map(capitalize).join(" + ");
  }
  return profile.generated ? "Generated" : "Configured";
}

export function runtimeProfileUnavailableReason(profile: RuntimeProfileView): string | null {
  if (!profile.enabled) {
    return "This Runtime Profile is disabled.";
  }
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

export function unsupportedRequiredAgentContributions(
  agent: WorkbenchAgent | null,
  profile: RuntimeProfileView | null
): AgentContribution[] {
  if (!agent || !profile || profile.runtime === "native") {
    return [];
  }
  const optional = new Set(agent.optionalContributions);
  const supported = profile.runtime === "codex" || profile.runtime === "opencode" || profile.runtime === "acp"
    ? new Set<AgentContribution>(["instructions"])
    : new Set<AgentContribution>();
  return agent.contributions.filter((contribution) => (
    !optional.has(contribution) && !supported.has(contribution)
  ));
}

export function agentPairingUnavailableReason(
  agent: WorkbenchAgent | null,
  profile: RuntimeProfileView | null
): string | null {
  if (agent && profile?.runtime === "acp" && agent.backend?.ref !== profile.backendRef) {
    return `${runtimeProfileDisplayLabel(profile)} can only use the Agent Definition backed by ${profile.backendRef ?? "its configured ACP backend"}. Choose that ACP agent or another Runtime Profile.`;
  }
  const unsupported = unsupportedRequiredAgentContributions(agent, profile);
  if (!profile || unsupported.length === 0) {
    return null;
  }
  const labels = unsupported.map(agentContributionLabel);
  const contributions = labels.length === 1
    ? labels[0]
    : `${labels.slice(0, -1).join(", ")} and ${labels.at(-1)}`;
  return `${runtimeProfileDisplayLabel(profile)} cannot faithfully apply the required Agent Definition ${contributions} contribution${labels.length === 1 ? "" : "s"}. Mark ${contributions} optional or choose Native.`;
}

function agentContributionLabel(contribution: AgentContribution): string {
  switch (contribution) {
    case "instructions": return "instructions";
    case "tools": return "tool policy";
    case "mcp": return "MCP servers";
    case "skills": return "skills";
  }
}

export function runtimeControlValueLabel(
  control: RuntimeControlDescriptorView,
  value: unknown = control.currentValue
): string {
  const choice = control.choices.find((candidate) => valuesEqual(candidate.value, value));
  if (choice) {
    return choice.label;
  }
  if (value == null || value === "") {
    return "Runtime default";
  }
  return typeof value === "string" ? value : JSON.stringify(value);
}

export function runtimeControlSelections(
  controls: RuntimeControlDescriptorView[],
  values: Record<string, unknown>
): Record<string, string> {
  return Object.fromEntries(controls.flatMap((control) => {
    if (
      control.state !== "selectable"
      || !runtimeControlDependencyMatches(control, controls, values)
      || !Object.prototype.hasOwnProperty.call(values, control.id)
    ) {
      return [];
    }
    const serialized = serializeRuntimeControlValue(values[control.id]);
    return serialized == null ? [] : [[control.id, serialized]];
  }));
}

export function runtimeControlDependencyMatches(
  control: RuntimeControlDescriptorView,
  controls: RuntimeControlDescriptorView[],
  values: Record<string, unknown>
): boolean {
  const dependency = control.dependsOn;
  if (!dependency) return true;
  if (Object.prototype.hasOwnProperty.call(values, dependency.controlId)) {
    return valuesEqual(values[dependency.controlId], dependency.value);
  }
  const source = controls.find((candidate) => candidate.id === dependency.controlId);
  if (!source) return false;
  // An unset selectable parent means "Runtime default". Dependent descriptors
  // are emitted only for that catalog default, so they remain applicable until
  // the user explicitly selects a different parent choice.
  return source.currentValue == null || valuesEqual(source.currentValue, dependency.value);
}

export function runtimeOptionsWithModeFallback(
  selections: Record<string, string>,
  mode: string
): Record<string, string> {
  return mode && !Object.prototype.hasOwnProperty.call(selections, "mode")
    ? { ...selections, mode }
    : selections;
}

function serializeRuntimeControlValue(value: unknown): string | null {
  if (typeof value === "string") return value;
  if (typeof value === "number" && Number.isFinite(value)) return String(value);
  if (typeof value === "boolean") return value ? "true" : "false";
  if (value == null) return null;
  try {
    return JSON.stringify(value);
  } catch {
    return null;
  }
}

export function formatRuntimeCheckedAt(checkedAtMs: number | null): string {
  if (checkedAtMs == null || !Number.isFinite(checkedAtMs)) {
    return "Never checked";
  }
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short"
  }).format(new Date(checkedAtMs));
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

function parseRuntimeControl(value: unknown): RuntimeControlDescriptorView {
  const record = objectValue(value);
  const state = stringValue(record.state);
  const dependency = objectValue(record.dependsOn);
  const dependencyControlId = stringValue(dependency.controlId);
  return {
    id: stringValue(record.id),
    label: stringValue(record.label) || stringValue(record.id),
    state: state === "readOnlyCurrent" || state === "selectable" ? state : "runtimeDefault",
    currentValue: record.currentValue ?? null,
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
    channelSafe: record.channelSafe === true,
    capabilityRevision: unsignedDecimalString(record.capabilityRevision)
  };
}

function parseRuntimeStability(value: unknown): RuntimeProfileView["stability"] {
  return value === "stable" || value === "experimental" || value === "unavailable"
    ? value
    : null;
}

function parseRuntimeCapabilities(value: unknown): RuntimeProfileView["capabilities"] {
  return arrayValue(value).map((capability) => {
    const record = objectValue(capability);
    return {
      id: stringValue(record.id),
      enabled: record.enabled === true,
      stability: parseRuntimeStability(record.stability) ?? "unavailable"
    };
  }).filter((capability) => capability.id);
}

function parseRuntimeBinding(value: unknown): RuntimeBindingView {
  const record = objectValue(value);
  const ownership = stringValue(record.ownership) as RuntimeSessionOwnershipView;
  return {
    threadId: stringValue(record.threadId),
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

function parseRuntimeSession(value: unknown): RuntimeSessionView {
  const record = objectValue(value);
  const fidelity = stringValue(record.fidelity) as RuntimeHistoryFidelityView;
  const ownership = stringValue(record.ownership) as RuntimeSessionOwnershipView;
  return {
    sessionHandle: stringValue(record.sessionHandle),
    threadId: nullableString(record.threadId),
    title: nullableString(record.title),
    archived: record.archived === true,
    updatedAtMs: nullableNumber(record.updatedAtMs),
    parentThreadId: nullableString(record.parentThreadId),
    status: runtimeChildStatus(record.status),
    dedupKey: stringValue(record.dedupKey) || stringValue(record.sessionHandle),
    fidelity: HISTORY_FIDELITY.has(fidelity) ? fidelity : "partial",
    ownership: SESSION_OWNERSHIP.has(ownership) ? ownership : "readOnly",
    actions: stringArray(record.actions)
  };
}

function parseRuntimeGoal(value: unknown): RuntimeGoalView | null {
  if (!isUnknownRecord(value)) return null;
  const status = stringValue(value.status) as RuntimeGoalStatusView;
  const tokenBudget = nullableNonNegativeSafeInteger(value.tokenBudget);
  const tokensUsed = requiredNonNegativeSafeInteger(value.tokensUsed);
  const timeUsedSeconds = requiredNonNegativeSafeInteger(value.timeUsedSeconds);
  const createdAt = requiredNonNegativeSafeInteger(value.createdAt);
  const updatedAt = requiredNonNegativeSafeInteger(value.updatedAt);
  const objective = stringValue(value.objective);
  if (
    !objective
    || !GOAL_STATUSES.has(status)
    || tokenBudget === undefined
    || tokensUsed == null
    || timeUsedSeconds == null
    || createdAt == null
    || updatedAt == null
  ) {
    return null;
  }
  return {
    objective,
    status,
    tokenBudget,
    tokensUsed,
    timeUsedSeconds,
    createdAt,
    updatedAt
  };
}

function parseRuntimeAccountRateLimits(value: unknown): RuntimeAccountRateLimitsView | null {
  if (!isUnknownRecord(value) || !isUnknownRecord(value.rateLimits)) return null;
  const rateLimits = parseRuntimeRateLimitSnapshot(value.rateLimits);
  if (!rateLimits || !isUnknownRecord(value.rateLimitsByLimitId)) return null;
  const rateLimitRows: Array<[string, RuntimeRateLimitSnapshotView]> = [];
  for (const [id, snapshotValue] of Object.entries(value.rateLimitsByLimitId)) {
    const snapshot = parseRuntimeRateLimitSnapshot(snapshotValue);
    if (!id || !snapshot) return null;
    rateLimitRows.push([id, snapshot]);
  }
  const rateLimitsByLimitId: RuntimeAccountRateLimitsView["rateLimitsByLimitId"] =
    Object.fromEntries(rateLimitRows);
  const resetCreditsAvailable = nullableNonNegativeSafeInteger(value.resetCreditsAvailable);
  if (resetCreditsAvailable === undefined) return null;
  return { rateLimits, rateLimitsByLimitId, resetCreditsAvailable };
}

function parseRuntimeRateLimitSnapshot(value: unknown): RuntimeRateLimitSnapshotView | null {
  if (!isUnknownRecord(value)) return null;
  const limitId = parsedNullableString(value.limitId);
  const limitName = parsedNullableString(value.limitName);
  const primary = parsedNullableObject(value.primary, parseRuntimeRateLimitWindow);
  const secondary = parsedNullableObject(value.secondary, parseRuntimeRateLimitWindow);
  const credits = parsedNullableObject(value.credits, parseRuntimeCreditsSnapshot);
  const individualLimit = parsedNullableObject(
    value.individualLimit,
    parseRuntimeSpendControlLimitSnapshot
  );
  const planType = parsedNullableString(value.planType);
  const reachedType = value.rateLimitReachedType;
  const rateLimitReachedType = reachedType == null
    ? null
    : stringValue(reachedType) as RuntimeRateLimitReachedTypeView;
  if (
    limitId === undefined
    || limitName === undefined
    || primary === undefined
    || secondary === undefined
    || credits === undefined
    || individualLimit === undefined
    || planType === undefined
    || (rateLimitReachedType != null && !RATE_LIMIT_REACHED_TYPES.has(rateLimitReachedType))
  ) {
    return null;
  }
  return {
    limitId,
    limitName,
    primary,
    secondary,
    credits,
    individualLimit,
    planType,
    rateLimitReachedType
  };
}

function parseRuntimeRateLimitWindow(value: unknown): RuntimeRateLimitWindowView | null {
  if (!isUnknownRecord(value)) return null;
  const usedPercent = requiredSafeInteger(value.usedPercent);
  const windowDurationMins = nullableNonNegativeSafeInteger(value.windowDurationMins);
  const resetsAt = nullableNonNegativeSafeInteger(value.resetsAt);
  if (usedPercent == null || windowDurationMins === undefined || resetsAt === undefined) return null;
  return { usedPercent, windowDurationMins, resetsAt };
}

function parseRuntimeCreditsSnapshot(value: unknown): RuntimeCreditsSnapshotView | null {
  if (
    !isUnknownRecord(value)
    || typeof value.hasCredits !== "boolean"
    || typeof value.unlimited !== "boolean"
  ) {
    return null;
  }
  const balance = parsedNullableString(value.balance);
  if (balance === undefined) return null;
  return { hasCredits: value.hasCredits, unlimited: value.unlimited, balance };
}

function parseRuntimeSpendControlLimitSnapshot(
  value: unknown
): RuntimeSpendControlLimitSnapshotView | null {
  if (!isUnknownRecord(value)) return null;
  const limit = stringValue(value.limit);
  const used = stringValue(value.used);
  const remainingPercent = requiredSafeInteger(value.remainingPercent);
  const resetsAt = requiredNonNegativeSafeInteger(value.resetsAt);
  if (!limit || !used || remainingPercent == null || resetsAt == null) return null;
  return { limit, used, remainingPercent, resetsAt };
}

function parsedNullableObject<T>(
  value: unknown,
  parse: (candidate: unknown) => T | null
): T | null | undefined {
  if (value == null) return null;
  return parse(value) ?? undefined;
}

function parsedNullableString(value: unknown): string | null | undefined {
  if (value == null) return null;
  return typeof value === "string" ? value : undefined;
}

function nullableNonNegativeSafeInteger(value: unknown): number | null | undefined {
  if (value == null) return null;
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : undefined;
}

function requiredSafeInteger(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) ? value : null;
}

function requiredNonNegativeSafeInteger(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null;
}

function defaultRuntimeProvenance(runtime: string): string {
  if (runtime === "acp") return "ACP";
  if (runtime === "native") return "Native";
  return "Direct";
}

function runtimeRefShortLabel(runtimeRef: string): string {
  if (runtimeRef === "codex") return "Codex";
  if (runtimeRef === "opencode") return "OpenCode";
  if (runtimeRef === "native") return "Native";
  return runtimeRef;
}

function objectValue(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function isUnknownRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function arrayValue(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function stringValue(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function nullableString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function runtimeChildStatus(value: unknown): string | null {
  if (typeof value !== "string" || !/^[A-Za-z][A-Za-z0-9_-]{0,63}$/.test(value)) return null;
  return value;
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
}

function nullableNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function nonNegativeNumber(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : 0;
}

function unsignedDecimalString(value: unknown): string {
  if (typeof value !== "string" || !/^(?:0|[1-9][0-9]*)$/.test(value)) return "0";
  try {
    return BigInt(value) <= 18_446_744_073_709_551_615n ? value : "0";
  } catch {
    return "0";
  }
}

function isBackendConfigTarget(value: string): value is BackendConfigTarget {
  return value === "project" || value === "profile";
}

function capitalize(value: string): string {
  return value ? `${value.charAt(0).toUpperCase()}${value.slice(1)}` : value;
}

function valuesEqual(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true;
  try {
    return JSON.stringify(left) === JSON.stringify(right);
  } catch {
    return false;
  }
}
