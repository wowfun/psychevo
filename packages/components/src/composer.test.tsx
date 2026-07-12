// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
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

    fireEvent.click(screen.getByRole("button", { name: "Add attachments and options" }));
    const attachment = screen.getByRole("menuitem", { name: "Add images and files" });
    expect(within(attachment).getByText("Add images and files")).toBeTruthy();
    expect(attachment.querySelector(".lucide-paperclip")).toBeTruthy();
  });
});
