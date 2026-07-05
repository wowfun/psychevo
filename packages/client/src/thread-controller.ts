import type {
  GatewayEvent,
  GatewayInputPart,
  GatewayMention,
  GatewayRequestScope,
  GatewaySource,
  GatewayThread,
  ThreadSnapshot,
  TranscriptEntry,
  TurnErrorPayload,
  TurnResultPayload,
  TurnStartParams,
  TurnStartResult,
  WorkbenchControlsView
} from "@psychevo/protocol";
import {
  appendOptimisticPrompt,
  applyLiveTranscriptEvent
} from "./transcript";

export interface ThreadTurnPreparation {
  requestedThreadId: string | null;
  snapshot: ThreadSnapshot;
}

export interface ThreadTurnAcceptance {
  threadId: string;
  snapshot: ThreadSnapshot;
}

export interface ThreadTurnControls {
  agentName?: string | null;
  mode?: string | null;
  model?: string | null;
  permissionMode?: string | null;
  reasoningEffort?: string | null;
  runtimeOptions?: Record<string, string>;
  runtimeRef?: string | null;
  runtimeSessionId?: string | null;
}

export interface ThreadTurnStartInput {
  controls?: ThreadTurnControls;
  input: GatewayInputPart[];
  mentions?: GatewayMention[];
  optimisticText: string;
  scope: GatewayRequestScope;
  text?: string | null;
  threadId?: string | null;
}

export interface ThreadTurnStartPlan {
  params: TurnStartParams;
  prepared: ThreadTurnPreparation;
  snapshot: ThreadSnapshot;
}

export interface ThreadGatewayEventApplication {
  applied: boolean;
  completed: boolean;
  running: boolean | null;
  snapshot: ThreadSnapshot | null;
}

export interface ThreadTurnResultApplication {
  applied: boolean;
  snapshot: ThreadSnapshot | null;
  threadId: string | null;
}

export interface ThreadTurnErrorApplication {
  applied: boolean;
  message: string;
}

export class ThreadTranscriptController {
  private activeThreadId: string | null = null;
  private activeTurnId: string | null = null;
  private acceptingFirstTurn = false;
  private currentSnapshot: ThreadSnapshot | null;

  constructor(snapshot: ThreadSnapshot | null = null) {
    this.currentSnapshot = snapshot;
    this.activeThreadId = snapshot?.thread?.id ?? null;
    this.activeTurnId = snapshot?.activity.activeTurnId ?? null;
  }

  snapshot(): ThreadSnapshot | null {
    return this.currentSnapshot;
  }

  threadId(): string | null {
    return this.activeThreadId;
  }

  turnId(): string | null {
    return this.activeTurnId;
  }

  reset(snapshot: ThreadSnapshot | null): void {
    this.currentSnapshot = snapshot;
    this.activeThreadId = snapshot?.thread?.id ?? null;
    this.activeTurnId = snapshot?.activity.activeTurnId ?? null;
    this.acceptingFirstTurn = false;
  }

  setThreadId(threadId: string | null): void {
    this.activeThreadId = threadId;
  }

