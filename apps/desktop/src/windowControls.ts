import type { FloatingRuntime } from "@psychevo/floating";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

type FloatingWindowControls = Pick<FloatingRuntime, "closeFloatingWindow" | "fitWindowToContent" | "startWindowDrag">;

const FLOATING_MIN_SIZE = new LogicalSize(320, 48);

export function createDesktopFloatingWindowControls(): FloatingWindowControls {
  return {
    async closeFloatingWindow() {
      if (!tauriWindowBridgeAvailable()) {
        return;
      }
      await getCurrentWindow().hide();
    },
    async fitWindowToContent(size) {
      if (!tauriWindowBridgeAvailable()) {
        return;
      }
      const currentWindow = getCurrentWindow();
      await currentWindow.setMinSize(FLOATING_MIN_SIZE);
      await currentWindow.setSize(new LogicalSize(
        Math.max(FLOATING_MIN_SIZE.width, size.width),
        Math.max(FLOATING_MIN_SIZE.height, size.height)
      ));
    },
    async startWindowDrag() {
      if (!tauriWindowBridgeAvailable()) {
        return;
      }
      await getCurrentWindow().startDragging();
    }
  };
}

function tauriWindowBridgeAvailable(): boolean {
  return typeof window !== "undefined"
    && typeof (window as { __TAURI_INTERNALS__?: { invoke?: unknown } }).__TAURI_INTERNALS__?.invoke === "function";
}
