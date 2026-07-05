// @vitest-environment jsdom

import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
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
    render(<App />);

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
