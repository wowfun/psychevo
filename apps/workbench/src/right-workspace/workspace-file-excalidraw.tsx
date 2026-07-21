import { useEffect, useRef } from "react";
import type { WorkspaceExcalidrawScene } from "./workspace-file-excalidraw-data";

export type { WorkspaceExcalidrawScene } from "./workspace-file-excalidraw-data";
export {
  readExcalidrawScene,
  workspaceExcalidrawPolicy
} from "./workspace-file-excalidraw-data";

export function ExcalidrawPreview({
  active,
  onStateChange,
  path,
  scene
}: {
  active: boolean;
  onStateChange(ready: boolean, error: unknown | null): void;
  path: string;
  scene: WorkspaceExcalidrawScene;
}) {
  const hostRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    hostRef.current?.replaceChildren();
  }, [path, scene]);

  useEffect(() => {
    if (!active) {
      return;
    }
    let stale = false;
    configureExcalidrawAssetPath();
    onStateChange(false, null);
    void import("./workspace-file-excalidraw-renderer").then(
      ({ renderExcalidrawScene }) => renderExcalidrawScene(scene, path),
      (error) => Promise.reject(error)
    ).then(
      (svg) => {
        if (stale) {
          return;
        }
        hostRef.current?.replaceChildren(svg);
        onStateChange(true, null);
      },
      (error) => {
        if (!stale) {
          onStateChange(false, error);
        }
      }
    );
    return () => {
      stale = true;
    };
  }, [active, onStateChange, path, scene]);

  useEffect(() => () => {
    hostRef.current?.replaceChildren();
  }, []);

  return <div className="workspaceExcalidrawPreview" ref={hostRef} />;
}

function configureExcalidrawAssetPath() {
  const assetUrl = new URL("/excalidraw/", window.location.href).href;
  (window as typeof window & { EXCALIDRAW_ASSET_PATH?: string }).EXCALIDRAW_ASSET_PATH = assetUrl;
}
