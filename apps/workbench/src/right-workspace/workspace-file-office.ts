import JSZip from "jszip";

const OFFICE_ENTRY_LIMIT = 5_000;
const OFFICE_UNCOMPRESSED_LIMIT_BYTES = 128 * 1024 * 1024;
const OFFICE_OUTPUT_LIMIT_BYTES = 32 * 1024 * 1024;
const ZIP_OFFICE_EXTENSIONS = new Set([
  "docx", "docm", "dotx", "dotm",
  "xlsx", "xlsm", "xlsb", "xltx", "xltm",
  "pptx", "pptm", "potx", "potm", "ppsx", "ppsm",
  "odt", "ods", "odp", "ofd"
]);

export async function sanitizeOfficePreview(
  bytes: Uint8Array,
  filename: string,
  signal: AbortSignal
): Promise<Uint8Array> {
  throwIfAborted(signal);
  const extension = filename.split(/[\\/]/).pop()?.split(".").pop()?.toLowerCase() ?? "";
  if (extension === "rtf") {
    ensureOfficeOutputLimit(bytes);
    return ensureOfficeOutputLimit(sanitizeRtf(bytes));
  }
  if (!ZIP_OFFICE_EXTENSIONS.has(extension)) {
    return bytes;
  }

  const archive = await JSZip.loadAsync(bytes, { createFolders: true });
  throwIfAborted(signal);
  const entries = Object.values(archive.files);
  if (entries.length > OFFICE_ENTRY_LIMIT) {
    throw new Error("Office preview is limited to 5,000 package entries.");
  }
  const uncompressedBytes = entries.reduce((total, entry) => {
    const metadata = entry as unknown as { _data?: { uncompressedSize?: number } };
    return total + Math.max(0, metadata._data?.uncompressedSize ?? 0);
  }, 0);
  if (uncompressedBytes > OFFICE_UNCOMPRESSED_LIMIT_BYTES) {
    throw new Error("Office preview package expands beyond the 128 MiB safety limit.");
  }

  let mimeTypeEntry: string | null = null;
  for (const entry of entries) {
    throwIfAborted(signal);
    if (entry.dir) {
      continue;
    }
    if (isExecutableOfficePart(entry.name)) {
      archive.remove(entry.name);
      continue;
    }
    if (entry.name === "mimetype") {
      mimeTypeEntry = await entry.async("text");
      continue;
    }
    if (/\.rels$/i.test(entry.name)) {
      const xml = await entry.async("text");
      archive.file(entry.name, sanitizeRelationships(xml));
      continue;
    }
    if (/\.xml$/i.test(entry.name)) {
      const xml = await entry.async("text");
      archive.file(entry.name, sanitizeExternalXmlAttributes(xml));
    }
  }
  if (mimeTypeEntry !== null) {
    archive.file("mimetype", mimeTypeEntry, { compression: "STORE" });
  }
  throwIfAborted(signal);
  const output = await archive.generateAsync({
    type: "uint8array",
    compression: "DEFLATE",
    compressionOptions: { level: 6 },
    streamFiles: false
  });
  throwIfAborted(signal);
  return ensureOfficeOutputLimit(output);
}

function sanitizeRelationships(xml: string): string {
  return sanitizeExternalXmlAttributes(xml).replace(
    /<(?:[A-Za-z_][\w.-]*:)?Relationship\b(?:"[^"]*"|'[^']*'|[^>])*\/?\s*>/gi,
    (relationship) => {
      const targetMode = xmlAttribute(relationship, "TargetMode");
      const target = xmlAttribute(relationship, "Target");
      return targetMode?.toLowerCase() === "external"
        || (target !== null && (isNetworkTarget(target) || isExecutableOfficePart(target)))
        ? ""
        : relationship;
    }
  );
}

function sanitizeExternalXmlAttributes(xml: string): string {
  return xml.replace(
    /(\b(?:xlink:href|href|src)\s*=\s*)(["'])(.*?)\2/gi,
    (attribute, prefix: string, quote: string, target: string) => (
      isNetworkTarget(target) ? `${prefix}${quote}${quote}` : attribute
    )
  );
}

function sanitizeRtf(bytes: Uint8Array): Uint8Array {
  const source = bytesToLatin1(bytes);
  const sanitized = source
    .replace(/https?:\/\/[^\\}\s]+/gi, "")
    .replace(/\\(?:includePicture|hyperlink)\b[^\\}]+/gi, "")
    .replace(/\{\\object\b(?:[^{}]|\{[^{}]*\})*\}/gi, "");
  return latin1ToBytes(sanitized);
}

function bytesToLatin1(bytes: Uint8Array): string {
  const chunks: string[] = [];
  for (let offset = 0; offset < bytes.byteLength; offset += 32_768) {
    chunks.push(String.fromCharCode(...bytes.subarray(offset, offset + 32_768)));
  }
  return chunks.join("");
}

function latin1ToBytes(source: string): Uint8Array {
  const bytes = new Uint8Array(source.length);
  for (let index = 0; index < source.length; index += 1) {
    bytes[index] = source.charCodeAt(index);
  }
  return bytes;
}

function ensureOfficeOutputLimit(bytes: Uint8Array): Uint8Array {
  if (bytes.byteLength > OFFICE_OUTPUT_LIMIT_BYTES) {
    throw new Error("Sanitized Office preview exceeds the 32 MiB limit.");
  }
  return bytes;
}

function xmlAttribute(element: string, name: string): string | null {
  const match = element.match(new RegExp(`\\b${name}\\s*=\\s*(["'])(.*?)\\1`, "i"));
  return match?.[2] === undefined ? null : decodeXmlEntities(match[2]);
}

function isNetworkTarget(rawTarget: string): boolean {
  const target = decodeXmlEntities(rawTarget)
    .replace(/[\u0000-\u0020\u007f]+/g, "")
    .toLowerCase();
  return /^[a-z][a-z0-9+.-]*:/.test(target) || /^[/\\]{2}/.test(target);
}

function decodeXmlEntities(value: string): string {
  return value
    .replace(/&#x([0-9a-f]+);/gi, (_entity, digits: string) => safeCodePoint(digits, 16))
    .replace(/&#([0-9]+);/g, (_entity, digits: string) => safeCodePoint(digits, 10))
    .replace(/&(amp|quot|apos|lt|gt);/gi, (_entity, name: string) => ({
      amp: "&",
      apos: "'",
      gt: ">",
      lt: "<",
      quot: '"'
    })[name.toLowerCase()] ?? "");
}

function safeCodePoint(value: string, radix: number): string {
  const codePoint = Number.parseInt(value, radix);
  return Number.isInteger(codePoint) && codePoint >= 0 && codePoint <= 0x10ffff
    ? String.fromCodePoint(codePoint)
    : "�";
}

function isExecutableOfficePart(rawPath: string): boolean {
  const path = rawPath.replace(/\\/g, "/").toLowerCase();
  return /(?:^|\/)vbaproject\.bin$/.test(path)
    || /(?:^|\/)vbadata\.xml$/.test(path)
    || /(?:^|\/)(?:activex|embeddings|macros|scripts|basic)(?:\/|$)/.test(path)
    || /(?:^|\/)oleobject[^/]*$/.test(path);
}

function throwIfAborted(signal: AbortSignal) {
  if (signal.aborted) {
    throw new DOMException("Aborted", "AbortError");
  }
}

export const workspaceOfficePreviewPolicy = {
  entryLimit: OFFICE_ENTRY_LIMIT,
  uncompressedLimitBytes: OFFICE_UNCOMPRESSED_LIMIT_BYTES
} as const;
