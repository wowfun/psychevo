// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { HistoryPanel, StatusPanel, TranscriptPanel } from "@psychevo/components";
import type { SessionSummary, TranscriptBlock, TranscriptEntry } from "@psychevo/protocol";

const noop = vi.fn();

beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, "scrollTo", {
    configurable: true,
    value: vi.fn()
  });
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("component fallback rendering", () => {
  it("renders older session summaries without activity metadata", () => {
    const session = {
      id: "thread-old",
      workdir: "/tmp/project",
      project: { workdir: "/tmp/project", label: "project", displayPath: "/tmp/project" },
      model: null,
      provider: null,
      startedAtMs: 1,
      updatedAtMs: null,
      endedAtMs: null,
      endReason: null,
      archivedAtMs: null,
      messageCount: 1,
      toolCallCount: 0,
      visibleEntryCount: 1,
      activity: { running: false, activeTurnId: null, queuedTurns: 0 },
      displayTitle: "Old session",
      preview: null,
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
      />
    );

    expect(html).toContain("Old session");
    expect(html).not.toContain("entries");
    expect(html).toContain("title=\"Collapse all workspaces\"");
    expect(html).not.toContain("title=\"Expand all workspaces\"");
  });

  it("renders pin and unpin controls inside the session actions menu", () => {
    const html = renderToStaticMarkup(
      <HistoryPanel
        archived={false}
        pinnedSessionIds={["thread-1"]}
        sessions={[
          sessionSummary({ id: "thread-1", title: "Pinned session" }),
          sessionSummary({ id: "thread-2", title: "Unpinned session" })
        ]}
        onArchive={noop}
        onDelete={noop}
        onExport={noop}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onShare={noop}
        onTogglePinned={noop}
      />
    );

    expect(html).toContain("pevo-sessionMenu");
    expect(html).toContain("role=\"menu\"");
    expect(html).toContain("role=\"menuitem\"");
    expect(html).toContain("title=\"Unpin\"");
    expect(html).toContain("title=\"Pin\"");
    expect(html).not.toContain("pevo-sessionActions");
  });

  it("closes the session actions menu on outside click", async () => {
    const { container } = render(
      <HistoryPanel
        archived={false}
        sessions={[sessionSummary({ id: "thread-1", title: "Menu session" })]}
        onArchive={noop}
        onDelete={noop}
        onExport={noop}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onShare={noop}
      />
    );
    const menu = container.querySelector(".pevo-sessionMenu") as HTMLDetailsElement | null;
    const trigger = container.querySelector(".pevo-sessionMenu summary") as HTMLElement | null;

    fireEvent.click(trigger!);
    await waitFor(() => expect(menu?.open).toBe(true));
    fireEvent.mouseDown(document.body);

    await waitFor(() => expect(menu?.open).toBe(false));
  });

  it("closes the session actions menu on Escape and restores trigger focus", async () => {
    const { container } = render(
      <HistoryPanel
        archived={false}
        sessions={[sessionSummary({ id: "thread-1", title: "Escape session" })]}
        onArchive={noop}
        onDelete={noop}
        onExport={noop}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onShare={noop}
      />
    );
    const trigger = container.querySelector(".pevo-sessionMenu summary") as HTMLElement | null;
    const menu = container.querySelector(".pevo-sessionMenu") as HTMLDetailsElement | null;

    fireEvent.click(trigger!);
    await waitFor(() => expect(menu?.open).toBe(true));
    fireEvent.keyDown(document, { key: "Escape" });

    await waitFor(() => expect(menu?.open).toBe(false));
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it("closes the session actions menu after an enabled menu item fires", async () => {
    const onExport = vi.fn();
    const { container } = render(
      <HistoryPanel
        archived={false}
        sessions={[sessionSummary({ id: "thread-1", title: "Export session" })]}
        onArchive={noop}
        onDelete={noop}
        onExport={onExport}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onShare={noop}
      />
    );
    const menu = container.querySelector(".pevo-sessionMenu") as HTMLDetailsElement | null;
    const trigger = container.querySelector(".pevo-sessionMenu summary") as HTMLElement | null;

    fireEvent.click(trigger!);
    await waitFor(() => expect(menu?.open).toBe(true));
    fireEvent.click(screen.getByRole("menuitem", { name: "Export" }));

    expect(onExport).toHaveBeenCalledWith("thread-1");
    await waitFor(() => expect(menu?.open).toBe(false));
  });

  it("renders workspace creation as a sessions header action", () => {
    const html = renderToStaticMarkup(
      <HistoryPanel
        archived={false}
        sessions={[sessionSummary({ id: "thread-1", title: "Workspace session" })]}
        onArchive={noop}
        onCreateWorkspace={noop}
        onDelete={noop}
        onExport={noop}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onShare={noop}
      />
    );

    expect(html).toContain("title=\"New Workspace\"");
    expect(html.indexOf("title=\"New Workspace\"")).toBeLessThan(html.indexOf("title=\"Collapse all workspaces\""));
  });

  it("does not mark the first history row active without a current thread", () => {
    const sessions = [
      sessionSummary({ id: "thread-1", title: "First session" }),
      sessionSummary({ id: "thread-2", title: "Second session" })
    ];

    const html = renderToStaticMarkup(
      <HistoryPanel
        archived={false}
        sessions={sessions}
        onArchive={noop}
        onDelete={noop}
        onExport={noop}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onShare={noop}
      />
    );

    expect(html).toContain("First session");
    expect(html).toContain("Second session");
    expect(html).not.toContain("is-active");
  });

  it("renders a local draft row without session actions", () => {
    const html = renderToStaticMarkup(
        <HistoryPanel
          archived={false}
          draftSession={{ id: "draft:1", title: "New session", createdAtMs: Date.now(), workdir: "/tmp/project" }}
          sessions={[]}
        onArchive={noop}
        onDelete={noop}
        onExport={noop}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onResumeDraft={noop}
        onShare={noop}
      />
    );

    expect(html).toContain("New session");
    expect(html).toContain("project");
    expect(html).toContain("0d");
    expect(html).toContain("is-active is-draft");
    expect(html).not.toContain("title=\"Session actions\"");
    expect(html).not.toContain("title=\"Rename\"");
    expect(html).not.toContain("title=\"Export\"");
    expect(html).not.toContain("title=\"Share\"");
    expect(html).not.toContain("title=\"Archive\"");
    expect(html).not.toContain("title=\"Delete\"");
  });

  it("renders the local draft row inside its project group", () => {
    const html = renderToStaticMarkup(
      <HistoryPanel
        archived={false}
        draftSession={{ id: "draft:1", title: "New session", createdAtMs: Date.now(), workdir: "/tmp/other" }}
        sessions={[
          sessionSummary({ id: "thread-1", title: "First session" }),
          sessionSummary({
            id: "thread-2",
            title: "Other session",
            workdir: "/tmp/other",
            project: { workdir: "/tmp/other", label: "other", displayPath: "/tmp/other" }
          })
        ]}
        onArchive={noop}
        onDelete={noop}
        onExport={noop}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onResumeDraft={noop}
        onShare={noop}
      />
    );

    expect(html.indexOf("other")).toBeLessThan(html.indexOf("New session"));
    expect(html.indexOf("New session")).toBeLessThan(html.indexOf("Other session"));
  });

  it("does not lift a project group just because its session is active", () => {
    const html = renderToStaticMarkup(
      <HistoryPanel
        archived={false}
        currentThreadId="thread-older"
        sessions={[
          sessionSummary({
            id: "thread-newer",
            title: "Newer session",
            workdir: "/tmp/newer-project",
            project: { workdir: "/tmp/newer-project", label: "newer-project", displayPath: "/tmp/newer-project" },
            startedAtMs: 3_000,
            updatedAtMs: 3_000
          }),
          sessionSummary({
            id: "thread-older",
            title: "Older active session",
            workdir: "/tmp/older-project",
            project: { workdir: "/tmp/older-project", label: "older-project", displayPath: "/tmp/older-project" },
            startedAtMs: 1_000,
            updatedAtMs: 1_000
          })
        ]}
        onArchive={noop}
        onDelete={noop}
        onExport={noop}
        onNew={noop}
        onRename={noop}
        onRestore={noop}
        onResume={noop}
        onShare={noop}
      />
    );

    expect(html.indexOf("newer-project")).toBeLessThan(html.indexOf("older-project"));
    expect(html).toContain("pevo-sessionRow is-active");
  });

  it("renders compact relative day labels for persisted sessions", () => {
    const nowSpy = vi.spyOn(Date, "now").mockReturnValue(86_400_000 * 5);
    try {
      const html = renderToStaticMarkup(
        <HistoryPanel
          archived={false}
          sessions={[
            sessionSummary({
              id: "thread-1",
              title: "Recent session",
              startedAtMs: 86_400_000 * 2,
              updatedAtMs: 86_400_000 * 2
            })
          ]}
          onArchive={noop}
          onDelete={noop}
          onExport={noop}
          onNew={noop}
          onRename={noop}
          onRestore={noop}
          onResume={noop}
          onShare={noop}
        />
      );

      expect(html).toContain(">3d</time>");
    } finally {
      nowSpy.mockRestore();
    }
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

  it("renders partial settings and missing activity as idle status", () => {
    const html = renderToStaticMarkup(
      <StatusPanel
        sessionId="thread-status"
        status="connected"
        onRefresh={noop}
      />
    );

    expect(html).toContain("thread-status");
    expect(html).toContain("pevo-statusSessionId");
    expect(html).not.toContain("pevo-statusMetric");
    expect(html).toContain("No active context");
    expect(html).toContain("No changes");
  });

  it("renders status usage with a single prompt token disclosure", () => {
    const html = renderToStaticMarkup(
      <StatusPanel
        sessionId="thread-status"
        status="connected"
        context={{
          available: true,
          label: "200/1.0k (20.0%)",
          status: "provider_usage",
          usedTokens: 200,
          contextLimit: 1000,
          percent: 20,
          categories: [
            {
              id: "developer_prompt",
              label: "Developer prompt",
              tokens: 60,
              estimated: true,
              status: "estimated",
              percent: 6,
              details: { skill_entries: [{ name: "design", tokens: 42 }] }
            },
            {
              id: "history",
              label: "History",
              tokens: 140,
              estimated: true,
              status: "estimated",
              percent: 14,
              details: { roles: { user: { count: 1, tokens: 50 } } }
            }
          ],
          advice: []
        }}
        usage={{
          available: true,
          sessionId: "thread-status",
          provider: "mock",
          model: "mock-model",
          messageCount: 2,
          assistantMessageCount: 1,
          contextInputTokens: 200,
          billableInputTokens: 150,
          billableOutputTokens: 50,
          reasoningTokens: 12,
          cacheReadTokens: 80,
          cacheWriteTokens: 10,
          reportedTotalTokens: 250,
          estimatedCostNanodollars: 10_000_000,
          costStatus: "estimated",
          estimatedPricingCount: 1,
          freePricingCount: 0,
          includedPricingCount: 0,
          unknownPricingCount: 0,
          cacheReadPercent: 40
        }}
        onRefresh={noop}
      />
    );

    expect(html).toContain("pevo-promptTokenStack");
    expect(html).toContain("pevo-promptTokensDisclosure");
    expect(html).toContain("Prompt tokens");
    expect(html).toContain("Developer prompt");
    expect(html).toContain("design");
    expect(html).not.toContain("pevo-contextRing");
    expect(html).not.toContain("pevo-contextBar");
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
        args: { cmd: "python fetch.py", workdir: "/tmp/project" }
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
    expect(runningRow.indexOf("pevo-evidenceSpinner")).toBeLessThan(runningRow.indexOf("<code>"));

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

function sessionSummary(overrides: Partial<SessionSummary> = {}): SessionSummary {
  const summary: SessionSummary = {
    id: "thread-1",
    workdir: "/tmp/project",
    project: { workdir: "/tmp/project", label: "project", displayPath: "/tmp/project" },
    model: null,
    provider: null,
    startedAtMs: 1,
    updatedAtMs: null,
    endedAtMs: null,
    endReason: null,
    archivedAtMs: null,
    messageCount: 1,
    toolCallCount: 0,
    visibleEntryCount: 1,
    activity: {
      running: false,
      activeTurnId: null,
      queuedTurns: 0
    },
    title: "Session",
    displayTitle: "Session",
    preview: null,
    ...overrides
  };
  if (overrides.displayTitle === undefined && overrides.title !== undefined) {
    summary.displayTitle = overrides.title;
  }
  return summary;
}
