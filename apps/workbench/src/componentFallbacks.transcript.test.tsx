// @vitest-environment jsdom

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { HistoryPanel, StatusPanel, TranscriptPanel } from "@psychevo/components";
import type { TranscriptBlock } from "@psychevo/protocol";
import {
  noop,
  sessionSummary,
  setupComponentFallbackTests,
  transcriptBlock,
  transcriptEntry
} from "./componentFallbacks.test-support";

setupComponentFallbackTests();

describe("component fallback rendering", () => {
  it("does not render the empty transcript state for visible history entries", () => {
    const html = renderToStaticMarkup(
      <TranscriptPanel
        entries={[
          transcriptEntry({
            id: "message:1",
            role: "user",
            blocks: [
              transcriptBlock({
                id: "message:1:block:0",
                body: "hello history",
                preview: "hello history",
                detail: "hello history"
              })
            ]
          }),
          transcriptEntry({
            id: "message:2",
            messageSeq: 2,
            role: "assistant",
            blocks: [
              transcriptBlock({
                id: "message:2:block:0",
                body: "hello from assistant",
                preview: "hello from assistant",
                detail: "hello from assistant"
              })
            ]
          })
        ]}
      />
    );

    expect(html).toContain("hello history");
    expect(html).toContain("hello from assistant");
    expect(html).not.toContain("No messages yet");
  });

  it("hides side-inherited parent context from transcript rendering", () => {
    const html = renderToStaticMarkup(
      <TranscriptPanel
        entries={[
          transcriptEntry({
            id: "message:parent",
            metadata: { side_inherited: { hidden: true, parent_session_id: "parent-thread" } },
            role: "user",
            blocks: [
              transcriptBlock({
                id: "message:parent:block:0",
                body: "parent history",
                preview: "parent history",
                detail: "parent history"
              })
            ]
          }),
          transcriptEntry({
            id: "message:side",
            messageSeq: 2,
            role: "user",
            blocks: [
              transcriptBlock({
                id: "message:side:block:0",
                body: "side prompt",
                preview: "side prompt",
                detail: "side prompt"
              })
            ]
          })
        ]}
      />
    );

    expect(html).not.toContain("parent history");
    expect(html).toContain("side prompt");
    expect(html).not.toContain("No messages yet");
  });

  it("renders hover copy and timestamp affordances on user and assistant rows", () => {
    const html = renderToStaticMarkup(
      <TranscriptPanel
        entries={[
          transcriptEntry({
            id: "message:user",
            role: "user",
            blocks: [
              transcriptBlock({
                id: "message:user:block:0",
                body: "user text",
                preview: "user text",
                detail: "user text",
                createdAtMs: Date.UTC(2026, 5, 7, 14, 2),
                updatedAtMs: Date.UTC(2026, 5, 7, 14, 2)
              })
            ]
          }),
          transcriptEntry({
            id: "message:assistant",
            messageSeq: 2,
            role: "assistant",
            blocks: [
              transcriptBlock({
                id: "message:assistant:block:0",
                body: "assistant text",
                preview: "assistant text",
                detail: "assistant text",
                createdAtMs: Date.UTC(2026, 5, 7, 14, 3),
                updatedAtMs: Date.UTC(2026, 5, 7, 14, 3),
                metadata: { elapsed_ms: 65_000 }
              })
            ]
          })
        ]}
        onCopyText={noop}
      />
    );

    expect(html).toContain("Message actions");
    expect(html).toContain("pevo-messageFrame is-user");
    expect(html).toContain("pevo-messageFrame is-assistant");
    expect(html).toContain("Copy message");
    expect(html).toContain("1m05s");
    expect(html).toContain("2026-06-07T14:02:00.000Z");
    expect(html).toContain("2026-06-07T14:03:00.000Z");
    expect(html.match(/Copy message/g)?.length).toBe(2);
  });

  it("does not render decorative transcript role or evidence icons", () => {
    const html = renderToStaticMarkup(
      <TranscriptPanel
        entries={[
          transcriptEntry({
            id: "message:user",
            role: "user",
            blocks: [
              transcriptBlock({
                id: "message:user:text",
                body: "$x-daily",
                preview: "$x-daily",
                detail: "$x-daily"
              })
            ]
          }),
          transcriptEntry({
            id: "message:assistant",
            messageSeq: 2,
            role: "assistant",
            blocks: [
              transcriptBlock({
                id: "message:assistant:reasoning",
                kind: "reasoning",
                title: "Thinking",
                body: "thinking",
                preview: "thinking",
                detail: "thinking"
              }),
              transcriptBlock({
                id: "message:assistant:text",
                order: 1,
                body: "assistant text",
                preview: "assistant text",
                detail: "assistant text"
              }),
              transcriptBlock({
                id: "message:assistant:tool",
                kind: "shell",
                order: 2,
                title: "exec_command",
                body: "printf ok",
                preview: "printf ok",
                detail: "printf ok"
              })
            ]
          })
        ]}
      />
    );

    expect(html).not.toContain("pevo-transcriptHeader");
    expect(html).not.toContain("<h2>Transcript</h2>");
    expect(html).toContain("$x-daily");
    expect(html).toContain("Thinking");
    expect(html).toContain("exec_command");
    expect(html).not.toMatch(/lucide-(activity|user|bot|brain|wrench|terminal|file-text)/);
  });

  it("does not render empty prompt placeholder blocks", () => {
    const html = renderToStaticMarkup(
      <TranscriptPanel
        entries={[
          transcriptEntry({
            id: "empty-prompt",
            role: "user",
            messageSeq: 1,
            blocks: [
              transcriptBlock({
                id: "empty-prompt:text",
                kind: "text",
                body: null,
                detail: null,
                preview: null
              })
            ]
          }),
          transcriptEntry({
            id: "real-prompt",
            role: "user",
            messageSeq: 2,
            blocks: [
              transcriptBlock({
                id: "real-prompt:text",
                kind: "text",
                body: "visible prompt"
              })
            ]
          })
        ]}
      />
    );

    expect(html).toContain("visible prompt");
    expect(html).not.toContain("empty-prompt");
  });

  it("renders compaction checkpoint dividers with collapsed summary detail", () => {
    render(
      <TranscriptPanel
        entries={[
          transcriptEntry({
            id: "compaction:7",
            messageSeq: null,
            role: "diagnostic",
            source: "runtime.compaction",
            blocks: [
              transcriptBlock({
                id: "compaction:7:block",
                kind: "compaction",
                source: "runtime.compaction",
                title: "Session compacted",
                preview: "manual | 120 -> 42 tokens | keeps from #3",
                detail: "Keep the decision trail.",
                metadata: { projection: "compaction", checkpoint_id: 7 }
              })
            ]
          })
        ]}
      />
    );

    expect(screen.getByText("Session compacted")).toBeTruthy();
    expect(screen.getByText("manual | 120 -> 42 tokens | keeps from #3")).toBeTruthy();
    expect(screen.queryByText("Keep the decision trail.")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /Session compacted/ }));
    expect(screen.getByText("Keep the decision trail.")).toBeTruthy();
  });

  it("renders transcript blocks by message and block order even when input is shuffled", () => {
    const html = renderToStaticMarkup(
      <TranscriptPanel
        entries={[
          transcriptEntry({
            id: "assistant-final",
            messageSeq: 3,
            blocks: [transcriptBlock({ id: "assistant-final:text", kind: "text", body: "final answer" })]
          }),
          transcriptEntry({
            id: "assistant-work",
            messageSeq: 2,
            blocks: [
              transcriptBlock({
                id: "reasoning-before-final",
                kind: "reasoning",
                order: 1,
                title: "last reasoning",
                status: "running",
                body: "last reasoning"
              }),
              transcriptBlock({
                id: "tool-write",
                kind: "file",
                order: 0,
                title: "write",
                preview: "feeds/report.md"
              })
            ]
          })
        ]}
      />
    );

    expect(html.indexOf("feeds/report.md")).toBeLessThan(html.indexOf("last reasoning"));
    expect(html.indexOf("last reasoning")).toBeLessThan(html.indexOf("final answer"));
  });

  it("renders assistant phase text as assistant text instead of Thinking", () => {
    const html = renderToStaticMarkup(
      <TranscriptPanel
        entries={[
          transcriptEntry({
            blocks: [
              transcriptBlock({
                id: "assistant-phase",
                kind: "text",
                order: 0,
                body: "I will write the report now.",
                metadata: { projection: "assistant_phase" }
              }),
              transcriptBlock({
                id: "tool-write",
                kind: "file",
                order: 1,
                title: "write",
                preview: "feeds/report.md"
              }),
              transcriptBlock({
                id: "assistant-final",
                kind: "text",
                order: 2,
                body: "Report written."
              })
            ]
          })
        ]}
      />
    );

    expect(html).not.toContain("Thinking");
    expect(html).not.toContain("Preamble");
    expect(html).not.toContain("Reasoning");
    expect(html).not.toContain(">completed<");
    expect(html).toContain("I will write the report now.");
    expect(html.indexOf("I will write the report now.")).toBeLessThan(html.indexOf("feeds/report.md"));
    expect(html.indexOf("feeds/report.md")).toBeLessThan(html.indexOf("Report written."));
    expect(html.match(/class="pevo-message is-assistant/g)?.length ?? 0).toBe(2);
  });

  it("keeps real Thinking separate from assistant phase text and tools", () => {
    const phaseText = "好的，开始执行 X 日报流程。先运行 fetch.py 抓取今日推文数据。";
    const html = renderToStaticMarkup(
      <TranscriptPanel
        entries={[
          transcriptEntry({
            blocks: [
              transcriptBlock({
                id: "reasoning",
                kind: "reasoning",
                order: 0,
                status: "running",
                title: "Thinking",
                body: "The user wants the X daily report."
              }),
              transcriptBlock({
                id: "assistant-phase",
                kind: "text",
                order: 1,
                body: phaseText,
                metadata: { projection: "assistant_phase" }
              }),
              transcriptBlock({
                id: "fetch-tool",
                kind: "shell",
                order: 2,
                title: "exec_command",
                preview: "python fetch.py",
                metadata: {
                  projection: "tool",
                  tool_name: "exec_command",
                  tool_call_id: "call-fetch",
                  args: { cmd: "python fetch.py" }
                }
              })
            ]
          })
        ]}
      />
    );

    expect(html).toContain("Thinking");
    expect(html).toContain("The user wants the X daily report.");
    expect(html).toContain(phaseText);
    expect(html.indexOf("The user wants the X daily report.")).toBeLessThan(html.indexOf(phaseText));
    expect(html.indexOf(phaseText)).toBeLessThan(html.indexOf("exec_command"));
    expect(html.match(/class="pevo-reasoning /g)?.length ?? 0).toBe(1);
    expect(html.match(/class="pevo-message is-assistant/g)?.length ?? 0).toBe(1);
    expect(html).not.toContain("Preamble");
    expect(html).not.toContain("Reasoning");
  });

  it("renders authoritative live entries with preserved Thinking before assistant text and tools", () => {
    const phaseText = "好的，开始执行 X 日报流程。";
    const html = renderToStaticMarkup(
      <TranscriptPanel
        entries={[
          transcriptEntry({
            id: "live:turn-1:assistant:0",
            source: "runtime.stream",
            metadata: { projection: "assistant_segment", authoritativeBlocks: true },
            blocks: [
              transcriptBlock({
                id: "live:turn-1:assistant:0:reasoning",
                kind: "reasoning",
                order: 0,
                status: "completed",
                title: "Thinking",
                body: "The user wants the X daily report.",
                metadata: { projection: "reasoning", origin: "run_stream_reasoning" }
              }),
              transcriptBlock({
                id: "live:turn-1:assistant:0:text:0",
                kind: "text",
                order: 1,
                body: phaseText,
                metadata: { projection: "assistant_phase" }
              }),
              transcriptBlock({
                id: "live:turn-1:tool:call-fetch",
                kind: "shell",
                order: 2,
                title: "exec_command",
                preview: "python fetch.py",
                metadata: {
                  projection: "tool",
                  tool_name: "exec_command",
                  tool_call_id: "call-fetch",
                  args: { cmd: "python fetch.py" }
                }
              })
            ]
          })
        ]}
      />
    );

    expect(html.match(/class="pevo-reasoning /g)?.length ?? 0).toBe(1);
    expect(html.match(/class="pevo-message is-assistant/g)?.length ?? 0).toBe(1);
    expect(html.indexOf("Thinking")).toBeLessThan(html.indexOf(phaseText));
    expect(html.indexOf(phaseText)).toBeLessThan(html.indexOf("exec_command"));
  });
});
