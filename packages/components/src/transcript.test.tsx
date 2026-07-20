// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { TranscriptBlock, TranscriptEntry } from "@psychevo/protocol";
import { TranscriptPanel } from "./transcript";
import { evidenceDisplay } from "./toolEvidence";

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

describe("TranscriptPanel Markdown rendering", () => {
  it("renders text blocks through the shared Markdown renderer", () => {
    const block = transcriptBlock({
      kind: "text",
      body: "---\ntitle: Shared\n---\n# Shared Markdown\n\nBody",
      metadata: null
    });

    render(<TranscriptPanel entries={[transcriptEntry([block])]} />);

    expect(screen.getByRole("table", { name: "YAML frontmatter" })).toBeTruthy();
    expect(screen.getByText("Shared")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Shared Markdown" })).toBeTruthy();
    expect(screen.getByText("Body")).toBeTruthy();
  });

  it("keeps copy controls at the message level instead of the Markdown renderer", () => {
    const block = transcriptBlock({
      kind: "text",
      body: "# Shared Markdown",
      metadata: null
    });
    const onCopyText = vi.fn();

    render(<TranscriptPanel entries={[transcriptEntry([block])]} onCopyText={onCopyText} />);

    expect(screen.queryByRole("button", { name: "Copy Markdown" })).toBeNull();
    const copyButtons = screen.getAllByRole("button", { name: "Copy message" });
    expect(copyButtons).toHaveLength(1);
    fireEvent.click(copyButtons[0] as HTMLElement);
    expect(onCopyText).toHaveBeenCalledWith("# Shared Markdown");
  });

  it("exposes assistant read-aloud at the message level", () => {
    const block = transcriptBlock({
      kind: "text",
      body: "Read this response",
      metadata: null
    });
    const onReadAloudText = vi.fn();

    render(<TranscriptPanel entries={[transcriptEntry([block])]} onReadAloudText={onReadAloudText} />);

    fireEvent.click(screen.getByRole("button", { name: "Read aloud" }));
    expect(onReadAloudText).toHaveBeenCalledWith("Read this response");
  });
});

describe("TranscriptPanel history editing", () => {
  it("edits ordered text and image parts and distinguishes update from fork", async () => {
    const entry = {
      ...transcriptEntry([transcriptBlock({ kind: "text", body: "Original", metadata: null })]),
      role: "user" as const
    };
    const onReadUserMessageDraft = vi.fn().mockResolvedValue({
      threadId: "thread-1",
      messageId: "entry-1",
      messageSeq: 1,
      parts: [
        { type: "text" as const, text: "Before" },
        { type: "image" as const, input: { kind: "url" as const, url: "https://example.test/image.png" } },
        { type: "text" as const, text: "After" }
      ],
      fidelity: "bestEffort" as const,
      warning: "Older message reconstructed.",
      unavailableReason: null
    });
    const onUpdateUserMessage = vi.fn();
    const onForkUserMessage = vi.fn();

    render(
      <TranscriptPanel
        entries={[entry]}
        onForkUserMessage={onForkUserMessage}
        onReadUserMessageDraft={onReadUserMessageDraft}
        onUpdateUserMessage={onUpdateUserMessage}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: /Edit this message/ }));
    const firstText = await screen.findByRole("textbox", { name: "Message text 1" });
    expect(screen.getByText("Older message reconstructed.")).toBeTruthy();
    expect(screen.getByText("https://example.test/image.png")).toBeTruthy();
    fireEvent.change(firstText, { target: { value: "Updated before" } });
    fireEvent.click(screen.getByRole("button", { name: "Update this message and run in the same thread" }));

    await waitFor(() => expect(onUpdateUserMessage).toHaveBeenCalledTimes(1));
    expect(onUpdateUserMessage.mock.calls[0]?.[1]).toEqual({
      parts: [
        { type: "text", text: "Updated before" },
        { type: "image", input: { kind: "url", url: "https://example.test/image.png" } },
        { type: "text", text: "After" }
      ]
    });
    expect(onForkUserMessage).not.toHaveBeenCalled();
  });
});

