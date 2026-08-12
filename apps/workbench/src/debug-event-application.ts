import type { DebugEvent } from "./types";

const MAX_DEBUG_EVENTS = 120;

export class DebugEventApplication {
  private readonly listeners = new Set<() => void>();
  private sequence = 0;
  private snapshot: DebugEvent[] = [];

  getSnapshot = (): DebugEvent[] => this.snapshot;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  append(method: string, payload: unknown): void {
    this.sequence += 1;
    const at = Date.now();
    this.snapshot = [
      { id: `${at}:${method}:${this.sequence}`, at, method, payload },
      ...this.snapshot.slice(0, MAX_DEBUG_EVENTS - 1)
    ];
    for (const listener of this.listeners) listener();
  }
}
