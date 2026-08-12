import type { ThreadSessionView } from "@psychevo/client";

export type ThreadSessionViewSource = {
  getView(): ThreadSessionView;
  subscribe(listener: () => void): () => void;
};

export type ThreadSessionOperationalSnapshot = Pick<
  ThreadSessionView,
  "context" | "threadSnapshot"
>;

export class ThreadSessionOperationalApplication {
  private readonly listeners = new Set<() => void>();
  private unsubscribeSource: (() => void) | null = null;
  private snapshot: ThreadSessionOperationalSnapshot;

  constructor(private readonly source: ThreadSessionViewSource) {
    this.snapshot = operationalSnapshot(source.getView());
  }

  getSnapshot = (): ThreadSessionOperationalSnapshot => this.snapshot;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    if (this.listeners.size === 1) this.attach();
    return () => {
      this.listeners.delete(listener);
      if (this.listeners.size === 0) this.detach();
    };
  };

  dispose(): void {
    this.detach();
    this.listeners.clear();
  }

  private attach(): void {
    if (this.unsubscribeSource) return;
    const next = this.source.getView();
    if (
      next.context !== this.snapshot.context
      || next.threadSnapshot !== this.snapshot.threadSnapshot
    ) {
      this.snapshot = operationalSnapshot(next);
    }
    this.unsubscribeSource = this.source.subscribe(() => this.publish());
  }

  private detach(): void {
    this.unsubscribeSource?.();
    this.unsubscribeSource = null;
  }

  private publish(): void {
    const next = this.source.getView();
    if (
      next.context === this.snapshot.context
      && next.threadSnapshot === this.snapshot.threadSnapshot
    ) {
      return;
    }
    this.snapshot = operationalSnapshot(next);
    for (const listener of this.listeners) listener();
  }
}

function operationalSnapshot(view: ThreadSessionView): ThreadSessionOperationalSnapshot {
  return {
    context: view.context,
    threadSnapshot: view.threadSnapshot
  };
}
