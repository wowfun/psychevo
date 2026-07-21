import { readdir, readFile } from "node:fs/promises";
import { dirname, extname, isAbsolute, relative, resolve, sep } from "node:path";
import type { Plugin } from "vite";

const PUBLIC_PREFIX = "/excalidraw/fonts/";

type PathSemantics = {
  isAbsolute(path: string): boolean;
  relative(from: string, to: string): string;
  resolve(...paths: string[]): string;
  sep: string;
};

const nativePathSemantics: PathSemantics = { isAbsolute, relative, resolve, sep };

export function resolveAssetRequestPath(
  root: string,
  requestedPath: string,
  paths: PathSemantics = nativePathSemantics
): string | null {
  const file = paths.resolve(root, requestedPath);
  const relativePath = paths.relative(root, file);
  if (
    !relativePath
    || paths.isAbsolute(relativePath)
    || relativePath === ".."
    || relativePath.startsWith(`..${paths.sep}`)
  ) {
    return null;
  }
  return file;
}

export function excalidrawAssets({ packageEntry }: { packageEntry: string }): Plugin {
  const fontsRoot = resolve(dirname(packageEntry), "fonts");
  let build = false;
  return {
    name: "psychevo-excalidraw-assets",
    configResolved(config) {
      build = config.command === "build";
    },
    async buildStart() {
      if (!build) {
        return;
      }
      for (const file of await filesBelow(fontsRoot)) {
        const source = await readFile(file);
        this.emitFile({
          fileName: `excalidraw/fonts/${slashPath(relative(fontsRoot, file))}`,
          source,
          type: "asset"
        });
      }
    },
    configureServer(server) {
      server.middlewares.use(async (request, response, next) => {
        let pathname: string;
        try {
          pathname = new URL(request.url ?? "/", "http://localhost").pathname;
        } catch {
          next();
          return;
        }
        if (!pathname.startsWith(PUBLIC_PREFIX)) {
          next();
          return;
        }
        let requestedPath: string;
        try {
          requestedPath = decodeURIComponent(pathname.slice(PUBLIC_PREFIX.length));
        } catch {
          response.statusCode = 400;
          response.end();
          return;
        }
        const file = resolveAssetRequestPath(fontsRoot, requestedPath);
        if (!file) {
          response.statusCode = 404;
          response.end();
          return;
        }
        try {
          const source = await readFile(file);
          response.statusCode = 200;
          response.setHeader("Cache-Control", "no-store");
          response.setHeader("Content-Type", fontContentType(file));
          response.end(source);
        } catch {
          response.statusCode = 404;
          response.end();
        }
      });
    }
  };
}

async function filesBelow(directory: string): Promise<string[]> {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = await Promise.all(entries.map((entry) => {
    const path = resolve(directory, entry.name);
    return entry.isDirectory() ? filesBelow(path) : Promise.resolve([path]);
  }));
  return files.flat().sort();
}

function fontContentType(path: string): string {
  switch (extname(path).toLowerCase()) {
    case ".woff2":
      return "font/woff2";
    case ".woff":
      return "font/woff";
    case ".ttf":
      return "font/ttf";
    default:
      return "application/octet-stream";
  }
}

function slashPath(path: string): string {
  return path.split(sep).join("/");
}
