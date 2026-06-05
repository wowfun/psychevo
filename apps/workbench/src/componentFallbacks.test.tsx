import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { HistoryPanel, StatusPanel, TranscriptPanel } from "@psychevo/components";
import type { SessionSummary, SettingsReadResult, TranscriptBlock, TranscriptEntry } from "@psychevo/protocol";

const noop = vi.fn();

describe("component fallback rendering", () => {
  it("renders older session summaries without activity metadata", () => {
    const session = {
      id: "thread-old",
      source: "web",
      workdir: "/tmp/project",
      model: null,
      provider: null,
      startedAtMs: 1,
      updatedAtMs: null,
      endedAtMs: null,
      endReason: null,
      archivedAtMs: null,
      messageCount: 1,
      toolCallCount: 0,
      title: "Old session"
    } as SessionSummary;

    const html = renderToStaticMarkup(
      <HistoryPanel
        archived={false}
        sessions={[session]}
        onArchive={noop}
        onDelete={noop}
        onExport={noop}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onShare={noop}
        onToggleArchived={noop}
      />
    );

    expect(html).toContain("Old session");
    expect(html).toContain("1 msg");
  });

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

    expect(html).toContain("Transcript");
    expect(html).toContain("$x-daily");
    expect(html).toContain("Thinking");
    expect(html).toContain("exec_command");
    expect(html).not.toMatch(/lucide-(activity|user|bot|brain|wrench|terminal|file-text)/);
  });

  it("renders partial settings and missing activity as idle status", () => {
    const settings = { workdir: "/tmp/project" } as SettingsReadResult;

    const html = renderToStaticMarkup(
      <StatusPanel
        settings={settings}
        status="connected"
        onClarify={noop}
        onPermission={noop}
        onRefresh={noop}
      />
    );

    expect(html).toContain("idle");
    expect(html).toContain("status_only");
    expect(html).toContain("disabled");
  });

  it("renders tool headers from arguments instead of result JSON", () => {
    const tool = transcriptBlock({
      kind: "file",
      title: "write",
      preview: "{\"bytes_written\":34093}",
      detail: "{\"bytes_written\":34093}",
      metadata: {
        projection: "tool",
        tool_name: "write",
        tool_call_id: "call-write",
        args: { path: "feeds/report.md", content: "large markdown body" }
      },
      result: {
        resultMessageSeq: 2,
        status: "completed",
        content: "{\"bytes_written\":34093,\"status\":\"ok\"}",
        isError: false,
        metadata: null,
        createdAtMs: 2,
        updatedAtMs: 2
      }
    });

    const html = renderToStaticMarkup(
      <TranscriptPanel entries={[transcriptEntry({ blocks: [tool] })]} />
    );

    expect(html).toContain("feeds/report.md");
    expect(html).not.toContain("bytes_written");
  });

  it("renders tool headers from stringified live arguments", () => {
    const tool = transcriptBlock({
      kind: "file",
      title: "write",
      preview: "{\"content\":\"large markdown body\",\"path\":\"feeds/live.md\"}",
      detail: "{\"content\":\"large markdown body\",\"path\":\"feeds/live.md\"}",
      metadata: {
        projection: "tool",
        tool_name: "write",
        tool_call_id: "call-write",
        args: "{\"content\":\"large markdown body\",\"path\":\"feeds/live.md\"}"
      }
    });

    const html = renderToStaticMarkup(
      <TranscriptPanel entries={[transcriptEntry({ blocks: [tool] })]} />
    );

    expect(html).toContain("feeds/live.md");
    expect(html).not.toContain("large markdown body");
  });

  it("renders live tool blocks that omit artifact ids", () => {
    const tool = transcriptBlock({
      kind: "shell",
      title: "exec_command",
      preview: "python fetch.py",
      metadata: {
        projection: "tool",
        tool_name: "exec_command",
        args: { cmd: "python fetch.py" }
      }
    });
    delete (tool as Partial<TranscriptBlock>).artifactIds;

    const html = renderToStaticMarkup(
      <TranscriptPanel entries={[transcriptEntry({ blocks: [tool] })]} />
    );

    expect(html).toContain("exec_command");
    expect(html).toContain("python fetch.py");
  });

  it("does not render hidden write_stdin poll blocks", () => {
    const exec = transcriptBlock({
      id: "tool-exec",
      kind: "shell",
      title: "exec_command",
      metadata: {
        projection: "tool",
        tool_name: "exec_command",
        args: { cmd: "python fetch.py" }
      },
      result: {
        resultMessageSeq: 2,
        status: "completed",
        content: "{\"session_id\":7,\"exit_code\":0,\"output\":\"first\\nsecond\\n\"}",
        isError: false,
        metadata: null,
        createdAtMs: 2,
        updatedAtMs: 2
      }
    });
    const poll = transcriptBlock({
      id: "tool-poll",
      kind: "shell",
      title: "write_stdin",
      metadata: {
        projection: "tool",
        tool_name: "write_stdin",
        hidden: true,
        args: { session_id: 7, yield_time_ms: 30000 }
      }
    });

    const html = renderToStaticMarkup(
      <TranscriptPanel entries={[transcriptEntry({ blocks: [exec, poll] })]} />
    );

    expect(html).toContain("exec_command");
    expect(html).toContain("python fetch.py");
    expect(html).not.toContain("write_stdin");
    expect(html).not.toContain("yield_time_ms");
    expect(html).not.toContain("second");
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

    expect(html).toContain("1 entry");
    expect(html).toContain("visible prompt");
    expect(html).not.toContain("empty-prompt");
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

  it("renders reasoning under Thinking and hides empty reasoning completions", () => {
    const html = renderToStaticMarkup(
      <TranscriptPanel
        entries={[
          transcriptEntry({
            id: "assistant-reasoning",
            blocks: [
              transcriptBlock({
                id: "empty-reasoning",
                kind: "reasoning",
                title: "Reasoning",
                body: null,
                detail: null,
                preview: null
              }),
              transcriptBlock({
                id: "visible-reasoning",
                kind: "reasoning",
                title: "Reasoning",
                status: "running",
                body: "I should inspect the feed data first."
              })
            ]
          })
        ]}
      />
    );

    expect(html).toContain("Thinking");
    expect(html).toContain("I should inspect the feed data first.");
    expect(html).not.toContain("Reasoning");
    expect(html).not.toContain(">completed<");
    expect(html).not.toContain("empty-reasoning");
  });

  it("hides completed status badges while keeping actionable statuses visible", () => {
    const html = renderToStaticMarkup(
      <TranscriptPanel
        entries={[
          transcriptEntry({
            id: "tools",
            blocks: [
              transcriptBlock({
                id: "completed-tool",
                kind: "file",
                title: "write",
                preview: "feeds/report.md",
                status: "completed"
              }),
              transcriptBlock({
                id: "running-tool",
                kind: "shell",
                title: "exec_command",
                preview: "python fetch.py",
                status: "running"
              })
            ]
          })
        ]}
      />
    );

    expect(html).toContain("feeds/report.md");
    expect(html).not.toContain(">completed<");
    expect(html).toContain(">running<");
  });
});

function transcriptEntry(overrides: Partial<TranscriptEntry> = {}): TranscriptEntry {
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

function transcriptBlock(overrides: Partial<TranscriptBlock> = {}): TranscriptBlock {
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
