// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Composer } from "./composer";

afterEach(() => {
  cleanup();
});

describe("Composer attachments", () => {
  it("renders image attachments with thumbnails", () => {
    const { container } = render(
      <Composer
        attachments={[{
          id: "image-1",
          kind: "image",
          name: "pixel.png",
          previewUrl: "data:image/png;base64,abc",
          sizeLabel: "1 KiB"
        }]}
        running={false}
        onInterrupt={vi.fn()}
        onSteer={vi.fn()}
        onSubmit={vi.fn()}
      />
    );

    expect(screen.getByText("pixel.png")).toBeTruthy();
    const thumbnail = container.querySelector(".pevo-attachmentThumb") as HTMLImageElement | null;
    expect(thumbnail?.getAttribute("src")).toBe("data:image/png;base64,abc");
  });

  it("forwards pasted files to the host attachment handler", () => {
    const onAttachFiles = vi.fn();
    const file = new File(["pixels"], "pixel.png", { type: "image/png" });
    render(
      <Composer
        running={false}
        onAttachFiles={onAttachFiles}
        onInterrupt={vi.fn()}
        onSteer={vi.fn()}
        onSubmit={vi.fn()}
      />
    );

    fireEvent.paste(screen.getByPlaceholderText("Ask Psychevo..."), {
      clipboardData: { files: [file] }
    });

    expect(onAttachFiles).toHaveBeenCalledWith([file]);
  });

  it("renders the attachment drawer action with a leading paperclip icon", () => {
    render(
      <Composer
        running={false}
        onAttach={vi.fn()}
        onInterrupt={vi.fn()}
        onSteer={vi.fn()}
        onSubmit={vi.fn()}
      />
    );

    const add = screen.getByRole("button", { name: "Add attachments and options" });
    expect(add.getAttribute("aria-haspopup")).toBe("dialog");
    fireEvent.click(add);
    const attachment = screen.getByRole("button", { name: "Add images and files" });
    expect(within(attachment).getByText("Add images and files")).toBeTruthy();
    expect(attachment.querySelector(".lucide-paperclip")).toBeTruthy();
  });
});

describe("Composer submission feedback", () => {
  it("shows Preparing immediately and preserves input while acceptance is pending", async () => {
    let resolveSubmission!: (accepted: boolean) => void;
    const onSubmit = vi.fn(() => new Promise<boolean>((resolve) => {
      resolveSubmission = resolve;
    }));
    render(
      <Composer
        retainDraftUntilAccepted
        running={false}
        onInterrupt={vi.fn()}
        onSteer={vi.fn()}
        onSubmit={onSubmit}
      />
    );

    const input = screen.getByPlaceholderText("Ask Psychevo...");
    fireEvent.change(input, { target: { value: "keep this draft" } });
    fireEvent.submit(input.closest("form")!);

    expect(screen.getByLabelText("Submission preparing elapsed").textContent).toContain("Preparing");
    expect((input as HTMLTextAreaElement).value).toBe("keep this draft");
    expect((screen.getByRole("button", { name: "Send message" }) as HTMLButtonElement).disabled).toBe(true);

    await act(async () => resolveSubmission(false));
    expect(screen.queryByLabelText("Submission preparing elapsed")).toBeNull();
    expect((input as HTMLTextAreaElement).value).toBe("keep this draft");
  });

  it("invalidates a pending submission when its attachment set changes", async () => {
    let resolveSubmission!: (accepted: boolean) => void;
    let isInputCurrent: (() => boolean) | undefined;
    const onSubmit = vi.fn((
      _text: string,
      _mentions: unknown[],
      _orderedInput: unknown,
      current?: () => boolean
    ) => {
      isInputCurrent = current;
      return new Promise<boolean>((resolve) => {
        resolveSubmission = resolve;
      });
    });
    const attachment = {
      id: "file-1",
      kind: "file" as const,
      name: "notes.txt",
      sizeLabel: "1 KiB"
    };
    const props = {
      retainDraftUntilAccepted: true,
      running: false,
      onInterrupt: vi.fn(),
      onSteer: vi.fn(),
      onSubmit
    };
    const view = render(<Composer {...props} attachments={[attachment]} />);

    fireEvent.submit(screen.getByPlaceholderText("Ask Psychevo...").closest("form")!);
    expect(isInputCurrent?.()).toBe(true);

    view.rerender(<Composer {...props} attachments={[]} />);
    expect(isInputCurrent?.()).toBe(false);

    await act(async () => resolveSubmission(true));
  });
});
