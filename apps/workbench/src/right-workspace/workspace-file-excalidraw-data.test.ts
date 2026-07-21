import { describe, expect, it } from "vitest";
import {
  readExcalidrawScene,
  workspaceExcalidrawPolicy
} from "./workspace-file-excalidraw-data";

describe("Excalidraw security projection", () => {
  it("keeps supported scene structure while removing active and external content", () => {
    const scene = readExcalidrawScene(encode({
      appState: {
        exportBackground: true,
        theme: "dark",
        viewBackgroundColor: "#101827"
      },
      elements: [
        {
          customData: { url: "https://preview-security.invalid/custom" },
          id: "box",
          link: "https://preview-security.invalid/link",
          type: "rectangle"
        },
        { id: "arrow", points: [[0, 0], [40, 20]], type: "arrow" },
        { id: "stroke", points: [[0, 0], [10, 10]], type: "freedraw" },
        { fileId: "safe-image", id: "image", type: "image" },
        { fileId: "remote-image", id: "remote", type: "image" },
        {
          id: "embed",
          link: "https://preview-security.invalid/embed",
          type: "embeddable"
        },
        {
          customData: { generationData: { html: "<script>bad()</script>" } },
          id: "iframe",
          type: "iframe"
        }
      ],
      files: {
        "safe-image": {
          created: 7,
          dataURL: "data:image/png;base64,iVBORw0KGgo=",
          id: "untrusted-id",
          lastRetrieved: 9,
          mimeType: "text/html"
        },
        "remote-image": {
          dataURL: "https://preview-security.invalid/image.png"
        }
      }
    }));

    expect(scene.elements.map((element) => element.type)).toEqual([
      "rectangle", "arrow", "freedraw", "image"
    ]);
    expect(scene.elements[0]).not.toHaveProperty("link");
    expect(scene.elements[0]?.customData).toEqual({});
    expect(scene.files).toEqual({
      "safe-image": {
        created: 7,
        dataURL: "data:image/png;base64,iVBORw0KGgo=",
        id: "safe-image",
        lastRetrieved: 9,
        mimeType: "image/png"
      }
    });
    expect(scene.appState).toMatchObject({
      exportEmbedScene: false,
      exportWithDarkMode: true,
      theme: "dark",
      viewBackgroundColor: "#101827"
    });
  });

  it("enforces the element limit before projection", () => {
    const elements = Array.from(
      { length: workspaceExcalidrawPolicy.elementLimit + 1 },
      (_, index) => ({ id: `element-${index}`, type: "rectangle" })
    );
    expect(() => readExcalidrawScene(encode({ elements }))).toThrow(
      "Excalidraw preview is limited to 5,000 elements."
    );
  });
});

function encode(value: unknown): Uint8Array {
  return new TextEncoder().encode(JSON.stringify(value));
}
