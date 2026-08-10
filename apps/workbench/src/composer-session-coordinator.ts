import type { GatewayRequestScope } from "@psychevo/protocol";

export type DraftOpenToken = {
  epoch: number;
  id: number;
};

type ReadinessKind = "draftOpen" | "draftPrepare";

type ActiveReadiness = DraftOpenToken & {
  completionTarget: { scope: GatewayRequestScope; workspaceId: string | null } | null;
  kind: ReadinessKind;
  promise: Promise<boolean>;
  settle(ready: boolean): void;
  settled: boolean;
};

/** Owns the one readiness boundary between a local draft shell and Gateway context. */
export class ComposerSessionCoordinator {
  private nextId = 0;
  private activeReadiness: ActiveReadiness | null = null;
  private pendingSubmissionReadinessId: number | null = null;

  beginDraftOpen(
    epoch: number,
    completionTarget: { scope: GatewayRequestScope; workspaceId: string | null } | null = null
  ): DraftOpenToken {
    return this.beginReadiness(epoch, "draftOpen", completionTarget);
  }

  beginDraftPrepare(epoch: number): DraftOpenToken {
    return this.beginReadiness(epoch, "draftPrepare", null);
  }

  private beginReadiness(
    epoch: number,
    kind: ReadinessKind,
    completionTarget: ActiveReadiness["completionTarget"]
  ): DraftOpenToken {
    this.cancelPending();
    let settlePromise!: (ready: boolean) => void;
    const token: DraftOpenToken = { epoch, id: this.nextId + 1 };
    this.nextId = token.id;
    this.activeReadiness = {
      ...token,
      completionTarget,
      kind,
      promise: new Promise<boolean>((resolve) => {
        settlePromise = resolve;
      }),
      settle(ready) {
        settlePromise(ready);
      },
      settled: false
    };
    return token;
  }

  completeDraftOpen(token: DraftOpenToken): void {
    this.settleReadiness(token, true);
  }

  failDraftOpen(token: DraftOpenToken): void {
    this.settleReadiness(token, false);
  }

  completeDraftPrepare(token: DraftOpenToken): void {
    this.settleReadiness(token, true);
  }

  failDraftPrepare(token: DraftOpenToken): void {
    this.settleReadiness(token, false);
  }

  cancelPending(): void {
    const active = this.activeReadiness;
    if (!active || active.settled) {
      this.activeReadiness = null;
      return;
    }
    active.settled = true;
    active.settle(false);
    this.activeReadiness = null;
  }

  isDraftOpenPending(epoch: number): boolean {
    return this.activeReadiness?.kind === "draftOpen"
      && this.activeReadiness.epoch === epoch
      && !this.activeReadiness.settled;
  }

  isReadinessPending(epoch: number): boolean {
    return this.activeReadiness?.epoch === epoch && !this.activeReadiness.settled;
  }

  pendingCompletionTarget(
    epoch: number
  ): { scope: GatewayRequestScope; workspaceId: string | null } | null {
    const active = this.activeReadiness;
    return active?.epoch === epoch && !active.settled ? active.completionTarget : null;
  }

  async waitToSubmit(epoch: number, isInputCurrent: () => boolean): Promise<boolean> {
    const active = this.activeReadiness;
    if (!active || active.epoch !== epoch || active.settled) {
      return isInputCurrent();
    }
    if (this.pendingSubmissionReadinessId !== null) {
      return false;
    }
    this.pendingSubmissionReadinessId = active.id;
    const ready = await active.promise;
    if (this.pendingSubmissionReadinessId === active.id) {
      this.pendingSubmissionReadinessId = null;
    }
    return ready && isInputCurrent();
  }

  private settleReadiness(token: DraftOpenToken, ready: boolean): void {
    const active = this.activeReadiness;
    if (!active || active.id !== token.id || active.epoch !== token.epoch || active.settled) {
      return;
    }
    active.settled = true;
    active.settle(ready);
  }
}