describe("TranscriptPanel Thinking lifecycle", () => {
  function reasoningEntry(status: TranscriptBlock["status"], body = "Inspect the workspace"): TranscriptEntry {
    return transcriptEntry([transcriptBlock({
      id: "reasoning-1",
      kind: "reasoning",
      status,
      body,
      metadata: null
    })]);
  }

  it.each(["completed", "failed", "cancelled"] as const)(
    "collapses Thinking when running reasoning becomes %s",
    (status) => {
      const { rerender } = render(<TranscriptPanel entries={[reasoningEntry("running")]} />);

      expect(screen.getByRole("button", { name: /Thinking/ }).getAttribute("aria-expanded")).toBe("true");
      expect(screen.getByText("Inspect the workspace")).toBeTruthy();

      rerender(<TranscriptPanel entries={[reasoningEntry(status)]} />);

      expect(screen.getByRole("button", { name: /Thinking/ }).getAttribute("aria-expanded")).toBe("false");
      expect(screen.queryByText("Inspect the workspace")).toBeNull();
    }
  );

  it("preserves manual folding choices across same-state updates", () => {
    const { rerender } = render(<TranscriptPanel entries={[reasoningEntry("running")]} />);
    const header = screen.getByRole("button", { name: /Thinking/ });

    fireEvent.click(header);
    expect(header.getAttribute("aria-expanded")).toBe("false");

    rerender(<TranscriptPanel entries={[reasoningEntry("running", "Inspect the workspace again")]} />);
    expect(screen.getByRole("button", { name: /Thinking/ }).getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByText("Inspect the workspace again")).toBeNull();

    rerender(<TranscriptPanel entries={[reasoningEntry("completed", "Inspection complete")]} />);
    const completedHeader = screen.getByRole("button", { name: /Thinking/ });
    fireEvent.click(completedHeader);
    expect(completedHeader.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText("Inspection complete")).toBeTruthy();

    rerender(<TranscriptPanel entries={[reasoningEntry("completed", "Inspection remains complete")]} />);
    expect(screen.getByRole("button", { name: /Thinking/ }).getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText("Inspection remains complete")).toBeTruthy();
  });

  it("loads terminal Thinking collapsed", () => {
    render(<TranscriptPanel entries={[reasoningEntry("completed")]} />);

    expect(screen.getByRole("button", { name: /Thinking/ }).getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByText("Inspect the workspace")).toBeNull();
  });
});

