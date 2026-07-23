import type {
  GatewayEvent,
  GatewayInputPart,
  GatewayMention,
  GatewayRequestScope,
  GatewaySource,
  GatewayThread,
  ThreadContextReadResult,
  ThreadControlDescriptorView,
  ThreadControlSetParams,
  ThreadControlSetResult,
  ThreadSnapshot,
  TranscriptEntry,
  TurnStartParams,
  TurnStartResult,
  RunnableTargetInput,
  RunnableTargetView
} from "@psychevo/protocol";
import {
  appendOptimisticPrompt,
  applyLiveTranscriptEvent,
  reconcileThreadSnapshot
} from "./transcript";

export interface ThreadTurnPreparation {
  clientTurnId: string;
  previousSnapshot: ThreadSnapshot;
  requestedThreadId: string | null;
  snapshot: ThreadSnapshot;
}

export interface ThreadTurnAcceptance {
  threadId: string;
  thread: GatewayThread;
  snapshot: ThreadSnapshot;
}

export interface ThreadTurnControls {
  targetId: string;
  omitTarget?: boolean;
  turnOverrides?: Record<string, unknown>;
  expectedContextRevision?: string | null;
  expectedControlRevision?: string | null;
}

export interface ThreadTurnStartInput {
  controls?: ThreadTurnControls;
  input: GatewayInputPart[];
  mentions?: GatewayMention[];
  optimisticText: string;
  scope: GatewayRequestScope;
  startedAtMs?: number;
  threadId?: string | null;
}

export interface ThreadTurnStartPlan {
  params: TurnStartParams;
  prepared: ThreadTurnPreparation;
  snapshot: ThreadSnapshot;
}

export interface ThreadTurnAdmission {
  allowed: boolean;
  reason: string | null;
}

export interface ThreadGatewayEventApplication {
  applied: boolean;
  completed: boolean;
  running: boolean | null;
  snapshot: ThreadSnapshot | null;
}

export class ThreadController {
  private activeThreadId: string | null = null;
  private activeTurnId: string | null = null;
  private acceptingFirstTurn = false;
  private awaitingTurnStartAcceptance = false;
  private settledBeforeAcceptanceTurnId: string | null = null;
  private settledTurnId: string | null = null;
  private currentSnapshot: ThreadSnapshot | null;
  private currentContext: ThreadContextReadResult | null = null;
  private readonly snapshotListeners = new Set<() => void>();
  private snapshotBatchDepth = 0;
  private snapshotNotificationPending = false;

  constructor(snapshot: ThreadSnapshot | null = null) {
    this.currentSnapshot = snapshot;
    this.activeThreadId = snapshot?.thread?.id ?? null;
    this.activeTurnId = snapshot?.activity.activeTurnId ?? null;
  }

  snapshot(): ThreadSnapshot | null {
    return this.currentSnapshot;
  }

  subscribe(listener: () => void): () => void {
    this.snapshotListeners.add(listener);
    return () => this.snapshotListeners.delete(listener);
  }

  context(): ThreadContextReadResult | null {
    return this.currentContext;
  }

  setContext(context: ThreadContextReadResult | null): void {
    this.currentContext = context;
  }

  target(targetId: string): RunnableTargetView | null {
    return this.currentContext?.compatibleTargets.find((target) => target.targetId === targetId) ?? null;
  }

  contextReadTarget(targetId: string): RunnableTargetInput | null {
    const target = this.target(targetId);
    return target ? {
      agentRef: target.agentRef ?? null,
      runtimeProfileRef: target.runtimeProfileRef
    } : null;
  }

  applyControlReceipt(receipt: ThreadControlSetResult): void {
    this.currentContext = receipt.context;
  }

  turnControls(
    targetId: string,
    turnOverrides: Record<string, unknown>
  ): ThreadTurnControls {
    const context = this.currentContext;
    return {
      targetId,
      omitTarget: Boolean(context?.binding),
      turnOverrides,
      expectedContextRevision: context?.contextRevision ?? null,
      expectedControlRevision: context?.controlRevision ?? null
    };
  }

