// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ActionReceiptProvider, useActionReceipts } from "./receipts";
import { ConfirmActionProvider, ConfirmDialog, Menu, useConfirmAction } from "./overlays";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("ConfirmDialog", () => {
  it("is modal, focuses Cancel, ignores backdrop presses, and cancels with Escape", () => {
    const onCancel = vi.fn();
    const onConfirm = vi.fn();
    render(
      <ConfirmDialog
        confirmLabel="Delete session"
        description="This permanently removes the session."
        onCancel={onCancel}
        onConfirm={onConfirm}
        open
        title="Delete session?"
        tone="danger"
      />
    );

    const dialog = screen.getByRole("dialog", { name: "Delete session?" });
    expect(dialog.getAttribute("aria-modal")).toBe("true");
    expect(screen.getByRole("button", { name: "Delete session" }).getAttribute("data-variant")).toBe("danger");
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Cancel" }));
    fireEvent.mouseDown(dialog.parentElement!);
    expect(onCancel).not.toHaveBeenCalled();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("resolves requested confirmations without native browser prompts", async () => {
    function Harness() {
      const confirm = useConfirmAction();
      return <button onClick={() => void confirm({ title: "Discard edits?", description: "Unsaved work will be lost.", confirmLabel: "Discard" })}>Ask</button>;
    }
    render(<ConfirmActionProvider><Harness /></ConfirmActionProvider>);
    fireEvent.click(screen.getByRole("button", { name: "Ask" }));
    expect(await screen.findByRole("dialog", { name: "Discard edits?" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("stays open and disables dismissal until the confirmed action settles", async () => {
    let releaseAction: (() => void) | undefined;
    const action = vi.fn(() => new Promise<void>((resolve) => {
      releaseAction = resolve;
    }));
    function Harness() {
      const confirm = useConfirmAction();
      return (
        <button onClick={() => void confirm({
          action,
          confirmLabel: "Delete",
          description: "This cannot be undone.",
          title: "Delete item?",
          tone: "danger"
        })}>
          Ask
        </button>
      );
    }
    render(<ConfirmActionProvider><Harness /></ConfirmActionProvider>);
    fireEvent.click(screen.getByRole("button", { name: "Ask" }));
    const dialog = await screen.findByRole("dialog", { name: "Delete item?" });
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));

    expect(action).toHaveBeenCalledOnce();
    expect(dialog.getAttribute("aria-busy")).toBe("true");
    expect((screen.getByRole("button", { name: "Close" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("button", { name: "Cancel" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("button", { name: "Delete" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.keyDown(dialog, { key: "Escape" });
    fireEvent.mouseDown(dialog.parentElement!);
    expect(screen.getByRole("dialog", { name: "Delete item?" })).toBeTruthy();

    releaseAction?.();
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });
});

describe("Menu", () => {
  it("uses menuitem semantics and closes after selection", () => {
    const onOpenChange = vi.fn();
    const onSelect = vi.fn();
    render(
      <Menu
        items={[{ id: "rename", label: "Rename", onSelect }]}
        label="Session actions"
        onOpenChange={onOpenChange}
        open
      />
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "Rename" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});

function ReceiptHarness() {
  const receipts = useActionReceipts();
  return (
    <>
      <button onClick={() => receipts.push({ message: "First saved" })}>First</button>
      <button onClick={() => receipts.push({ message: "Second saved" })}>Second</button>
      <button onClick={() => receipts.push({ message: "Third saved" })}>Third</button>
      <button onClick={() => receipts.push({ message: "Deleted", undo: vi.fn().mockResolvedValue(undefined) })}>Delete</button>
    </>
  );
}

describe("ActionReceiptProvider", () => {
  it("keeps only the two newest receipts and expires them", () => {
    vi.useFakeTimers();
    render(<ActionReceiptProvider durationMs={8_000}><ReceiptHarness /></ActionReceiptProvider>);
    fireEvent.click(screen.getByRole("button", { name: "First" }));
    fireEvent.click(screen.getByRole("button", { name: "Second" }));
    fireEvent.click(screen.getByRole("button", { name: "Third" }));
    expect(screen.queryByText("First saved")).toBeNull();
    expect(screen.getByText("Second saved")).toBeTruthy();
    expect(screen.getByText("Third saved")).toBeTruthy();
    act(() => vi.advanceTimersByTime(8_000));
    expect(screen.queryByText("Second saved")).toBeNull();
  });

  it("renders Undo as an action while the receipt remains display-only", () => {
    render(<ActionReceiptProvider><ReceiptHarness /></ActionReceiptProvider>);
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(screen.getByRole("status").textContent).toContain("Deleted");
    expect(screen.getByRole("button", { name: "Undo Deleted" })).toBeTruthy();
  });

  it("pauses expiry while a receipt is hovered", () => {
    vi.useFakeTimers();
    render(<ActionReceiptProvider durationMs={8_000}><ReceiptHarness /></ActionReceiptProvider>);
    fireEvent.click(screen.getByRole("button", { name: "First" }));
    const receipt = screen.getByRole("status");
    act(() => vi.advanceTimersByTime(4_000));
    fireEvent.mouseEnter(receipt);
    act(() => vi.advanceTimersByTime(8_000));
    expect(screen.getByText("First saved")).toBeTruthy();
    fireEvent.mouseLeave(receipt);
    act(() => vi.advanceTimersByTime(8_000));
    expect(screen.queryByText("First saved")).toBeNull();
  });
});
