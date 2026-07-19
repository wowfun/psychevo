import { describe, expect, it } from "vitest";
import type { GatewayEvent, SessionSummary } from "@psychevo/protocol";
import { patchSessionSummariesFromGatewayEvent } from "./session-utils";

describe("Session summary live patches", () => {
  it("patches only authoritative activity, title, and completion fields", () => {
    const initial = [session("thread-1"), session("thread-2")];
    const running = patchSessionSummariesFromGatewayEvent(initial, {
      type: "activityChanged",
      threadId: "thread-1",
      activity: { running: true, activeTurnId: "turn-1", queuedTurns: 0, updatedAtMs: 20 }
    });
    const titled = patchSessionSummariesFromGatewayEvent(running, {
      type: "titleChanged",
      threadId: "thread-1",
      title: "Authoritative title",
      displayTitle: "Visible title"
    });
    const completed = patchSessionSummariesFromGatewayEvent(titled, completion());

    expect(completed[0]).toMatchObject({
      title: "Authoritative title",
      displayTitle: "Visible title",
      updatedAtMs: 30,
      activity: { running: false, activeTurnId: null, queuedTurns: 0 }
    });
    expect(completed[1]).toBe(initial[1]);
    expect(completed[0]?.messageCount).toBe(3);
  });
});

function session(id: string): SessionSummary {
  return {
    id,
    cwd: "/tmp/project",
    project: { cwd: "/tmp/project", label: "project", displayPath: "/tmp/project" },
    model: null,
    provider: null,
    startedAtMs: 1,
    updatedAtMs: 2,
    endedAtMs: null,
    endReason: null,
    archivedAtMs: null,
    messageCount: 3,
    toolCallCount: 0,
    activity: { running: false, activeTurnId: null, queuedTurns: 0 },
    title: null,
    displayTitle: null
  };
}

function completion(): Extract<GatewayEvent, { type: "turnCompleted" }> {
  return {
    type: "turnCompleted",
    threadId: "thread-1",
    turnId: "turn-1",
    turn: {
      id: "turn-1",
      threadId: "thread-1",
      status: "completed",
      outcome: "normal",
      error: null,
      completedAtMs: 30
    },
    committedEntries: []
  };
}
