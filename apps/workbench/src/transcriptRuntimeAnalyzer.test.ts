import { describe, expect, it } from "vitest";
import { analyzeTranscriptRuntimeRows, type TranscriptRuntimeRowSample } from "./transcriptRuntimeAnalyzer";

function row(overrides: Partial<TranscriptRuntimeRowSample> = {}): TranscriptRuntimeRowSample {
  return {
    blockId: "block-1",
    entryId: "entry-1",
    header: "exec_command",
    kind: "tool",
    running: false,
    source: "runtime.message",
    status: "pending",
    text: "exec_command pending",
    turnId: "turn-1",
    ...overrides
  };
}

describe("transcript runtime analyzer", () => {
  it("flags bare pending exec_command rows as projection defects", () => {
    const analysis = analyzeTranscriptRuntimeRows([row()]);

    expect(analysis.errors).toContain(
      "barePendingToolInvocation: pending exec_command row lacks invocation identity block-1"
    );
  });

  it("allows pending exec_command rows that expose the command invocation", () => {
    const analysis = analyzeTranscriptRuntimeRows([
      row({
        header: "exec_command sqlite3 /tmp/hn.db \"SELECT id FROM stories;\"",
        text: "exec_command sqlite3 /tmp/hn.db \"SELECT id FROM stories;\" pending"
      })
    ]);

    expect(analysis.errors).toEqual([]);
  });

  it("allows active message-derived tool rows to own a running live update", () => {
    const analysis = analyzeTranscriptRuntimeRows([
      row({
        header: "exec_command python fetch.py 34s",
        running: true,
        status: "34s",
        text: "exec_command python fetch.py 34s"
      })
    ], { activeTurnRunning: true });

    expect(analysis.errors).toEqual([]);
  });

  it("flags message-derived running tool rows after activity has settled", () => {
    const analysis = analyzeTranscriptRuntimeRows([
      row({
        header: "exec_command python fetch.py 34s",
        running: true,
        status: "34s",
        text: "exec_command python fetch.py 34s"
      })
    ]);

    expect(analysis.errors).toContain(
      "activeRowAfterTerminal: committed tool row is still active block-1"
    );
  });
});
