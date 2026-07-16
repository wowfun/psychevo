import type { RightWorkspaceTab, RightWorkspaceTabKind } from "./types";

const UNSUPPORTED_PREVIEW_EXTENSIONS = new Set([
  "7z", "avif", "bin", "bmp", "bz2", "dylib", "exe", "gif", "gz", "ico",
  "jpeg", "jpg", "mov", "mp3", "mp4", "o", "parquet", "pdf", "png", "rar",
  "so", "tar", "tgz", "wasm", "webp", "xz", "zip", "zst"
]);

export function createRightTabId(kind: RightWorkspaceTabKind): string {
  return `${kind}:${Date.now()}:${Math.random().toString(16).slice(2)}`;
}

export function fileBasename(path: string): string {
  const normalized = path.replace(/\\/g, "/").replace(/\/+$/, "");
  return normalized.split("/").pop() || normalized || "workspace";
}

export function isUnsupportedPreviewFile(path: string): boolean {
  const extension = path.split(/[\\/]/).pop()?.split(".").pop()?.toLowerCase();
  return Boolean(extension && UNSUPPORTED_PREVIEW_EXTENSIONS.has(extension));
}

export function rightWorkspaceDefaultTitle(kind: RightWorkspaceTabKind): string {
  return rightWorkspaceTabLabel(kind);
}

export function rightWorkspaceTabLabel(kind: RightWorkspaceTabKind): string {
  switch (kind) {
    case "files": return "Files";
    case "terminal": return "Terminal";
    case "debug": return "Debug";
    case "sideConversation": return "Side chat";
    case "agentSession": return "Agent";
    case "team": return "Team";
    case "browser": return "Browser";
    case "preview": return "Preview";
    case "review":
    default: return "Review";
  }
}

export function rightWorkspaceTabVisibleForSession(
  tab: RightWorkspaceTab,
  sessionId: string | null
): boolean {
  if (tab.kind === "browser") {
    return Boolean(sessionId) && tab.threadId === sessionId;
  }
  if (tab.kind !== "sideConversation" && tab.kind !== "agentSession" && tab.kind !== "team") {
    return true;
  }
  return Boolean(sessionId) && (tab.parentThreadId ?? null) === sessionId;
}
