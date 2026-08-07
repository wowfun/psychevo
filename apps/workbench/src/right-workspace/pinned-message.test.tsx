// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { PinnedMessagePanel } from "./pinned-message";

afterEach(cleanup);

describe("PinnedMessagePanel", () => {
  it("renders a fixed Markdown snapshot with source provenance and inert workspace paths", () => {
    render(<PinnedMessagePanel message={{
      blockId: "block-1",
      createdAtMs: 1_786_000_000_000,
      entryId: "message:1",
      key: "pin-1",
      role: "assistant",
      sourceTitle: "Architecture review",
      status: "failed",
      text: "Compare `src/main.rs` with [the reference](https://example.com/reference).",
      threadId: "thread-source"
    }} />);

    expect(screen.getByRole("region", { name: "Pinned message" })).toBeTruthy();
    expect(screen.getByText("Architecture review").getAttribute("title")).toBe("thread-source");
    expect(screen.getByText("Failed")).toBeTruthy();
    expect(screen.getByRole("link", { name: "the reference" }).getAttribute("href")).toBe("https://example.com/reference");
    expect(screen.getByText("src/main.rs").closest("a")).toBeNull();
  });
});
