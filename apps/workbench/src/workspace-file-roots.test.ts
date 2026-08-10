import { describe, expect, it } from "vitest";
import { resolveWorkspaceFileRoots } from "./workspace-file-roots";

const containingWorkspace = [{
  id: "workspace-1",
  name: "Catalog group",
  roots: ["/primary", "/secondary"],
  sessionIds: ["thread-direct"],
  revision: 1,
  cwd: "/primary",
  displayPath: "Catalog group",
  hiddenCount: 0,
  nextCursor: null
}];

describe("resolveWorkspaceFileRoots", () => {
  it("keeps direct-cwd Threads cwd-only even when navigation groups them into a Workspace", () => {
    expect(resolveWorkspaceFileRoots({
      draftWorkspaceId: null,
      scopeCwd: "/primary",
      threadId: "thread-direct",
      threadWorkspaceRoots: ["/primary"],
      workspaces: containingWorkspace
    })).toEqual(["/primary"]);
  });

  it("uses the authoritative Thread roots for an explicit Workspace Thread", () => {
    expect(resolveWorkspaceFileRoots({
      draftWorkspaceId: null,
      scopeCwd: "/primary",
      threadId: "thread-workspace",
      threadWorkspaceRoots: ["/primary", "/secondary"],
      workspaces: containingWorkspace
    })).toEqual(["/primary", "/secondary"]);
  });

  it("uses roots returned by explicit draft open instead of a stale catalog", () => {
    expect(resolveWorkspaceFileRoots({
      draftWorkspaceId: "workspace-1",
      scopeCwd: "/new-primary",
      threadId: null,
      threadWorkspaceRoots: ["/new-primary", "/new-secondary"],
      workspaces: containingWorkspace
    })).toEqual(["/new-primary", "/new-secondary"]);
  });
});
