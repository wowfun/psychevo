import { describe, expect, it } from "vitest";
import type { GatewayEvent, PendingActionView } from "@psychevo/protocol";
import {
    EMPTY_GATEWAY_EVENT_FEED,
    GatewayEventJournal,
    appendGatewayEventFeed,
  confirmedSteerTurnId,
  gatewayEventsForThread
} from "./gateway-event-feed";

describe("Gateway thread event feed", () => {
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

  it("notifies only matching thread and event-family subscribers", () => {
    const journal = new GatewayEventJournal();
    const seen: number[] = [];
    journal.subscribe(({ seq }) => seen.push(seq), {
      eventTypes: ["titleChanged"],
      threadId: "thread-1"
    });

    journal.append({
      type: "activityChanged",
      threadId: "thread-1",
      activity: { running: false, activeTurnId: null, queuedTurns: 0 }
    });
    journal.append({
      type: "titleChanged",
      threadId: "thread-2",
      title: "Other",
      displayTitle: "Other"
    });
    journal.append({
      type: "titleChanged",
      threadId: "thread-1",
      title: "Match",
      displayTitle: "Match"
    });

    expect(seen).toEqual([3]);
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
