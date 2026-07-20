import {
  presentationRendererDefinition,
  renderFileViewerPresentation
} from "@file-viewer/renderer-presentation";
import type { WorkspaceFileRendererPlugin } from "./workspace-file-renderers";

export const modernPresentationRenderer: WorkspaceFileRendererPlugin = {
  id: "psychevo-office-presentation",
  definitions: [presentationRendererDefinition],
  handlers: [{
    rendererId: presentationRendererDefinition.id,
    handler: renderFileViewerPresentation
  }]
};
