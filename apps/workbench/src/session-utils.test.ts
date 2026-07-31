import { describe, expect, it } from "vitest";
import type { GatewayEvent, SessionSummary } from "@psychevo/protocol";
import { patchSessionSummariesFromGatewayEvent } from "./session-utils";

describe("Session summary live patches", () => {
  it("keeps a started session running through title changes until completion", () => {
    const initial = [session("thread-1"), session("thread-2")];
    const started = patchSessionSummariesFromGatewayEvent(initial, {
      type: "turnStarted",
      threadId: "thread-1",
      turnId: "turn-1",
      selectedSkills: []
    });
    const titled = patchSessionSummariesFromGatewayEvent(started, {
      type: "titleChanged",
      threadId: "thread-1",
      title: "Authoritative title",
      displayTitle: "Visible title"
    });
    const completed = patchSessionSummariesFromGatewayEvent(titled, completion());

    expect(started[0]?.activity).toMatchObject({
      running: true,
      activeTurnId: "turn-1",
      queuedTurns: 0
    });
    expect(titled[0]).toMatchObject({
      title: "Authoritative title",
      displayTitle: "Visible title",
      activity: {
        running: true,
        activeTurnId: "turn-1",
        queuedTurns: 0
      }
    });
    expect(completed[0]?.activity).toMatchObject({
      running: false,
      activeTurnId: null,
      queuedTurns: 0
    });
    expect(completed[1]).toBe(initial[1]);
  });

  it("updates queued turns without replacing the active turn", () => {
    const started = patchSessionSummariesFromGatewayEvent([session("thread-1")], {
      type: "turnStarted",
      threadId: "thread-1",
      turnId: "turn-active",
      selectedSkills: []
    });
    const queued = patchSessionSummariesFromGatewayEvent(started, {
      type: "turnQueued",
      threadId: "thread-1",
      turnId: "turn-queued",
      queuePosition: 2
    });

    expect(queued[0]?.activity).toMatchObject({
      running: true,
      activeTurnId: "turn-active",
      queuedTurns: 2
    });
  });

  it("marks an idle row running when queued acceptance arrives first", () => {
    const queued = patchSessionSummariesFromGatewayEvent([session("thread-1")], {
      type: "turnQueued",
      threadId: "thread-1",
      turnId: "turn-queued",
      queuePosition: 1
    });

    expect(queued[0]?.activity).toMatchObject({
      running: true,
      activeTurnId: null,
      queuedTurns: 1
    });
  });

  it("does not let a late terminal clear a newer active turn", () => {
    const current = session("thread-1");
    current.updatedAtMs = 40;
    current.activity = {
      running: true,
      activeTurnId: "turn-new",
      queuedTurns: 1
    };

    const next = patchSessionSummariesFromGatewayEvent(
      [current],
      completion("turn-old", 30)
    );

    expect(next[0]).toMatchObject({
      updatedAtMs: 40,
      activity: {
        running: true,
        activeTurnId: "turn-new",
        queuedTurns: 1
      }
    });
  });

  it("keeps running when completion arrives before a queued successor starts", () => {
    const current = session("thread-1");
    current.activity = {
      running: true,
      activeTurnId: "turn-active",
      queuedTurns: 1
    };

    const completed = patchSessionSummariesFromGatewayEvent(
      [current],
      completion("turn-active")
    );
    const successor = patchSessionSummariesFromGatewayEvent(completed, {
      type: "turnStarted",
      threadId: "thread-1",
      turnId: "turn-successor",
      selectedSkills: []
    });

    expect(completed[0]?.activity).toMatchObject({
      running: true,
      activeTurnId: null,
      queuedTurns: 1
    });
    expect(successor[0]?.activity).toMatchObject({
      running: true,
      activeTurnId: "turn-successor",
      queuedTurns: 0
    });
  });

  it("keeps the successor active when it starts before the prior completion", () => {
    const current = session("thread-1");
    current.activity = {
      running: true,
      activeTurnId: "turn-active",
      queuedTurns: 1
    };

    const successor = patchSessionSummariesFromGatewayEvent([current], {
      type: "turnStarted",
      threadId: "thread-1",
      turnId: "turn-successor",
      selectedSkills: []
    });
    const lateCompletion = patchSessionSummariesFromGatewayEvent(
      successor,
      completion("turn-active")
    );

    expect(successor[0]?.activity).toMatchObject({
      running: true,
      activeTurnId: "turn-successor",
      queuedTurns: 0
    });
    expect(lateCompletion[0]?.activity).toMatchObject({
      running: true,
      activeTurnId: "turn-successor",
      queuedTurns: 0
    });
  });

  it("does not regress the aggregate queue when acceptance observers arrive out of order", () => {
    const current = session("thread-1");
    current.activity = {
      frameworkRevision: "1",
      running: true,
      activeTurnId: "turn-a",
      queuedTurns: 0
    };

    const queuedC = patchSessionSummariesFromGatewayEvent([current], {
      type: "turnQueued",
      threadId: "thread-1",
      turnId: "turn-c",
      queuePosition: 2
    });
    const queuedB = patchSessionSummariesFromGatewayEvent(queuedC, {
      type: "turnQueued",
      threadId: "thread-1",
      turnId: "turn-b",
      queuePosition: 1
    });
    const newest = patchSessionSummariesFromGatewayEvent(queuedB, {
      type: "activityChanged",
      threadId: "thread-1",
      activity: {
        frameworkRevision: "3",
        running: true,
        activeTurnId: "turn-a",
        queuedTurns: 2
      }
    });
    const stale = patchSessionSummariesFromGatewayEvent(newest, {
      type: "activityChanged",
      threadId: "thread-1",
      activity: {
        frameworkRevision: "2",
        running: true,
        activeTurnId: "turn-a",
        queuedTurns: 1
      }
    });
    const completedA = patchSessionSummariesFromGatewayEvent(
      stale,
      completion("turn-a")
    );
    const successor = patchSessionSummariesFromGatewayEvent(completedA, {
      type: "activityChanged",
      threadId: "thread-1",
      activity: {
        frameworkRevision: "4",
        running: true,
        activeTurnId: "turn-b",
        queuedTurns: 1
      }
    });
    const startedB = patchSessionSummariesFromGatewayEvent(successor, {
      type: "turnStarted",
      threadId: "thread-1",
      turnId: "turn-b",
      selectedSkills: []
    });
    const terminalActivity = patchSessionSummariesFromGatewayEvent(startedB, {
      type: "activityChanged",
      threadId: "thread-1",
      activity: {
        frameworkRevision: "5",
        running: true,
        activeTurnId: "turn-c",
        queuedTurns: 0
      }
    });
    const completedB = patchSessionSummariesFromGatewayEvent(
      terminalActivity,
      completion("turn-b")
    );

    expect(completedB[0]?.activity).toMatchObject({
      running: true,
      activeTurnId: "turn-c",
      queuedTurns: 0,
      frameworkRevision: "5"
    });
  });

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

function completion(
  turnId = "turn-1",
  completedAtMs = 30
): Extract<GatewayEvent, { type: "turnCompleted" }> {
  return {
    type: "turnCompleted",
    threadId: "thread-1",
    turnId,
    turn: {
      id: turnId,
      threadId: "thread-1",
      status: "completed",
      outcome: "normal",
      error: null,
      completedAtMs
    },
    committedEntries: []
  };
}
