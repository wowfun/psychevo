// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ComposerEnvironment } from "./composer-environment";

afterEach(cleanup);

describe("ComposerEnvironment Workspace identity", () => {
  it("passes a known Workspace id instead of degrading the selection to cwd", async () => {
    const onWorkspaceChange = vi.fn(async () => undefined);
    render(
      <ComposerEnvironment
        branch={null}
        branchDisabled={false}
        controlValues={{}}
        controls={[]}
        cwd="/primary"
        disabled={false}
        draft
        isGitRepo={false}
        path="Primary"
        profile={null}
        workspaces={[{ id: "workspace-2", cwd: "/secondary", displayPath: "Product" }]}
        onBranchChange={vi.fn()}
        onOpenFiles={vi.fn()}
        onReadBranches={vi.fn()}
        onReadFolders={vi.fn()}
        onRuntimeControlChange={vi.fn()}
        onWorkspaceChange={onWorkspaceChange}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Workspace" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Product" }));

    await waitFor(() => expect(onWorkspaceChange)
      .toHaveBeenCalledWith("/secondary", "workspace-2"));
  });

  it("keeps direct-cwd and explicit Workspace choices distinct at the same cwd", async () => {
    const onWorkspaceChange = vi.fn(async () => undefined);
    render(
      <ComposerEnvironment
        activeWorkspaceId={null}
        branch={null}
        branchDisabled={false}
        controlValues={{}}
        controls={[]}
        cwd="/primary"
        disabled={false}
        draft
        isGitRepo={false}
        path="Primary"
        profile={null}
        workspaces={[{ id: "workspace-1", cwd: "/primary", displayPath: "Primary" }]}
        onBranchChange={vi.fn()}
        onOpenFiles={vi.fn()}
        onReadBranches={vi.fn()}
        onReadFolders={vi.fn()}
        onRuntimeControlChange={vi.fn()}
        onWorkspaceChange={onWorkspaceChange}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Workspace" }));
    const direct = screen.getByRole("menuitem", { name: "Primary (directory)" });
    const workspace = screen.getByRole("menuitem", { name: "Primary (Workspace)" });
    expect(direct.getAttribute("aria-current")).toBe("true");
    expect(workspace.getAttribute("aria-current")).toBeNull();

    fireEvent.click(workspace);
    await waitFor(() => expect(onWorkspaceChange)
      .toHaveBeenCalledWith("/primary", "workspace-1"));
  });
});
