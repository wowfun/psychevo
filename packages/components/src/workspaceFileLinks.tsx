import type { WorkspaceFileEntry } from "@psychevo/protocol";

export interface WorkspaceFileLinkContext {
  root: string;
  entries: readonly WorkspaceFileEntry[];
  onOpen(path: string): void | Promise<void>;
}

type MarkdownAstNode = {
  children?: MarkdownAstNode[];
  data?: {
    hProperties?: Record<string, unknown>;
  };
  type: string;
  url?: string;
  value?: string;
  position?: {
    end?: { offset?: number };
  };
};

type WorkspacePathMatch = {
  end: number;
  path: string;
  start: number;
  value: string;
};

type WorkspacePathAlias = {
  comparisonValue: string;
  path: string;
  value: string;
};

type WorkspacePathIndex = {
  aliasesByFirstCharacter: Map<string, WorkspacePathAlias[]>;
  windows: boolean;
};

const WORKSPACE_PATH_DATA_ATTRIBUTE = "data-pevo-workspace-path";

export function workspaceFileRemarkPlugin(context: WorkspaceFileLinkContext, streaming = false) {
  const index = workspacePathIndex(context.root, context.entries);
  return function attachWorkspaceFileLinks() {
    return function transformWorkspaceFileLinks(tree: MarkdownAstNode): void {
      visitMarkdownNode(tree, index, streaming, tree.position?.end?.offset);
    };
  };
}

export function workspacePathFromLinkNode(node: unknown): string | null {
  if (!node || typeof node !== "object") {
    return null;
  }
  const properties = (node as { properties?: Record<string, unknown> }).properties;
  const path = properties?.[WORKSPACE_PATH_DATA_ATTRIBUTE];
  return typeof path === "string" && path ? path : null;
}

function visitMarkdownNode(
  node: MarkdownAstNode,
  index: WorkspacePathIndex,
  streaming: boolean,
  documentEnd: number | undefined
): void {
  if (!node.children || node.children.length === 0) {
    return;
  }
  const children: MarkdownAstNode[] = [];
  let rawHtmlDepth = 0;
  for (const child of node.children) {
    if (child.type === "html") {
      rawHtmlDepth = rawHtmlNestingDepth(child.value ?? "", rawHtmlDepth);
      children.push(child);
      continue;
    }
    if (rawHtmlDepth > 0) {
      children.push(child);
      continue;
    }
    if (child.type === "link") {
      const path = typeof child.url === "string"
        ? exactWorkspacePath(decodedWorkspaceLinkDestination(child.url), index)
        : null;
      if (path) {
        child.data = {
          ...child.data,
          hProperties: {
            ...child.data?.hProperties,
            [WORKSPACE_PATH_DATA_ATTRIBUTE]: path
          }
        };
      }
      children.push(child);
      continue;
    }
    if (child.type === "text" && typeof child.value === "string") {
      children.push(...workspaceTextNodes(
        child.value,
        index,
        streaming && child.position?.end?.offset === documentEnd
      ));
      continue;
    }
    if (child.type === "inlineCode" && typeof child.value === "string") {
      const path = exactWorkspacePath(child.value, index);
      if (path) {
        children.push(workspaceLinkNode({
          end: child.value.length,
          path,
          start: 0,
          value: child.value
        }, child));
        continue;
      }
    }
    visitMarkdownNode(child, index, streaming, documentEnd);
    children.push(child);
  }
  node.children = children;
}

const VOID_HTML_ELEMENTS = new Set([
  "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta",
  "param", "source", "track", "wbr"
]);

function rawHtmlNestingDepth(value: string, initialDepth: number): number {
  let depth = initialDepth;
  for (const match of value.matchAll(/<\s*(\/?)\s*([A-Za-z][A-Za-z0-9:-]*)\b[^>]*>/gu)) {
    const closing = match[1] === "/";
    const name = match[2]?.toLowerCase() ?? "";
    const selfClosing = /\/\s*>$/u.test(match[0]) || VOID_HTML_ELEMENTS.has(name);
    if (closing) {
      depth = Math.max(0, depth - 1);
    } else if (!selfClosing) {
      depth += 1;
    }
  }
  return depth;
}

