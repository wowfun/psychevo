// @vitest-environment jsdom

import { TranscriptPanel } from "@psychevo/components";
import type { TranscriptBlock, TranscriptEntry } from "@psychevo/protocol";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

describe("transcript ordering", () => {
  it("renders an optimistic user message before a newer durable assistant message", () => {
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
                body: "newer durable assistant message",
                createdAtMs: 1500,
                updatedAtMs: 1500
              })
            ]
          })
        ]}
        liveEntries={[
          transcriptEntry({
            id: "optimistic:turn-1:user",
            messageSeq: null,
            role: "user",
            source: "client.optimistic",
            createdAtMs: 1300,
            updatedAtMs: 1300,
            metadata: {
              projection: "optimistic_prompt",
              liveOrder: -1
            },
            blocks: [
              transcriptBlock({
                id: "optimistic:turn-1:user:text",
                kind: "text",
                source: "client.optimistic",
                body: "older optimistic user message",
                createdAtMs: 1300,
                updatedAtMs: 1300
              })
            ]
          })
        ]}
      />
    );

    expect(html.indexOf("older optimistic user message")).toBeLessThan(
      html.indexOf("newer durable assistant message")
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
