import { describe, expect, it, vi } from "vitest";
import type { GatewayEvent, PendingActionView } from "@psychevo/protocol";
import {
  EMPTY_GATEWAY_EVENT_FEED,
  GatewayEventFeedApplication,
  GatewayEventJournal,
  appendGatewayEventFeed,
  confirmedSteerTurnId,
  gatewayEventsForThread,
  reconcileThreadSnapshotAfterGatewayBarrier
} from "./gateway-event-feed";
import { snapshot } from "./liveTranscript.test-support";

describe("Gateway thread event feed", () => {
  it("publishes only the subscribed semantic event families", () => {
    const application = new GatewayEventFeedApplication();
    const all = vi.fn();
    const team = vi.fn();
    const turns = vi.fn();
    application.subscribe(all);
    application.subscribe(team, "teamLifecycle");
    application.subscribe(turns, "turnLifecycle");

    application.append({
      displayTitle: "Renamed",
      threadId: "thread-1",
      title: "Renamed",
      type: "titleChanged"
    });
    application.append({
      entry: {
        blocks: [],
        createdAtMs: 1,
        id: "plain-text",
        role: "assistant",
        source: "runtime",
        status: "running",
        threadId: "thread-1",
        turnId: "turn-1",
        updatedAtMs: 1
      },
      type: "entryUpdated",
      turnId: "turn-1"
    });
    application.append({
      queuePosition: 1,
      threadId: "thread-1",
      turnId: "turn-1",
      type: "turnQueued"
    });

    expect(all).toHaveBeenCalledTimes(3);
    expect(team).not.toHaveBeenCalled();
    expect(turns).toHaveBeenCalledOnce();
    expect(application.getSnapshot().latestSeq).toBe(3);
  });

  it("keeps each action lifecycle on its originating thread and forgets terminal actions", () => {
    let feed = EMPTY_GATEWAY_EVENT_FEED;
    feed = appendGatewayEventFeed(feed, actionEvent("actionRequested", action("permission-resolve")));
    feed = appendGatewayEventFeed(feed, actionEvent("actionUpdated", action("permission-resolve", null)));
    feed = appendGatewayEventFeed(feed, terminalActionEvent("actionResolved", "permission-resolve"));
    feed = appendGatewayEventFeed(feed, actionEvent("actionRequested", action("permission-cancel")));
    feed = appendGatewayEventFeed(feed, terminalActionEvent("actionCancelled", "permission-cancel"));

    expect(gatewayEventsForThread(feed, "child-thread").map(({ event }) => event.type)).toEqual([
      "actionRequested",
      "actionUpdated",
      "actionResolved",
      "actionRequested",
      "actionCancelled"
    ]);

    feed = appendGatewayEventFeed(feed, terminalActionEvent("actionCancelled", "permission-resolve"));
    feed = appendGatewayEventFeed(feed, terminalActionEvent("actionResolved", "permission-cancel"));
    expect(gatewayEventsForThread(feed, "child-thread").map(({ event }) => event.type)).toEqual([
      "actionRequested",
      "actionUpdated",
      "actionResolved",
      "actionRequested",
      "actionCancelled"
    ]);
  });

  it("replays pending-action lifecycle events observed after a snapshot-read barrier", () => {
    const permission = action("permission-raced", "thread-1");
    const beforeRead = snapshot();
    const barrierSeq = 0;
    let feed = appendGatewayEventFeed(
      EMPTY_GATEWAY_EVENT_FEED,
      actionEvent("actionRequested", permission)
    );

    const afterRequest = reconcileThreadSnapshotAfterGatewayBarrier(
      { ...beforeRead, pendingActions: [permission] },
      beforeRead,
      feed,
      "thread-1",
      barrierSeq
    );
    expect(afterRequest.pendingActions).toEqual([permission]);

    feed = appendGatewayEventFeed(
      feed,
      terminalActionEvent("actionResolved", permission.actionId)
    );
    const afterResolution = reconcileThreadSnapshotAfterGatewayBarrier(
      { ...beforeRead, pendingActions: [] },
      { ...beforeRead, pendingActions: [permission] },
      feed,
      "thread-1",
      barrierSeq
    );
    expect(afterResolution.pendingActions).toEqual([]);
  });

  it("does not resurrect an action after its resolution leaves the Thread event ring", () => {
    const permission = action("permission-evicted-resolution", "thread-1");
    const beforeRead = snapshot();
    const barrierSeq = 0;
    let feed = appendGatewayEventFeed(
      EMPTY_GATEWAY_EVENT_FEED,
      actionEvent("actionRequested", permission)
    );
    feed = appendGatewayEventFeed(
      feed,
      terminalActionEvent("actionResolved", permission.actionId)
    );
    for (let index = 0; index < 501; index += 1) {
      feed = appendGatewayEventFeed(feed, {
        type: "titleChanged",
        threadId: "thread-1",
        title: `Title ${index}`,
        displayTitle: `Title ${index}`
      });
    }

    expect(gatewayEventsForThread(feed, "thread-1").some(
      ({ event }) => event.type === "actionResolved"
    )).toBe(false);
    const reconciled = reconcileThreadSnapshotAfterGatewayBarrier(
      { ...beforeRead, pendingActions: [] },
      { ...beforeRead, pendingActions: [permission] },
      feed,
      "thread-1",
      barrierSeq
    );

    expect(reconciled.pendingActions).toEqual([]);
  });

  it("treats pending-action state at or before the read barrier as snapshot-owned", () => {
    const permission = action("permission-before-read", "thread-1");
    const current = { ...snapshot(), pendingActions: [permission] };
    const feed = appendGatewayEventFeed(
      EMPTY_GATEWAY_EVENT_FEED,
      actionEvent("actionRequested", permission)
    );

    const reconciled = reconcileThreadSnapshotAfterGatewayBarrier(
      current,
      snapshot(),
      feed,
      "thread-1",
      feed.latestSeq
    );

    expect(reconciled.pendingActions).toEqual([]);
  });

  it("forgets unresolved actions when their turn completes", () => {
    let feed = appendGatewayEventFeed(
      EMPTY_GATEWAY_EVENT_FEED,
      actionEvent("actionRequested", action("permission-completed-turn"))
    );
    feed = appendGatewayEventFeed(
      feed,
      actionEvent("actionRequested", action("permission-sibling", "sibling-thread"))
    );
    feed = appendGatewayEventFeed(feed, {
      type: "turnCompleted",
      threadId: "child-thread",
      turnId: "parent-turn",
      turn: {
        id: "parent-turn",
        threadId: "child-thread",
        status: "completed",
        outcome: "normal",
        error: null,
        startedAtMs: 1,
        completedAtMs: 2
      },
      committedEntries: []
    });
    feed = appendGatewayEventFeed(
      feed,
      terminalActionEvent("actionResolved", "permission-completed-turn")
    );
    feed = appendGatewayEventFeed(
      feed,
      terminalActionEvent("actionResolved", "permission-sibling")
    );

    expect(gatewayEventsForThread(feed, "child-thread").map(({ event }) => event.type)).toEqual([
      "actionRequested",
      "turnCompleted"
    ]);
    expect(gatewayEventsForThread(feed, "sibling-thread").map(({ event }) => event.type)).toEqual([
      "actionRequested",
      "actionResolved"
    ]);
  });

  it("does not resurrect a completed Turn action after the completion leaves the ring", () => {
    const permission = action("permission-completed-then-evicted", "thread-1");
    const beforeRead = snapshot();
    let feed = appendGatewayEventFeed(
      EMPTY_GATEWAY_EVENT_FEED,
      actionEvent("actionRequested", permission)
    );
    feed = appendGatewayEventFeed(feed, turnCompletedEvent("thread-1", "parent-turn"));
    for (let index = 0; index < 2_001; index += 1) {
      feed = appendGatewayEventFeed(feed, {
        type: "titleChanged",
        threadId: "sibling-thread",
        title: `Sibling ${index}`,
        displayTitle: `Sibling ${index}`
      });
    }
    expect(feed.journal?.actionLifecyclesForThread(
      "thread-1",
      0,
      feed.latestSeq
    ).map(({ event }) => event.type)).toEqual(["turnCompleted"]);

    const reconciled = reconcileThreadSnapshotAfterGatewayBarrier(
      { ...beforeRead, pendingActions: [] },
      { ...beforeRead, pendingActions: [permission] },
      feed,
      "thread-1",
      0
    );

    expect(reconciled.pendingActions).toEqual([]);
  });

  it("rejects stale snapshot turns until the queued follow-up actually starts", () => {
    let feed = appendGatewayEventFeed(EMPTY_GATEWAY_EVENT_FEED, {
      type: "turnCompleted",
      threadId: "thread-1",
      turnId: "turn-finished",
      turn: {
        id: "turn-finished",
        threadId: "thread-1",
        status: "completed",
        outcome: "normal",
        error: null,
        startedAtMs: 1,
        completedAtMs: 2
      },
      committedEntries: []
    });
    expect(confirmedSteerTurnId(feed, "thread-1", "turn-finished")).toBeNull();

    feed = appendGatewayEventFeed(feed, {
      type: "turnQueued",
      threadId: "thread-1",
      turnId: "turn-follow-up",
      queuePosition: 1
    });
    expect(confirmedSteerTurnId(feed, "thread-1", "turn-finished")).toBeNull();

    feed = appendGatewayEventFeed(feed, {
      type: "turnStarted",
      threadId: "thread-1",
      turnId: "turn-follow-up",
      selectedSkills: []
    });
    expect(confirmedSteerTurnId(feed, "thread-1", "turn-finished")).toBe("turn-follow-up");
  });

  it("uses the snapshot active turn as a reload fallback before lifecycle events arrive", () => {
    expect(confirmedSteerTurnId(EMPTY_GATEWAY_EVENT_FEED, "thread-1", "turn-reloaded"))
      .toBe("turn-reloaded");
  });

  it("keeps O(1) bounded global and per-thread rings", () => {
    let feed = EMPTY_GATEWAY_EVENT_FEED;
    for (let index = 0; index < 2_100; index += 1) {
      feed = appendGatewayEventFeed(feed, {
        type: "titleChanged",
        threadId: `thread-${index % 3}`,
        title: `Title ${index}`,
        displayTitle: `Title ${index}`
      });
    }

    expect(feed.journal?.eventsThrough(feed.latestSeq)).toHaveLength(2_000);
    expect(gatewayEventsForThread(feed, "thread-0")).toHaveLength(500);
    expect(gatewayEventsForThread(feed, "thread-1")).toHaveLength(500);
    expect(gatewayEventsForThread(feed, "thread-2")).toHaveLength(500);
  });

});

function action(actionId: string, threadId: string | null = "child-thread"): PendingActionView {
  return {
    actionId,
    kind: "permission",
    payload: { reason: "approval required" },
    turnId: "parent-turn",
    ...(threadId ? { threadId } : {})
  };
}

function actionEvent(
  type: "actionRequested" | "actionUpdated",
  value: PendingActionView
): GatewayEvent {
  return { type, action: value };
}

function terminalActionEvent(
  type: "actionResolved" | "actionCancelled",
  actionId: string
): GatewayEvent {
  return type === "actionResolved"
    ? {
        type,
        actionId,
        kind: "permission",
        outcome: "accepted",
        payload: { decision: "allowOnce" }
      }
    : {
        type,
        actionId,
        kind: "permission",
        reason: "cancelled"
      };
}

function turnCompletedEvent(threadId: string, turnId: string): GatewayEvent {
  return {
    type: "turnCompleted",
    threadId,
    turnId,
    turn: {
      id: turnId,
      threadId,
      status: "completed",
      outcome: "normal",
      error: null,
      startedAtMs: 1,
      completedAtMs: 2
    },
    committedEntries: []
  };
}
