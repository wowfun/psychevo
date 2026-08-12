import { describe, expect, it, vi } from "vitest";
import { DebugEventApplication } from "./debug-event-application";

describe("DebugEventApplication", () => {
  it("retains the newest 120 notifications and emits one update per append", () => {
    vi.spyOn(Date, "now").mockReturnValue(42);
    const application = new DebugEventApplication();
    let updates = 0;
    const unsubscribe = application.subscribe(() => {
      updates += 1;
    });
    for (let index = 0; index < 150; index += 1) {
      application.append("gateway/event", { index });
    }
    unsubscribe();
    const events = application.getSnapshot();
    expect(events).toHaveLength(120);
    expect(events[0]?.id).toBe("42:gateway/event:150");
    expect(events.at(-1)?.id).toBe("42:gateway/event:31");
    expect(updates).toBe(150);
    vi.restoreAllMocks();
  });
});
