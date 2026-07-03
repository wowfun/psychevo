export function stripFileProtocol(input: string): string {
  if (!input.toLowerCase().startsWith("file://")) return input;
  const value = input.slice("file://".length);
  if (/^\/[A-Za-z]:[\\/]/.test(value)) return value.slice(1);
  if (/^[A-Za-z]:[\\/]/.test(value)) return value;
  if (value.startsWith("/")) return value;
  return `//${value}`;
}

export function stripQueryAndHash(input: string): string {
  const hashIndex = input.indexOf("#");
  const queryIndex = input.indexOf("?");
  if (hashIndex !== -1 && queryIndex !== -1) {
    return input.slice(0, Math.min(hashIndex, queryIndex));
  }
  if (hashIndex !== -1) return input.slice(0, hashIndex);
  if (queryIndex !== -1) return input.slice(0, queryIndex);
  return input;
}

function isUriLikePathInput(input: string): boolean {
  return /^[A-Za-z][A-Za-z0-9+.-]*:\/\//.test(input);
}

export function decodeFilePath(input: string): string {
  try {
    return decodeURIComponent(input);
  } catch {
    return input;
  }
}

export function unquoteGitPath(input: string): string {
  if (!input.startsWith("\"") || !input.endsWith("\"")) return input;
  const body = input.slice(1, -1);
  const bytes: number[] = [];
  for (let index = 0; index < body.length; index++) {
    const char = body[index]!;
    if (char !== "\\") {
      bytes.push(char.charCodeAt(0));
      continue;
    }
    const next = body[index + 1];
    if (!next) {
      bytes.push("\\".charCodeAt(0));
      continue;
    }
    if (next >= "0" && next <= "7") {
      const match = body.slice(index + 1, index + 4).match(/^[0-7]{1,3}/);
      if (match) {
        bytes.push(Number.parseInt(match[0], 8));
        index += match[0].length;
        continue;
      }
    }
    const escaped =
      next === "n" ? "\n" :
      next === "r" ? "\r" :
      next === "t" ? "\t" :
      next === "b" ? "\b" :
      next === "f" ? "\f" :
      next === "v" ? "\v" :
      next === "\\" || next === "\"" ? next :
      next;
    bytes.push(escaped.charCodeAt(0));
    index++;
  }
  return new TextDecoder().decode(new Uint8Array(bytes));
}

export function encodeFilePath(path: string): string {
  let normalized = path.replace(/\\/g, "/");
  if (/^[A-Za-z]:/.test(normalized)) {
    normalized = `/${normalized}`;
  }
  return normalized
    .split("/")
    .map((segment, index) => {
      if (index === 1 && /^[A-Za-z]:$/.test(segment)) return segment;
      return encodeURIComponent(segment);
    })
    .join("/");
}

export function normalizeFilePathInput(input: string, root: string): string {
  const withoutProtocol = stripFileProtocol(input);
  const pathInput = isUriLikePathInput(input)
    ? stripQueryAndHash(withoutProtocol)
    : withoutProtocol;
  let path = unquoteGitPath(decodeFilePath(pathInput));
  const windows = /^[A-Za-z]:/.test(root) || root.startsWith("\\\\");
  const canonRoot = canonicalPath(root, windows);
  const canonPath = canonicalPath(path, windows);
  if (
    canonPath.startsWith(canonRoot) &&
    (canonRoot.endsWith("/") || canonPath === canonRoot || canonPath[canonRoot.length] === "/")
  ) {
    path = path.slice(root.length);
  }
  if (path.startsWith("./") || path.startsWith(".\\")) {
    path = path.slice(2);
  }
  if (path.startsWith("/") || path.startsWith("\\")) {
    path = path.slice(1);
  }
  return path;
}

function canonicalPath(path: string, windows: boolean): string {
  const normalized = path.replace(/\\/g, "/");
  return windows ? normalized.toLowerCase() : normalized;
}
