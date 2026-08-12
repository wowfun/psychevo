// @vitest-environment jsdom

import { act, render, screen } from "@testing-library/react";
import { useSyncExternalStore } from "react";
import { describe, expect, it } from "vitest";
import { TerminalEventApplication } from "./terminal-event-application";

function TerminalProbe({ application }: { application: TerminalEventApplication }) {
  const events = useSyncExternalStore(
    application.subscribe,
    application.getSnapshot,
    application.getSnapshot
  );
  return <output>{events.length}</output>;
}

describe("TerminalEventApplication", () => {
  it("bounds retained output while preserving a monotonic sequence", () => {
    const application = new TerminalEventApplication();
    for (let index = 0; index < 300; index += 1) {
      application.appendOutput({
        terminalId: "terminal-1",
        stream: "stdout",
        dataBase64: String(index)
      });
    }
    const events = application.getSnapshot();
    expect(events).toHaveLength(240);
    expect(events[0]?.seq).toBe(61);
    expect(events.at(-1)?.seq).toBe(300);
  });

  it("rerenders only the subscribing terminal boundary", () => {
    const application = new TerminalEventApplication();
    let parentRenders = 0;
    function Parent() {
      parentRenders += 1;
      return <TerminalProbe application={application} />;
    }
    render(<Parent />);
    expect(parentRenders).toBe(1);
    act(() => {
      application.appendExit({
        terminalId: "terminal-1",
        exitCode: 0,
        reason: "complete"
      });
    });
    expect(screen.getByText("1")).toBeTruthy();
    expect(parentRenders).toBe(1);
  });
});
