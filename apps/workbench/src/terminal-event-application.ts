import type { TerminalExitedPayload, TerminalOutputPayload } from "@psychevo/protocol";
import type { TerminalNotificationEvent } from "./types";

const MAX_TERMINAL_EVENTS = 240;

export class TerminalEventApplication {
  private readonly listeners = new Set<() => void>();
  private sequence = 0;
  private snapshot: TerminalNotificationEvent[] = [];

  getSnapshot = (): TerminalNotificationEvent[] => this.snapshot;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  appendOutput(params: TerminalOutputPayload): void {
    this.append({ method: "terminal/output", params, seq: 0 });
  }

  appendExit(params: TerminalExitedPayload): void {
    this.append({ method: "terminal/exited", params, seq: 0 });
  }

  private append(event: TerminalNotificationEvent): void {
    this.sequence += 1;
    this.snapshot = [
      ...this.snapshot.slice(-(MAX_TERMINAL_EVENTS - 1)),
      { ...event, seq: this.sequence }
    ];
    for (const listener of this.listeners) listener();
  }
}
