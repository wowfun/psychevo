const BLOCKED_ELEMENTS = "a, audio, embed, foreignObject, iframe, metadata, object, script, video";
const SAFE_EMBEDDED_URL = /^data:(?:font\/woff2|application\/font-woff|image\/(?:png|jpeg|gif|webp|avif|bmp));base64,/i;

export function scrubExcalidrawSvg(svg: SVGSVGElement, label: string): SVGSVGElement {
  svg.querySelectorAll(BLOCKED_ELEMENTS).forEach((element) => element.remove());
  for (const element of [svg, ...svg.querySelectorAll("*")]) {
    for (const attribute of [...element.attributes]) {
      const name = attribute.name.toLowerCase();
      if (name.startsWith("on")) {
        element.removeAttribute(attribute.name);
        continue;
      }
      if (name === "style") {
        element.setAttribute(attribute.name, scrubCss(attribute.value));
        continue;
      }
      if (name === "href" || name.endsWith(":href")) {
        if (!isSafeReference(attribute.value)) {
          element.removeAttribute(attribute.name);
        }
      }
    }
    if (element.localName === "style") {
      element.textContent = scrubCss(element.textContent ?? "");
    }
  }
  svg.setAttribute("aria-label", label);
  svg.setAttribute("preserveAspectRatio", "xMidYMid meet");
  svg.setAttribute("role", "img");
  svg.removeAttribute("tabindex");
  svg.classList.add("workspaceExcalidrawPreviewGraphic");
  return svg;
}

function scrubCss(css: string): string {
  return css
    .replace(/@import\s+(?:url\()?[^;]+;?/gi, "")
    .replace(/url\(\s*(["']?)(.*?)\1\s*\)/gi, (_match, _quote, rawUrl: string) => (
      isSafeReference(rawUrl) ? `url("${rawUrl}")` : "url(\"\")"
    ));
}

function isSafeReference(value: string): boolean {
  const normalized = value.trim();
  return normalized.startsWith("#") || SAFE_EMBEDDED_URL.test(normalized);
}
