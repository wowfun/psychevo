import JSZip from "jszip";

const ZIP_ENTRY_LIMIT = 5_000;

export type ZipDirectoryEntry = {
  directory: boolean;
  path: string;
};

export async function readZipDirectory(
  bytes: Uint8Array,
  signal: AbortSignal
): Promise<ZipDirectoryEntry[]> {
  if (signal.aborted) {
    throw new DOMException("Aborted", "AbortError");
  }
  const archive = await JSZip.loadAsync(bytes, { createFolders: true });
  if (signal.aborted) {
    throw new DOMException("Aborted", "AbortError");
  }
  const entries = Object.values(archive.files);
  if (entries.length > ZIP_ENTRY_LIMIT) {
    throw new Error("ZIP preview is limited to 5,000 directory entries.");
  }
  return entries
    .map((entry) => ({
      directory: entry.dir,
      path: sanitizeZipPath(entry.name)
    }))
    .filter((entry) => entry.path.length > 0)
    .sort((left, right) => left.path.localeCompare(right.path));
}

function sanitizeZipPath(path: string): string {
  const segments: string[] = [];
  const normalized = path.replace(/\\/g, "/").replace(/^[A-Za-z]:/, "");
  for (const segment of normalized.split("/")) {
    if (!segment || segment === ".") {
      continue;
    }
    if (segment === "..") {
      segments.pop();
      continue;
    }
    segments.push(segment.replace(/[\u0000-\u001f\u007f]/g, "�"));
  }
  return segments.join("/");
}

export const workspaceZipPolicy = { entryLimit: ZIP_ENTRY_LIMIT } as const;
