import type { SessionBrowserWorkspaceState } from "./types";

export function resolveWorkspaceFileRoots({
  draftWorkspaceId,
  scopeCwd,
  threadId,
  threadWorkspaceRoots,
  workspaces
}: {
  draftWorkspaceId: string | null;
  scopeCwd: string;
  threadId: string | null;
  threadWorkspaceRoots: string[] | undefined;
  workspaces: SessionBrowserWorkspaceState[];
}): string[] {
  if (threadId) {
    return threadWorkspaceRoots?.length ? threadWorkspaceRoots : [scopeCwd];
  }
  if (draftWorkspaceId) {
    if (threadWorkspaceRoots?.length) return threadWorkspaceRoots;
    const workspace = workspaces.find((candidate) => candidate.id === draftWorkspaceId);
    if (workspace?.roots.length) return workspace.roots;
  }
  return [scopeCwd];
}
