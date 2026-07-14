// @vitest-environment jsdom

import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { TranscriptEntry } from "@psychevo/protocol";
import {
  agentRecord,
  commandItem,
  deferred,
  gatewayMock,
  observabilityResult,
  openAgentRuntimePopover,
  selectMainAgent,
  selectRuntime,
  sessionSummary,
  workspaceDiffAction
} from "./appComposerAgent.fixture";
import { App } from "./App";

describe("Workbench layout and workspace panels", () => {
  it("runs Native full fork from the session row and opens the authoritative child", async () => {
    gatewayMock.sessionSummaries = [{
      ...sessionSummary("thread-1", "Full fork source"),
      lifecycle: {
        targetLabel: "Psychevo (Native)",
        actions: [
          { id: "fork", enabled: true, unavailableReason: null },
          { id: "delete", enabled: true, unavailableReason: null }
        ]
      }
    }];
    gatewayMock.threadActionRun = () => ({
      kind: "fork",
      sourceThreadId: "thread-1",
      snapshot: {
        ...gatewayMock.snapshot,
        thread: {
          id: "fork-child",
          backend: { kind: "native", sessionHandle: null, runtimeRef: "native" },
          sourceKey: "source-fork-child",
          forkedFromThreadId: "thread-1"
        },
        entries: []
      }
    });

    const { container } = render(<App />);
    await screen.findByText("Full fork source");
    fireEvent.click(container.querySelector(".pevo-sessionMenu summary") as HTMLElement);
    fireEvent.click(screen.getByRole("menuitem", { name: "Fork" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/action/run",
        params: expect.objectContaining({
          threadId: "thread-1",
          action: { kind: "fork" }
        })
      });
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "fork-child" })
      });
    });
  });

  it("keeps missing fork provenance visible but disables source navigation", async () => {
    gatewayMock.sessionSummaries = [{
      ...sessionSummary("fork-child", "Detached fork"),
      forkedFromThreadId: "deleted-source-thread"
    }];
    gatewayMock.snapshot.thread = {
      id: "fork-child",
      backend: { kind: "native", sessionHandle: "fork-child", runtimeRef: "native" },
      sourceKey: "source-fork-child",
      forkedFromThreadId: "deleted-source-thread"
    };

    render(<App />);
    fireEvent.click(await screen.findByText("Detached fork"));

    const provenance = await screen.findByRole("button", { name: "Forked from deleted-" });
    expect((provenance as HTMLButtonElement).disabled).toBe(true);
    expect(provenance.getAttribute("title")).toContain("deleted-source-thread is unavailable");
  });

  it("keeps the inline edit available when turn admission fails and retries the same staged draft", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Retry history edit")];
    (gatewayMock.snapshot as { entries: TranscriptEntry[] }).entries = [userTextEntry("Original prompt")];
    gatewayMock.threadHistoryDraftRead = () => ({
      threadId: "thread-1",
      messageId: "message:1",
      messageSeq: 1,
      parts: [{ type: "text", text: "Original prompt" }],
      fidelity: "exact",
      warning: null,
      unavailableReason: null
    });
    gatewayMock.threadActionRun = () => ({
      kind: "revertConversation",
      threadId: "thread-1",
      staged: true,
      noOp: false,
      snapshot: {
        ...gatewayMock.snapshot,
        entries: [],
        historyEditing: {
          kind: "conversationEdit",
          boundaryMessageId: "message:1",
          hiddenEntryCount: 1,
          replacementDraft: { parts: [{ type: "text", text: "Edited prompt" }] },
          availableActions: ["restoreHistory"]
        }
      }
    });
    let turnAttempts = 0;
    gatewayMock.turnStart = () => {
      turnAttempts += 1;
      if (turnAttempts === 1) {
        throw new Error("The selected model became unavailable.");
      }
      return {
        accepted: true,
        threadId: "thread-1",
        turnId: "turn:thread-1",
        thread: gatewayMock.snapshot.thread
      };
    };

    render(<App />);
    fireEvent.click(await screen.findByText("Retry history edit"));
    await screen.findByText("Original prompt");
    fireEvent.click(await screen.findByRole("button", { name: /Edit this message/ }));
    const editor = await screen.findByRole("textbox", { name: "Message text 1" });
    fireEvent.change(editor, { target: { value: "Edited prompt" } });
    const update = screen.getByRole("button", { name: "Update this message and run in the same thread" });
    fireEvent.click(update);

    expect(await screen.findByText("The selected model became unavailable.")).toBeTruthy();
    expect(screen.getByRole("textbox", { name: "Message text 1" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Update this message and run in the same thread" }));

    await waitFor(() => expect(turnAttempts).toBe(2));
    expect(gatewayMock.requestLog.filter((entry) => (
      entry.method === "thread/action/run"
      && (entry.params as { action?: { kind?: string } }).action?.kind === "revertConversation"
    ))).toHaveLength(2);
    await waitFor(() => {
      expect(screen.queryByRole("textbox", { name: "Message text 1" })).toBeNull();
    });
  });

  it("restores staged conversation history and keeps the ordered replacement draft in Composer", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "History editing")];
    gatewayMock.snapshot.historyEditing = {
      kind: "conversationEdit",
      boundaryMessageId: "message:1",
      hiddenEntryCount: 2,
      replacementDraft: {
        parts: [
          { type: "text", text: "edited before" },
          { type: "image", input: { kind: "url", url: "https://example.test/history.png" } },
          { type: "text", text: "edited after" }
        ]
      },
      availableActions: ["restoreHistory"]
    };
    gatewayMock.threadActionRun = () => ({
      kind: "unrevertConversation",
      threadId: "thread-1",
      snapshot: { ...gatewayMock.snapshot, historyEditing: null },
      draft: gatewayMock.snapshot.historyEditing?.replacementDraft
    });

    render(<App />);
    fireEvent.click(await screen.findByText("History editing"));
    expect(await screen.findByText("2 hidden entries")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Restore history" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/action/run",
        params: expect.objectContaining({
          threadId: "thread-1",
          action: { kind: "unrevertConversation" }
        })
      });
    });
    expect((screen.getByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement).value).toBe(
      "edited before\nedited after"
    );
    expect(screen.getByText("history.png")).toBeTruthy();
  });

  it("requests initial history once and suppresses the empty state until it resolves", async () => {
    const browser = deferred<Record<string, unknown>>();
    gatewayMock.threadBrowser = () => browser.promise;

    render(<App />);

    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/browser")).toHaveLength(1);
    });
    expect(screen.getByRole("region", { name: "Sessions" }).getAttribute("aria-busy")).toBe("true");
    expect(screen.queryByText("No sessions")).toBeNull();

    browser.resolve({ workspaces: [] });
    expect(await screen.findByText("No sessions")).toBeTruthy();
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/browser")).toHaveLength(1);
  });

  it("starts in a hidden draft without rendering a history draft row", async () => {
    const { container } = render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    expect((container.querySelector(".workbench") as HTMLElement | null)?.style.getPropertyValue("--right-column-width")).toBe("520px");
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/start",
        params: expect.objectContaining({ scope: gatewayMock.scope })
      });
    });
    expect(container.querySelectorAll(".pevo-sessionRow.is-draft")).toHaveLength(0);
    expect(screen.queryByRole("region", { name: "Workspace status" })).toBeNull();
  });

  it("opens right workspace tabs from Home and the add menu", async () => {
    const { container } = render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    expect(within(home).queryByText("local PTY")).toBeNull();
    expect(within(home).queryByText("workspace tree")).toBeNull();
    fireEvent.click(within(home).getByRole("button", { name: /Review/ }));
    expect(await screen.findByRole("region", { name: "Review" })).toBeTruthy();

    fireEvent.click(document.querySelector(".rightAddMenu summary") as HTMLElement);
    const addMenuFiles = screen.getAllByRole("menuitem", { name: "Files" }).at(-1);
    expect(addMenuFiles).toBeTruthy();
    fireEvent.click(addMenuFiles!);
    expect(await screen.findByRole("region", { name: "Workspace files" })).toBeTruthy();

    fireEvent.click(screen.getByLabelText("Workspace home"));
    const visibleHome = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(visibleHome).getByRole("button", { name: /Terminal/ }));
    expect(await screen.findByRole("region", { name: "Terminal" })).toBeTruthy();
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "terminal/start")).toBe(true);
    });
  });

  it("opens a reusable preview-only Browser tab with safe URL handling", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Browser session")];
    const { container } = render(<App />);

    fireEvent.click(await screen.findByText("Browser session"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "thread-1" })
      });
    });
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Browser" }));
    const browser = await screen.findByRole("region", { name: "Browser" });

    const annotate = within(browser).getByLabelText("Annotate page") as HTMLButtonElement;
    expect(annotate.disabled).toBe(true);
    expect(annotate.getAttribute("title")).toBe("Desktop required");

    const openUrl = within(browser).getByLabelText("Open URL") as HTMLInputElement;
    fireEvent.change(openUrl, { target: { value: "file:///tmp/page.html" } });
    fireEvent.submit(openUrl.closest("form") as HTMLFormElement);
    expect((await within(browser).findByRole("alert")).textContent).toContain("Browser supports http and https URLs.");

    fireEvent.change(openUrl, { target: { value: "example.com:8080" } });
    fireEvent.submit(openUrl.closest("form") as HTMLFormElement);
    const iframe = await within(browser).findByTitle("example.com") as HTMLIFrameElement;
    expect(iframe.getAttribute("src")).toBe("https://example.com:8080/");
    expect(within(browser).getByText("Preview only")).toBeTruthy();

    const address = within(browser).getByLabelText("Browser address") as HTMLInputElement;
    for (const [input, title, expected] of [
      ["localhost:3000", "localhost", "http://localhost:3000/"],
      ["127.0.0.1:9222", "127.0.0.1", "http://127.0.0.1:9222/"],
      ["[::1]:4173", "[::1]", "http://[::1]:4173/"]
    ] as const) {
      fireEvent.change(address, { target: { value: input } });
      fireEvent.submit(address.closest("form") as HTMLFormElement);
      expect((await within(browser).findByTitle(title)).getAttribute("src")).toBe(expected);
    }

    fireEvent.change(address, { target: { value: "vscode://file/tmp/page.html" } });
    fireEvent.submit(address.closest("form") as HTMLFormElement);
    expect((await within(browser).findByRole("alert")).textContent).toContain("Browser supports http and https URLs.");

    fireEvent.click(screen.getByLabelText("Workspace home"));
    const visibleHome = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(visibleHome).getByRole("button", { name: "Browser" }));
    expect(container.querySelectorAll(".rightWorkspaceTab button[title='Browser']")).toHaveLength(1);
  });

  it("isolates Browser tabs and restores navigation state per thread", async () => {
    gatewayMock.sessionSummaries = [
      sessionSummary("thread-a", "Thread A"),
      sessionSummary("thread-b", "Thread B")
    ];
    const { container } = render(<App />);

    fireEvent.click(await screen.findByText("Thread A"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "thread-a" })
      });
    });
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    let home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Browser" }));
    let browser = await screen.findByRole("region", { name: "Browser" });
    let address = within(browser).getByLabelText("Open URL") as HTMLInputElement;
    fireEvent.change(address, { target: { value: "a.example" } });
    fireEvent.submit(address.closest("form") as HTMLFormElement);
    expect((await within(browser).findByTitle("a.example")).getAttribute("src")).toBe("https://a.example/");

    fireEvent.click(screen.getByText("Thread B"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "thread-b" })
      });
    });
    home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Browser" }));
    browser = await screen.findByRole("region", { name: "Browser" });
    expect(within(browser).queryByTitle("a.example")).toBeNull();
    address = within(browser).getByLabelText("Open URL") as HTMLInputElement;
    fireEvent.change(address, { target: { value: "b.example" } });
    fireEvent.submit(address.closest("form") as HTMLFormElement);
    expect((await within(browser).findByTitle("b.example")).getAttribute("src")).toBe("https://b.example/");
    expect(container.querySelectorAll(".rightWorkspaceTab button[title='Browser']")).toHaveLength(1);

    fireEvent.click(screen.getByText("Thread A"));
    home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Browser" }));
    browser = await screen.findByRole("region", { name: "Browser" });
    expect((within(browser).getByLabelText("Browser address") as HTMLInputElement).value).toBe("https://a.example/");
    expect((within(browser).getByTitle("a.example") as HTMLIFrameElement).getAttribute("src")).toBe("https://a.example/");
    expect(within(browser).queryByTitle("b.example")).toBeNull();
    expect(container.querySelectorAll(".rightWorkspaceTab button[title='Browser']")).toHaveLength(1);
  });

  it("closes the right workspace add menu on outside click and item activation", async () => {
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: /Review/ }));
    expect(await screen.findByRole("region", { name: "Review" })).toBeTruthy();

    const trigger = document.querySelector(".rightAddMenu summary") as HTMLElement | null;
    const menu = trigger!.closest("details") as HTMLDetailsElement | null;
    fireEvent.click(trigger!);
    await waitFor(() => expect(menu?.open).toBe(true));
    fireEvent.mouseDown(screen.getByRole("region", { name: "Transcript" }));
    await waitFor(() => expect(menu?.open).toBe(false));

    fireEvent.click(trigger!);
    await waitFor(() => expect(menu?.open).toBe(true));
    fireEvent.click(screen.getByRole("menuitem", { name: "Files" }));
    expect(await screen.findByRole("region", { name: "Workspace files" })).toBeTruthy();
    await waitFor(() => expect(menu?.open).toBe(false));

    fireEvent.click(trigger!);
    await waitFor(() => expect(menu?.open).toBe(true));
    fireEvent.click(screen.getByRole("menuitem", { name: "Terminal" }));
    expect(await screen.findByRole("region", { name: "Terminal" })).toBeTruthy();
    await waitFor(() => expect(menu?.open).toBe(false));
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "terminal/start")).toBe(true);
    });
  });

  it("restores and clamps the right workspace width preference", async () => {
    window.localStorage.setItem("psychevo.workbench.v0.prefs", JSON.stringify({
      appearance: "dark",
      debug: false,
      rightWidthPx: 9999
    }));
    const { container } = render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    const workbench = container.querySelector(".workbench") as HTMLElement | null;
    expect(workbench?.style.getPropertyValue("--right-column-width")).toBe("1200px");
  });

  it("toggles Review changed files and scopes the diff preview", async () => {
    gatewayMock.workspaceDiffResult = {
      isGitRepo: true,
      files: [
        { path: "docs/api.md", status: "modified", binary: false, unreadable: false, placeholder: null },
        { path: "src/main.rs", status: "modified", binary: false, unreadable: false, placeholder: null }
      ],
      unifiedDiff: [
        "diff --git a/docs/api.md b/docs/api.md",
        "--- a/docs/api.md",
        "+++ b/docs/api.md",
        "@@ -1 +1 @@",
        "-old docs",
        "+new docs",
        "diff --git a/src/main.rs b/src/main.rs",
        "--- a/src/main.rs",
        "+++ b/src/main.rs",
        "@@ -1 +1 @@",
        "-old main",
        "+new main"
      ].join("\n"),
      truncation: { truncated: false, maxBytes: 0, maxLines: 0, omittedBytes: 0, omittedLines: 0 },
      selectedPath: null
    };
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Review" }));
    const review = await screen.findByRole("region", { name: "Review" });
    expect(within(review).getByText("docs/api.md")).toBeTruthy();
    expect(within(review).getAllByText("M↓").length).toBeGreaterThan(0);
    expect(within(review).getAllByLabelText("1 additions, 1 deletions").length).toBeGreaterThan(0);
    expect(within(review).queryByText("diff --git a/docs/api.md b/docs/api.md")).toBeNull();

    fireEvent.click(within(review).getByRole("button", { name: "Show changed files" }));
    expect(within(review).getByLabelText("Filter changed files")).toBeTruthy();
    fireEvent.change(within(review).getByLabelText("Filter changed files"), { target: { value: "main" } });
    expect(within(review).getByRole("treeitem", { name: /main\.rs/ })).toBeTruthy();
    expect(within(review).queryByRole("treeitem", { name: /api\.md/ })).toBeNull();

    fireEvent.click(within(review).getByRole("treeitem", { name: /main\.rs/ }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "workspace/diff",
        params: expect.objectContaining({ path: "src/main.rs" })
      });
    });
    expect(await within(review).findByText("new selected")).toBeTruthy();
    expect(within(review).getByText("src/main.rs")).toBeTruthy();
  });

  it("rejects turn-scoped Review files through workspace change RPCs", async () => {
    gatewayMock.workspaceChangesResult = {
      groups: [
        {
          turnId: "turn-1",
          threadId: "thread-1",
          createdAtMs: 1,
          completedAtMs: 2,
          files: [
            {
              path: "docs/api.md",
              status: "modified",
              binary: false,
              unreadable: false,
              reviewStatus: "pending",
              canReject: true,
              message: null
            }
          ]
        }
      ]
    };
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Review" }));
    const review = await screen.findByRole("region", { name: "Review" });

    expect(within(review).getByText("docs/api.md")).toBeTruthy();
    fireEvent.click(within(review).getByLabelText("Reject docs/api.md"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "workspace/change/reject",
        params: expect.objectContaining({ path: "docs/api.md", turnId: "turn-1" })
      });
    });
  });

  it("renders Markdown file previews from the shared Markdown component", async () => {
    gatewayMock.workspaceFilesResult = {
      root: gatewayMock.scope.cwd,
      entries: [
        { path: "docs", name: "docs", kind: "directory", depth: 0 },
        { path: "docs/README.md", name: "README.md", kind: "file", depth: 1 },
        { path: "src", name: "src", kind: "directory", depth: 0 },
        { path: "src/main.rs", name: "main.rs", kind: "file", depth: 1 }
      ],
      truncated: false
    };
    const markdownSource = "---\ntitle: API Notes\ntags:\n  - docs\n  - guide\n---\n# API Notes\n\n- supports markdown";
    gatewayMock.workspaceFileReadResults.set("docs/README.md", {
      path: "docs/README.md",
      content: markdownSource,
      binary: false,
      unreadable: null,
      truncated: false
    });
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Files" }));
    const files = await screen.findByRole("region", { name: "Workspace files" });
    expect(within(files).getByLabelText("Filter workspace files")).toBeTruthy();
    expect(files.querySelector("header p")).toBeNull();

    fireEvent.click(within(files).getByRole("treeitem", { name: /README\.md/ }));
    expect(await within(files).findByText("/tmp/project/docs/README.md")).toBeTruthy();
    const table = await within(files).findByRole("table", { name: "YAML frontmatter" });
    expect(within(table).getByText("title")).toBeTruthy();
    expect(within(table).getByText("docs")).toBeTruthy();
    expect(within(table).getByText("guide")).toBeTruthy();
    expect(await within(files).findByRole("heading", { name: "API Notes" })).toBeTruthy();
    expect(within(files).getByText("supports markdown")).toBeTruthy();
    fireEvent.click(within(files).getByRole("button", { name: "Copy Markdown file" }));
    await waitFor(() => {
      expect(gatewayMock.clipboardWriteLog[gatewayMock.clipboardWriteLog.length - 1]).toBe(markdownSource);
    });
  });

  it("opens relative and Windows assistant file links in the locked Files preview", async () => {
    gatewayMock.scope.cwd = "C:\\repo";
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Artifact links", gatewayMock.scope.cwd)];
    gatewayMock.workspaceFilesResult = {
      root: gatewayMock.scope.cwd,
      entries: [
        { path: "site", name: "site", kind: "directory", depth: 0 },
        { path: "site/index.html", name: "index.html", kind: "file", depth: 1 }
      ],
      truncated: false
    };
    const htmlSource = "<!doctype html><html><body><h1>Artifact preview</h1></body></html>";
    gatewayMock.workspaceFileReadResults.set("site/index.html", {
      path: "site/index.html",
      content: htmlSource,
      binary: false,
      editable: true,
      editableReason: null,
      revision: "r1",
      sizeBytes: htmlSource.length,
      lineEnding: "lf",
      unreadable: null,
      truncated: false
    });
    (gatewayMock.snapshot as { entries: TranscriptEntry[] }).entries = [assistantTextEntry([
      "Relative site/index.html",
      "Windows C:\\repo\\site\\index.html",
      "Git Bash /c/repo/site/index.html"
    ].join("\n\n"))];

    render(<App />);
    fireEvent.click(await screen.findByText("Artifact links"));

    const pathButton = (label: string): HTMLButtonElement => {
      return screen.getByRole("button", { name: `Open file ${label}` }) as HTMLButtonElement;
    };
    await screen.findByRole("button", { name: "Open file site/index.html" });
    expect(pathButton("C:\\repo\\site\\index.html")).toBeTruthy();
    expect(pathButton("/c/repo/site/index.html")).toBeTruthy();

    fireEvent.click(pathButton("site/index.html"));
    const files = await screen.findByRole("region", { name: "Workspace files" });
    const frame = files.querySelector('iframe[title="site/index.html"]');
    if (!(frame instanceof HTMLIFrameElement)) {
      throw new Error("missing locked HTML artifact preview");
    }
    expect(frame.getAttribute("sandbox")).not.toContain("allow-scripts");
    expect(frame.getAttribute("srcdoc")).toContain("Artifact preview");

    fireEvent.click(pathButton("C:\\repo\\site\\index.html"));
    fireEvent.click(pathButton("/c/repo/site/index.html"));
    await waitFor(() => {
      const openedPaths = gatewayMock.requestLog
        .filter((entry) => entry.method === "workspace/file/read")
        .map((entry) => (entry.params as { path?: string }).path);
      expect(openedPaths).toEqual([
        "site/index.html",
        "site/index.html",
        "site/index.html"
      ]);
    });
  });

  it("keeps HTML locked until explicitly run and revokes trust when the document changes", async () => {
    gatewayMock.workspaceFilesResult = {
      root: gatewayMock.scope.cwd,
      entries: [
        { path: "site", name: "site", kind: "directory", depth: 0 },
        { path: "site/index.html", name: "index.html", kind: "file", depth: 1 },
        { path: "site/other.html", name: "other.html", kind: "file", depth: 1 }
      ],
      truncated: false
    };
    const htmlSource = "<!doctype html><html><body><div id=\"app\"></div><script>document.getElementById(\"app\").textContent = \"rendered\"</script></body></html>";
    const changedHtmlSource = htmlSource.replace("rendered", "changed");
    gatewayMock.workspaceFileReadResults.set("site/index.html", {
      path: "site/index.html",
      content: htmlSource,
      binary: false,
      editable: true,
      editableReason: null,
      revision: "r1",
      sizeBytes: htmlSource.length,
      lineEnding: "lf",
      unreadable: null,
      truncated: false
    });
    gatewayMock.workspaceFileReadResults.set("site/other.html", {
      path: "site/other.html",
      content: htmlSource,
      binary: false,
      unreadable: null,
      truncated: false
    });
    const { container } = render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Files" }));
    const files = await screen.findByRole("region", { name: "Workspace files" });
    fireEvent.click(within(files).getByRole("treeitem", { name: /index\.html/ }));

    expect(await within(files).findByText("/tmp/project/site/index.html")).toBeTruthy();
    await within(files).findByText("HTML preview");
    const inlineFrame = files.querySelector(".htmlStaticPreview iframe") as HTMLIFrameElement | null;
    if (!inlineFrame) {
      throw new Error("missing inline HTML preview iframe");
    }
    expect(inlineFrame.getAttribute("sandbox")).not.toContain("allow-scripts");
    expect(inlineFrame.getAttribute("sandbox")).not.toContain("allow-forms");
    expect(inlineFrame.getAttribute("sandbox")).not.toContain("allow-popups");
    expect(inlineFrame.getAttribute("sandbox")).not.toContain("allow-same-origin");
    expect(inlineFrame.getAttribute("tabindex")).toBe("-1");
    expect(inlineFrame.getAttribute("aria-hidden")).toBe("true");
    expect(inlineFrame.hasAttribute("inert")).toBe(true);
    expect(inlineFrame.closest(".htmlStaticPreview")?.classList.contains("is-locked")).toBe(true);
    const inlineDocument = inlineFrame.getAttribute("srcdoc") ?? "";
    expect(inlineDocument).toContain("default-src 'none'");
    expect(inlineDocument).toContain("connect-src 'none'");
    expect(inlineDocument).toContain("form-action 'none'");
    expect(inlineDocument).toContain("script-src 'none'");
    expect(inlineDocument).toContain("style-src 'unsafe-inline'");
    expect(inlineDocument).toContain("textContent = \"rendered\"");
    expect(container.querySelectorAll(".htmlStaticPreview iframe")).toHaveLength(1);
    expect(within(files).getByText("Locked · run enables scripts + network")).toBeTruthy();
    fireEvent.click(within(files).getByRole("button", { name: "Run interactive preview" }));
    let activeFrame = files.querySelector(".htmlStaticPreview iframe") as HTMLIFrameElement;
    expect(activeFrame.getAttribute("sandbox")).toContain("allow-scripts");
    expect(activeFrame.getAttribute("tabindex")).toBe("0");
    expect(activeFrame.hasAttribute("aria-hidden")).toBe(false);
    expect(activeFrame.hasAttribute("inert")).toBe(false);
    expect(activeFrame.closest(".htmlStaticPreview")?.classList.contains("is-interactive")).toBe(true);
    expect(within(files).getByText("Trusted · scripts + network on")).toBeTruthy();
    expect(within(files).getByRole("button", { name: "Stop interactive preview" })).toBeTruthy();

    fireEvent.click(within(files).getByLabelText("Edit site/index.html"));
    fireEvent.change(within(files).getByLabelText("Edit site/index.html"), { target: { value: changedHtmlSource } });
    gatewayMock.workspaceFileReadResults.set("site/index.html", {
      path: "site/index.html",
      content: changedHtmlSource,
      binary: false,
      editable: true,
      editableReason: null,
      revision: "written",
      sizeBytes: changedHtmlSource.length,
      lineEnding: "lf",
      unreadable: null,
      truncated: false
    });
    fireEvent.click(within(files).getByLabelText("Save file"));
    await waitFor(() => expect(within(files).queryByText("unsaved")).toBeNull());
    await waitFor(() => expect(files.querySelector(".htmlStaticPreview iframe")).toBeTruthy());
    activeFrame = files.querySelector(".htmlStaticPreview iframe") as HTMLIFrameElement;
    expect(activeFrame.getAttribute("sandbox")).not.toContain("allow-scripts");
    expect(within(files).getByRole("button", { name: "Run interactive preview" })).toBeTruthy();

    fireEvent.click(within(files).getByRole("button", { name: "Run interactive preview" }));
    expect((files.querySelector(".htmlStaticPreview iframe") as HTMLIFrameElement).getAttribute("sandbox")).toContain("allow-scripts");
    fireEvent.click(within(files).getByRole("treeitem", { name: /other\.html/ }));
    await waitFor(() => expect((files.querySelector(".htmlStaticPreview iframe") as HTMLIFrameElement | null)?.title).toBe("site/other.html"));
    activeFrame = files.querySelector(".htmlStaticPreview iframe") as HTMLIFrameElement;
    expect(activeFrame.getAttribute("sandbox")).not.toContain("allow-scripts");
    fireEvent.click(within(files).getByRole("treeitem", { name: /index\.html/ }));
    await waitFor(() => expect((files.querySelector(".htmlStaticPreview iframe") as HTMLIFrameElement | null)?.title).toBe("site/index.html"));
    activeFrame = files.querySelector(".htmlStaticPreview iframe") as HTMLIFrameElement;
    expect(activeFrame.getAttribute("sandbox")).not.toContain("allow-scripts");

    fireEvent.click(within(files).getByLabelText("Open HTML preview for site/index.html"));
    const preview = await screen.findByRole("region", { name: "Preview" });
    expect(within(preview).getByRole("heading", { name: "index.html" })).toBeTruthy();
    expect(within(preview).getByText("HTML preview")).toBeTruthy();
    let previewFrame = within(preview).getByTitle("index.html") as HTMLIFrameElement;
    expect(previewFrame.getAttribute("sandbox")).not.toContain("allow-scripts");
    expect(previewFrame.getAttribute("sandbox")).not.toContain("allow-forms");
    expect(previewFrame.getAttribute("sandbox")).not.toContain("allow-popups");
    expect(previewFrame.getAttribute("sandbox")).not.toContain("allow-same-origin");
    expect(previewFrame.getAttribute("srcdoc")).toContain("textContent = \"changed\"");
    expect(container.querySelectorAll(".htmlStaticPreview iframe")).toHaveLength(1);
    expect(files.querySelector(".htmlStaticPreview iframe")).toBeNull();
    fireEvent.click(within(preview).getByRole("button", { name: "Run interactive preview" }));
    previewFrame = within(preview).getByTitle("index.html") as HTMLIFrameElement;
    expect(previewFrame.getAttribute("sandbox")).toContain("allow-scripts");
    expect(container.querySelectorAll(".htmlStaticPreview iframe")).toHaveLength(1);
  });

  it("saves text edits manually without entering the Review queue", async () => {
    gatewayMock.workspaceFilesResult = {
      root: gatewayMock.scope.cwd,
      entries: [
        { path: "docs", name: "docs", kind: "directory", depth: 0 },
        { path: "docs/README.md", name: "README.md", kind: "file", depth: 1 }
      ],
      truncated: false
    };
    gatewayMock.workspaceFileReadResults.set("docs/README.md", {
      path: "docs/README.md",
      content: "before\n",
      binary: false,
      editable: true,
      editableReason: null,
      revision: "r1",
      sizeBytes: 7,
      lineEnding: "lf",
      unreadable: null,
      truncated: false
    });
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Files" }));
    const files = await screen.findByRole("region", { name: "Workspace files" });
    fireEvent.click(within(files).getByRole("treeitem", { name: /README\.md/ }));
    fireEvent.click(await within(files).findByLabelText("Edit docs/README.md"));
    const editor = within(files).getByLabelText("Edit docs/README.md");
    fireEvent.change(editor, { target: { value: "after\n" } });
    fireEvent.click(within(files).getByLabelText("Save file"));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "workspace/file/write",
        params: expect.objectContaining({
          path: "docs/README.md",
          content: "after\n",
          expectedRevision: "r1",
          force: false
        })
      });
    });
    expect(gatewayMock.requestLog.some((entry) => entry.method.startsWith("workspace/change/"))).toBe(false);
  });

  it("renders code previews with absolute paths, syntax tokens, and escaped source text", async () => {
    gatewayMock.workspaceFilesResult = {
      root: gatewayMock.scope.cwd,
      entries: [
        { path: "src", name: "src", kind: "directory", depth: 0 },
        { path: "src/main.py", name: "main.py", kind: "file", depth: 1 }
      ],
      truncated: false
    };
    gatewayMock.workspaceFileReadResults.set("src/main.py", {
      path: "src/main.py",
      content: "def greet():\n    return \"<script>alert(1)</script>\"\n",
      binary: false,
      unreadable: null,
      truncated: false
    });
    const { container } = render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Files" }));
    const files = await screen.findByRole("region", { name: "Workspace files" });

    fireEvent.click(within(files).getByRole("treeitem", { name: /main\.py/ }));
    expect(await within(files).findByText("/tmp/project/src/main.py")).toBeTruthy();
    const preview = container.querySelector(".rightCodePreview") as HTMLElement | null;
    expect(preview?.dataset.lang).toBe("python");
    expect(preview?.querySelector(".hljs-keyword, .hljs-title")).toBeTruthy();
    expect(preview?.querySelector("script")).toBeNull();
    expect(preview?.innerHTML).toContain("&lt;script&gt;");
  });

  it("keeps Terminal interactive without the persistent title and state header", async () => {
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Terminal" }));
    const terminal = await screen.findByRole("region", { name: "Terminal" });
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "terminal/start")).toBe(true);
    });

    expect(within(terminal).queryByRole("heading", { name: "project" })).toBeNull();
    expect(within(terminal).queryByText("/tmp/project")).toBeNull();
    expect(within(terminal).queryByText("running")).toBeNull();
  });

  it("uses a readable light xterm theme for Terminal tabs", async () => {
    gatewayMock.xtermTerminalOptions.length = 0;
    window.localStorage.setItem("psychevo.workbench.v0.prefs", JSON.stringify({
      appearance: "light",
      appearanceVersion: 1,
      debug: false,
      rightWidthPx: 520
    }));
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Terminal" }));
    expect(await screen.findByRole("region", { name: "Terminal" })).toBeTruthy();
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "terminal/start")).toBe(true);
    });

    const theme = gatewayMock.xtermTerminalOptions.at(-1)?.theme as Record<string, string> | undefined;
    expect(theme).toBeTruthy();
    expect(theme?.background).toBe("#f7f5ef");
    expect(theme?.foreground).toBe("#202225");
    expect(theme?.cursor).toBe("#202225");
    expect(theme?.selectionBackground).toBe("#d8dde5");
    expect(theme?.black).toBe("#202225");
    expect(theme?.white).toBe("#5f6670");
    expect(theme?.brightBlack).toBe("#6a6f78");
    expect(theme?.brightWhite).toBe("#3a3f46");
  });
});

function assistantTextEntry(body: string): TranscriptEntry {
  return {
    id: "artifact-links-entry",
    threadId: "thread-1",
    turnId: "turn-1",
    messageSeq: 1,
    role: "assistant",
    status: "completed",
    source: "runtime.message",
    blocks: [
      {
        id: "artifact-links-block",
        kind: "text",
        status: "completed",
        order: 0,
        source: "runtime.message",
        title: null,
        body,
        preview: null,
        detail: null,
        artifactIds: [],
        metadata: null,
        result: null,
        createdAtMs: 1,
        updatedAtMs: 1
      }
    ],
    metadata: null,
    usage: null,
    accounting: null,
    createdAtMs: 1,
    updatedAtMs: 1
  };
}

function userTextEntry(body: string): TranscriptEntry {
  return {
    ...assistantTextEntry(body),
    id: "message:1",
    messageSeq: 1,
    role: "user",
    blocks: [
      {
        ...assistantTextEntry(body).blocks[0]!,
        id: "message:1:block",
        body
      }
    ]
  };
}
