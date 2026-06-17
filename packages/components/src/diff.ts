export type ParsedDiffLineKind = "add" | "delete" | "context" | "meta";

export type ParsedDiffLine = {
  kind: ParsedDiffLineKind;
  marker: string;
  newNumber: number | null;
  oldNumber: number | null;
  text: string;
};

export type ParsedDiffHunk = {
  header: string;
  lines: ParsedDiffLine[];
  newStart: number;
  oldStart: number;
};

export type ParsedDiffFile = {
  headers: string[];
  hunks: ParsedDiffHunk[];
  newPath: string | null;
  oldPath: string | null;
  path: string;
};

export type DiffLineStats = {
  additions: number;
  deletions: number;
};

export type DiffParseMode = "tolerant" | "strict-git-patch";

export function parseUnifiedDiff(text: string, mode: DiffParseMode = "tolerant"): ParsedDiffFile[] {
  const trimmed = text.replace(/\r\n/g, "\n").replace(/\n$/, "");
  if (!trimmed.trim()) {
    return [];
  }

  const files = parseDiffLines(trimmed.split("\n"), mode);
  if (mode === "strict-git-patch" && !isStrictGitPatch(files)) {
    return [];
  }
  return files;
}

export function parseStrictGitPatchDiff(text: string): ParsedDiffFile[] {
  return parseUnifiedDiff(text, "strict-git-patch");
}

export function diffLineStats(file: ParsedDiffFile): DiffLineStats {
  let additions = 0;
  let deletions = 0;
  for (const hunk of file.hunks) {
    for (const line of hunk.lines) {
      if (line.kind === "add") {
        additions += 1;
      }
      if (line.kind === "delete") {
        deletions += 1;
      }
    }
  }
  return { additions, deletions };
}

export function diffFilesStats(files: ParsedDiffFile[]): DiffLineStats {
  return files.reduce<DiffLineStats>(
    (total, file) => {
      const stats = diffLineStats(file);
      total.additions += stats.additions;
      total.deletions += stats.deletions;
      return total;
    },
    { additions: 0, deletions: 0 }
  );
}

export function diffDisplayPath(file: ParsedDiffFile): string {
  if (file.oldPath && file.newPath && file.oldPath !== file.newPath) {
    return `${file.oldPath} -> ${file.newPath}`;
  }
  return file.path;
}

function parseDiffLines(lines: string[], mode: DiffParseMode): ParsedDiffFile[] {
  const files: ParsedDiffFile[] = [];
  let currentFile: ParsedDiffFile | null = null;
  let currentHunk: ParsedDiffHunk | null = null;
  let oldLineNumber = 0;
  let newLineNumber = 0;

  function ensureFile(path = "Diff"): ParsedDiffFile {
    if (currentFile) {
      return currentFile;
    }
    currentFile = {
      headers: [],
      hunks: [],
      newPath: path === "Diff" ? null : path,
      oldPath: null,
      path
    };
    files.push(currentFile);
    return currentFile;
  }

  for (const line of lines) {
    if (line.startsWith("diff --git ")) {
      const paths = diffPathsFromGitHeader(line);
      const path = paths.newPath ?? paths.oldPath ?? "Diff";
      currentFile = {
        headers: [line],
        hunks: [],
        newPath: paths.newPath,
        oldPath: paths.oldPath,
        path
      };
      currentHunk = null;
      files.push(currentFile);
      continue;
    }

    if (mode === "strict-git-patch" && !currentFile) {
      return [];
    }

    const file = ensureFile();
    if (line.startsWith("--- ") || line.startsWith("+++ ")) {
      file.headers.push(line);
      const path = cleanDiffPath(line.slice(4).trim());
      if (line.startsWith("--- ")) {
        file.oldPath = path && path !== "/dev/null" ? path : null;
      } else if (path && path !== "/dev/null") {
        file.newPath = path;
        file.path = path;
      }
      continue;
    }
    if (line.startsWith("@@ ")) {
      const range = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(line);
      if (mode === "strict-git-patch" && !range) {
        return [];
      }
      oldLineNumber = range?.[1] ? Number(range[1]) : 0;
      newLineNumber = range?.[2] ? Number(range[2]) : 0;
      currentHunk = { header: line, lines: [], newStart: newLineNumber, oldStart: oldLineNumber };
      file.hunks.push(currentHunk);
      continue;
    }
    if (!currentHunk) {
      file.headers.push(line);
      continue;
    }
    if (line.startsWith("+")) {
      currentHunk.lines.push({
        kind: "add",
        marker: "+",
        newNumber: newLineNumber,
        oldNumber: null,
        text: line.slice(1)
      });
      newLineNumber += 1;
      continue;
    }
    if (line.startsWith("-")) {
      currentHunk.lines.push({
        kind: "delete",
        marker: "-",
        newNumber: null,
        oldNumber: oldLineNumber,
        text: line.slice(1)
      });
      oldLineNumber += 1;
      continue;
    }
    if (line.startsWith(" ")) {
      currentHunk.lines.push({
        kind: "context",
        marker: "",
        newNumber: newLineNumber,
        oldNumber: oldLineNumber,
        text: line.slice(1)
      });
      oldLineNumber += 1;
      newLineNumber += 1;
      continue;
    }
    currentHunk.lines.push({
      kind: "meta",
      marker: "",
      newNumber: null,
      oldNumber: null,
      text: line
    });
  }

  return files;
}

function isStrictGitPatch(files: ParsedDiffFile[]): boolean {
  if (files.length === 0) {
    return false;
  }
  return files.every((file) => {
    const hasGitHeader = file.headers.some((line) => line.startsWith("diff --git "));
    if (!hasGitHeader) {
      return false;
    }
    if (file.hunks.length > 0) {
      return true;
    }
    return file.headers.some((line) => (
      line.startsWith("rename from ") ||
      line.startsWith("rename to ") ||
      line.startsWith("similarity index ") ||
      line.startsWith("new file mode ") ||
      line.startsWith("deleted file mode ") ||
      line.startsWith("old mode ") ||
      line.startsWith("new mode ") ||
      line.startsWith("Binary files ")
    ));
  });
}

function diffPathsFromGitHeader(line: string): { newPath: string | null; oldPath: string | null } {
  const rest = line.slice("diff --git ".length).trim();
  const parts = splitDiffHeaderArgs(rest);
  const oldPath = cleanDiffPath(parts[0] ?? "");
  const newPath = cleanDiffPath(parts[1] ?? "");
  return {
    newPath: newPath || null,
    oldPath: oldPath || null
  };
}

function splitDiffHeaderArgs(rest: string): string[] {
  const parts: string[] = [];
  let current = "";
  let quoted = false;
  let escaped = false;
  for (const char of rest) {
    if (escaped) {
      current += char;
      escaped = false;
      continue;
    }
    if (quoted && char === "\\") {
      escaped = true;
      continue;
    }
    if (char === "\"") {
      quoted = !quoted;
      current += char;
      continue;
    }
    if (!quoted && /\s/.test(char)) {
      if (current) {
        parts.push(current);
        current = "";
      }
      continue;
    }
    current += char;
  }
  if (current) {
    parts.push(current);
  }
  return parts;
}

function cleanDiffPath(path: string): string {
  const trimmed = path.trim();
  if (!trimmed) {
    return "";
  }
  const unquoted = trimmed.replace(/^"|"$/g, "");
  if (unquoted === "/dev/null") {
    return unquoted;
  }
  return unquoted.replace(/^[ab]\//, "");
}
