// @vitest-environment jsdom

import { fireEvent, render, screen } from "@testing-library/react";
import { ConfirmActionProvider } from "@psychevo/components";
import { createElement } from "react";
import { describe, expect, it, vi } from "vitest";
import { FilesPanel, requestFilesRootChange, workspaceRootOptionLabel } from "./files";

describe("Files root selection", () => {
  it("keeps the current root when discarding a dirty editor is rejected", async () => {
    const confirmDirty = vi.fn(async () => false);
    const onRootChange = vi.fn(async (_root: string, beforeCommit?: () => boolean | Promise<boolean>) => (
      beforeCommit ? Boolean(await beforeCommit()) : true
    ));

    await requestFilesRootChange({
      confirmDirty,
      currentRoot: "/primary",
      nextRoot: "/secondary",
      onRootChange
    });

    expect(confirmDirty).toHaveBeenCalledOnce();
    expect(onRootChange).toHaveBeenCalledWith("/secondary", confirmDirty);
  });

  it("switches after an explicit discard confirmation", async () => {
    const confirmDirty = vi.fn(async () => true);
    const onRootChange = vi.fn(async (_root: string, beforeCommit?: () => boolean | Promise<boolean>) => (
      beforeCommit ? Boolean(await beforeCommit()) : true
    ));

    await requestFilesRootChange({
      confirmDirty,
      currentRoot: "/primary",
      nextRoot: "/secondary",
      onRootChange
    });

    expect(onRootChange).toHaveBeenCalledWith("/secondary", confirmDirty);
  });

  it("lets the rendered current root cancel an in-flight switch", async () => {
    const onRootChange = vi.fn(async () => true);

    await requestFilesRootChange({
      confirmDirty: vi.fn(async () => true),
      currentRoot: "/primary",
      nextRoot: "/primary",
      onRootChange
    });

    expect(onRootChange).toHaveBeenCalledWith("/primary");
  });

  it("keeps identical basenames accessible as distinct roots", () => {
    expect([
      workspaceRootOptionLabel("/work/frontend/src"),
      workspaceRootOptionLabel("/work/backend/src")
    ]).toEqual(["/work/frontend/src", "/work/backend/src"]);
  });

  it("renders the multi-root chooser as a dismissible popup instead of a native select", () => {
    render(
      createElement(
        ConfirmActionProvider,
        null,
        createElement(FilesPanel, {
          client: null,
          files: [],
          root: "/primary",
          roots: ["/primary", "/secondary"],
          scope: null,
          selectedPath: null,
          tabId: "files-1",
          truncated: false,
          onCompare: vi.fn(),
          onDirtyChange: vi.fn(),
          onFileTreeOpenChange: vi.fn(),
          onOpen: vi.fn(),
          onRootChange: vi.fn(),
          htmlExecutionActive: false,
          fileTreeOpen: true
        })
      )
    );

    expect(screen.queryByRole("combobox", { name: "Workspace directory" })).toBeNull();
    const trigger = screen.getByRole("button", { name: "Workspace directory" });
    fireEvent.click(trigger);
    expect(screen.getByRole("menu", { name: "Workspace directory" })).toBeTruthy();
    expect(screen.getByRole("menuitemradio", { name: "/primary" }).getAttribute("aria-checked"))
      .toBe("true");
  });
});
