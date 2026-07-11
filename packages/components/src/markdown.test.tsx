// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MarkdownText } from "./markdown";

afterEach(() => {
  cleanup();
});

describe("MarkdownText frontmatter", () => {
  it("renders document-start YAML frontmatter as a table before the markdown body", () => {
    render(<MarkdownText text={"---\nname: stitch\ncount: 2\npublished: false\n---\n# Extract\n\n- body"} />);

    const table = screen.getByRole("table", { name: "YAML frontmatter" });
    expect(table).toBeTruthy();
    expect(screen.getByText("name")).toBeTruthy();
    expect(screen.getByText("stitch")).toBeTruthy();
    expect(screen.getByText("count")).toBeTruthy();
    expect(screen.getByText("2")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Extract" })).toBeTruthy();
    expect(screen.getByText("body")).toBeTruthy();
    expect(screen.queryByText("---")).toBeNull();
  });

  it("renders scalar arrays as chips and nested values as bounded JSON code", () => {
    render(
      <MarkdownText
        text={
          "---\nallowed-tools:\n  - Bash\n  - Read\nmetadata:\n  nested: true\n  level: 2\n---\nBody"
        }
      />
    );

    expect(screen.getByText("allowed-tools")).toBeTruthy();
    expect(screen.getByText("Bash").classList.contains("pevo-frontmatterChip")).toBe(true);
    expect(screen.getByText("Read").classList.contains("pevo-frontmatterChip")).toBe(true);
    const table = screen.getByRole("table", { name: "YAML frontmatter" });
    expect(table.textContent).toContain('"nested": true');
    expect(table.textContent).toContain('"level": 2');
    expect(screen.getByText("Body")).toBeTruthy();
  });

  it("keeps frontmatter scalar values as text instead of reparsing markdown", () => {
    const { container } = render(<MarkdownText text={'---\ndescription: "**bold**"\n---\nBody'} />);

    const table = screen.getByRole("table", { name: "YAML frontmatter" });
    expect(screen.getByText("**bold**")).toBeTruthy();
    expect(table.querySelector("strong")).toBeNull();
    expect(container.querySelectorAll("strong")).toHaveLength(0);
  });

  it("falls back to ordinary markdown for invalid, non-mapping, and unclosed frontmatter", () => {
    const { rerender } = render(<MarkdownText text={"---\nname: [broken\n---\n# Body"} />);
    expect(screen.queryByRole("table", { name: "YAML frontmatter" })).toBeNull();
    expect(screen.getByText("name: [broken")).toBeTruthy();

    rerender(<MarkdownText text={"---\n- one\n---\n# Body"} />);
    expect(screen.queryByRole("table", { name: "YAML frontmatter" })).toBeNull();
    expect(screen.getByText("one")).toBeTruthy();

    rerender(<MarkdownText text={"---\nname: open\n# Body"} />);
    expect(screen.queryByRole("table", { name: "YAML frontmatter" })).toBeNull();
    expect(screen.getByText("name: open")).toBeTruthy();
  });

  it("does not treat horizontal rules after the first line as frontmatter", () => {
    render(<MarkdownText text={"# Body\n\n---\nname: ordinary text"} />);

    expect(screen.queryByRole("table", { name: "YAML frontmatter" })).toBeNull();
    expect(screen.getByRole("heading", { name: "Body" })).toBeTruthy();
    expect(screen.getByText("name: ordinary text")).toBeTruthy();
  });
});

describe("MarkdownText Mermaid rendering", () => {
  it("lazy-renders complete fenced Mermaid blocks", async () => {
    const initialize = vi.fn();
    const renderMermaid = vi.fn(async () => ({
      svg: '<svg role="img" aria-label="flow"><text>Rendered flow</text></svg>'
    }));
    const mermaidLoader = vi.fn(async () => ({
      default: { initialize, render: renderMermaid }
    }));
    const { container } = render(
      <MarkdownText
        mermaidLoader={mermaidLoader}
        text={"Before\n\n```mermaid\nflowchart TD\n  A --> B\n```\n\nAfter"}
      />
    );

    await waitFor(() => {
      expect(renderMermaid).toHaveBeenCalledWith(expect.stringMatching(/^pevo-mermaid-/), "flowchart TD\n  A --> B");
    });
    expect(initialize).toHaveBeenCalledWith(expect.objectContaining({ securityLevel: "strict", startOnLoad: false }));
    expect(container.querySelector(".pevo-mermaidCanvas svg")).toBeTruthy();
    expect(screen.getByText("Before")).toBeTruthy();
    expect(screen.getByText("After")).toBeTruthy();
  });

  it("keeps incomplete Mermaid fences as code while streaming", () => {
    const mermaidLoader = vi.fn(async () => ({
      default: { render: vi.fn() }
    }));

    render(
      <MarkdownText
        mermaidLoader={mermaidLoader}
        streaming
        text={"```mermaid\nflowchart TD\n  A --> B"}
      />
    );

    expect(mermaidLoader).not.toHaveBeenCalled();
    expect(screen.getByText(/flowchart TD/)).toBeTruthy();
  });

  it("keeps a later incomplete Mermaid occurrence as code when its source matches a complete block", async () => {
    const renderMermaid = vi.fn(async () => ({
      svg: '<svg role="img" aria-label="flow"><text>Rendered flow</text></svg>'
    }));
    const mermaidLoader = vi.fn(async () => ({
      default: { render: renderMermaid }
    }));
    const source = "flowchart TD\n  A --> B";
    const { container } = render(
      <MarkdownText
        mermaidLoader={mermaidLoader}
        text={`\`\`\`mermaid\n${source}\n\`\`\`\n\n\`\`\`mermaid\n${source}`}
      />
    );

    await waitFor(() => {
      expect(renderMermaid).toHaveBeenCalledTimes(1);
    });
    expect(container.querySelectorAll(".pevo-mermaidBlock")).toHaveLength(1);
    expect(container.querySelectorAll("pre code.language-mermaid")).toHaveLength(1);
  });

  it("renders Mermaid errors inline and copies the diagram source", async () => {
    const onCopyText = vi.fn(async () => undefined);
    const mermaidLoader = vi.fn(async () => ({
      default: {
        render: vi.fn(async () => {
          throw new Error("Parse failed");
        })
      }
    }));
    render(
      <MarkdownText
        mermaidLoader={mermaidLoader}
        onCopyText={onCopyText}
        text={"```mermaid\nnot diagram\n```"}
      />
    );

    expect(await screen.findByText("Diagram error")).toBeTruthy();
    expect(screen.getByText("Parse failed")).toBeTruthy();
    expect(screen.getByText("not diagram")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Copy Mermaid source" }));
    await waitFor(() => {
      expect(onCopyText).toHaveBeenCalledWith("not diagram");
    });
  });

  it("supports Mermaid zoom, original-size, expanded view, and source copy controls", async () => {
    const onCopyText = vi.fn(async () => undefined);
    const mermaidLoader = vi.fn(async () => ({
      default: {
        render: vi.fn(async () => ({
          svg: '<svg width="640" height="280" viewBox="0 0 640 280" role="img" aria-label="wide gantt"><text>Wide Gantt</text></svg>'
        }))
      }
    }));
    const { container } = render(
      <MarkdownText
        mermaidLoader={mermaidLoader}
        onCopyText={onCopyText}
        text={"```mermaid\ngantt\ntitle Wide\n```"}
      />
    );

    await screen.findByText("Wide Gantt");
    const canvas = container.querySelector(".pevo-mermaidCanvas");
    const viewport = container.querySelector(".pevo-mermaidViewport") as HTMLElement | null;
    expect(canvas?.classList.contains("is-fit")).toBe(true);
    expect(viewport?.style.width).toBe("100%");

    fireEvent.click(screen.getByRole("button", { name: "Zoom out Mermaid diagram" }));
    await waitFor(() => {
      expect(viewport?.style.width).toBe("75%");
      expect(viewport?.style.minWidth).toBe("0px");
    });
    fireEvent.click(screen.getByRole("button", { name: "Reset Mermaid view" }));

    fireEvent.click(screen.getByRole("button", { name: "Zoom in Mermaid diagram" }));
    await waitFor(() => {
      expect(viewport?.style.width).toBe("125%");
    });

    fireEvent.click(screen.getByRole("button", { name: "View Mermaid diagram at original size" }));
    expect(canvas?.classList.contains("is-actual")).toBe(true);
    expect(viewport?.style.width).toBe("800px");

    fireEvent.click(screen.getByRole("button", { name: "Reset Mermaid view" }));
    expect(canvas?.classList.contains("is-fit")).toBe(true);
    expect(viewport?.style.width).toBe("100%");

    fireEvent.click(screen.getByRole("button", { name: "Expand Mermaid diagram" }));
    const dialog = await screen.findByRole("dialog", { name: "Mermaid diagram" });
    expect(within(dialog).getByText("Wide Gantt")).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "Close Mermaid diagram" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Mermaid diagram" })).toBeNull();
    });

    fireEvent.click(screen.getByRole("button", { name: "Copy Mermaid source" }));
    await waitFor(() => {
      expect(onCopyText).toHaveBeenCalledWith("gantt\ntitle Wide");
    });
  });
});