describe("TranscriptPanel write argument previews", () => {
  function writePreviewBlock(
    text: string,
    phase: "generating" | "writing" | "failed" | "cancelled" = "generating",
    status: TranscriptBlock["status"] = "pending"
  ): TranscriptBlock {
    return transcriptBlock({
      id: "write-preview-1",
      kind: "file",
      status,
      title: "write report.md",
      metadata: {
        projection: "tool",
        tool_name: "write",
        tool_call_id: "call-write",
        args: null,
        write_argument_preview: {
          phase,
          path: "report.md",
          text,
          bytes_seen: new TextEncoder().encode(text).length,
          lines_seen: text ? text.split("\n").length : 0,
          omitted_bytes: 0,
          truncated: false
        }
      }
    });
  }

  it("opens the first preview and preserves manual collapse across deltas", () => {
    const { rerender } = render(
      <TranscriptPanel entries={[transcriptEntry([writePreviewBlock("first")])]} />
    );
    const header = screen.getByRole("button", { name: /write report\.md/ });
    expect(header.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText("File content")).toBeTruthy();
    expect(screen.getByText("first")).toBeTruthy();
    expect(screen.getByText(/Generating · 5 bytes · 1 line/)).toBeTruthy();

    fireEvent.click(header);
    rerender(
      <TranscriptPanel entries={[transcriptEntry([writePreviewBlock("first second")])]} />
    );
    expect(screen.getByRole("button", { name: /write report\.md/ }).getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByText("first second")).toBeNull();
  });

  it("collapses once after success and preserves later manual expansion", () => {
    const { rerender } = render(
      <TranscriptPanel entries={[transcriptEntry([writePreviewBlock("complete body", "writing", "running")])]} />
    );
    expect(screen.getByRole("button", { name: /write report\.md/ }).getAttribute("aria-expanded")).toBe("true");

    const completed = transcriptBlock({
      id: "write-preview-1",
      kind: "file",
      status: "completed",
      title: "write report.md",
      metadata: {
        projection: "tool",
        tool_name: "write",
        tool_call_id: "call-write",
        args: { path: "report.md", content: "complete body" },
        write_argument_preview: null
      },
      result: {
        resultMessageSeq: 2,
        status: "completed",
        content: JSON.stringify({ path: "report.md", bytes_written: 13 }),
        isError: false,
        metadata: null,
        createdAtMs: 2,
        updatedAtMs: 2
      }
    });
    rerender(<TranscriptPanel entries={[transcriptEntry([completed])]} />);
    const completedHeader = screen.getByRole("button", { name: /write report\.md/ });
    expect(completedHeader.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByText("complete body")).toBeNull();

    fireEvent.click(completedHeader);
    rerender(<TranscriptPanel entries={[transcriptEntry([{ ...completed, updatedAtMs: 3 }])]} />);
    expect(screen.getByRole("button", { name: /write report\.md/ }).getAttribute("aria-expanded")).toBe("true");
  });

  it("keeps a failed write preview open with the error result", () => {
    const block = writePreviewBlock("unfinished body", "failed", "failed");
    block.result = {
      resultMessageSeq: 2,
      status: "failed",
      content: JSON.stringify({ error: "permission denied" }),
      isError: true,
      metadata: null,
      createdAtMs: 2,
      updatedAtMs: 2
    };
    render(<TranscriptPanel entries={[transcriptEntry([block])]} />);

    expect(screen.getByRole("button", { name: /write report\.md/ }).getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText("unfinished body")).toBeTruthy();
    expect(screen.getAllByText("permission denied").length).toBeGreaterThan(0);
  });
});

describe("TranscriptPanel evidence titles", () => {
  it("gives parallel live web search titles the remaining row width without provider summaries", () => {
    const queries = [
      "most popular AI agent frameworks 2026",
      "2026 年最流行的 AI Agent 框架"
    ];
    const titles = queries.map((query) => `Searching the web ${query}`);
    const blocks = queries.map((query, index) => transcriptBlock({
      id: `block-web-search-${index}`,
      kind: "web",
      status: "running",
      order: index,
      title: titles[index]!,
      metadata: {
        projection: "tool",
        tool_name: "web_search",
        tool_call_id: `call-web-search-${index}`,
        args: { query },
        display: {
          category: "explore",
          title_arg_keys: ["query"],
          title_result_keys: ["query", "provider"],
          summary_keys: ["provider", "truncated", "error"],
          body_keys: ["payload", "error"],
          body_policy: "body"
        },
        result: { query, provider: "exa" }
      }
    }));

    const { container } = render(<TranscriptPanel entries={[transcriptEntry(blocks)]} />);

    const titleElements = Array.from(container.querySelectorAll(".pevo-evidenceLine code"));
    expect(titleElements.map((element) => element.textContent)).toEqual(titles);
    expect(titleElements.map((element) => element.getAttribute("title"))).toEqual(titles);
    expect(titleElements.every((element) => element.closest("button")?.classList.contains("is-singleTitle"))).toBe(true);
    expect(container.querySelectorAll(".pevo-evidenceLine span:not(.pevo-evidenceSpinner)")).toHaveLength(0);
    expect(container.textContent).not.toContain("exa");
  });

  it("keeps a status summary in the split layout while disclosing the complete title", () => {
    const title = "Model request failed after the provider connection closed unexpectedly";
    const block = transcriptBlock({
      id: "block-model-failure",
      kind: "status",
      status: "failed",
      title,
      preview: "Retry with another model or inspect provider settings.",
      metadata: null
    });

    const { container } = render(<TranscriptPanel entries={[transcriptEntry([block])]} />);

    const titleElement = container.querySelector(".pevo-evidenceLine code");
    const row = titleElement?.closest("button");
    expect(titleElement?.getAttribute("title")).toBe(title);
    expect(row?.classList.contains("is-singleTitle")).toBe(false);
    expect(screen.getByText("Retry with another model or inspect provider settings.")).toBeTruthy();
  });
});

