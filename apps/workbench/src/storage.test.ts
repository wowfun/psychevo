// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  DEFAULT_RIGHT_WIDTH_PX,
  PREFS_APPEARANCE_VERSION,
  PREFS_KEY,
  readWorkbenchPrefs
} from "./storage";

describe("Workbench storage preferences", () => {
  let originalLocalStorage: Storage;
  let localStorageItems: Map<string, string>;

  beforeEach(() => {
    originalLocalStorage = window.localStorage;
    localStorageItems = new Map();
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      value: {
        clear: vi.fn(() => localStorageItems.clear()),
        getItem: vi.fn((key: string) => localStorageItems.get(key) ?? null),
        key: vi.fn((index: number) => Array.from(localStorageItems.keys())[index] ?? null),
        removeItem: vi.fn((key: string) => {
          localStorageItems.delete(key);
        }),
        setItem: vi.fn((key: string, value: string) => {
          localStorageItems.set(key, value);
        }),
        get length() {
          return localStorageItems.size;
        }
      } satisfies Storage
    });
  });

  afterEach(() => {
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      value: originalLocalStorage
    });
    vi.restoreAllMocks();
  });

  it("migrates legacy light appearance preferences to warm", () => {
    window.localStorage.setItem(PREFS_KEY, JSON.stringify({
      appearance: "light",
      debug: true,
      rightWidthPx: 640
    }));

    expect(readWorkbenchPrefs()).toEqual({
      appearance: "warm",
      appearanceVersion: PREFS_APPEARANCE_VERSION,
      debug: true,
      rightWidthPx: 640
    });
  });

  it("keeps versioned light appearance preferences neutral", () => {
    window.localStorage.setItem(PREFS_KEY, JSON.stringify({
      appearance: "light",
      appearanceVersion: PREFS_APPEARANCE_VERSION,
      rightWidthPx: "wide"
    }));

    expect(readWorkbenchPrefs()).toEqual({
      appearance: "light",
      appearanceVersion: PREFS_APPEARANCE_VERSION,
      debug: false,
      rightWidthPx: DEFAULT_RIGHT_WIDTH_PX
    });
  });
});
