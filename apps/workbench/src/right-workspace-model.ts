import type { TranscriptPinnedMessage } from "@psychevo/components";
import type { RightWorkspaceTab, RightWorkspaceTabKind } from "./types";

export function createRightTabId(kind: RightWorkspaceTabKind): string {
  return `${kind}:${Date.now()}:${Math.random().toString(16).slice(2)}`;
}

export function fileBasename(path: string): string {
  const normalized = path.replace(/\\/g, "/").replace(/\/+$/, "");
  return normalized.split("/").pop() || normalized || "workspace";
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
    case "pinnedMessage": return "Pinned";
    case "review":
    default: return "Review";
  }
}

export function pinnedMessageTabTitle(message: TranscriptPinnedMessage): string {
  const role = message.role === "user" ? "You" : "Assistant";
  const normalized = message.text.replace(/\s+/g, " ").trim();
  const characters = Array.from(normalized);
  const excerpt = characters.length > 48
    ? `${characters.slice(0, 48).join("")}…`
    : normalized;
  return excerpt ? `${role} · ${excerpt}` : "Pinned message";
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
