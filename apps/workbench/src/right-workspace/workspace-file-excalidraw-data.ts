const EXCALIDRAW_ELEMENT_LIMIT = 5_000;
const EXCALIDRAW_JSON_DEPTH_LIMIT = 40;

const SAFE_ELEMENT_TYPES = new Set([
  "rectangle",
  "diamond",
  "ellipse",
  "line",
  "arrow",
  "freedraw",
  "text",
  "image",
  "frame",
  "magicframe"
]);

const UNSAFE_KEYS = new Set([
  "__proto__",
  "constructor",
  "prototype",
  "link",
  "href",
  "src",
  "url"
]);

const SAFE_IMAGE_DATA_URL = /^data:(image\/(?:png|jpeg|gif|webp|avif|bmp));base64,([A-Za-z0-9+/=\r\n]+)$/i;

type JsonPrimitive = boolean | number | string | null;
type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue };

export type WorkspaceExcalidrawElement = { [key: string]: JsonValue };

export type WorkspaceExcalidrawBinaryFile = {
  created: number;
  dataURL: string;
  id: string;
  lastRetrieved: number;
  mimeType: string;
};

export type WorkspaceExcalidrawScene = {
  appState: {
    exportBackground: boolean;
    exportEmbedScene: false;
    exportWithDarkMode: boolean;
    theme: "dark" | "light";
    viewBackgroundColor: string;
  };
  elements: WorkspaceExcalidrawElement[];
  files: Record<string, WorkspaceExcalidrawBinaryFile>;
};

export function readExcalidrawScene(bytes: Uint8Array): WorkspaceExcalidrawScene {
  const parsed = JSON.parse(new TextDecoder().decode(bytes)) as unknown;
  if (!isRecord(parsed) || !Array.isArray(parsed.elements)) {
    throw new Error("The Excalidraw document has no element list.");
  }
  if (parsed.elements.length > EXCALIDRAW_ELEMENT_LIMIT) {
    throw new Error("Excalidraw preview is limited to 5,000 elements.");
  }

  const files = sanitizeFiles(parsed.files);
  const elements = parsed.elements.flatMap((raw): WorkspaceExcalidrawElement[] => {
    if (!isRecord(raw) || raw.isDeleted === true || typeof raw.type !== "string") {
      return [];
    }
    if (!SAFE_ELEMENT_TYPES.has(raw.type)) {
      return [];
    }
    if (raw.type === "image" && (
      typeof raw.fileId !== "string" || !Object.hasOwn(files, raw.fileId)
    )) {
      return [];
    }
    return [sanitizeObject(raw, 0)];
  });

  const rawAppState = isRecord(parsed.appState) ? parsed.appState : {};
  const theme = rawAppState.theme === "dark" ? "dark" : "light";
  return {
    appState: {
      exportBackground: rawAppState.exportBackground !== false,
      exportEmbedScene: false,
      exportWithDarkMode: theme === "dark",
      theme,
      viewBackgroundColor: safeColor(rawAppState.viewBackgroundColor, "#ffffff")
    },
    elements,
    files
  };
}

function sanitizeFiles(value: unknown): Record<string, WorkspaceExcalidrawBinaryFile> {
  const files: Record<string, WorkspaceExcalidrawBinaryFile> = {};
  if (!isRecord(value)) {
    return files;
  }
  for (const [key, raw] of Object.entries(value)) {
    if (UNSAFE_KEYS.has(key) || !isRecord(raw) || typeof raw.dataURL !== "string") {
      continue;
    }
    const match = SAFE_IMAGE_DATA_URL.exec(raw.dataURL);
    if (!match) {
      continue;
    }
    files[key] = {
      created: finiteNumber(raw.created, 0),
      dataURL: raw.dataURL,
      id: key,
      lastRetrieved: finiteNumber(raw.lastRetrieved, 0),
      mimeType: match[1]!.toLowerCase()
    };
  }
  return files;
}

function sanitizeObject(value: Record<string, unknown>, depth: number): WorkspaceExcalidrawElement {
  const result: WorkspaceExcalidrawElement = {};
  for (const [key, item] of Object.entries(value)) {
    if (UNSAFE_KEYS.has(key)) {
      continue;
    }
    result[key] = sanitizeJsonValue(item, depth + 1);
  }
  return result;
}

function sanitizeJsonValue(value: unknown, depth: number): JsonValue {
  if (depth > EXCALIDRAW_JSON_DEPTH_LIMIT) {
    throw new Error("The Excalidraw document is nested too deeply.");
  }
  if (value === null || typeof value === "string" || typeof value === "boolean") {
    return value;
  }
  if (typeof value === "number") {
    return Number.isFinite(value) ? value : 0;
  }
  if (Array.isArray(value)) {
    return value.map((item) => sanitizeJsonValue(item, depth + 1));
  }
  if (isRecord(value)) {
    const result: { [key: string]: JsonValue } = {};
    for (const [key, item] of Object.entries(value)) {
      if (!UNSAFE_KEYS.has(key)) {
        result[key] = sanitizeJsonValue(item, depth + 1);
      }
    }
    return result;
  }
  return null;
}

function finiteNumber(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function safeColor(value: unknown, fallback: string): string {
  if (typeof value !== "string") return fallback;
  return /^(?:#[0-9a-f]{3,8}|transparent)$/i.test(value) ? value : fallback;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

export const workspaceExcalidrawPolicy = {
  elementLimit: EXCALIDRAW_ELEMENT_LIMIT,
  jsonDepthLimit: EXCALIDRAW_JSON_DEPTH_LIMIT
} as const;