describe("TranscriptPanel externally owned history", () => {
  it("keeps a retained earlier-turn live answer before a later detached optimistic prompt", () => {
    const firstQuestion: TranscriptEntry = {
      ...transcriptEntry([transcriptBlock({
        id: "message:1:text",
        kind: "text",
        body: "first question",
        metadata: null
      })]),
      id: "message:1",
      role: "user",
      source: "runtime.message",
      turnId: "turn-1",
      messageSeq: 1,
      createdAtMs: 100,
      updatedAtMs: 100
    };
    const firstAnswer: TranscriptEntry = {
      ...transcriptEntry([transcriptBlock({
        id: "live:turn-1:assistant:0:text",
        kind: "text",
        body: "first answer",
        metadata: null
      })]),
      id: "live:turn-1:assistant:0",
      source: "runtime.stream",
      turnId: "turn-1",
      messageSeq: null,
      metadata: { liveOrder: 0, projection: "assistant_segment" },
      createdAtMs: 200,
      updatedAtMs: 200
    };
    const secondQuestion: TranscriptEntry = {
      ...transcriptEntry([transcriptBlock({
        id: "optimistic:300:second:text",
        kind: "text",
        body: "second question",
        metadata: null
      })]),
      id: "optimistic:300:second",
      role: "user",
      source: "client.optimistic",
      turnId: null,
      messageSeq: null,
      metadata: { liveOrder: -1, projection: "optimistic_prompt" },
      createdAtMs: 300,
      updatedAtMs: 300
    };

    const { container } = render(
      <TranscriptPanel entries={[firstQuestion, firstAnswer, secondQuestion]} threadId="thread-1" />
    );

    expect(Array.from(container.querySelectorAll("[data-entry-id]"), (node) => (
      node.getAttribute("data-entry-id")
    ))).toEqual([
      "message:1",
      "live:turn-1:assistant:0",
      "optimistic:300:second"
    ]);
  });

  it("keeps native phase labels collapsed until the user asks for them", () => {
    const phaseOne = transcriptBlock({
      id: "phase-1",
      kind: "reasoning",
      body: "Inspect the workspace",
      metadata: null,
      phaseOrdinal: 1
    });
    const phaseTwo = transcriptBlock({
      id: "phase-2",
      kind: "text",
      body: "Implemented the change",
      metadata: null,
      phaseOrdinal: 2
    });

    const { container } = render(<TranscriptPanel entries={[transcriptEntry([phaseOne, phaseTwo])]} />);

    expect(screen.getByRole("button", { name: "Show native phases" }).getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByRole("region", { name: "Phase 1" })).toBeNull();
    expect(container.querySelector('[data-phase-ordinal="2"]')).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Show native phases" }));
    expect(screen.getByRole("region", { name: "Phase 1" })).toBeTruthy();
    expect(screen.getByRole("region", { name: "Phase 2" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Hide native phases" }).getAttribute("aria-expanded")).toBe("true");
  });

  it("states degraded agent history fidelity without adapter-specific labels", () => {
    render(
      <TranscriptPanel
        entries={[]}
        history={{
          owner: "agent",
          fidelity: "partial",
          cursor: "opaque-agent-cursor",
          hint: "Earlier turns are available only as a partial agent history."
        }}
      />
    );

    const notice = screen.getByRole("note");
    expect(notice.getAttribute("data-history-owner")).toBe("agent");
    expect(notice.getAttribute("data-history-fidelity")).toBe("partial");
    expect(notice.textContent).toContain("Earlier turns are available only as a partial agent history.");
    expect(notice.textContent).not.toContain("Codex");
    expect(notice.textContent).not.toContain("OpenCode");
  });

  it("identifies degraded process-owned history", () => {
    render(
      <TranscriptPanel
        entries={[]}
        history={{
          owner: "process",
          fidelity: "unavailable",
          cursor: null,
          hint: null
        }}
      />
    );

    const notice = screen.getByRole("note");
    expect(notice.getAttribute("data-history-owner")).toBe("process");
    expect(notice.textContent).toBe("Earlier history is unavailable from this process.");
  });
});

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

  it("derives pending exec_command titles from metadata args when block title is bare", () => {
    const block = transcriptBlock({
      kind: "shell",
      status: "pending",
      title: "exec_command",
      metadata: {
        projection: "tool",
        tool_name: "exec_command",
        tool_call_id: "call-exec",
        args: {
          cmd: "sqlite3 /home/kevin/Projects/feedgarden/feeds/.cache/hn.db \"SELECT id FROM stories;\""
        }
      }
    });

    const display = evidenceDisplay(block, "");

    expect(display.title).toBe(
      "exec_command sqlite3 /home/kevin/Projects/feedgarden/feeds/.cache/hn.db \"SELECT id FROM stories;\""
    );
    expect(display.sections[0]).toMatchObject({
      kind: "text",
      text: "sqlite3 /home/kevin/Projects/feedgarden/feeds/.cache/hn.db \"SELECT id FROM stories;\"",
      title: "Command"
    });
  });

  it("renders ACP Agent shell snapshots through the standard typed tool evidence path", () => {
    const block = transcriptBlock({
      kind: "shell",
      title: "exec_command",
      metadata: {
        projection: "tool",
        origin: "acp_peer",
        runtimeProjection: "acp_peer",
        tool_name: "exec_command",
        args: { command: "cargo test" },
        result: { output: "33 passed" }
      }
    });

    const display = evidenceDisplay(block, "");

    expect(display.title).toBe("exec_command cargo test");
    expect(display.sections).toEqual(expect.arrayContaining([
      expect.objectContaining({ kind: "text", text: "cargo test", title: "Command" })
    ]));
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

describe("TranscriptPanel generated image artifacts", () => {
  it("renders generated image artifacts as image cards with lightbox and download controls", () => {
    const block = transcriptBlock({
      kind: "artifact",
      title: "Generated image",
      body: "Generated image\nPrompt: a red cube\nSaved: /tmp/img_test.png",
      artifactIds: ["img_test"],
      metadata: {
        result: {
          mediaKind: "generated_image",
          artifactId: "img_test",
          prompt: "a red cube",
          savedPath: "/tmp/img_test.png",
          displayUrl: "/_gateway/media/img_test",
          agentVisibleSource: "psychevo-media://img_test",
          mimeType: "image/png",
          provider: "fake",
          model: "fake-image",
          width: 1,
          height: 1
        }
      }
    });

    render(<TranscriptPanel entries={[transcriptEntry([block])]} />);

    const image = screen.getByRole("img", { name: "Generated image: a red cube" }) as HTMLImageElement;
    expect(image.src).toContain("/_gateway/media/img_test");
    expect(screen.getByText("a red cube")).toBeTruthy();
    expect(screen.getByText("/tmp/img_test.png")).toBeTruthy();
    expect(
      screen
        .getByRole("link", { name: "Download generated image" })
        .getAttribute("download")
    ).toBe("img_test.png");

    fireEvent.click(screen.getByRole("button", { name: "Open generated image preview" }));
    expect(screen.getByRole("dialog", { name: "Generated image preview" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Close image preview" }));
    expect(screen.queryByRole("dialog", { name: "Generated image preview" })).toBeNull();
  });

  it("hides duplicate saved-path prose immediately after a generated image artifact", () => {
    const savedPath = "/tmp/img_test.png";
    const artifact = transcriptBlock({
      id: "artifact",
      kind: "artifact",
      title: "Generated image",
      order: 0,
      artifactIds: ["img_test"],
      metadata: {
        result: {
          mediaKind: "generated_image",
          artifactId: "img_test",
          prompt: "a red cube",
          savedPath,
          displayUrl: "/_gateway/media/img_test",
          agentVisibleSource: "psychevo-media://img_test",
          mimeType: "image/png"
        }
      }
    });
    const duplicateText = transcriptBlock({
      id: "text",
      kind: "text",
      body: `Saved: ${savedPath}`,
      order: 1,
      metadata: null
    });

    render(<TranscriptPanel entries={[transcriptEntry([artifact, duplicateText])]} />);

    expect(screen.getAllByText(savedPath)).toHaveLength(1);
  });
});

describe("TranscriptPanel web search sources", () => {
  it("renders URL citations as external links", () => {
    const block = transcriptBlock({
      kind: "web",
      title: "Rust",
      metadata: { projection: "url_citation", title: "Rust", url: "https://example.com/rust" }
    });
    render(<TranscriptPanel entries={[transcriptEntry([block])]} />);
    expect(screen.getByRole("link", { name: "Rust" }).getAttribute("href")).toBe("https://example.com/rust");
  });

  it("loads image-result thumbnails only after expansion and shows failure state", () => {
    const block = transcriptBlock({
      kind: "web",
      title: "Ferris",
      metadata: {
        projection: "web_image_source",
        caption: "Ferris",
        image_url: "https://images.example/ferris.png",
        thumbnail_url: "https://images.example/ferris-thumb.png",
        source_website_url: "https://example.com/ferris"
      }
    });
    render(<TranscriptPanel entries={[transcriptEntry([block])]} />);
    expect(screen.queryByRole("img", { name: "Ferris" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Ferris" }));
    const image = screen.getByRole("img", { name: "Ferris" });
    expect(image.getAttribute("src")).toBe("https://images.example/ferris-thumb.png");
    fireEvent.error(image);
    expect(screen.getByText("Image preview unavailable.")).toBeTruthy();
  });
});

describe("TranscriptPanel session scroll behavior", () => {
  const scrollHeightDescriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollHeight");
  const clientHeightDescriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "clientHeight");
  const originalScrollTo = Element.prototype.scrollTo;
  let scrollHeight = 1200;
  let clientHeight = 400;
  let scrollToMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    scrollHeight = 1200;
    clientHeight = 400;
    Object.defineProperty(HTMLElement.prototype, "scrollHeight", {
      configurable: true,
      get() {
        return scrollHeight;
      }
    });
    Object.defineProperty(HTMLElement.prototype, "clientHeight", {
      configurable: true,
      get() {
        return clientHeight;
      }
    });
    scrollToMock = vi.fn(function (this: Element, options?: ScrollToOptions | number, y?: number) {
      const top = typeof options === "number" ? y ?? 0 : options?.top ?? 0;
      setScrollTop(this as HTMLElement, top);
    });
    Element.prototype.scrollTo = scrollToMock as typeof Element.prototype.scrollTo;
  });

  afterEach(() => {
    restorePrototypeDescriptor("scrollHeight", scrollHeightDescriptor);
    restorePrototypeDescriptor("clientHeight", clientHeightDescriptor);
    Element.prototype.scrollTo = originalScrollTo;
  });

  it("positions an initial long thread at the latest message without smooth scrolling", () => {
    render(<TranscriptPanel entries={[scrollEntry("thread-a")]} threadId="thread-a" />);

    expect(scrollToMock).toHaveBeenLastCalledWith({ top: 1200, behavior: "auto" });
    expect(scrollToMock).not.toHaveBeenCalledWith(expect.objectContaining({ behavior: "smooth" }));
  });

  it("positions an unvisited switched thread at the latest message without animation", () => {
    const { rerender } = render(<TranscriptPanel entries={[scrollEntry("thread-a")]} threadId="thread-a" />);
    scrollToMock.mockClear();

    rerender(<TranscriptPanel entries={[scrollEntry("thread-b")]} threadId="thread-b" />);

    expect(scrollToMock).toHaveBeenLastCalledWith({ top: 1200, behavior: "auto" });
    expect(scrollToMock).not.toHaveBeenCalledWith(expect.objectContaining({ behavior: "smooth" }));
  });

  it("restores a visited thread's in-memory scroll position", () => {
    const { container, rerender } = render(<TranscriptPanel entries={[scrollEntry("thread-a")]} threadId="thread-a" />);
    const scroller = transcriptScroller(container);
    setScrollTop(scroller, 220);
    fireEvent.scroll(scroller);

    rerender(<TranscriptPanel entries={[scrollEntry("thread-b")]} threadId="thread-b" />);
    scrollToMock.mockClear();

    rerender(<TranscriptPanel entries={[scrollEntry("thread-a")]} threadId="thread-a" />);

    expect(scrollToMock).toHaveBeenLastCalledWith({ top: 220, behavior: "auto" });
  });

  it("does not force same-thread updates to the bottom after the user scrolls away", () => {
    const { container, rerender } = render(<TranscriptPanel entries={[scrollEntry("thread-a")]} threadId="thread-a" />);
    const scroller = transcriptScroller(container);
    scrollToMock.mockClear();
    setScrollTop(scroller, 180);
    fireEvent.scroll(scroller);

    rerender(<TranscriptPanel entries={[scrollEntry("thread-a"), scrollEntry("thread-a", "entry-a-2")]} threadId="thread-a" />);

    expect(scrollToMock).not.toHaveBeenCalled();
  });

  it("jumps directly to latest and hides the jump control", () => {
    const { container } = render(<TranscriptPanel entries={[scrollEntry("thread-a")]} threadId="thread-a" />);
    const scroller = transcriptScroller(container);
    scrollToMock.mockClear();
    setScrollTop(scroller, 160);
    fireEvent.scroll(scroller);

    fireEvent.click(screen.getByRole("button", { name: "Jump to latest" }));

    expect(scrollToMock).toHaveBeenLastCalledWith({ top: 1200, behavior: "auto" });
    expect(screen.queryByRole("button", { name: "Jump to latest" })).toBeNull();
  });
});

function scrollEntry(threadId: string, id = `entry-${threadId}`): TranscriptEntry {
  return {
    ...transcriptEntry([
      transcriptBlock({
        id: `${id}:text`,
        kind: "text",
        body: `message for ${threadId}`,
        metadata: null
      })
    ]),
    id,
    threadId
  };
}

function transcriptScroller(container: HTMLElement): HTMLElement {
  const scroller = container.querySelector(".pevo-threadItems");
  expect(scroller).toBeTruthy();
  return scroller as HTMLElement;
}

function setScrollTop(scroller: HTMLElement, top: number): void {
  Object.defineProperty(scroller, "scrollTop", {
    configurable: true,
    value: top,
    writable: true
  });
}

function restorePrototypeDescriptor(
  property: "clientHeight" | "scrollHeight",
  descriptor: PropertyDescriptor | undefined
): void {
  if (descriptor) {
    Object.defineProperty(HTMLElement.prototype, property, descriptor);
    return;
  }
  delete (HTMLElement.prototype as unknown as Record<string, unknown>)[property];
}