  controlSetParams(
    targetId: string,
    control: ThreadControlDescriptorView,
    value: unknown,
    scope: GatewayRequestScope,
    threadId: string | null
  ): ThreadControlSetParams {
    const context = this.currentContext;
    const target = this.target(targetId);
    if (!context || !target || context.selectedTargetId !== targetId) {
      throw new Error("The selected Agent target does not match the current Thread Context.");
    }
    if (!control.enabled || control.mutability !== "selectable") {
      throw new Error(control.unavailableReason ?? `${control.label} is unavailable.`);
    }
    return {
      threadId,
      targetId: target.targetId,
      controlId: control.id,
      value,
      expectedCapabilityRevision: control.capabilityRevision,
      expectedBindingRevision: context.binding?.bindingRevision ?? 0,
      expectedContextRevision: context.contextRevision,
      expectedControlRevision: context.controlRevision,
      scope
    };
  }

  sendability(): ThreadTurnAdmission {
    return this.currentContext?.sendability ?? {
      allowed: false,
      reason: "Thread Context is required before starting a turn."
    };
  }

  admitTurn(input: Pick<ThreadTurnStartInput, "controls" | "input" | "mentions">): ThreadTurnAdmission {
    const context = this.currentContext;
    if (!context) {
      return this.sendability();
    }
    if (!context.sendability.allowed) {
      return {
        allowed: false,
        reason: context.sendability.reason ?? "This Agent target cannot start a turn."
      };
    }
    if (!context.contextRevision.trim() || !context.controlRevision.trim()) {
      return {
        allowed: false,
        reason: "Thread Context revisions are required before starting a turn."
      };
    }
    const targetAdmission = admitTurnTarget(context, input.controls);
    if (!targetAdmission.allowed) return targetAdmission;

    for (const control of context.controls) {
      if (!control.required) continue;
      if (!control.enabled) {
        return {
          allowed: false,
          reason: control.unavailableReason ?? `${control.label} is required but unavailable.`
        };
      }
      const override = input.controls?.turnOverrides?.[control.id];
      if (override == null && control.effectiveValue == null) {
        return {
          allowed: false,
          reason: control.unavailableReason ?? `${control.label} is required before starting a turn.`
        };
      }
    }

    return this.admitInput(input.input, input.mentions);
  }

  admitInput(input: GatewayInputPart[], mentions: GatewayMention[] = []): ThreadTurnAdmission {
    const context = this.currentContext;
    if (!context) {
      return {
        allowed: false,
        reason: "Thread Context is required before adding turn input."
      };
    }
    for (const part of input) {
      const admission = admitInputCapability(context, inputCapabilityKind(part));
      if (!admission.allowed) return admission;
    }
    if (mentions.some((mention) => mention.target.kind === "agent")) {
      return admitInputCapability(context, "agentMention");
    }
    return { allowed: true, reason: null };
  }

  threadId(): string | null {
    return this.activeThreadId;
  }

  turnId(): string | null {
    return this.activeTurnId;
  }

  reset(snapshot: ThreadSnapshot | null): void {
    const sameThread = this.activeThreadId !== null && snapshot?.thread?.id === this.activeThreadId;
    const preservePendingAcceptance = this.awaitingTurnStartAcceptance &&
      sameThread;
    this.activeThreadId = snapshot?.thread?.id ?? null;
    this.activeTurnId = snapshot?.activity.activeTurnId ?? null;
    if (!preservePendingAcceptance) {
      this.acceptingFirstTurn = false;
      this.awaitingTurnStartAcceptance = false;
      this.settledBeforeAcceptanceTurnId = null;
    }
    if (!sameThread) this.settledTurnId = null;
    if (!snapshot?.thread) this.currentContext = null;
    this.replaceSnapshot(snapshot);
  }

  setThreadId(threadId: string | null): void {
    this.activeThreadId = threadId;
  }

