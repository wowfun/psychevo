import type { HostStorage } from "@psychevo/host";
import type { WorkbenchPrefs } from "./types";

export const PREFS_KEY = "psychevo.workbench.v0.prefs";
export const PINNED_SESSIONS_KEY = "psychevo.workbench.v0.pinnedSessions";
export const DEFAULT_RIGHT_WIDTH_PX = 520;

const MIN_RIGHT_WIDTH_PX = 300;
const MAX_RIGHT_WIDTH_PX = 1200;

export function readWorkbenchPrefs(): WorkbenchPrefs {
  try {
    const raw = window.localStorage.getItem(PREFS_KEY);
    const value = raw ? JSON.parse(raw) as Partial<WorkbenchPrefs> : {};
    return {
      appearance: value.appearance === "light" ? "light" : "dark",
      debug: value.debug === true,
      rightWidthPx: clampRightWidth(value.rightWidthPx)
    };
  } catch {
    return { appearance: "dark", debug: false, rightWidthPx: DEFAULT_RIGHT_WIDTH_PX };
  }
}

export function readPinnedSessionIds(): string[] {
  try {
    const raw = window.localStorage.getItem(PINNED_SESSIONS_KEY);
    return normalizePinnedSessionIds(raw ? JSON.parse(raw) : []);
  } catch {
    return [];
  }
}

export function readPinnedSessionIdsFromStorage(storage: HostStorage): string[] {
  return normalizePinnedSessionIds(storage.getJson(PINNED_SESSIONS_KEY, []));
}

function normalizePinnedSessionIds(value: unknown): string[] {
  return Array.isArray(value)
    ? Array.from(new Set(value.filter((item): item is string => typeof item === "string" && item.trim() !== "")))
    : [];
}

export function clampRightWidth(value: unknown): number {
  const numeric = typeof value === "number" ? value : DEFAULT_RIGHT_WIDTH_PX;
  return Math.max(MIN_RIGHT_WIDTH_PX, Math.min(MAX_RIGHT_WIDTH_PX, Math.round(numeric)));
}
