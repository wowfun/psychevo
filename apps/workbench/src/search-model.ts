import type { ThreadSnapshot } from "@psychevo/protocol";
import { asRecord, stringField } from "./data";

export function transcriptSearchText(entries: ThreadSnapshot["entries"]): string {
  return entries
    .flatMap((entry) => [
      entry.role,
      ...entry.blocks.flatMap((block) => {
        const record = asRecord(block);
        return [
          stringField(record.title),
          stringField(record.body),
          stringField(record.preview),
          stringField(record.detail)
        ];
      })
    ])
    .filter(Boolean)
    .join("\n");
}

export function transcriptMayContainWorkspaceFile(entries: ThreadSnapshot["entries"]): boolean {
  if (completedToolMayReferenceWorkspaceFile(entries)) {
    return true;
  }
  const text = transcriptSearchText(entries);
  for (const match of text.matchAll(/\]\(\s*(?:<([^>\n]+)>|([^\s)]+))/gu)) {
    if (looksLikeWorkspaceFileReference(match[1] ?? match[2] ?? "")) {
      return true;
    }
  }
  for (const match of text.matchAll(/`([^`\n]+)`/gu)) {
    if (looksLikeWorkspaceFileReference(match[1] ?? "")) {
      return true;
    }
  }
  for (const match of text.matchAll(/(?:^|[\s([{])([^\s`<>"')\]}]+)/gmu)) {
    if (looksLikeWorkspaceFileReference(match[1] ?? "")) {
      return true;
    }
  }
  return false;
}

function completedToolMayReferenceWorkspaceFile(entries: ThreadSnapshot["entries"]): boolean {
  for (const entry of entries) {
    for (const block of entry.blocks) {
      const record = asRecord(block);
      const metadata = asRecord(record.metadata);
      const result = asRecord(record.result);
      const toolName = stringField(metadata.tool_name ?? metadata.toolName);
      const status = stringField(result.status) || stringField(record.status);
      if (
        metadata.projection !== "tool"
        || (toolName !== "read" && toolName !== "edit" && toolName !== "write")
        || status !== "completed"
        || result.isError === true
      ) {
        continue;
      }
      const args = jsonLikeRecord(metadata.args ?? metadata.arguments);
      if (stringField(args.path).trim()) {
        return true;
      }
    }
  }
  return false;
}

function jsonLikeRecord(value: unknown): Record<string, unknown> {
  const record = asRecord(value);
  if (Object.keys(record).length > 0 || typeof value !== "string") {
    return record;
  }
  try {
    return asRecord(JSON.parse(value) as unknown);
  } catch {
    return {};
  }
}

function looksLikeWorkspaceFileReference(rawValue: string): boolean {
  let value = rawValue.trim();
  try {
    value = decodeURIComponent(value);
  } catch {
    // A malformed escape cannot prevent the cheap demand check from examining
    // the literal value. Exact inventory matching remains authoritative.
  }
  if (/^(?:[a-z][a-z0-9+.-]*:|#)/iu.test(value) && !/^[A-Za-z]:[\\/]/u.test(value)) {
    return false;
  }
  value = value
    .replace(/#L\d+(?:-L?\d+)?$/iu, "")
    .replace(/:\d+(?::\d+)?$/u, "")
    .replace(/[),.;:!?]+$/u, "");
  const basename = value.split(/[\\/]/u).at(-1) ?? "";
  return /(?:^|.)\.[A-Za-z0-9][A-Za-z0-9._-]*$/u.test(basename);
}
