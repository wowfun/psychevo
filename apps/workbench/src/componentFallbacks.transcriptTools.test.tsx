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

    expect(html).toContain("pevo-evidenceLine is-singleTitle");
    expect(html).toContain("exec_command python fetch.py");
    expect(html).not.toContain("<code>exec_command</code><span>python fetch.py</span>");
  });

  it("prefers ACP peer display titles over generated exec command titles", () => {
    const tool = transcriptBlock({
      kind: "shell",
      title: "Run visual tool",
      preview: "echo done",
      metadata: {
        projection: "tool",
        tool_name: "exec_command",
        tool_call_id: "call-visual",
        source: "acp_peer",
        display: "Run visual tool",
        args: { cmd: "echo done" }
      }
    });

    const html = renderToStaticMarkup(
      <TranscriptPanel entries={[transcriptEntry({ blocks: [tool] })]} />
    );

    expect(html).toContain("Run visual tool");
    expect(html).not.toContain("exec_command echo done");
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

    expect(html).toContain("exec_command python fetch.py");
    expect(html).not.toContain("<code>exec_command</code><span>python fetch.py</span>");
    expect(html).not.toContain("write_stdin");
    expect(html).not.toContain("yield_time_ms");
    expect(html).not.toContain("second");
  });

  it("renders expanded exec tool details without raw JSON", () => {
    const exec = transcriptBlock({
      id: "tool-exec",
      kind: "shell",
      title: "exec_command",
      metadata: {
        projection: "tool",
        tool_name: "exec_command",
        args: { cmd: "python fetch.py", cwd: "/tmp/project" }
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

    const { container } = render(
      <TranscriptPanel entries={[transcriptEntry({ blocks: [exec] })]} />
    );

    fireEvent.click(screen.getByRole("button", { name: /exec_command python fetch\.py/ }));

    expect(screen.getByText("Command")).toBeTruthy();
    expect(screen.getByText("Output")).toBeTruthy();
    expect(container.textContent).toContain("first");
    expect(container.textContent).toContain("second");
    expect(container.textContent).not.toContain("{\"session_id\"");
    expect(container.textContent).not.toContain("\"exit_code\"");
    expect(container.textContent).not.toContain("\"output\"");
  });

  it("does not expose raw write arguments or result keys in expanded tool details", () => {
    const tool = transcriptBlock({
      kind: "file",
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
        content: "{\"bytes_written\":34093,\"status\":\"ok\"}",
        isError: false,
        metadata: null,
        createdAtMs: 2,
        updatedAtMs: 2
      }
    });

    const { container } = render(
      <TranscriptPanel entries={[transcriptEntry({ blocks: [tool] })]} />
    );

    fireEvent.click(screen.getByRole("button", { name: /write feeds\/report\.md/ }));

    expect(screen.getByText("Input")).toBeTruthy();
    expect(screen.getByText("Change")).toBeTruthy();
    expect(container.textContent).toContain("feeds/report.md");
    expect(container.textContent).not.toContain("large markdown body");
    expect(container.textContent).not.toContain("bytes_written");
  });

  it("uses tool display specs for custom tool projection", () => {
    const tool = transcriptBlock({
      kind: "toolCall",
      title: "custom_publish",
      metadata: {
        projection: "tool",
        tool_name: "custom_publish",
        tool_call_id: "call-custom",
        args: { target: "draft.md", body: "hidden raw payload" },
        display: {
          category: "update",
          title_arg_keys: ["target"],
          title_result_keys: ["target"],
          summary_keys: ["status"],
          body_keys: ["content"],
          body_policy: "summary"
        }
      },
      result: {
        resultMessageSeq: 2,
        status: "completed",
        content: "{\"status\":\"ok\",\"content\":\"published\"}",
        isError: false,
        metadata: null,
        createdAtMs: 2,
        updatedAtMs: 2
      }
    });

    const html = renderToStaticMarkup(
      <TranscriptPanel entries={[transcriptEntry({ blocks: [tool] })]} />
    );

    expect(html).toContain("is-tool-update");
    expect(html).toContain("custom_publish draft.md");
    expect(html).toContain("ok");
    expect(html).not.toContain("hidden raw payload");
    expect(html).not.toContain("body_policy");
  });

  it("renders reasoning under Thinking and hides empty reasoning completions", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-06-14T00:01:05.000Z"));
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
                body: "I should inspect the feed data first.",
                createdAtMs: new Date("2026-06-14T00:00:00.000Z").getTime(),
                updatedAtMs: new Date("2026-06-14T00:00:00.000Z").getTime()
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

    const reasoningRowStart = html.indexOf('data-block-id="visible-reasoning"');
    const reasoningRowEnd = html.indexOf("</article>", reasoningRowStart);
    const reasoningRow = html.slice(reasoningRowStart, reasoningRowEnd);
    expect(reasoningRow).toContain("pevo-evidenceSpinner");
    expect(reasoningRow).not.toContain("lucide-chevron");
    expect(reasoningRow).not.toContain(">running<");
    expect(reasoningRow).toContain(">1m05s<");
  });

  it("does not render persisted elapsed on completed Thinking rows", () => {
    const html = renderToStaticMarkup(
      <TranscriptPanel
        entries={[
          transcriptEntry({
            id: "assistant-reasoning-completed",
            blocks: [
              transcriptBlock({
                id: "completed-reasoning",
                kind: "reasoning",
                title: "Reasoning",
                status: "completed",
                body: "I inspected the feed data first.",
                metadata: { elapsed_ms: 65_000 }
              })
            ]
          })
        ]}
      />
    );

    const rowStart = html.indexOf('data-block-id="completed-reasoning"');
    const rowEnd = html.indexOf("</article>", rowStart);
    const row = html.slice(rowStart, rowEnd);
    expect(row).toContain("Thinking");
    expect(row).not.toContain("1m05s");
  });

  it("renders Open for completed agent rows from structured result metadata", () => {
    const onOpenAgentSession = vi.fn();
    render(
      <TranscriptPanel
        entries={[
          transcriptEntry({
            id: "entry-agent",
            blocks: [
              transcriptBlock({
                id: "block-agent",
                kind: "agent",
                title: "agent",
                body: JSON.stringify({ child_session_id: "child-thread" }),
                metadata: {
                  projection: "tool",
                  tool_name: "agent",
                  tool_call_id: "call-agent"
                },
                result: {
                  resultMessageSeq: 2,
                  status: "completed",
                  content: JSON.stringify({ summary: "done" }),
                  isError: false,
                  metadata: {
                    result: {
                      agent_name: "explore",
                      task_name: "Investigate",
                      parent_session_id: "thread-1",
                      child_session: {
                        session_id: "child-thread"
                      }
                    }
                  },
                  createdAtMs: 2,
                  updatedAtMs: 2
                }
              })
            ]
          })
        ]}
        onOpenAgentSession={onOpenAgentSession}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Open Investigate agent session" }));

    expect(onOpenAgentSession).toHaveBeenCalledWith(expect.objectContaining({
      agentName: "explore",
      childSessionId: "child-thread",
      parentSessionId: "thread-1",
      taskName: "Investigate",
      title: "Investigate"
    }));
  });

  it("hides completed badges and renders running tool activity", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-06-14T00:01:05.000Z"));
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
                status: "completed",
                metadata: { elapsed_ms: 8_000 }
              }),
              transcriptBlock({
                id: "subsecond-completed-tool",
                kind: "file",
                title: "read",
                preview: "feeds/raw.json",
                status: "completed",
                metadata: { elapsed_ms: 800 }
              }),
              transcriptBlock({
                id: "subsecond-running-tool",
                kind: "shell",
                title: "exec_command",
                preview: "python quick.py",
                status: "running",
                createdAtMs: new Date("2026-06-14T00:01:04.500Z").getTime(),
                updatedAtMs: new Date("2026-06-14T00:01:04.500Z").getTime()
              }),
              transcriptBlock({
                id: "running-tool",
                kind: "shell",
                title: "exec_command",
                preview: "python fetch.py",
                status: "running",
                createdAtMs: new Date("2026-06-14T00:00:00.000Z").getTime(),
                updatedAtMs: new Date("2026-06-14T00:00:00.000Z").getTime()
              })
            ]
          })
        ]}
      />
    );

    expect(html).toContain("feeds/report.md");
    expect(html).toContain("feeds/raw.json");
    expect(html).toContain("python quick.py");
    expect(html).not.toContain(">completed<");
    expect(html).not.toContain(">running<");
    expect(html).not.toContain(">0s<");
    expect(html).toContain(">8s<");
    expect(html).toContain(">1m05s<");

    const completedRowStart = html.indexOf('data-block-id="completed-tool"');
    const completedRowEnd = html.indexOf("</article>", completedRowStart);
    const completedRow = html.slice(completedRowStart, completedRowEnd);
    expect(completedRow).toContain("feeds/report.md");
    expect(completedRow).toContain(">8s<");

    const runningRowStart = html.indexOf('data-block-id="running-tool"');
    const runningRowEnd = html.indexOf("</article>", runningRowStart);
    const runningRow = html.slice(runningRowStart, runningRowEnd);
    expect(runningRow).toContain("pevo-evidenceSpinner");
    expect(runningRow).not.toContain("lucide-chevron");
    expect(runningRow.indexOf("pevo-evidenceSpinner")).toBeLessThan(runningRow.indexOf("<code"));

    const subsecondCompletedStart = html.indexOf('data-block-id="subsecond-completed-tool"');
    const subsecondCompletedEnd = html.indexOf("</article>", subsecondCompletedStart);
    const subsecondCompletedRow = html.slice(subsecondCompletedStart, subsecondCompletedEnd);
    expect(subsecondCompletedRow).not.toContain(">0s<");

    const subsecondRunningStart = html.indexOf('data-block-id="subsecond-running-tool"');
    const subsecondRunningEnd = html.indexOf("</article>", subsecondRunningStart);
    const subsecondRunningRow = html.slice(subsecondRunningStart, subsecondRunningEnd);
    expect(subsecondRunningRow).toContain("pevo-evidenceSpinner");
    expect(subsecondRunningRow).not.toContain(">0s<");
  });
});