  beginTurn(input: ThreadTurnStartInput): ThreadTurnStartPlan {
    if (this.awaitingTurnStartAcceptance) {
      throw new Error("A turn is already awaiting Gateway acceptance.");
    }
    const admission = this.admitTurn(input);
    if (!admission.allowed) {
      throw new Error(admission.reason ?? "This turn is not admitted by Thread Context.");
    }
    const snapshot = this.currentSnapshot ?? emptyThreadSnapshot(input.scope, input.threadId ?? null);
    const requestedThreadId = input.threadId ?? snapshot.thread?.id ?? null;
    const clientTurnId = createClientTurnId();
    const prepared = prepareThreadTurn(
      snapshot,
      input.optimisticText,
      requestedThreadId,
      input.startedAtMs ?? Date.now(),
      clientTurnId
    );
    this.activeThreadId = requestedThreadId;
    this.activeTurnId = prepared.snapshot.activity.activeTurnId;
    this.acceptingFirstTurn = !requestedThreadId;
    this.awaitingTurnStartAcceptance = true;
    this.settledBeforeAcceptanceTurnId = null;
    this.replaceSnapshot(prepared.snapshot);
    return {
      params: threadTurnStartParams({
        controls: input.controls,
        context: this.currentContext,
        input: input.input,
        mentions: input.mentions,
        scope: input.scope,
        threadId: prepared.requestedThreadId,
        clientTurnId
      }),
      prepared,
      snapshot: prepared.snapshot
    };
  }

  acceptTurnStart(
    result: TurnStartResult,
    prepared: ThreadTurnPreparation,
    label = "turn"
  ): ThreadTurnAcceptance {
    const accepted = acceptThreadTurn(
      prepared.snapshot,
      result,
      prepared.requestedThreadId,
      label
    );
    if (
      this.settledBeforeAcceptanceTurnId &&
      this.settledBeforeAcceptanceTurnId !== result.turnId
    ) {
      throw new Error(`Gateway accepted the ${label} with a different turn id.`);
    }
    if (this.activeThreadId && this.activeThreadId !== accepted.threadId) {
      throw new Error(`Gateway accepted the ${label} for a different thread.`);
    }
    this.activeThreadId = accepted.threadId;
    this.activeTurnId = this.settledBeforeAcceptanceTurnId === result.turnId
      ? null
      : result.turnId;
    this.acceptingFirstTurn = false;
    this.awaitingTurnStartAcceptance = false;
    this.settledBeforeAcceptanceTurnId = null;
    this.replaceSnapshot(bindThreadSnapshot(
      this.currentSnapshot ?? accepted.snapshot,
      accepted.thread
    ));
    return {
      threadId: accepted.threadId,
      thread: accepted.thread,
      snapshot: this.currentSnapshot!
    };
  }

  rejectTurnStart(prepared: ThreadTurnPreparation): ThreadSnapshot | null {
    if (!this.awaitingTurnStartAcceptance) return this.currentSnapshot;
    this.activeThreadId = prepared.previousSnapshot.thread?.id ?? null;
    this.activeTurnId = prepared.previousSnapshot.activity.activeTurnId ?? null;
    this.acceptingFirstTurn = false;
    this.awaitingTurnStartAcceptance = false;
    this.settledBeforeAcceptanceTurnId = null;
    this.replaceSnapshot(prepared.previousSnapshot);
    return this.currentSnapshot;
  }

  reconcileUncertainTurnStart(
    prepared: ThreadTurnPreparation,
    incoming: ThreadSnapshot
  ): { accepted: boolean; snapshot: ThreadSnapshot } {
    const accepted = (incoming.turnStartReceipts ?? []).some((receipt) => (
      receipt.clientTurnId === prepared.clientTurnId
    ));
    this.activeThreadId = incoming.thread?.id ?? null;
    this.activeTurnId = incoming.activity.activeTurnId ?? null;
    this.acceptingFirstTurn = false;
    this.awaitingTurnStartAcceptance = false;
    this.settledBeforeAcceptanceTurnId = null;
    if (!incoming.thread) {
      this.currentContext = null;
    }
    const snapshot = accepted
      ? reconcileThreadSnapshot(prepared.snapshot, incoming)
      : incoming;
    this.replaceSnapshot(snapshot);
    return { accepted, snapshot };
  }

