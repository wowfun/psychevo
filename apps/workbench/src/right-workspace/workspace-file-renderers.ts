import {
  setDefaultFileViewerAssetBaseUrl,
  type FileRenderHandler,
  type FileViewerRenderedInstance,
  type FileViewerRendererPlugin
} from "@file-viewer/core";

const WORD_OPENXML_EXTENSIONS = new Set(["docx", "docm", "dotx", "dotm"]);
const SPREADSHEET_EXTENSIONS = new Set([
  "xlsx", "xlsm", "xlsb", "xltx", "xltm", "ods"
]);
const PRESENTATION_EXTENSIONS = new Set([
  "pptx", "pptm", "potx", "potm", "ppsx", "ppsm"
]);

export type WorkspaceFileRendererFamily =
  | "pdf"
  | "word-openxml"
  | "open-document"
  | "spreadsheet"
  | "presentation"
  | "ofd"
  | "image";

export type WorkspaceFileRendererPlugin = FileViewerRendererPlugin<
  FileRenderHandler<FileViewerRenderedInstance, HTMLDivElement>
>;

export function workspaceFileRendererFamily(
  filename: string
): WorkspaceFileRendererFamily | null {
  const extension = fileExtension(filename);
  if (extension === "pdf") return "pdf";
  if (WORD_OPENXML_EXTENSIONS.has(extension)) return "word-openxml";
  if (extension === "rtf" || extension === "odt" || extension === "odp") {
    return "open-document";
  }
  if (SPREADSHEET_EXTENSIONS.has(extension)) return "spreadsheet";
  if (PRESENTATION_EXTENSIONS.has(extension)) return "presentation";
  if (extension === "ofd") return "ofd";
  if (extension === "heic" || extension === "heif") return "image";
  return null;
}

export async function loadWorkspaceFileRenderers(
  filename: string
): Promise<WorkspaceFileRendererPlugin[]> {
  setDefaultFileViewerAssetBaseUrl("/file-viewer/");
  const family = workspaceFileRendererFamily(filename);
  switch (family) {
    case "pdf": {
      const module = await import("@file-viewer/renderer-pdf");
      return [module.pdfRenderer];
    }
    case "word-openxml":
    case "open-document": {
      const module = await import("@file-viewer/renderer-word");
      const rendererId = family === "word-openxml"
        ? "office-word-openxml"
        : "open-document";
      const definition = module.wordRendererDefinitions.find(
        (candidate) => candidate.id === rendererId
      );
      if (!definition) {
        throw new Error(`Word renderer definition ${rendererId} is unavailable.`);
      }
      const handler = family === "word-openxml"
        ? module.renderFileViewerWordDocx
        : module.renderFileViewerOpenDocument;
      return [{
        id: `psychevo-${rendererId}`,
        definitions: [definition],
        handlers: [{ rendererId, handler }]
      }];
    }
    case "spreadsheet": {
      const module = await import("@file-viewer/renderer-spreadsheet");
      return [module.spreadsheetRenderer];
    }
    case "presentation": {
      const module = await import("./workspace-file-presentation-renderer");
      return [module.modernPresentationRenderer];
    }
    case "ofd": {
      const module = await import("@file-viewer/renderer-ofd");
      return [module.ofdRenderer];
    }
    case "image": {
      const module = await import("@file-viewer/renderer-image");
      return [module.imageRenderer];
    }
    case null:
      throw new Error("No document renderer is registered for this file type.");
  }
}

export function fileExtension(filename: string): string {
  return filename.split(/[\\/]/).pop()?.split(".").pop()?.toLowerCase() ?? "";
}
