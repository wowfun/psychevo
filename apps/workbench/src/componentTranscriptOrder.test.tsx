// @vitest-environment jsdom

import { TranscriptPanel } from "@psychevo/components";
import type { TranscriptBlock, TranscriptEntry } from "@psychevo/protocol";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

describe("transcript ordering", () => {
  it("renders older live overlays before newer durable messages", () => {
    const html = renderToStaticMarkup(
      <TranscriptPanel
        entries={[
          transcriptEntry({
            id: "message:15",
            messageSeq: 15,
            createdAtMs: 1500,
            updatedAtMs: 1500,
            blocks: [
              transcriptBlock({
                id: "message:15:text",
                kind: "text",
                body: "newer durable answer",
                createdAtMs: 1500,
                updatedAtMs: 1500
              })
            ]
          }),
          transcriptEntry({
            id: "live:turn-1:assistant:13",
            messageSeq: null,
            source: "runtime.stream",
            createdAtMs: 1300,
            updatedAtMs: 1300,
            metadata: {
              projection: "assistant_segment",
              liveOrder: 0,
              streamSeq: 13
            },
            blocks: [
              transcriptBlock({
                id: "live:turn-1:assistant:13:text",
                kind: "text",
                source: "runtime.stream",
                body: "older live answer",
                createdAtMs: 1300,
                updatedAtMs: 1300
              })
            ]
          })
        ]}
      />
    );

    expect(html.indexOf("older live answer")).toBeLessThan(
      html.indexOf("newer durable answer")
    );
  });
});

function transcriptEntry(
  overrides: Partial<TranscriptEntry> = {}
): TranscriptEntry {
  return {
    id: "entry-1",
    threadId: "thread-1",
    turnId: "turn-1",
    messageSeq: 1,
    role: "assistant",
    status: "completed",
    source: "runtime.message",
    blocks: [],
    metadata: null,
    usage: null,
    accounting: null,
    createdAtMs: 1,
    updatedAtMs: 1,
    ...overrides
  };
}

function transcriptBlock(
  overrides: Partial<TranscriptBlock> = {}
): TranscriptBlock {
  return {
    id: "block-1",
    kind: "text",
    status: "completed",
    order: 0,
    source: "runtime.message",
    title: null,
    body: null,
    preview: null,
    detail: null,
    artifactIds: [],
    metadata: null,
    result: null,
    createdAtMs: 1,
    updatedAtMs: 1,
    ...overrides
  };
}
