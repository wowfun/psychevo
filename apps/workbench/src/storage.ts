import type { WorkbenchPrefs } from "./types";

export const PREFS_KEY = "psychevo.workbench.v0.prefs";
export const DEFAULT_RIGHT_WIDTH_PX = 520;
export const PREFS_APPEARANCE_VERSION = 1;

const MIN_RIGHT_WIDTH_PX = 300;
const MAX_RIGHT_WIDTH_PX = 1200;

export function readWorkbenchPrefs(): WorkbenchPrefs {
  try {
    const raw = window.localStorage.getItem(PREFS_KEY);
    const value = raw ? JSON.parse(raw) as Partial<WorkbenchPrefs> : {};
    return {
      appearance: normalizeAppearance(value.appearance, value.appearanceVersion),
      appearanceVersion: PREFS_APPEARANCE_VERSION,
      debug: value.debug === true,
      rightWidthPx: clampRightWidth(value.rightWidthPx)
    };
  } catch {
    return defaultWorkbenchPrefs();
  }
}

export function clampRightWidth(value: unknown): number {
  const numeric = typeof value === "number" ? value : DEFAULT_RIGHT_WIDTH_PX;
  return Math.max(MIN_RIGHT_WIDTH_PX, Math.min(MAX_RIGHT_WIDTH_PX, Math.round(numeric)));
}

function defaultWorkbenchPrefs(): WorkbenchPrefs {
  return {
    appearance: "dark",
    appearanceVersion: PREFS_APPEARANCE_VERSION,
    debug: false,
    rightWidthPx: DEFAULT_RIGHT_WIDTH_PX
  };
}

function normalizeAppearance(value: unknown, appearanceVersion: unknown): WorkbenchPrefs["appearance"] {
  if (value === "dark" || value === "warm") {
    return value;
  }
  if (value === "light") {
    return appearanceVersion === PREFS_APPEARANCE_VERSION ? "light" : "warm";
  }
  return "dark";
}