  applyGatewayEvent(event: GatewayEvent): ThreadGatewayEventApplication {
    if (!this.currentSnapshot) {
      return { applied: false, completed: false, running: null, snapshot: this.currentSnapshot };
    }
    const acceptingDetachedTurn = this.acceptingFirstTurn && hasUnboundOptimisticPrompt(this.currentSnapshot);
    if (!belongsToActiveThreadTurn(
      event,
      this.activeThreadId,
      this.activeTurnId,
      acceptingDetachedTurn,
      this.settledTurnId
    )) {
      return { applied: false, completed: false, running: null, snapshot: this.currentSnapshot };
    }
    if (event.type === "turnStarted" || event.type === "turnQueued") {
      this.acceptingFirstTurn = false;
      this.activeTurnId = event.turnId;
    } else if (!this.activeTurnId && acceptingDetachedTurn && isLiveTranscriptObservation(event)) {
      this.acceptingFirstTurn = false;
      this.activeTurnId = event.turnId;
    }
    const observedThreadId = eventThreadIdForEvent(event);
    if (this.awaitingTurnStartAcceptance && !this.activeThreadId && observedThreadId) {
      this.activeThreadId = observedThreadId;
    }
    this.replaceSnapshot(applyGatewayEventToThreadSnapshot(this.currentSnapshot, event));
    this.activeThreadId = this.currentSnapshot.thread?.id ?? this.activeThreadId;
    if (event.type === "turnCompleted") {
      if (this.awaitingTurnStartAcceptance) {
        this.settledBeforeAcceptanceTurnId = event.turnId;
      }
      this.settledTurnId = event.turnId;
      this.acceptingFirstTurn = false;
      this.activeTurnId = null;
      return { applied: true, completed: true, running: false, snapshot: this.currentSnapshot };
    }
    if (event.type === "activityChanged") {
      return {
        applied: true,
        completed: false,
        running: event.activity.running,
        snapshot: this.currentSnapshot
      };
    }
    if (event.type === "turnStarted" || event.type === "turnQueued") {
      return { applied: true, completed: false, running: true, snapshot: this.currentSnapshot };
    }
    return { applied: true, completed: false, running: null, snapshot: this.currentSnapshot };
  }

  applyGatewayEvents(events: GatewayEvent[]): ThreadGatewayEventApplication[] {
    this.snapshotBatchDepth += 1;
    try {
      return events.map((event) => this.applyGatewayEvent(event));
    } finally {
      this.snapshotBatchDepth -= 1;
      if (this.snapshotBatchDepth === 0 && this.snapshotNotificationPending) {
        this.snapshotNotificationPending = false;
        for (const listener of this.snapshotListeners) listener();
      }
    }
  }

  private replaceSnapshot(snapshot: ThreadSnapshot | null): void {
    if (this.currentSnapshot === snapshot) return;
    this.currentSnapshot = snapshot;
    if (this.snapshotBatchDepth > 0) {
      this.snapshotNotificationPending = true;
      return;
    }
    for (const listener of this.snapshotListeners) listener();
  }
}

function admitTurnTarget(
  context: ThreadContextReadResult,
  controls: ThreadTurnControls | undefined
): ThreadTurnAdmission {
  const targetId = controls?.targetId.trim() ?? "";
  if (context.binding) {
    if (targetId && targetId !== context.selectedTargetId) {
      return {
        allowed: false,
        reason: "The requested Agent target conflicts with this Thread binding."
      };
    }
    return { allowed: true, reason: null };
  }
  if (!targetId) {
    return {
      allowed: false,
      reason: "Select an Agent target before starting a turn."
    };
  }
  if (targetId !== context.selectedTargetId) {
    return {
      allowed: false,
      reason: "The selected Agent target does not match the current Thread Context."
    };
  }
  const target = context.compatibleTargets.find((candidate) => candidate.targetId === targetId);
  if (!target) {
    return {
      allowed: false,
      reason: "The selected Agent target is not compatible with this Thread Context."
    };
  }
  if (!target.ready) {
    return {
      allowed: false,
      reason: target.unavailableReason ?? `${target.label || target.targetId} is not ready.`
    };
  }
  return { allowed: true, reason: null };
}

function admitInputCapability(
  context: ThreadContextReadResult,
  kind: string
): ThreadTurnAdmission {
  const capability = context.inputCapabilities.find((candidate) => candidate.kind === kind) ?? null;
  if (capability?.enabled) return { allowed: true, reason: null };
  return {
    allowed: false,
    reason: capability?.unavailableReason ?? `Input capability \`${kind}\` is unavailable for this Agent target.`
  };
}

function inputCapabilityKind(part: GatewayInputPart): string {
  return part.type === "context" ? "embeddedContext" : part.type;
}

