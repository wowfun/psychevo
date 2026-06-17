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
    expect(container.querySelectorAll(".pevo-toolSection h4")).toHaveLength(0);
    expect(screen.queryByText(/diff --git/)).toBeNull();
    expect(screen.queryByText("Input")).toBeNull();
    expect(screen.queryByText("Change")).toBeNull();
    expect(screen.queryByText("Diff")).toBeNull();
    expect(screen.queryByText("path")).toBeNull();
    expect(screen.queryByText("status")).toBeNull();
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

describe("TranscriptPanel read evidence", () => {
  it("renders successful reads as a title plus file content only", () => {
    const readPath = "/home/kevin/Projects/psychevo/.agents/skills/x-daily/config/users.json";
    const fileContent = [
      "{",
      "  \"users\": [",
      "    {",
      "      \"id\": \"daily\",",
      "      \"enabled\": true",
      "    }",
      "  ]",
      "}",
      ""
    ].join("\n");
    const block = transcriptBlock({
      id: "block-read",
      kind: "file",
      title: "read",
      metadata: {
        projection: "tool",
        tool_name: "read",
        tool_call_id: "call-read",
        args: { path: readPath }
      },
      result: {
        resultMessageSeq: 2,
        status: "completed",
        content: JSON.stringify({
          path: readPath,
          content: fileContent,
          total_lines: 8,
          file_size: 703,
          truncated: false,
          similar_files: ["/home/kevin/Projects/psychevo/.agents/skills/x-daily/config/users.example.json"],
          shown_start_line: 1,
          shown_end_line: 8,
          output_lines: 8,
          output_bytes: 703,
          first_line_exceeds_limit: false
        }),
        isError: false,
        metadata: null,
        createdAtMs: 2,
        updatedAtMs: 2
      }
    });

    const { container } = render(<TranscriptPanel entries={[transcriptEntry([block])]} />);

    const row = screen.getByRole("button", { name: `read ${readPath}` });
    expect(row).toBeTruthy();
    expect(row.classList.contains("is-singleTitle")).toBe(true);
    expect(screen.queryByText(/output bytes/)).toBeNull();

    fireEvent.click(row);

    const pre = container.querySelector(".pevo-toolDetail pre");
    expect(pre).toBeTruthy();
    expect(pre?.textContent).toBe(fileContent);
    expect(container.querySelectorAll(".pevo-toolDetail h4")).toHaveLength(0);
    expect(screen.queryByText("Input")).toBeNull();
    expect(screen.queryByText("Result")).toBeNull();
    expect(screen.queryByText("Content")).toBeNull();
    expect(screen.queryByText("path")).toBeNull();
    expect(screen.queryByText("file size")).toBeNull();
    expect(screen.queryByText("output bytes")).toBeNull();
    expect(screen.queryByText("shown start line")).toBeNull();
    expect(screen.queryByText("shown end line")).toBeNull();
    expect(screen.queryByText("similar files")).toBeNull();
  });

  it("keeps failed reads informative without generic metadata sections", () => {
    const readPath = "/home/kevin/Projects/psychevo/missing.json";
    const block = transcriptBlock({
      id: "block-read-error",
      kind: "file",
      title: "read",
      metadata: {
        projection: "tool",
        tool_name: "read",
        tool_call_id: "call-read-error",
        args: { path: readPath }
      },
      result: {
        resultMessageSeq: 2,
        status: "failed",
        content: JSON.stringify({
          path: readPath,
          error: "No such file or directory",
          similar_files: ["/home/kevin/Projects/psychevo/example.json"]
        }),
        isError: true,
        metadata: null,
        createdAtMs: 2,
        updatedAtMs: 2
      }
    });

    render(<TranscriptPanel entries={[transcriptEntry([block])]} />);

    const row = screen.getByRole("button", { name: `read ${readPath}` });
    fireEvent.click(row);

    expect(screen.getByText("No such file or directory")).toBeTruthy();
    expect(screen.queryByText("Input")).toBeNull();
    expect(screen.queryByText("Result")).toBeNull();
    expect(screen.queryByText("similar files")).toBeNull();
    expect(screen.queryByText("path")).toBeNull();
  });
});
