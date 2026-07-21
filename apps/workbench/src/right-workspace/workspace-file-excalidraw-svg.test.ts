// @vitest-environment jsdom

import { describe, expect, it } from "vitest";
import { scrubExcalidrawSvg } from "./workspace-file-excalidraw-svg";

describe("Excalidraw exported SVG scrubber", () => {
  it("keeps inert drawing data and removes executable or external references", () => {
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.innerHTML = [
      '<script>window.bad = true</script>',
      '<foreignObject><iframe src="https://preview-security.invalid/frame"></iframe></foreignObject>',
      '<a href="https://preview-security.invalid/link"><text>linked</text></a>',
      '<image id="safe" href="data:image/png;base64,iVBORw0KGgo="/>',
      '<image id="remote" href="https://preview-security.invalid/image.png"/>',
      '<rect id="event" onclick="bad()" style="fill:url(https://preview-security.invalid/fill.svg)"/>',
      '<style>@import "https://preview-security.invalid/theme.css"; @font-face{src:url(data:font/woff2;base64,d09GMg==)}</style>'
    ].join("");

    const result = scrubExcalidrawSvg(svg, "Preview safe.excalidraw");

    expect(result.querySelector("script, foreignObject, iframe, a")).toBeNull();
    expect(result.querySelector("#safe")?.getAttribute("href")).toMatch(/^data:image\/png/);
    expect(result.querySelector("#remote")?.hasAttribute("href")).toBe(false);
    expect(result.querySelector("#event")?.hasAttribute("onclick")).toBe(false);
    expect(result.innerHTML).not.toContain("preview-security.invalid");
    expect(result.querySelector("style")?.textContent).toContain("data:font/woff2");
    expect(result.getAttribute("aria-label")).toBe("Preview safe.excalidraw");
    expect(result.getAttribute("role")).toBe("img");
  });
});
