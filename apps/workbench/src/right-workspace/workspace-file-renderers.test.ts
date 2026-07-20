import { describe, expect, it } from "vitest";
import { workspaceFileRendererFamily } from "./workspace-file-renderers";

describe("workspace file renderer catalog", () => {
  it("routes only the accepted browser renderer families", () => {
    expect(workspaceFileRendererFamily("report.pdf")).toBe("pdf");
    expect(workspaceFileRendererFamily("brief.docm")).toBe("word-openxml");
    expect(workspaceFileRendererFamily("notes.rtf")).toBe("open-document");
    expect(workspaceFileRendererFamily("slides.odp")).toBe("open-document");
    expect(workspaceFileRendererFamily("sheet.xlsb")).toBe("spreadsheet");
    expect(workspaceFileRendererFamily("deck.ppsm")).toBe("presentation");
    expect(workspaceFileRendererFamily("invoice.ofd")).toBe("ofd");
    expect(workspaceFileRendererFamily("photo.heic")).toBe("image");
  });

  it("does not route explicitly unsupported legacy Office files", () => {
    expect(workspaceFileRendererFamily("legacy.doc")).toBeNull();
    expect(workspaceFileRendererFamily("legacy.xls")).toBeNull();
    expect(workspaceFileRendererFamily("legacy.ppt")).toBeNull();
  });
});
