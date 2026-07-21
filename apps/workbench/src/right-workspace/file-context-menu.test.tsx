// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WorkspaceFileContextMenu } from "./file-context-menu";

afterEach(cleanup);

describe("WorkspaceFileContextMenu", () => {
  it("renders through a portal, clamps to the viewport, and restores focus on Escape", async () => {
    const anchor = document.createElement("button");
    document.body.append(anchor);
    anchor.focus();
    const onClose = vi.fn();
    const rectSpy = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
      bottom: 140,
      height: 120,
      left: 0,
      right: 240,
      top: 0,
      width: 240,
      x: 0,
      y: 0,
      toJSON: () => ({})
    });
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 300 });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 200 });

    const { unmount } = render(
      <WorkspaceFileContextMenu
        anchor={{ element: anchor, x: 290, y: 190 }}
        ariaLabel="Actions for README.md"
        items={[{ id: "vscode", label: "Open in VS Code" }, { id: "reveal", label: "Show in Finder", separatorBefore: true }]}
        loading={false}
        onClose={onClose}
        onSelect={vi.fn()}
      />
    );

    const menu = screen.getByRole("menu", { name: "Actions for README.md" });
    expect(menu.parentElement).toBe(document.body);
    await waitFor(() => {
      expect(menu.style.left).toBe("52px");
      expect(menu.style.top).toBe("72px");
      expect(menu.style.visibility).toBe("visible");
    });
    expect(document.activeElement).toBe(screen.getByRole("menuitem", { name: "Open in VS Code" }));

    fireEvent.keyDown(menu, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    unmount();
    expect(document.activeElement).toBe(anchor);
    rectSpy.mockRestore();
    anchor.remove();
  });

  it("moves focus between actions and selects the focused action", () => {
    const anchor = document.createElement("button");
    document.body.append(anchor);
    const onSelect = vi.fn();
    render(
      <WorkspaceFileContextMenu
        anchor={{ element: anchor, x: 20, y: 20 }}
        ariaLabel="File actions"
        items={[{ id: "vscode", label: "Open in VS Code" }, { id: "reveal", label: "Show in Finder", separatorBefore: true }]}
        loading={false}
        onClose={vi.fn()}
        onSelect={onSelect}
      />
    );

    const first = screen.getByRole("menuitem", { name: "Open in VS Code" });
    const reveal = screen.getByRole("menuitem", { name: "Show in Finder" });
    expect(document.activeElement).toBe(first);
    fireEvent.keyDown(first, { key: "ArrowDown" });
    expect(document.activeElement).toBe(reveal);
    fireEvent.click(reveal);
    expect(onSelect).toHaveBeenCalledWith("reveal");
    anchor.remove();
  });

  it("closes when pointer input happens outside the menu", () => {
    const anchor = document.createElement("button");
    document.body.append(anchor);
    const onClose = vi.fn();
    render(
      <WorkspaceFileContextMenu
        anchor={{ element: anchor, x: 20, y: 20 }}
        ariaLabel="File actions"
        items={[{ id: "reveal", label: "Show in File Manager" }]}
        loading={false}
        onClose={onClose}
        onSelect={vi.fn()}
      />
    );

    fireEvent.pointerDown(document.body);
    expect(onClose).toHaveBeenCalledTimes(1);
    anchor.remove();
  });

  it("closes on Escape even when focus has moved outside the menu", () => {
    const anchor = document.createElement("button");
    const outside = document.createElement("button");
    document.body.append(anchor, outside);
    const onClose = vi.fn();
    render(
      <WorkspaceFileContextMenu
        anchor={{ element: anchor, x: 20, y: 20 }}
        ariaLabel="File actions"
        items={[{ id: "reveal", label: "Show in File Manager" }]}
        loading={false}
        onClose={onClose}
        onSelect={vi.fn()}
      />
    );

    outside.focus();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    anchor.remove();
    outside.remove();
  });

  it("focuses the first action when application detection completes", () => {
    const anchor = document.createElement("button");
    document.body.append(anchor);
    const { rerender } = render(
      <WorkspaceFileContextMenu
        anchor={{ element: anchor, x: 20, y: 20 }}
        ariaLabel="File actions"
        items={[]}
        loading
        onClose={vi.fn()}
        onSelect={vi.fn()}
      />
    );
    const loadingMenu = screen.getByRole("menu", { name: "File actions" });
    expect(screen.getByRole("menuitem", { name: "Detecting applications…" }).getAttribute("aria-disabled")).toBe("true");
    expect(document.activeElement).toBe(loadingMenu);

    rerender(
      <WorkspaceFileContextMenu
        anchor={{ element: anchor, x: 20, y: 20 }}
        ariaLabel="File actions"
        items={[{ id: "vscode", label: "Open in VS Code" }]}
        loading={false}
        onClose={vi.fn()}
        onSelect={vi.fn()}
      />
    );

    expect(document.activeElement).toBe(screen.getByRole("menuitem", { name: "Open in VS Code" }));
    anchor.remove();
  });

  it("handles Escape while application detection is loading", () => {
    const anchor = document.createElement("button");
    document.body.append(anchor);
    anchor.focus();
    const onClose = vi.fn();
    const { unmount } = render(
      <WorkspaceFileContextMenu
        anchor={{ element: anchor, x: 20, y: 20 }}
        ariaLabel="File actions"
        items={[]}
        loading
        onClose={onClose}
        onSelect={vi.fn()}
      />
    );

    const menu = screen.getByRole("menu", { name: "File actions" });
    expect(document.activeElement).toBe(menu);
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    unmount();
    expect(document.activeElement).toBe(anchor);
    anchor.remove();
  });
});
