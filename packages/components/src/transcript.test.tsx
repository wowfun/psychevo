// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { TranscriptBlock, TranscriptEntry } from "@psychevo/protocol";
import { TranscriptPanel } from "./transcript";

beforeAll(() => {
  Element.prototype.scrollTo = vi.fn();
});

afterEach(() => {
  cleanup();
});

function transcriptEntry(blocks: TranscriptBlock[]): TranscriptEntry {
  return {
    id: "entry-1",
    threadId: "thread-1",
    turnId: "turn-1",
    messageSeq: 1,
    role: "assistant",
    status: "completed",
    source: "test",
    blocks,
    metadata: null,
    usage: null,
    accounting: null,
    createdAtMs: 1,
    updatedAtMs: 1
  };
}

function transcriptBlock(overrides: Partial<TranscriptBlock> = {}): TranscriptBlock {
  return {
    id: "block-1",
    kind: "file",
    status: "completed",
    order: 0,
    source: "test",
    title: "edit",
    body: null,
    preview: null,
    detail: null,
    artifactIds: [],
    metadata: {
      projection: "tool",
      tool_name: "edit",
      tool_call_id: "call-edit",
      args: { path: "primes.py" }
    },
    result: null,
    createdAtMs: 1,
    updatedAtMs: 1,
    ...overrides
  };
}

function editDiff(): string {
  return [
    "diff --git a/primes.py b/primes.py",
    "index 1111111..2222222 100644",
    "--- a/primes.py",
    "+++ b/primes.py",
    "@@ -1,3 +1,3 @@",
    " def is_prime(n):",
    "-    return False",
    "+    return n > 1"
  ].join("\n");
}

describe("TranscriptPanel inline diff evidence", () => {
  it("default-opens successful edit diffs with a compact edited title", () => {
    const block = transcriptBlock({
      metadata: {
        projection: "tool",
        tool_name: "edit",
        tool_call_id: "call-edit",
        args: {
          path: "primes.py",
          old_string: "return False",
          new_string: "return n > 1"
        }
      },
      result: {
        resultMessageSeq: 2,
        status: "completed",
        content: JSON.stringify({ diff: editDiff(), status: "ok" }),
        isError: false,
        metadata: null,
        createdAtMs: 2,
        updatedAtMs: 2
      }
    });

    const { container } = render(<TranscriptPanel entries={[transcriptEntry([block])]} />);

    expect(screen.getByText("Edited primes.py (+1 -1)")).toBeTruthy();
    expect(screen.getByLabelText("Inline diff")).toBeTruthy();
    expect(screen.getByText("return n > 1")).toBeTruthy();
    expect(container.querySelectorAll(".pevo-inlineDiffNumber").length).toBeGreaterThan(0);
    expect(container.querySelectorAll(".diffLineNumber")).toHaveLength(0);
    expect(screen.queryByText(/diff --git/)).toBeNull();
    expect(screen.queryByText("old_string")).toBeNull();
    expect(screen.queryByText("new_string")).toBeNull();
  });

  it("keeps write results without diff on the existing structured path", () => {
    const block = transcriptBlock({
      title: "write",
      metadata: {
        projection: "tool",
        tool_name: "write",
        tool_call_id: "call-write",
        args: { path: "feeds/report.md", content: "large markdown body" }
      },
      result: {
        resultMessageSeq: 2,
        status: "completed",
        content: JSON.stringify({ bytes_written: 34093, status: "ok" }),
        isError: false,
        metadata: null,
        createdAtMs: 2,
        updatedAtMs: 2
      }
    });

    render(<TranscriptPanel entries={[transcriptEntry([block])]} />);

    expect(screen.getByText("write feeds/report.md")).toBeTruthy();
    expect(screen.queryByLabelText("Inline diff")).toBeNull();
  });

  it("falls back to raw diff detail when update diff parsing fails", () => {
    const block = transcriptBlock({
      result: {
        resultMessageSeq: 2,
        status: "completed",
        content: JSON.stringify({ diff: "not a git patch", status: "ok" }),
        isError: false,
        metadata: null,
        createdAtMs: 2,
        updatedAtMs: 2
      }
    });

    render(<TranscriptPanel entries={[transcriptEntry([block])]} />);

    expect(screen.getByText("edit primes.py")).toBeTruthy();
    expect(screen.queryByLabelText("Inline diff")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /edit primes.py/ }));
    expect(screen.getByText("not a git patch")).toBeTruthy();
  });
});
