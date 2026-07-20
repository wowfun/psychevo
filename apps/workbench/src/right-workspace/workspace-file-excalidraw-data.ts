const EXCALIDRAW_ELEMENT_LIMIT = 5_000;

export type ExcalidrawElement = {
  id: string;
  type: string;
  x: number;
  y: number;
  width: number;
  height: number;
  angle: number;
  strokeColor: string;
  backgroundColor: string;
  text: string | null;
};

export type ExcalidrawDocument = {
  elements: ExcalidrawElement[];
  viewBox: string;
};

export function readExcalidrawDocument(bytes: Uint8Array): ExcalidrawDocument {
  const parsed = JSON.parse(new TextDecoder().decode(bytes)) as { elements?: unknown };
  if (!Array.isArray(parsed.elements)) {
    throw new Error("The Excalidraw document has no element list.");
  }
  if (parsed.elements.length > EXCALIDRAW_ELEMENT_LIMIT) {
    throw new Error("Excalidraw preview is limited to 5,000 elements.");
  }
  const elements = parsed.elements.flatMap((raw, index): ExcalidrawElement[] => {
    if (!raw || typeof raw !== "object") return [];
    const item = raw as Record<string, unknown>;
    if (item.isDeleted === true || item.type === "image" || item.type === "embeddable") return [];
    const width = finiteNumber(item.width, 0);
    const height = finiteNumber(item.height, 0);
    return [{
      id: typeof item.id === "string" ? item.id : `element-${index}`,
      type: typeof item.type === "string" ? item.type : "rectangle",
      x: finiteNumber(item.x, 0),
      y: finiteNumber(item.y, 0),
      width,
      height,
      angle: finiteNumber(item.angle, 0),
      strokeColor: safeColor(item.strokeColor, "#1f2937"),
      backgroundColor: safeColor(item.backgroundColor, "transparent"),
      text: typeof item.text === "string" ? item.text.slice(0, 10_000) : null
    }];
  });
  const bounds = elements.reduce((value, element) => ({
    minX: Math.min(value.minX, element.x),
    minY: Math.min(value.minY, element.y),
    maxX: Math.max(value.maxX, element.x + Math.max(element.width, 1)),
    maxY: Math.max(value.maxY, element.y + Math.max(element.height, 1))
  }), { minX: 0, minY: 0, maxX: 640, maxY: 360 });
  const padding = 24;
  return {
    elements,
    viewBox: [
      bounds.minX - padding,
      bounds.minY - padding,
      Math.max(1, bounds.maxX - bounds.minX + padding * 2),
      Math.max(1, bounds.maxY - bounds.minY + padding * 2)
    ].join(" ")
  };
}

function finiteNumber(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function safeColor(value: unknown, fallback: string): string {
  if (typeof value !== "string") return fallback;
  return /^(?:#[0-9a-f]{3,8}|transparent)$/i.test(value) ? value : fallback;
}

export const workspaceExcalidrawPolicy = { elementLimit: EXCALIDRAW_ELEMENT_LIMIT } as const;
