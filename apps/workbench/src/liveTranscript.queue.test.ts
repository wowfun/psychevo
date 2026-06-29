import { describe, expect, it } from "vitest";
import type { GatewayEvent } from "@psychevo/protocol";
import { applyTurnCompletionQueueBarrier } from "./liveTranscript";
import {
  block,
  completedTurn,
  entry,
  eventWithEntry
} from "./liveTranscript.test-support";

describe("applyTurnCompletionQueueBarrier", () => {
  it("drops queued live observations for the turn that just completed", () => {
    const sameTurnEntry = eventWithEntry("entryUpdated", entry({
      id: "live:turn-1:assistant:late",
      blocks: [block({ id: "live:turn-1:assistant:late:text", body: "late" })]
    }));
    const sameTurnDelta: GatewayEvent = {
      type: "entryDelta",
      turnId: "turn-1",
      entryId: "live:turn-1:assistant:late",
      blockId: "live:turn-1:assistant:late:text",
      delta: " stale"
    };
    const otherTurnEntry = eventWithEntry("entryUpdated", entry({
      id: "live:turn-2:assistant",
      turnId: "turn-2",
      blocks: [block({ id: "live:turn-2:assistant:text", body: "other" })]
    }));
    const completion: GatewayEvent = {
      type: "turnCompleted",
      threadId: "thread-1",
      turnId: "turn-1",
      turn: completedTurn("turn-1", "thread-1"),
      committedEntries: []
    };

    expect(applyTurnCompletionQueueBarrier([
      sameTurnEntry,
      sameTurnDelta,
      otherTurnEntry
    ], completion)).toEqual([otherTurnEntry]);
  });
});
