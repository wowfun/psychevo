// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Composer } from "@psychevo/components";
import type { CompletionListResult } from "@psychevo/protocol";

afterEach(() => {
  cleanup();
});

describe("Composer completion mentions", () => {
  it("submits an accepted skill completion as a structured mention after more typing", async () => {
    const onSubmit = vi.fn();
    const completionProvider = vi.fn(async (): Promise<CompletionListResult> => ({
      replacement: { start: 0, end: 2 },
      items: [
        {
          id: "skill:x-daily",
          sigil: "$",
          label: "$x-daily",
          insertText: "$x-daily",
          kind: "skill",
          detail: "Daily feed skill",
          sortText: "1:x-daily",
          target: {
            kind: "skill",
            name: "x-daily",
            path: "/project/.agents/skills/x-daily/SKILL.md"
          }
        }
      ]
    }));

    render(
      <Composer
        completionProvider={completionProvider}
        running={false}
        onInterrupt={vi.fn()}
        onSteer={vi.fn()}
        onSubmit={onSubmit}
      />
    );

    const textarea = screen.getByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: "$x" } });

    const option = await screen.findByRole("option", { name: /\$x-daily/ });
    fireEvent.mouseDown(option);
    await waitFor(() => expect(textarea.value).toBe("$x-daily "));

    fireEvent.change(textarea, { target: { value: "$x-daily fetch today" } });
    fireEvent.submit(textarea.closest("form")!);

    expect(onSubmit).toHaveBeenCalledWith("$x-daily fetch today", [
      {
        visibleText: "$x-daily",
        range: { start: 0, end: 8 },
        target: {
          kind: "skill",
          name: "x-daily",
          path: "/project/.agents/skills/x-daily/SKILL.md"
        }
      }
    ]);
  });

  it("does not reopen the completion popover when a stale request resolves after submit", async () => {
    const onSubmit = vi.fn();
    let resolveCompletion: (result: CompletionListResult) => void = () => {};
    const completionPromise = new Promise<CompletionListResult>((resolve) => {
      resolveCompletion = resolve;
    });
    const completionProvider = vi.fn(() => completionPromise);

    render(
      <Composer
        completionProvider={completionProvider}
        running={false}
        onInterrupt={vi.fn()}
        onSteer={vi.fn()}
        onSubmit={onSubmit}
      />
    );

    const textarea = screen.getByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: "$x" } });
    await waitFor(() => expect(completionProvider).toHaveBeenCalled());

    fireEvent.submit(textarea.closest("form")!);
    expect(onSubmit).toHaveBeenCalledWith("$x", []);

    await act(async () => {
      resolveCompletion({
        replacement: { start: 0, end: 2 },
        items: [
          {
            id: "skill:x-daily",
            sigil: "$",
            label: "$x-daily",
            insertText: "$x-daily",
            kind: "skill",
            detail: null,
            target: null,
            sortText: "1:x-daily"
          }
        ]
      });
      await completionPromise;
    });

    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("accepts slash completion and submits it as a command", async () => {
    const onCommand = vi.fn();
    const onSubmit = vi.fn();
    const completionProvider = vi.fn(async (): Promise<CompletionListResult> => ({
      replacement: { start: 0, end: 2 },
      items: [
        {
          id: "command:help",
          sigil: "/",
          label: "/help",
          insertText: "/help",
          kind: "command",
          detail: "show commands and shortcuts",
          sortText: "command:help",
          target: null
        }
      ]
    }));

    render(
      <Composer
        completionProvider={completionProvider}
        running={false}
        onCommand={onCommand}
        onInterrupt={vi.fn()}
        onSteer={vi.fn()}
        onSubmit={onSubmit}
      />
    );

    const textarea = screen.getByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: "/h" } });

    await waitFor(() => expect(screen.getByRole("option", { name: /\/help/ })).toBeTruthy());
    fireEvent.keyDown(textarea, { key: "Enter" });
    await waitFor(() => expect(textarea.value).toBe("/help "));

    fireEvent.submit(textarea.closest("form")!);

    expect(onCommand).toHaveBeenCalledWith("/help");
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("ignores completion results that omit items", async () => {
    const completionProvider = vi.fn(async () => ({
      replacement: null
    }) as CompletionListResult);

    render(
      <Composer
        completionProvider={completionProvider}
        running={false}
        onInterrupt={vi.fn()}
        onSteer={vi.fn()}
        onSubmit={vi.fn()}
      />
    );

    const textarea = screen.getByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: "$x" } });

    await waitFor(() => expect(completionProvider).toHaveBeenCalled());
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("scrolls the active completion option into view during keyboard navigation", async () => {
    const scrollIntoView = vi.fn();
    const originalScrollIntoView = HTMLElement.prototype.scrollIntoView;
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: scrollIntoView
    });
    const items = Array.from({ length: 24 }, (_, index) => ({
      id: `skill:skill-${index}`,
      sigil: "$",
      label: `$skill-${String(index).padStart(2, "0")}`,
      insertText: `$skill-${String(index).padStart(2, "0")}`,
      kind: "skill",
      detail: `Skill ${index}`,
      sortText: `1:skill-${String(index).padStart(2, "0")}`,
      target: null
    }));
    const completionProvider = vi.fn(async (): Promise<CompletionListResult> => ({
      replacement: { start: 0, end: 1 },
      items
    }));

    try {
      render(
        <Composer
          completionProvider={completionProvider}
          running={false}
          onInterrupt={vi.fn()}
          onSteer={vi.fn()}
          onSubmit={vi.fn()}
        />
      );

      const textarea = screen.getByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement;
      fireEvent.change(textarea, { target: { value: "$" } });

      await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(24));
      scrollIntoView.mockClear();

      fireEvent.keyDown(textarea, { key: "ArrowUp" });

      const lastOption = screen.getByRole("option", { name: /\$skill-23/ });
      await waitFor(() => expect(lastOption.getAttribute("aria-selected")).toBe("true"));
      expect(scrollIntoView).toHaveBeenCalledWith({ block: "nearest" });
    } finally {
      Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
        configurable: true,
        value: originalScrollIntoView
      });
    }
  });

  it("enters shell mode from bang and submits the stripped shell command", () => {
    const onShell = vi.fn();
    const onSubmit = vi.fn();

    render(
      <Composer
        running={false}
        onInterrupt={vi.fn()}
        onShell={onShell}
        onSteer={vi.fn()}
        onSubmit={onSubmit}
      />
    );

    const textarea = screen.getByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement;
    fireEvent.keyDown(textarea, { key: "!" });

    const shellTextarea = screen.getByPlaceholderText("shell command") as HTMLTextAreaElement;
    expect(screen.getByText("shell mode: type !<command> to run a local shell command")).toBeTruthy();

    fireEvent.change(shellTextarea, { target: { value: "pwd" } });
    fireEvent.submit(shellTextarea.closest("form")!);

    expect(onShell).toHaveBeenCalledWith("pwd");
    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.getByPlaceholderText("Ask Psychevo...")).toBeTruthy();
  });

  it("imports pasted bang-prefixed text as shell mode without the bang", () => {
    const onShell = vi.fn();

    render(
      <Composer
        running={false}
        onInterrupt={vi.fn()}
        onShell={onShell}
        onSteer={vi.fn()}
        onSubmit={vi.fn()}
      />
    );

    const textarea = screen.getByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: "!printf ok" } });

    const shellTextarea = screen.getByPlaceholderText("shell command") as HTMLTextAreaElement;
    expect(shellTextarea.value).toBe("printf ok");

    fireEvent.submit(shellTextarea.closest("form")!);
    expect(onShell).toHaveBeenCalledWith("printf ok");
  });

  it("suppresses slash completion in shell mode but keeps file completion", async () => {
    const completionProvider = vi.fn(async (): Promise<CompletionListResult> => ({
      replacement: { start: 0, end: 4 },
      items: [
        {
          id: "file:src",
          sigil: "@",
          label: "@src/",
          insertText: "@src/",
          kind: "directory",
          detail: "src/",
          sortText: "src",
          target: null
        }
      ]
    }));

    render(
      <Composer
        completionProvider={completionProvider}
        running={false}
        onInterrupt={vi.fn()}
        onShell={vi.fn()}
        onSteer={vi.fn()}
        onSubmit={vi.fn()}
      />
    );

    const textarea = screen.getByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement;
    fireEvent.keyDown(textarea, { key: "!" });
    const shellTextarea = screen.getByPlaceholderText("shell command") as HTMLTextAreaElement;

    fireEvent.change(shellTextarea, { target: { value: "/h" } });
    expect(completionProvider).not.toHaveBeenCalled();
    expect(screen.queryByRole("listbox")).toBeNull();

    fireEvent.change(shellTextarea, { target: { value: "@src" } });
    await waitFor(() => expect(completionProvider).toHaveBeenCalled());
    expect(await screen.findByRole("option", { name: /@src\// })).toBeTruthy();
  });
});
