// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { GatewayClientError } from "@psychevo/client";
import { WorkspaceEditorDialog } from "./workspace-editor-dialog";

afterEach(cleanup);

describe("WorkspaceEditorDialog", () => {
  it("renames independently and saves the complete ordered directory set", async () => {
    const onSave = vi.fn(async () => undefined);
    render(
      <WorkspaceEditorDialog
        disabled={false}
        onCancel={vi.fn()}
        onSave={onSave}
        workspace={{
          id: "workspace-1",
          name: "Original",
          roots: ["/repo/web", "/repo/api"],
          sessionIds: [],
          revision: 4,
          cwd: "/repo/web",
          displayPath: "Original",
          hiddenCount: 0,
          nextCursor: null
        }}
      />
    );

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Product" } });
    fireEvent.click(screen.getByRole("button", { name: "Make /repo/api primary" }));
    fireEvent.click(screen.getByRole("button", { name: "Add directory" }));
    fireEvent.change(screen.getByLabelText("Working directory 3"), {
      target: { value: "/repo/docs" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Save workspace" }));

    await waitFor(() => expect(onSave).toHaveBeenCalledWith(
      "Product",
      ["/repo/api", "/repo/web", "/repo/docs"],
      4
    ));
  });

  it("does not allow removing the last directory", () => {
    render(
      <WorkspaceEditorDialog
        disabled={false}
        onCancel={vi.fn()}
        onSave={vi.fn(async () => undefined)}
        workspace={{
          id: "workspace-1",
          name: "Only root",
          roots: ["/repo"],
          sessionIds: [],
          revision: 1,
          cwd: "/repo",
          displayPath: "Only root",
          hiddenCount: 0,
          nextCursor: null
        }}
      />
    );

    expect((screen.getByRole("button", { name: "Remove /repo" }) as HTMLButtonElement).disabled)
      .toBe(true);
  });

  it("returns to idle when the save owner cancels a dirty-draft rebind", async () => {
    const onSave = vi.fn(async () => false);
    render(
      <WorkspaceEditorDialog
        disabled={false}
        onCancel={vi.fn()}
        onSave={onSave}
        workspace={{
          id: "workspace-1",
          name: "Workspace",
          roots: ["/repo"],
          sessionIds: [],
          revision: 1,
          cwd: "/repo",
          displayPath: "Workspace",
          hiddenCount: 0,
          nextCursor: null
        }}
      />
    );

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Cancelled" } });
    fireEvent.click(screen.getByRole("button", { name: "Save workspace" }));

    await waitFor(() => expect(onSave).toHaveBeenCalledOnce());
    expect((screen.getByRole("button", { name: "Save workspace" }) as HTMLButtonElement).disabled)
      .toBe(false);
    expect((screen.getByRole("button", { name: "Cancel" }) as HTMLButtonElement).disabled)
      .toBe(false);
  });

  it("keeps a directory input mounted and focused while its value changes", () => {
    render(
      <WorkspaceEditorDialog
        disabled={false}
        onCancel={vi.fn()}
        onSave={vi.fn(async () => undefined)}
        workspace={{
          id: "workspace-1",
          name: "Workspace",
          roots: ["/repo"],
          sessionIds: [],
          revision: 1,
          cwd: "/repo",
          displayPath: "Workspace",
          hiddenCount: 0,
          nextCursor: null
        }}
      />
    );
    const input = screen.getByLabelText("Primary working directory") as HTMLInputElement;
    input.focus();

    fireEvent.change(input, { target: { value: "/repo-2" } });

    expect(screen.getByLabelText("Primary working directory")).toBe(input);
    expect(document.activeElement).toBe(input);
  });

  it("loads every latest Workspace value from a revision conflict", async () => {
    const latest = {
      id: "workspace-1",
      name: "Remote name",
      roots: ["/repo", "/remote"],
      revision: 5
    };
    const onSave = vi.fn()
      .mockRejectedValueOnce(new GatewayClientError(
        "server_error",
        "acknowledged",
        "Workspace revision conflict",
        { kind: "server", data: { workspace: latest } }
      ))
      .mockResolvedValueOnce(undefined);
    render(
      <WorkspaceEditorDialog
        disabled={false}
        onCancel={vi.fn()}
        onSave={onSave}
        workspace={{
          ...latest,
          name: "Original",
          roots: ["/repo"],
          revision: 4,
          sessionIds: [],
          cwd: "/repo",
          displayPath: "Original",
          hiddenCount: 0,
          nextCursor: null
        }}
      />
    );

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Local name" } });
    fireEvent.click(screen.getByRole("button", { name: "Save workspace" }));
    await screen.findByRole("alert");
    expect((screen.getByLabelText("Name") as HTMLInputElement).value).toBe("Remote name");
    expect((screen.getByLabelText("Working directory 2") as HTMLInputElement).value)
      .toBe("/remote");
    fireEvent.click(screen.getByRole("button", { name: "Save workspace" }));

    await waitFor(() => expect(onSave).toHaveBeenLastCalledWith(
      "Remote name",
      ["/repo", "/remote"],
      5
    ));
  });

  it("adopts the committed Workspace while a post-save refresh is still pending", async () => {
    let rejectSave!: (reason?: unknown) => void;
    const onSave = vi.fn(() => new Promise<void>((_resolve, reject) => {
      rejectSave = reject;
    }));
    const base = {
      id: "workspace-1",
      name: "Original",
      roots: ["/repo"],
      sessionIds: [],
      revision: 1,
      cwd: "/repo",
      displayPath: "Original",
      hiddenCount: 0,
      nextCursor: null
    };
    const view = render(
      <WorkspaceEditorDialog
        disabled={false}
        onCancel={vi.fn()}
        onSave={onSave}
        workspace={base}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Save workspace" }));
    view.rerender(
      <WorkspaceEditorDialog
        disabled={false}
        onCancel={vi.fn()}
        onSave={onSave}
        workspace={{
          ...base,
          name: "Committed",
          roots: ["/repo", "/shared"],
          revision: 2
        }}
      />
    );
    rejectSave(new Error("History refresh failed"));

    await screen.findByRole("alert");
    expect((screen.getByLabelText("Name") as HTMLInputElement).value).toBe("Committed");
    expect((screen.getByLabelText("Working directory 2") as HTMLInputElement).value).toBe("/shared");
    expect(screen.getByRole("dialog")).toBeTruthy();
  });

  it("traps keyboard focus, closes on idle Escape, and restores the invoker", () => {
    const onCancel = vi.fn();
    const invoker = document.createElement("button");
    document.body.append(invoker);
    invoker.focus();
    const view = render(
      <WorkspaceEditorDialog
        disabled={false}
        onCancel={onCancel}
        onSave={vi.fn(async () => undefined)}
        workspace={{
          id: "workspace-1",
          name: "Workspace",
          roots: ["/repo"],
          sessionIds: [],
          revision: 1,
          cwd: "/repo",
          displayPath: "Workspace",
          hiddenCount: 0,
          nextCursor: null
        }}
      />
    );
    const dialog = screen.getByRole("dialog", { name: "Edit workspace Workspace" });
    expect(dialog.getAttribute("aria-modal")).toBe("true");
    const save = screen.getByRole("button", { name: "Save workspace" });
    save.focus();
    fireEvent.keyDown(save, { key: "Tab" });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Close workspace editor" }));
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(onCancel).toHaveBeenCalledTimes(1);
    view.unmount();
    expect(document.activeElement).toBe(invoker);
    invoker.remove();
  });

  it("keeps idle Escape local while the transport is disconnected", () => {
    const onCancel = vi.fn();
    render(
      <WorkspaceEditorDialog
        disabled
        onCancel={onCancel}
        onSave={vi.fn(async () => undefined)}
        workspace={{
          id: "workspace-1",
          name: "Workspace",
          roots: ["/repo"],
          sessionIds: [],
          revision: 1,
          cwd: "/repo",
          displayPath: "Workspace",
          hiddenCount: 0,
          nextCursor: null
        }}
      />
    );

    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it("retains a modal focus target while every save control is disabled", async () => {
    let finishSave!: (saved: boolean) => void;
    render(
      <WorkspaceEditorDialog
        disabled={false}
        onCancel={vi.fn()}
        onSave={vi.fn(() => new Promise<boolean>((resolve) => {
          finishSave = resolve;
        }))}
        workspace={{
          id: "workspace-1",
          name: "Workspace",
          roots: ["/repo"],
          sessionIds: [],
          revision: 1,
          cwd: "/repo",
          displayPath: "Workspace",
          hiddenCount: 0,
          nextCursor: null
        }}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Save workspace" }));
    const dialog = screen.getByRole("dialog");
    await waitFor(() => expect(document.activeElement).toBe(dialog));
    expect(fireEvent.keyDown(dialog, { key: "Tab" })).toBe(false);
    expect(document.activeElement).toBe(dialog);

    finishSave(false);
    await waitFor(() => expect(screen.getByRole("button", { name: "Save workspace" })).toBeTruthy());
  });
});
