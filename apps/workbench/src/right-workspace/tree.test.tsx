// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { WorkspaceFileTreeItem } from "../types";
import { WorkspaceFileTree } from "./tree";

afterEach(cleanup);

const ITEMS: WorkspaceFileTreeItem[] = [
  { kind: "directory", name: "assets", path: "assets", depth: 0 },
  { kind: "file", name: "photo.png", path: "assets/photo.png", depth: 1, previewDisabled: true },
  { kind: "file", name: "README.md", path: "README.md", depth: 0 }
];

function renderTree(onFileContextMenu = vi.fn(), onOpen = vi.fn()) {
  return {
    onFileContextMenu,
    onOpen,
    ...render(
      <WorkspaceFileTree
        emptyLabel="No files"
        filterLabel="Filter files"
        filterPlaceholder="Filter..."
        items={ITEMS}
        selectedPath={null}
        onFileContextMenu={onFileContextMenu}
        onOpen={onOpen}
      />
    )
  };
}

describe("WorkspaceFileTree context menu", () => {
  it("keeps files without an internal preview available to the context menu", () => {
    const { onFileContextMenu, onOpen } = renderTree();
    const image = screen.getByRole("treeitem", {
      name: /photo\.png/,
      description: "Only the built-in preview is unavailable. External file actions remain available from the context menu."
    });

    expect((image as HTMLButtonElement).disabled).toBe(false);
    expect(image.hasAttribute("aria-disabled")).toBe(false);
    fireEvent.click(image);
    expect(onOpen).not.toHaveBeenCalled();

    fireEvent.contextMenu(image, { clientX: 84, clientY: 126 });
    expect(onFileContextMenu).toHaveBeenCalledWith({
      anchor: image,
      clientX: 84,
      clientY: 126,
      path: "assets/photo.png"
    });
  });

  it("does not open a file context menu for directories", () => {
    const { onFileContextMenu } = renderTree();
    fireEvent.contextMenu(screen.getByRole("treeitem", { name: /assets/ }));
    expect(onFileContextMenu).not.toHaveBeenCalled();
  });

  it.each([
    { key: "ContextMenu" },
    { key: "F10", shiftKey: true }
  ])("supports the $key keyboard gesture", (keyboard) => {
    const { onFileContextMenu } = renderTree();
    const readme = screen.getByRole("treeitem", { name: /README\.md/ });
    vi.spyOn(readme, "getBoundingClientRect").mockReturnValue({
      bottom: 72,
      height: 28,
      left: 20,
      right: 220,
      top: 44,
      width: 200,
      x: 20,
      y: 44,
      toJSON: () => ({})
    });

    fireEvent.keyDown(readme, keyboard);
    expect(onFileContextMenu).toHaveBeenCalledWith({
      anchor: readme,
      clientX: 44,
      clientY: 72,
      path: "README.md"
    });
  });
});
