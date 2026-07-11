// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { TranscriptBlock, TranscriptEntry, TranscriptEntryRole } from "@psychevo/protocol";
import { MarkdownText, TranscriptPanel, type WorkspaceFileLinkContext } from "./index";

beforeAll(() => {
  Element.prototype.scrollTo = vi.fn();
});

afterEach(() => {
  cleanup();
});

function workspaceFileLinks(
  onOpen: WorkspaceFileLinkContext["onOpen"] = vi.fn()
): WorkspaceFileLinkContext {
  return {
    root: "/workspace/project",
    entries: [
      { depth: 1, kind: "file", name: "result.html", path: "reports/result.html" }
    ],
    onOpen
  };
}

function transcriptEntry(
  id: string,
  role: TranscriptEntryRole,
  kind: TranscriptBlock["kind"]
): TranscriptEntry {
  return {
    accounting: null,
    blocks: [{
      artifactIds: [],
      body: "reports/result.html",
      createdAtMs: 1,
      detail: null,
      id: `${id}-block`,
      kind,
      metadata: null,
      order: 0,
      preview: null,
      result: null,
      source: "test",
      status: "completed",
      title: null,
      updatedAtMs: 1
    }],
    createdAtMs: 1,
    id,
    messageSeq: id === "user" ? 1 : id === "reasoning" ? 2 : 3,
    metadata: null,
    role,
    source: "test",
    status: "completed",
    threadId: "thread-1",
    turnId: "turn-1",
    updatedAtMs: 1,
    usage: null
  };
}