export function emptyThreadSnapshot(
  scope: GatewayRequestScope,
  threadId: string | null = null
): ThreadSnapshot {
  return {
    activity: { activeTurnId: null, queuedTurns: 0, running: false },
    history: { owner: "psychevo", fidelity: "full", cursor: null, hint: null },
    entries: [],
    pendingActions: [],
    turnStartReceipts: [],
    scope,
    source: sourceFromScope(scope),
    thread: threadId ? gatewayThread(threadId) : null
  };
}

export function prepareThreadTurn(
  snapshot: ThreadSnapshot,
  prompt: string,
  requestedThreadId: string | null = snapshot.thread?.id ?? null,
  now = Date.now(),
  clientTurnId = createClientTurnId()
): ThreadTurnPreparation {
  const optimistic = appendOptimisticPrompt(snapshot, prompt, now);
  return {
    clientTurnId,
    previousSnapshot: snapshot,
    requestedThreadId,
    snapshot: {
      ...optimistic,
      activity: {
        ...optimistic.activity,
        activeTurnId: null,
        running: true,
        startedAtMs: now
      }
    }
  };
}

export function threadTurnStartParams({
  controls,
  context,
  input,
  mentions,
  scope,
  threadId,
  clientTurnId = createClientTurnId()
}: {
  controls?: ThreadTurnControls | undefined;
  context: ThreadContextReadResult | null;
  input: GatewayInputPart[];
  mentions?: GatewayMention[] | undefined;
  scope: GatewayRequestScope;
  threadId: string | null;
  clientTurnId?: string;
}): TurnStartParams {
  const target = controls && !controls.omitTarget
    ? context?.compatibleTargets.find((candidate) => candidate.targetId === controls.targetId) ?? null
    : null;
  if (controls && !controls.omitTarget && !target) {
    throw new Error("The selected Agent target is not present in the current Thread Context.");
  }
  return {
    clientTurnId,
    input,
    mentions: mentions ?? [],
    scope,
    target: target ? {
      agentRef: target.agentRef ?? null,
      runtimeProfileRef: target.runtimeProfileRef
    } : null,
    threadId,
    turnOverrides: controls?.turnOverrides ?? {},
    expectedContextRevision: controls?.expectedContextRevision ?? null,
    expectedControlRevision: controls?.expectedControlRevision ?? null
  };
}

