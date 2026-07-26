import {
  GatewayEventSchema,
  type GatewayEvent,
  type GatewayInputPart,
  type GatewayMention,
  type GatewayMethod,
  type GatewayRequestParams,
  type GatewayRequestScope,
  type GatewayRequestResults,
  type RpcNotification,
  type RunnableTargetInput,
  type ThreadContextReadResult,
  type ThreadControlDescriptorView,
  type ThreadDraftOpenParams,
  type ThreadDraftOpenResult,
  type ThreadInteractionRespondParams,
  type ThreadInteractionRespondResult,
  type ThreadSnapshot
} from "@psychevo/protocol";

import {
  GatewayClientError,
  parseThreadSnapshot,
  type GatewayConnectionSnapshot,
  type GatewayRequestInit,
  type GatewayRequestOptions
} from "./index";
import {
  ThreadController,
  type ThreadTurnAdmission,
  type ThreadTurnControls,
  type ThreadTurnPreparation,
  type ThreadTurnStartInput
} from "./thread-controller";

export interface ThreadSessionClient {
  connectionSnapshot(): GatewayConnectionSnapshot;
  request<M extends GatewayMethod>(
    method: M,
    params?: GatewayRequestInit<M>,
    options?: GatewayRequestOptions
  ): Promise<GatewayRequestResults[M]>;
  subscribe(handler: (notification: RpcNotification) => void): () => void;
  subscribeConnectionState(
    handler: (snapshot: GatewayConnectionSnapshot) => void
  ): () => void;
}

export interface ThreadSessionOptions {
  client?: ThreadSessionClient | null;
  context?: ThreadContextReadResult | null;
  snapshot?: ThreadSnapshot | null;
}

export interface ThreadSessionLoadInput {
  scope: GatewayRequestScope;
  target?: RunnableTargetInput | null;
  threadId: string | null;
}

export type ThreadSessionSendInput = ThreadTurnStartInput;

export interface ThreadSessionSendObserver {
  deliveryUnknown?(): void;
}

export type ThreadSessionSendOutcome =
  | {
      status: "accepted";
      detached: boolean;
      threadId: string;
      turnId: string;
      snapshot: ThreadSnapshot | null;
    }
  | {
      status: "reconciled";
      accepted: boolean;
      threadId: string | null;
      snapshot: ThreadSnapshot;
    }
  | {
      status: "not_sent";
      error: Error;
      snapshot: ThreadSnapshot | null;
    }
  | {
      status: "cancelled";
      delivery: "unknown";
      reason: "disposed" | "view_changed";
      snapshot: ThreadSnapshot | null;
    };

export interface ThreadSessionControlInput {
  control: ThreadControlDescriptorView;
  scope: GatewayRequestScope;
  targetId: string;
  threadId: string | null;
  value: unknown;
}

type PendingRecovery = {
  epoch: number;
  prepared: ThreadTurnPreparation;
  resolve(outcome: ThreadSessionSendOutcome): void;
  scope: GatewayRequestScope;
};

type ScheduledFrame = ReturnType<typeof setTimeout> | number;

export class ThreadSession {
  private client: ThreadSessionClient | null = null;
  private readonly controller: ThreadController;
  private readonly listeners = new Set<() => void>();
  private unsubscribeClient: (() => void) | null = null;
  private unsubscribeConnection: (() => void) | null = null;
  private viewEpoch = 0;
  private disposed = false;
  private pendingRecovery: PendingRecovery | null = null;
  private recoveryPromise: Promise<void> | null = null;
  private eventQueue: GatewayEvent[] = [];
  private eventFrame: ScheduledFrame | null = null;
  private readonly firstAssistantTurns = new Set<string>();

  constructor(options: ThreadSessionOptions = {}) {
    this.controller = new ThreadController(options.snapshot ?? null);
    this.controller.setContext(options.context ?? null);
    this.controller.subscribe(() => {
      for (const listener of this.listeners) listener();
    });
    this.attachClient(options.client ?? null);
  }

  attachClient(client: ThreadSessionClient | null): void {
    const reactivating = this.disposed && client !== null;
    if (reactivating) this.disposed = false;
    if (this.client === client && !reactivating) return;
    this.unsubscribeClient?.();
    this.unsubscribeConnection?.();
    this.unsubscribeClient = null;
    this.unsubscribeConnection = null;
    this.client = client;
    if (!client || this.disposed) return;
    this.unsubscribeClient = client.subscribe((notification) => {
      if (notification.method !== "gateway/event") return;
      const event = GatewayEventSchema.safeParse(notification.params);
      if (event.success) this.enqueueGatewayEvent(event.data);
    });
    this.unsubscribeConnection = client.subscribeConnectionState((connection) => {
      if (connection.state === "connected" && this.pendingRecovery) {
        void this.recoverPendingSend();
      }
    });
  }

  getSnapshot(): ThreadSnapshot | null {
    return this.controller.snapshot();
  }

  getContext(): ThreadContextReadResult | null {
    return this.controller.context();
  }

