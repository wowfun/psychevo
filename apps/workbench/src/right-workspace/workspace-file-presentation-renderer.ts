import {
  DEFAULT_RENDERER_DEFINITIONS
} from "@file-viewer/core";
import renderPptx from "@file-viewer/renderer-presentation/pptx";
import type { WorkspaceFileRendererPlugin } from "./workspace-file-renderers";

const presentationRendererDefinition = DEFAULT_RENDERER_DEFINITIONS.find(
  (definition) => definition.id === "office-presentation"
);
if (!presentationRendererDefinition) {
  throw new Error("File Viewer presentation renderer definition is unavailable.");
}

export const modernPresentationRenderer: WorkspaceFileRendererPlugin = {
  id: "psychevo-office-presentation",
  definitions: [presentationRendererDefinition],
  handlers: [{
    rendererId: presentationRendererDefinition.id,
    handler: renderPptx
  }]
};