  beginTurn(input: ThreadTurnStartInput): ThreadTurnStartPlan {
    const snapshot = this.currentSnapshot ?? emptyThreadSnapshot(input.scope, input.threadId ?? null);
    const requestedThreadId = input.threadId ?? snapshot.thread?.id ?? null;
    const prepared = prepareThreadTurn(snapshot, input.optimisticText, requestedThreadId);
    this.currentSnapshot = prepared.snapshot;
    this.activeThreadId = requestedThreadId;
    this.activeTurnId = prepared.snapshot.activity.activeTurnId;
    this.acceptingFirstTurn = !requestedThreadId;
    return {
      params: threadTurnStartParams({
        controls: input.controls,
        input: input.input,
        mentions: input.mentions,
        scope: input.scope,
        text: input.text,
        threadId: prepared.requestedThreadId
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
    this.activeThreadId = accepted.threadId;
    this.acceptingFirstTurn = false;
    this.currentSnapshot = bindThreadSnapshot(this.currentSnapshot ?? accepted.snapshot, accepted.threadId);
    return {
      threadId: accepted.threadId,
      snapshot: this.currentSnapshot
    };
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
      acceptingDetachedTurn
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
    this.currentSnapshot = applyGatewayEventToThreadSnapshot(this.currentSnapshot, event);
    this.activeThreadId = this.currentSnapshot.thread?.id ?? this.activeThreadId;
    if (event.type === "turnCompleted") {
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

  applyTurnResult(payload: TurnResultPayload): ThreadTurnResultApplication {
    if (!this.currentSnapshot) {
      return { applied: false, snapshot: this.currentSnapshot, threadId: null };
    }
    if (payload.thread.id !== this.activeThreadId && payload.turn.id !== this.activeTurnId) {
      return { applied: false, snapshot: this.currentSnapshot, threadId: null };
    }
    this.activeThreadId = payload.thread.id;
    this.activeTurnId = null;
    this.acceptingFirstTurn = false;
    this.currentSnapshot = applyTurnResultToThreadSnapshot(this.currentSnapshot, payload);
    return { applied: true, snapshot: this.currentSnapshot, threadId: payload.thread.id };
  }

  applyTurnError(payload: TurnErrorPayload): ThreadTurnErrorApplication {
    this.acceptingFirstTurn = false;
    this.activeTurnId = null;
    return { applied: true, message: payload.message || "Turn failed." };
  }
}

export function emptyThreadSnapshot(
  scope: GatewayRequestScope,
  threadId: string | null = null
): ThreadSnapshot {
  return {
    activity: { activeTurnId: null, queuedTurns: 0, running: false },
    entries: [],
    pendingActions: [],
    scope,
    source: sourceFromScope(scope),
    thread: threadId ? gatewayThread(threadId) : null
  };
}

export function prepareThreadTurn(
  snapshot: ThreadSnapshot,
  prompt: string,
  requestedThreadId: string | null = snapshot.thread?.id ?? null
): ThreadTurnPreparation {
  return {
    requestedThreadId,
    snapshot: appendOptimisticPrompt(snapshot, prompt)
  };
}

export function threadTurnStartParams({
  controls,
  input,
  mentions,
  scope,
  text,
  threadId
}: {
  controls?: ThreadTurnControls | undefined;
  input: GatewayInputPart[];
  mentions?: GatewayMention[] | undefined;
  scope: GatewayRequestScope;
  text?: string | null | undefined;
  threadId: string | null;
}): TurnStartParams {
  return {
    agentName: controls?.agentName ?? null,
    input,
    mentions: mentions ?? [],
    mode: controls?.mode ?? null,
    model: controls?.model ?? null,
    permissionMode: controls?.permissionMode ?? null,
    reasoningEffort: controls?.reasoningEffort ?? null,
    runtimeOptions: controls?.runtimeOptions ?? {},
    runtimeRef: controls?.runtimeRef ?? null,
    runtimeSessionId: controls?.runtimeSessionId ?? null,
    scope,
    text: text ?? null,
    threadId
  };
}

export function threadTurnControlsFromWorkbenchControls(
  controls: WorkbenchControlsView | null | undefined,
  overrides: Partial<ThreadTurnControls> = {}
): ThreadTurnControls {
  const runtimeRef = overrides.runtimeRef ?? controls?.runtimeRef ?? null;
  const mode = controls?.mode ?? null;
  return {
    agentName: overrides.agentName ?? controls?.agent ?? null,
    mode: overrides.mode ?? (runtimeRef === "native" ? mode : null),
    model: overrides.model ?? controls?.model ?? null,
    permissionMode: overrides.permissionMode ?? controls?.permissionMode ?? null,
    reasoningEffort: overrides.reasoningEffort ?? (controls?.variant === "none" ? null : controls?.variant ?? null),
    runtimeOptions: overrides.runtimeOptions ?? (runtimeRef && runtimeRef !== "native" && mode ? { mode } : {}),
    runtimeRef,
    runtimeSessionId: overrides.runtimeSessionId ?? null
  };
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
  const threadId = requestedThreadId ?? result.threadId;
  if (!threadId) {
    throw new Error(`Gateway accepted the ${label} without a thread id.`);
  }
  return {
    threadId,
    snapshot: bindThreadSnapshot(snapshot, threadId)
  };
}

export function bindThreadSnapshot(
  snapshot: ThreadSnapshot,
  threadId: string
): ThreadSnapshot {
  return {
    ...snapshot,
    entries: snapshot.entries.map((entry) => (
      entry.threadId ? entry : { ...entry, threadId }
    )),
    thread: gatewayThread(threadId)
  };
}

export function applyGatewayEventToThreadSnapshot(
  snapshot: ThreadSnapshot,
  event: GatewayEvent
): ThreadSnapshot {
  return applyLiveTranscriptEvent(snapshot, event);
}

export function applyTurnResultToThreadSnapshot(
  snapshot: ThreadSnapshot,
  payload: TurnResultPayload
): ThreadSnapshot {
  return applyGatewayEventToThreadSnapshot(snapshot, turnCompletedEventFromResult(payload));
}

export function turnCompletedEventFromResult(payload: TurnResultPayload): GatewayEvent {
  return {
    committedEntries: Array.isArray(payload.committedEntries) ? payload.committedEntries : [],
    threadId: payload.thread.id,
    turn: payload.turn,
    turnId: payload.turn.id,
    type: "turnCompleted"
  };
}

export function turnResultThreadId(payload: TurnResultPayload): string {
  return payload.thread.id;
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
  acceptingDetachedTurn: boolean
): boolean {
  const eventThreadId = eventThreadIdForEvent(event);
  if (eventThreadId && threadId && eventThreadId !== threadId) {
    return false;
  }
  const eventTurnId = eventTurnIdForEvent(event);
  if (eventTurnId && turnId) {
    return eventTurnId === turnId;
  }
  if (eventThreadId && threadId) {
    return true;
  }
  if (event.type === "turnStarted" || event.type === "turnQueued") {
    return threadId ? !eventThreadId || eventThreadId === threadId : acceptingDetachedTurn;
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
    backend: { kind: "psychevo", nativeId: threadId },
    id: threadId,
    sourceKey: null
  };
}
