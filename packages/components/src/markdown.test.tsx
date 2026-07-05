// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