  setContext(context: ThreadContextReadResult | null): void {
    this.controller.setContext(context);
    this.emitChange();
  }

  contextReadTarget(targetId: string): RunnableTargetInput | null {
    return this.controller.contextReadTarget(targetId);
  }

  sendability(): ThreadTurnAdmission {
    return this.controller.sendability();
  }

  admitTurn(
    input: Pick<ThreadTurnStartInput, "controls" | "input" | "mentions">
  ): ThreadTurnAdmission {
    return this.controller.admitTurn(input);
  }

  admitInput(input: GatewayInputPart[], mentions: GatewayMention[] = []): ThreadTurnAdmission {
    return this.controller.admitInput(input, mentions);
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  async openDraft(params: ThreadDraftOpenParams): Promise<ThreadDraftOpenResult> {
    const client = this.requireClient();
    const epoch = this.advanceView("view_changed");
    const result = await client.request("thread/draft/open", params);
    if (epoch !== this.viewEpoch || this.disposed) return result;
    this.controller.reset(parseThreadSnapshot(result.snapshot));
    this.controller.setContext(result.context);
    return result;
  }

  async load(input: ThreadSessionLoadInput): Promise<ThreadSnapshot> {
    const client = this.requireClient();
    const epoch = this.advanceView("view_changed");
    const [snapshot, context] = await Promise.all([
      client.request("thread/resume", {
        scope: input.scope,
        threadId: input.threadId
      }),
      client.request("thread/context/read", {
        scope: input.scope,
        target: input.target ?? null,
        threadId: input.threadId
      })
    ]);
    const parsed = parseThreadSnapshot(snapshot);
    if (epoch !== this.viewEpoch || this.disposed) return parsed;
    this.controller.reset(parsed);
    this.controller.setContext(context);
    return parsed;
  }

  async send(
    input: ThreadSessionSendInput,
    observer: ThreadSessionSendObserver = {}
  ): Promise<ThreadSessionSendOutcome> {
    const client = this.requireClient();
    const epoch = this.viewEpoch;
    const plan = this.controller.beginTurn(input);
    try {
      const result = await client.request("turn/start", plan.params);
      if (epoch !== this.viewEpoch || this.disposed) {
        if (
          !this.disposed
          && this.controller.snapshot()?.thread?.id === result.threadId
        ) {
          const accepted = this.controller.acceptTurnStart(result, plan.prepared);
          return {
            status: "accepted",
            detached: false,
            threadId: accepted.threadId,
            turnId: result.turnId,
            snapshot: accepted.snapshot
          };
        }
        this.controller.rejectTurnStart(plan.prepared);
        return {
          status: "accepted",
          detached: true,
          threadId: result.threadId,
          turnId: result.turnId,
          snapshot: this.controller.snapshot()
        };
      }
      const accepted = this.controller.acceptTurnStart(result, plan.prepared);
      return {
        status: "accepted",
        detached: false,
        threadId: accepted.threadId,
        turnId: result.turnId,
        snapshot: accepted.snapshot
      };
    } catch (error) {
      const failure = error instanceof Error ? error : new Error(String(error));
      if (
        failure instanceof GatewayClientError
        && failure.delivery === "unknown"
        && epoch === this.viewEpoch
        && !this.disposed
      ) {
        observer.deliveryUnknown?.();
        return new Promise<ThreadSessionSendOutcome>((resolve) => {
          this.pendingRecovery = {
            epoch,
            prepared: plan.prepared,
            resolve,
            scope: input.scope
          };
          void this.recoverPendingSend();
        });
      }
      this.controller.rejectTurnStart(plan.prepared);
      return {
        status: "not_sent",
        error: failure,
        snapshot: this.controller.snapshot()
      };
    }
  }

  async retryRecovery(): Promise<void> {
    await this.recoverPendingSend();
  }

  async setControl(input: ThreadSessionControlInput) {
    const client = this.requireClient();
    const params = this.controller.controlSetParams(
      input.targetId,
      input.control,
      input.value,
      input.scope,
      input.threadId
    );
    const receipt = await client.request("thread/control/set", params);
    this.controller.applyControlReceipt(receipt);
    return receipt;
  }

  async interrupt(scope: GatewayRequestScope, threadId: string) {
    return this.requireClient().request("thread/action/run", {
      action: { kind: "interrupt" },
      scope,
      threadId
    });
  }

  async respond(
    params: ThreadInteractionRespondParams
  ): Promise<ThreadInteractionRespondResult> {
    return this.requireClient().request("thread/interaction/respond", params);
  }

  turnControls(targetId: string, turnOverrides: Record<string, unknown>): ThreadTurnControls {
    return this.controller.turnControls(targetId, turnOverrides);
  }

  ingestGatewayEvent(event: GatewayEvent): void {
    this.enqueueGatewayEvent(event);
  }

  reset(snapshot: ThreadSnapshot | null, context: ThreadContextReadResult | null = null): void {
    this.advanceView("view_changed");
    this.controller.reset(snapshot);
    this.controller.setContext(context);
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.advanceView("disposed");
    this.unsubscribeClient?.();
    this.unsubscribeConnection?.();
    this.unsubscribeClient = null;
    this.unsubscribeConnection = null;
    this.cancelFrame();
    this.listeners.clear();
  }

  private requireClient(): ThreadSessionClient {
    if (!this.client) throw new Error("ThreadSession is not attached to a Gateway client.");
    return this.client;
  }

  private emitChange(): void {
    for (const listener of this.listeners) listener();
  }

  private advanceView(reason: "disposed" | "view_changed"): number {
    this.viewEpoch += 1;
    const pending = this.pendingRecovery;
    this.pendingRecovery = null;
    if (pending) {
      this.controller.rejectTurnStart(pending.prepared);
      pending.resolve({
        status: "cancelled",
        delivery: "unknown",
        reason,
        snapshot: this.controller.snapshot()
      });
    }
    return this.viewEpoch;
  }

  private async recoverPendingSend(): Promise<void> {
    if (this.recoveryPromise) return this.recoveryPromise;
    const pending = this.pendingRecovery;
    const client = this.client;
    if (
      !pending
      || !client
      || client.connectionSnapshot().state !== "connected"
      || this.disposed
    ) {
      return;
    }
    this.recoveryPromise = (async () => {
      try {
        const incoming = parseThreadSnapshot(await client.request("thread/resume", {
          scope: pending.scope,
          threadId: pending.prepared.requestedThreadId
        }));
        if (
          this.pendingRecovery !== pending
          || pending.epoch !== this.viewEpoch
          || this.disposed
        ) {
          return;
        }
        const reconciled = this.controller.reconcileUncertainTurnStart(
          pending.prepared,
          incoming
        );
        this.pendingRecovery = null;
        pending.resolve({
          status: "reconciled",
          accepted: reconciled.accepted,
          threadId: reconciled.snapshot.thread?.id ?? null,
          snapshot: reconciled.snapshot
        });
      } catch {
        // Keep the pending receipt recoverable for the next connected generation
        // or an explicit retry. Unknown delivery is never replayed.
      }
    })().finally(() => {
      this.recoveryPromise = null;
    });
    return this.recoveryPromise;
  }

  private enqueueGatewayEvent(event: GatewayEvent): void {
    if (event.type === "turnCompleted") {
      const sameTurn: GatewayEvent[] = [];
      this.eventQueue = this.eventQueue.filter((queued) => {
        if ("turnId" in queued && queued.turnId === event.turnId) {
          sameTurn.push(queued);
          return false;
        }
        return true;
      });
      if (this.eventQueue.length === 0) this.cancelFrame();
      this.firstAssistantTurns.delete(event.turnId);
      this.controller.applyGatewayEvents([...sameTurn, event]);
      return;
    }
    if (!pacedGatewayEvent(event) || this.isFirstAssistantText(event)) {
      this.controller.applyGatewayEvent(event);
      return;
    }
    if (event.type === "entryUpdated") {
      const existing = this.eventQueue.findIndex((queued) => (
        queued.type === "entryUpdated"
        && queued.turnId === event.turnId
        && queued.entry.id === event.entry.id
      ));
      if (existing >= 0) {
        this.eventQueue[existing] = event;
        return;
      }
    }
    this.eventQueue.push(event);
    this.scheduleFrame();
  }

  private isFirstAssistantText(event: GatewayEvent): boolean {
    if (
      event.type !== "entryStarted"
      && event.type !== "entryUpdated"
      && event.type !== "entryCompleted"
    ) {
      return false;
    }
    if (event.entry.role !== "assistant") return false;
    const nonEmpty = event.entry.blocks.some((block) => (
      block.kind === "text"
      && [block.body, block.preview, block.detail].some((value) => (
        typeof value === "string" && Boolean(value.trim())
      ))
    ));
    if (!nonEmpty || this.firstAssistantTurns.has(event.turnId)) return false;
    this.firstAssistantTurns.add(event.turnId);
    return true;
  }

  private scheduleFrame(): void {
    if (this.eventFrame !== null) return;
    if (typeof globalThis.requestAnimationFrame === "function") {
      this.eventFrame = globalThis.requestAnimationFrame(() => this.flushEventQueue());
    } else {
      this.eventFrame = setTimeout(() => this.flushEventQueue(), 0);
    }
  }

  private cancelFrame(): void {
    if (this.eventFrame === null) return;
    if (
      typeof this.eventFrame === "number"
      && typeof globalThis.cancelAnimationFrame === "function"
    ) {
      globalThis.cancelAnimationFrame(this.eventFrame);
    } else {
      clearTimeout(this.eventFrame as ReturnType<typeof setTimeout>);
    }
    this.eventFrame = null;
  }

  private flushEventQueue(): void {
    this.eventFrame = null;
    const events = this.eventQueue.splice(0);
    this.controller.applyGatewayEvents(events);
  }
}

function pacedGatewayEvent(event: GatewayEvent): boolean {
  return event.type === "entryStarted"
    || event.type === "entryUpdated"
    || event.type === "entryCompleted";
}