describe("MarkdownText workspace file links", () => {
  it("opens an exact workspace-relative file from assistant prose", () => {
    const onOpen = vi.fn();

    render(
      <MarkdownText
        text="Open reports/result.html to inspect the result."
        workspaceFileLinks={workspaceFileLinks(onOpen)}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Open file reports/result.html" }));
    expect(onOpen).toHaveBeenCalledWith("reports/result.html");
  });

  it("maps Windows native, slash, Git Bash, and relative aliases to one inventory path", () => {
    const onOpen = vi.fn();
    const context: WorkspaceFileLinkContext = {
      root: "C:\\Repo",
      entries: [
        { depth: 1, kind: "file", name: "Result HTML.html", path: "docs/Result HTML.html" }
      ],
      onOpen
    };
    const aliases = [
      "docs/Result HTML.html",
      "./docs/Result HTML.html",
      "docs\\Result HTML.html",
      ".\\docs\\Result HTML.html",
      "C:\\REPO\\docs\\result html.html",
      "C:/Repo/docs/Result HTML.html",
      "/c/Repo/docs/Result HTML.html",
      "/c:/Repo/docs/Result HTML.html"
    ];

    render(<MarkdownText text={aliases.join("\n\n")} workspaceFileLinks={context} />);

    const buttons = aliases.map((alias) => screen.getByRole("button", { name: `Open file ${alias}` }));
    for (const button of buttons) {
      fireEvent.click(button);
    }
    expect(onOpen).toHaveBeenCalledTimes(aliases.length);
    expect(onOpen).toHaveBeenCalledWith("docs/Result HTML.html");
  });

  it("opens a whole inline-code path with a line suffix at the inventory file", () => {
    const onOpen = vi.fn();

    render(
      <MarkdownText
        text="Inspect `reports/result.html:42:7` for the generated markup."
        workspaceFileLinks={workspaceFileLinks(onOpen)}
      />
    );

    const button = screen.getByRole("button", { name: "Open file reports/result.html:42:7" });
    expect(button.querySelector("code")?.textContent).toBe("reports/result.html:42:7");
    fireEvent.click(button);
    expect(onOpen).toHaveBeenCalledWith("reports/result.html");
  });

  it("promotes an existing Markdown file link without changing its label", () => {
    const onOpen = vi.fn();

    render(
      <MarkdownText
        text="Open the [rendered output](reports/result.html#L8)."
        workspaceFileLinks={workspaceFileLinks(onOpen)}
      />
    );

    const button = screen.getByRole("button", { name: "Open file rendered output" });
    expect(button.textContent).toContain("rendered output");
    expect(screen.queryByRole("link")).toBeNull();
    fireEvent.click(button);
    expect(onOpen).toHaveBeenCalledWith("reports/result.html");
  });

  it("keeps an unfinished path at the streaming tail as plain text", () => {
    render(
      <MarkdownText
        streaming
        text="Generating reports/result.html"
        workspaceFileLinks={workspaceFileLinks()}
      />
    );

    expect(screen.queryByRole("button", { name: "Open file reports/result.html" })).toBeNull();
    expect(screen.getByText(/reports\/result\.html/)).toBeTruthy();
  });

  it("links a streaming path once a stable sentence delimiter arrives", () => {
    render(
      <MarkdownText
        streaming
        text="Generated reports/result.html."
        workspaceFileLinks={workspaceFileLinks()}
      />
    );

    expect(screen.getByRole("button", { name: "Open file reports/result.html" })).toBeTruthy();
  });

  it("defers a streaming path with an unfinished colon line suffix", () => {
    for (const text of ["reports/result.html:", "reports/result.html:42:"]) {
      const view = render(
        <MarkdownText
          streaming
          text={text}
          workspaceFileLinks={workspaceFileLinks()}
        />
      );

      expect(screen.queryByRole("button", { name: /Open file/u })).toBeNull();
      view.rerender(
        <MarkdownText
          text={text}
          workspaceFileLinks={workspaceFileLinks()}
        />
      );
      expect(screen.getByRole("button", { name: /Open file/u })).toBeTruthy();
      view.unmount();
    }
  });

  it("does not promote text enclosed by raw HTML", () => {
    render(
      <MarkdownText
        text="<span>reports/result.html</span>"
        workspaceFileLinks={workspaceFileLinks()}
      />
    );

    expect(screen.queryByRole("button")).toBeNull();
  });

  it("matches POSIX, Unicode, special-character, overlap, and UNC inventory aliases", () => {
    const onOpen = vi.fn();
    const posixContext: WorkspaceFileLinkContext = {
      root: "/home/kevin/工作 区",
      entries: [
        { depth: 1, kind: "file", name: "markdown.png", path: "dist/markdown.png" },
        { depth: 1, kind: "file", name: "markdown.png.map", path: "dist/markdown.png.map" },
        { depth: 1, kind: "file", name: "图 #1?.html", path: "dist/图 #1?.html" }
      ],
      onOpen
    };
    const { unmount } = render(
      <MarkdownText
        text={[
          "/home/kevin/工作 区/dist/markdown.png",
          "dist\\markdown.png",
          "dist/markdown.png.map",
          "dist/图 #1?.html，",
          "dist/markdown.png#L12-L20"
        ].join("\n\n")}
        workspaceFileLinks={posixContext}
      />
    );

    const posixLabels = [
      "/home/kevin/工作 区/dist/markdown.png",
      "dist\\markdown.png",
      "dist/markdown.png.map",
      "dist/图 #1?.html",
      "dist/markdown.png#L12-L20"
    ];
    for (const label of posixLabels) {
      fireEvent.click(screen.getByRole("button", { name: `Open file ${label}` }));
    }
    expect(onOpen.mock.calls).toEqual([
      ["dist/markdown.png"],
      ["dist/markdown.png"],
      ["dist/markdown.png.map"],
      ["dist/图 #1?.html"],
      ["dist/markdown.png"]
    ]);
    unmount();

    const uncOnOpen = vi.fn();
    const uncContext: WorkspaceFileLinkContext = {
      root: "\\\\server\\share\\Repo",
      entries: [{ depth: 1, kind: "file", name: "report.html", path: "out/report.html" }],
      onOpen: uncOnOpen
    };
    render(
      <MarkdownText
        text={"//SERVER/share/repo/OUT/report.HTML and `\\\\server\\share\\Repo\\out\\report.html`"}
        workspaceFileLinks={uncContext}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Open file //SERVER/share/repo/OUT/report.HTML" }));
    fireEvent.click(screen.getByRole("button", { name: "Open file \\\\server\\share\\Repo\\out\\report.html" }));
    expect(uncOnOpen).toHaveBeenNthCalledWith(1, "out/report.html");
    expect(uncOnOpen).toHaveBeenNthCalledWith(2, "out/report.html");
  });

  it("keeps a native POSIX /c workspace case-sensitive", () => {
    const context: WorkspaceFileLinkContext = {
      root: "/c/repo",
      entries: [
        { depth: 1, kind: "file", name: "Result.html", path: "docs/Result.html" }
      ],
      onOpen: vi.fn()
    };

    render(
      <MarkdownText
        text={"/c/repo/docs/Result.html\n\nDOCS/result.HTML\n\nC:\\repo\\docs\\Result.html"}
        workspaceFileLinks={context}
      />
    );

    expect(screen.getAllByRole("button", { name: /Open file/u })).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Open file /c/repo/docs/Result.html" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Open file DOCS/result.HTML" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Open file C:\\repo\\docs\\Result.html" })).toBeNull();
  });

  it("does not promote an inventory path that is only a prefix of a path-like token", () => {
    render(
      <MarkdownText
        text={[
          "reports/result.html~backup",
          "reports/result.html%2Ebackup",
          "reports/result.html+draft",
          "reports/result.html@copy",
          "reports/result.html$tmp",
          "reports/result.html&more",
          "reports/result.html=preview"
        ].join("\n\n")}
        workspaceFileLinks={workspaceFileLinks()}
      />
    );

    expect(screen.queryByRole("button", { name: /Open file/u })).toBeNull();
  });

  it("leaves case-mismatched, missing, directory, outside, code, image, and external paths unlinked", () => {
    const context: WorkspaceFileLinkContext = {
      root: "/workspace/project",
      entries: [
        { depth: 1, kind: "directory", name: "reports", path: "reports" },
        { depth: 1, kind: "file", name: "result.html", path: "reports/result.html" }
      ],
      onOpen: vi.fn()
    };
    render(
      <MarkdownText
        mermaidLoader={() => new Promise(() => undefined)}
        streaming
        text={[
          "REPORTS/result.html; reports; reports/missing.html; /elsewhere/reports/result.html; reports/result.html.bak",
          "",
          "```text",
          "reports/result.html",
          "```",
          "",
          "```mermaid",
          "reports/result.html",
          "```",
          "",
          "![preview](reports/result.html)",
          "",
          "[reports/result.html](https://example.com/result.html)"
        ].join("\n")}
        workspaceFileLinks={context}
      />
    );

    expect(screen.queryByRole("button", { name: /Open file/u })).toBeNull();
    expect(screen.getByRole("img", { name: "preview" })).toBeTruthy();
    expect(screen.getByRole("link", { name: "reports/result.html" }).getAttribute("href")).toBe(
      "https://example.com/result.html"
    );
  });
});

describe("TranscriptPanel workspace file links", () => {
  it("enables file links only for assistant text blocks", () => {
    const onOpen = vi.fn();

    render(
      <TranscriptPanel
        entries={[
          transcriptEntry("user", "user", "text"),
          transcriptEntry("reasoning", "assistant", "reasoning"),
          transcriptEntry("assistant", "assistant", "text")
        ]}
        workspaceFileLinks={workspaceFileLinks(onOpen)}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Thinking" }));
    const buttons = screen.getAllByRole("button", { name: "Open file reports/result.html" });
    expect(buttons).toHaveLength(1);
    fireEvent.click(buttons[0] as HTMLElement);
    expect(onOpen).toHaveBeenCalledWith("reports/result.html");
  });
});
