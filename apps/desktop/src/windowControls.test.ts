import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createDesktopFloatingWindowControls } from "./windowControls";

const tauriWindow = vi.hoisted(() => ({
  getCurrentWindow: vi.fn(),
  hide: vi.fn(),
  setMinSize: vi.fn(),
  setSize: vi.fn(),
  startDragging: vi.fn()
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: tauriWindow.getCurrentWindow,
  LogicalSize: class LogicalSize {
    constructor(
      public readonly width: number,
      public readonly height: number
    ) {}
  }
}));

beforeEach(() => {
  tauriWindow.getCurrentWindow.mockReturnValue({
    hide: tauriWindow.hide,
    setMinSize: tauriWindow.setMinSize,
    setSize: tauriWindow.setSize,
    startDragging: tauriWindow.startDragging
  });
});

afterEach(() => {
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

describe("createDesktopFloatingWindowControls", () => {
  it("does nothing outside a Tauri window bridge", async () => {
    const controls = createDesktopFloatingWindowControls();

    await controls.fitWindowToContent?.({ height: 120, width: 780 });
    await controls.startWindowDrag?.();
    await controls.closeFloatingWindow?.();

    expect(tauriWindow.getCurrentWindow).not.toHaveBeenCalled();
  });

  it("forwards fit and drag requests to the current Tauri window", async () => {
    vi.stubGlobal("window", {
      __TAURI_INTERNALS__: { invoke: vi.fn() }
    });
    const controls = createDesktopFloatingWindowControls();

    await controls.fitWindowToContent?.({ height: 120, width: 780 });
    await controls.startWindowDrag?.();
    await controls.closeFloatingWindow?.();

    expect(tauriWindow.setMinSize).toHaveBeenCalledWith({ height: 48, width: 320 });
    expect(tauriWindow.setSize).toHaveBeenCalledWith({ height: 120, width: 780 });
    expect(tauriWindow.startDragging).toHaveBeenCalledOnce();
    expect(tauriWindow.hide).toHaveBeenCalledOnce();
  });
});