function createClientTurnId(): string {
  const crypto = globalThis.crypto;
  if (typeof crypto?.randomUUID === "function") {
    return crypto.randomUUID();
  }
  if (typeof crypto?.getRandomValues === "function") {
    const words = crypto.getRandomValues(new Uint32Array(4));
    return Array.from(words, (word) => word.toString(16).padStart(8, "0")).join("");
  }
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}-${Math.random().toString(36).slice(2)}`;
}

export function acceptThreadTurn(
  snapshot: ThreadSnapshot,
  result: TurnStartResult,
  requestedThreadId: string | null,
  label = "turn"
): ThreadTurnAcceptance {
  if (!result.accepted) {
    throw new Error(`Gateway rejected the ${label}.`);
  }
  const thread = result.thread;
  if (!thread || !result.threadId) {
    throw new Error(`Gateway accepted the ${label} without its required thread identity.`);
  }
  if (result.threadId !== thread.id) {
    throw new Error(`Gateway accepted the ${label} with conflicting thread identities.`);
  }
  if (requestedThreadId && requestedThreadId !== thread.id) {
    throw new Error(`Gateway accepted the ${label} for a different thread.`);
  }
  return {
    threadId: result.threadId,
    thread,
    snapshot: bindThreadSnapshot(snapshot, thread)
  };
}

export function bindThreadSnapshot(
  snapshot: ThreadSnapshot,
  thread: GatewayThread | string
): ThreadSnapshot {
  const authoritativeThread = typeof thread === "string"
    ? (snapshot.thread?.id === thread ? snapshot.thread : null)
    : thread;
  if (!authoritativeThread) {
    throw new Error("Binding a Thread snapshot requires authoritative GatewayThread metadata.");
  }
  const threadId = authoritativeThread.id;
  return {
    ...snapshot,
    entries: snapshot.entries.map((entry) => (
      entry.threadId ? entry : { ...entry, threadId }
    )),
    thread: authoritativeThread
  };
}

export function applyGatewayEventToThreadSnapshot(
  snapshot: ThreadSnapshot,
  event: GatewayEvent
): ThreadSnapshot {
  return applyLiveTranscriptEvent(snapshot, event);
}

export function latestAssistantTranscriptText(entries: TranscriptEntry[]): string | null {
  const latest = [...entries].reverse().find((entry) => entry.role === "assistant");
  const text = latest?.blocks
    .filter((block) => block.kind === "text")
    .map((block) => block.body ?? block.detail ?? block.preview ?? "")
    .filter(Boolean)
    .join("\n\n")
    .trim();
  return text || null;
}

function belongsToActiveThreadTurn(
  event: GatewayEvent,
  threadId: string | null,
  turnId: string | null,
  acceptingDetachedTurn: boolean,
  settledTurnId: string | null
): boolean {
  const eventThreadId = eventThreadIdForEvent(event);
  if (eventThreadId && threadId && eventThreadId !== threadId) {
    return false;
  }
  const eventTurnId = eventTurnIdForEvent(event);
  if (eventTurnId && eventTurnId === settledTurnId && event.type !== "turnCompleted") {
    return false;
  }
  if (eventTurnId && turnId) {
    return eventTurnId === turnId;
  }
  if (eventThreadId && threadId) {
    return true;
  }
  if (event.type === "turnStarted" || event.type === "turnQueued") {
    return threadId ? !eventThreadId || eventThreadId === threadId : acceptingDetachedTurn;
  }
  if (event.type === "turnCompleted" && eventTurnId && acceptingDetachedTurn && !threadId) {
    return true;
  }
  if (isLiveTranscriptObservation(event) && eventTurnId && acceptingDetachedTurn && !threadId) {
    return true;
  }
  if (eventTurnId) {
    return false;
  }
  if (eventThreadId) {
    return Boolean(threadId && eventThreadId === threadId);
  }
  return true;
}

function eventThreadIdForEvent(event: GatewayEvent): string | null {
  switch (event.type) {
    case "turnStarted":
    case "turnQueued":
    case "activityChanged":
    case "titleChanged":
      return event.threadId || null;
    case "turnCompleted":
      return event.threadId || event.turn.threadId || firstEntryThreadId(event.committedEntries);
    case "entryStarted":
    case "entryUpdated":
    case "entryCompleted":
      return event.entry.threadId || null;
    case "actionRequested":
    case "actionUpdated":
      return event.action.threadId || null;
    default:
      return null;
  }
}

function eventTurnIdForEvent(event: GatewayEvent): string | null {
  switch (event.type) {
    case "turnStarted":
    case "turnQueued":
    case "turnCompleted":
    case "entryStarted":
    case "entryUpdated":
    case "entryCompleted":
      return event.turnId;
    case "actionRequested":
    case "actionUpdated":
      return event.action.turnId || event.action.activityId || null;
    default:
      return null;
  }
}

function firstEntryThreadId(entries: Array<{ threadId?: string | null }>): string | null {
  return entries.find((entry) => entry.threadId)?.threadId ?? null;
}

function isLiveTranscriptObservation(event: GatewayEvent): event is Extract<
  GatewayEvent,
  { type: "entryStarted" | "entryUpdated" | "entryCompleted" }
> {
  return event.type === "entryStarted" ||
    event.type === "entryUpdated" ||
    event.type === "entryCompleted";
}

function hasUnboundOptimisticPrompt(snapshot: ThreadSnapshot): boolean {
  return Boolean(snapshot.entries.some((entry) => (
    entry.role === "user" &&
    !entry.threadId &&
    !entry.turnId &&
    entry.messageSeq === null
  )));
}

function sourceFromScope(scope: GatewayRequestScope): GatewaySource {
  return {
    kind: scope.source.kind,
    lifetime: scope.source.lifetime ?? "process",
    rawId: scope.source.rawId ?? "",
    rawIdentity: scope.source.rawIdentity,
    visibleName: scope.source.visibleName
  };
}

function gatewayThread(threadId: string): GatewayThread {
  return {
    backend: { kind: "native", sessionHandle: threadId, runtimeRef: "native" },
    id: threadId,
    sourceKey: null
  };
}
