import type {
  WorkspaceExternalFileAction,
  WorkspaceExternalFileCategory,
  WorkspaceExternalHostPlatform,
  WorkspaceFileExternalActionsResult
} from "@psychevo/protocol";
import type { WorkspaceFileMenuItem } from "./file-context-menu";

export function workspaceExternalActionMenuItems(
  result: WorkspaceFileExternalActionsResult,
  disabled = false
): WorkspaceFileMenuItem<WorkspaceExternalFileAction>[] {
  return result.availableActions.map((action) => ({
    disabled,
    id: action,
    label: workspaceExternalActionLabel(action, result.category, result.platform),
    separatorBefore: action === "reveal"
  }));
}

export function workspaceExternalActionLabel(
  action: WorkspaceExternalFileAction,
  category: WorkspaceExternalFileCategory,
  platform: WorkspaceExternalHostPlatform
): string {
  if (action === "vscode") {
    return "Open in VS Code";
  }
  if (action === "reveal") {
    if (platform === "macos") {
      return "Show in Finder";
    }
    if (platform === "windows") {
      return "Show in File Explorer";
    }
    return "Show in File Manager";
  }
  switch (category) {
    case "webpage":
      return "Open in Default Browser";
    case "image":
      return "Open in Default Image Viewer";
    case "media":
      return "Open in Default Player";
    case "pdf":
      return "Open in Default PDF Reader";
    case "office":
      return "Open in Default Office Application";
    case "other":
    case "text":
      return "Open with Default Application";
  }
}
