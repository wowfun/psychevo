import { exportToSvg, restore } from "@excalidraw/excalidraw";
import type { WorkspaceExcalidrawScene } from "./workspace-file-excalidraw-data";
import { scrubExcalidrawSvg } from "./workspace-file-excalidraw-svg";

export async function renderExcalidrawScene(
  scene: WorkspaceExcalidrawScene,
  path: string
): Promise<SVGSVGElement> {
  const restored = restore(
    scene as unknown as Parameters<typeof restore>[0],
    null,
    null,
    { refreshDimensions: true, repairBindings: true }
  );
  const elements = restored.elements.filter((element) => !element.isDeleted);
  const svg = await exportToSvg({
    appState: {
      ...restored.appState,
      exportBackground: scene.appState.exportBackground,
      exportEmbedScene: false,
      exportWithDarkMode: scene.appState.exportWithDarkMode,
      viewBackgroundColor: scene.appState.viewBackgroundColor
    },
    elements,
    exportPadding: 24,
    files: restored.files,
    renderEmbeddables: false
  });
  return scrubExcalidrawSvg(svg, `Preview ${path}`);
}