function decodedWorkspaceLinkDestination(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

function workspaceTextNodes(
  text: string,
  index: WorkspacePathIndex,
  skipUnfinishedTail: boolean
): MarkdownAstNode[] {
  const matches = workspacePathMatches(text, index);
  const trailingMatch = matches.at(-1);
  if (
    skipUnfinishedTail &&
    trailingMatch &&
    (trailingMatch.end === text.length || text.slice(trailingMatch.end) === ":")
  ) {
    matches.pop();
  }
  if (matches.length === 0) {
    return [{ type: "text", value: text }];
  }
  const nodes: MarkdownAstNode[] = [];
  let cursor = 0;
  for (const match of matches) {
    if (match.start > cursor) {
      nodes.push({ type: "text", value: text.slice(cursor, match.start) });
    }
    nodes.push(workspaceLinkNode(match));
    cursor = match.end;
  }
  if (cursor < text.length) {
    nodes.push({ type: "text", value: text.slice(cursor) });
  }
  return nodes;
}

function workspaceLinkNode(match: WorkspacePathMatch, child?: MarkdownAstNode): MarkdownAstNode {
  return {
    children: [child ?? { type: "text", value: match.value }],
    data: {
      hProperties: {
        [WORKSPACE_PATH_DATA_ATTRIBUTE]: match.path
      }
    },
    type: "link",
    url: match.value
  };
}

function workspacePathIndex(root: string, entries: readonly WorkspaceFileEntry[]): WorkspacePathIndex {
  const rootIdentity = workspaceRootIdentity(root);
  const windows = rootIdentity.kind !== "posix";
  const aliasesByFirstCharacter = new Map<string, WorkspacePathAlias[]>();
  const seen = new Set<string>();
  for (const entry of entries) {
    if (entry.kind !== "file") {
      continue;
    }
    const relativePath = normalizedRelativePath(entry.path);
    if (!relativePath) {
      continue;
    }
    for (const value of workspaceAliasesForPath(rootIdentity, relativePath)) {
      const comparisonValue = comparablePath(value, windows);
      if (!comparisonValue || seen.has(comparisonValue)) {
        continue;
      }
      seen.add(comparisonValue);
      const firstCharacter = comparisonValue[0];
      if (!firstCharacter) {
        continue;
      }
      const aliases = aliasesByFirstCharacter.get(firstCharacter) ?? [];
      aliases.push({ comparisonValue, path: entry.path, value });
      aliasesByFirstCharacter.set(firstCharacter, aliases);
    }
  }
  for (const aliases of aliasesByFirstCharacter.values()) {
    aliases.sort((left, right) => right.value.length - left.value.length);
  }
  return { aliasesByFirstCharacter, windows };
}

function workspacePathMatches(text: string, index: WorkspacePathIndex): WorkspacePathMatch[] {
  const matches: WorkspacePathMatch[] = [];
  let cursor = 0;
  while (cursor < text.length) {
    const firstCharacter = comparablePath(text[cursor] ?? "", index.windows);
    const aliases = index.aliasesByFirstCharacter.get(firstCharacter) ?? [];
    const alias = aliases.find((candidate) =>
      comparablePath(text.slice(cursor, cursor + candidate.value.length), index.windows) === candidate.comparisonValue
    );
    if (!alias) {
      cursor += 1;
      continue;
    }
    const aliasEnd = cursor + alias.value.length;
    const suffixLength = workspaceLineSuffixLength(text.slice(aliasEnd));
    const end = aliasEnd + suffixLength;
    if (!hasPathBoundaries(text, cursor, end)) {
      cursor += 1;
      continue;
    }
    matches.push({
      end,
      path: alias.path,
      start: cursor,
      value: text.slice(cursor, end)
    });
    cursor = end;
  }
  return matches;
}

function exactWorkspacePath(value: string, index: WorkspacePathIndex): string | null {
  const firstCharacter = comparablePath(value[0] ?? "", index.windows);
  const aliases = index.aliasesByFirstCharacter.get(firstCharacter) ?? [];
  const comparableValue = comparablePath(value, index.windows);
  for (const alias of aliases) {
    if (!comparableValue.startsWith(alias.comparisonValue)) {
      continue;
    }
    const suffix = value.slice(alias.value.length);
    if (!suffix || workspaceLineSuffixLength(suffix) === suffix.length) {
      return alias.path;
    }
  }
  return null;
}

function workspaceLineSuffixLength(value: string): number {
  const match = /^(?::\d+(?::\d+)?|#L\d+(?:-L?\d+)?)/u.exec(value);
  return match?.[0].length ?? 0;
}

type WorkspaceRootIdentity =
  | { base: string; drive: string; kind: "drive" }
  | { base: string; kind: "posix" }
  | { base: string; kind: "unc" };

function workspaceRootIdentity(root: string): WorkspaceRootIdentity {
  const withoutExtendedPrefix = root
    .replace(/^\\\\\?\\UNC\\/iu, "//")
    .replace(/^\\\\\?\\/u, "");
  const driveRoot = /^([A-Za-z]):(?:[\\/](.*))?$/u.exec(withoutExtendedPrefix);
  if (driveRoot?.[1]) {
    return {
      base: (driveRoot[2] ?? "").replace(/\\/g, "/").replace(/\/+$/u, ""),
      drive: driveRoot[1],
      kind: "drive"
    };
  }
  const slashRoot = withoutExtendedPrefix.replace(/\\/g, "/").replace(/\/+$/u, "");
  if (slashRoot.startsWith("//")) {
    return { base: slashRoot, kind: "unc" };
  }
  return { base: slashRoot || "/", kind: "posix" };
}

function workspaceAliasesForPath(root: WorkspaceRootIdentity, relativePath: string): string[] {
  const relativeBackslash = relativePath.replace(/\//g, "\\");
  const aliases = [relativePath, `./${relativePath}`, relativeBackslash, `.\\${relativeBackslash}`];
  if (root.kind === "drive") {
    const rootRelative = root.base ? `${root.base}/${relativePath}` : relativePath;
    aliases.push(
      `${root.drive}:/${rootRelative}`,
      `${root.drive}:\\${rootRelative.replace(/\//g, "\\")}`,
      `/${root.drive}/${rootRelative}`,
      `/${root.drive}:/${rootRelative}`
    );
  } else if (root.kind === "unc") {
    const absolute = `${root.base}/${relativePath}`;
    aliases.push(absolute, absolute.replace(/\//g, "\\"));
  } else {
    aliases.push(`${root.base === "/" ? "" : root.base}/${relativePath}`);
  }
  return aliases;
}

function normalizedRelativePath(path: string): string {
  return path
    .replace(/\\/g, "/")
    .replace(/^\.\/+/, "")
    .replace(/^\/+|\/+$/g, "");
}

function comparablePath(value: string, windows: boolean): string {
  return windows ? value.toLocaleLowerCase("en-US") : value;
}

function hasPathBoundaries(text: string, start: number, end: number): boolean {
  const before = text[start - 1];
  const after = text[end];
  const sentenceDelimiter = (after === "." || after === "?" || after === ":") && (
    !text[end + 1] || isPathBoundary(text[end + 1] ?? "")
  );
  return (
    (!before || !isPathContinuation(before)) &&
    (!after || sentenceDelimiter || !isPathContinuation(after))
  );
}

function isPathContinuation(value: string): boolean {
  return !isPathBoundary(value);
}

function isPathBoundary(value: string): boolean {
  return /[\s,;!()\[\]{}'"`<>|，。；：！？、—–]/u.test(value);
}
