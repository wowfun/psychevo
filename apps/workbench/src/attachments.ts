import type { PendingAttachment } from "./types";

const MAX_TEXT_ATTACHMENT_BYTES = 256 * 1024;
const MAX_IMAGE_ATTACHMENT_BYTES = 6 * 1024 * 1024;

export async function attachmentFromFile(file: File): Promise<PendingAttachment> {
  const id = `${Date.now()}:${file.name}:${file.size}:${Math.random().toString(16).slice(2)}`;
  const sizeLabel = formatBytes(file.size);
  if (file.type.startsWith("image/")) {
    if (file.size > MAX_IMAGE_ATTACHMENT_BYTES) {
      throw new Error(`Image attachment is too large: ${file.name} (${sizeLabel})`);
    }
    const dataUrl = await fileToDataUrl(file);
    return {
      id,
      input: { type: "image", input: { kind: "url", url: dataUrl } },
      kind: "image",
      name: file.name || "image",
      previewUrl: dataUrl,
      size: file.size,
      sizeLabel
    };
  }

  if (isTextLikeFile(file)) {
    const truncated = file.size > MAX_TEXT_ATTACHMENT_BYTES;
    const text = await file.slice(0, MAX_TEXT_ATTACHMENT_BYTES).text();
    return {
      id,
      input: {
        type: "context",
        label: `Attachment: ${file.name || "file"}`,
        text: [
          `Attached text file: ${file.name || "file"}`,
          `MIME: ${file.type || "unknown"}`,
          `Size: ${sizeLabel}`,
          truncated ? `Content is truncated to ${formatBytes(MAX_TEXT_ATTACHMENT_BYTES)}.` : "",
          "",
          text
        ].filter(Boolean).join("\n"),
        visibleToModel: true
      },
      kind: "text",
      name: file.name || "file",
      size: file.size,
      sizeLabel
    };
  }

  const blob = await fileToBase64(file);
  return {
    id,
    input: {
      type: "resource",
      uri: `attachment://${encodeURIComponent(file.name || "file")}`,
      mimeType: file.type || "application/octet-stream",
      text: null,
      blob
    },
    kind: "file",
    name: file.name || "file",
    size: file.size,
    sizeLabel
  };
}

async function fileToBase64(file: File): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return btoa(binary);
}

function fileToDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.addEventListener("load", () => resolve(String(reader.result ?? "")), { once: true });
    reader.addEventListener("error", () => reject(reader.error ?? new Error("failed to read file")), { once: true });
    reader.readAsDataURL(file);
  });
}

function isTextLikeFile(file: File): boolean {
  if (file.type.startsWith("text/")) {
    return true;
  }
  const name = file.name.toLowerCase();
  return [
    ".css",
    ".csv",
    ".html",
    ".js",
    ".json",
    ".jsx",
    ".md",
    ".py",
    ".rs",
    ".toml",
    ".ts",
    ".tsx",
    ".txt",
    ".xml",
    ".yaml",
    ".yml"
  ].some((extension) => name.endsWith(extension));
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  const kib = bytes / 1024;
  if (kib < 1024) {
    return `${Math.round(kib * 10) / 10} KiB`;
  }
  const mib = kib / 1024;
  return `${Math.round(mib * 10) / 10} MiB`;
}