describe("MarkdownText copy affordance", () => {
  it("does not render a copy button by default", () => {
    const { container } = render(<MarkdownText text={"# Body"} />);

    expect(screen.queryByRole("button", { name: "Copy Markdown" })).toBeNull();
    expect(container.firstElementChild?.classList.contains("pevo-markdown")).toBe(true);
  });

  it("copies the raw Markdown text when enabled", async () => {
    const onCopyText = vi.fn(async () => undefined);
    const source = "# Copy\n\n- raw";
    render(<MarkdownText onCopyText={onCopyText} text={source} />);

    fireEvent.click(screen.getByRole("button", { name: "Copy Markdown" }));

    await waitFor(() => {
      expect(onCopyText).toHaveBeenCalledWith(source);
    });
    expect(screen.getByText("Copied")).toBeTruthy();
  });

  it("supports a custom copy source and label", async () => {
    const onCopyText = vi.fn(async () => undefined);
    render(
      <MarkdownText
        copyLabel="Copy full Markdown"
        copyText={"---\ntitle: Full\n---\n# Full"}
        onCopyText={onCopyText}
        text={"# Bounded"}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Copy full Markdown" }));

    await waitFor(() => {
      expect(onCopyText).toHaveBeenCalledWith("---\ntitle: Full\n---\n# Full");
    });
  });

  it("suppresses duplicate clicks while a copy is pending", async () => {
    let resolveCopy = () => {};
    const onCopyText = vi.fn(() => new Promise<void>((resolve) => {
      resolveCopy = resolve;
    }));
    render(<MarkdownText onCopyText={onCopyText} text={"# Pending"} />);

    const button = screen.getByRole("button", { name: "Copy Markdown" }) as HTMLButtonElement;
    fireEvent.click(button);
    fireEvent.click(button);

    expect(onCopyText).toHaveBeenCalledTimes(1);
    expect(button.disabled).toBe(true);
    resolveCopy();
    await waitFor(() => {
      expect(button.disabled).toBe(false);
    });
  });
});
