import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { readdir, readFile, stat, writeFile } from "node:fs/promises";
import { isAbsolute, relative, resolve, sep } from "node:path";
import type { Plugin, ResolvedConfig } from "vite";

const RELEASE_ASSET_MANIFEST = "psychevo-release-assets.json";
const RELEASE_ASSET_HASH_WORKERS = 8;

interface ReleaseAsset {
  path: string;
  sha256: string;
  size: number;
}

interface ReleaseAssetManifest {
  schemaVersion: 1;
  files: ReleaseAsset[];
}

async function listFiles(root: string, directory = root): Promise<string[]> {
  const files: string[] = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...await listFiles(root, path));
    } else if (entry.isFile()) {
      files.push(relative(root, path).split(sep).join("/"));
    }
  }
  return files;
}

async function sha256(path: string): Promise<string> {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}

export async function createReleaseAssetManifest(
  outputRoot: string
): Promise<ReleaseAssetManifest> {
  const root = resolve(outputRoot);
  const paths = (await listFiles(root))
    .filter((path) => path !== RELEASE_ASSET_MANIFEST && !path.endsWith(".map"))
    .sort();
  const files = new Array<ReleaseAsset>(paths.length);
  let nextIndex = 0;
  async function hashNext(): Promise<void> {
    while (nextIndex < paths.length) {
      const index = nextIndex++;
      const path = paths[index];
      if (path === undefined) break;
      if (isAbsolute(path) || path.split("/").includes("..")) {
        throw new Error(`Unsafe release asset path: ${path}`);
      }
      const source = resolve(root, ...path.split("/"));
      const metadata = await stat(source);
      files[index] = { path, sha256: await sha256(source), size: metadata.size };
    }
  }
  await Promise.all(Array.from(
    { length: Math.min(RELEASE_ASSET_HASH_WORKERS, paths.length) },
    () => hashNext()
  ));
  const manifest: ReleaseAssetManifest = { schemaVersion: 1, files };
  await writeFile(
    resolve(root, RELEASE_ASSET_MANIFEST),
    `${JSON.stringify(manifest, null, 2)}\n`,
    "utf8"
  );
  return manifest;
}

export async function readReleaseAssetManifest(
  outputRoot: string
): Promise<ReleaseAssetManifest> {
  return JSON.parse(await readFile(
    resolve(outputRoot, RELEASE_ASSET_MANIFEST),
    "utf8"
  )) as ReleaseAssetManifest;
}

export function releaseAssets(): Plugin {
  let config: ResolvedConfig;
  return {
    name: "psychevo-release-assets",
    enforce: "post",
    configResolved(resolved) {
      config = resolved;
    },
    async closeBundle() {
      await createReleaseAssetManifest(resolve(config.root, config.build.outDir));
    }
  };
}
