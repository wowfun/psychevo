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
