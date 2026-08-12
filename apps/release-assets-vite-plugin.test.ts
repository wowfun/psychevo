import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { build } from "vite";
import {
  createReleaseAssetManifest,
  readReleaseAssetManifest,
  releaseAssets
} from "./release-assets-vite-plugin";

describe("release asset manifest", () => {
  it("is deterministic, relative, and excludes diagnostic source maps", async () => {
    const root = await mkdtemp(join(tmpdir(), "psychevo-release-assets-"));
    try {
      await mkdir(join(root, "assets"));
      await writeFile(join(root, "index.html"), "index");
      await writeFile(join(root, "assets", "app.js"), "app");
      await writeFile(join(root, "assets", "app.js.map"), "diagnostic");

      const first = await createReleaseAssetManifest(root);
      const firstBytes = JSON.stringify(await readReleaseAssetManifest(root));
      const second = await createReleaseAssetManifest(root);
      const secondBytes = JSON.stringify(await readReleaseAssetManifest(root));

      expect(second).toEqual(first);
      expect(secondBytes).toBe(firstBytes);
      expect(first.files.map((file) => file.path)).toEqual([
        "assets/app.js",
        "index.html"
      ]);
      expect(first.files.every((file) => !file.path.startsWith(root))).toBe(true);
    } finally {
      await rm(root, { recursive: true });
    }
  });

  it("waits for earlier asset-copy hooks before inventorying the distribution", async () => {
    const root = await mkdtemp(join(tmpdir(), "psychevo-release-assets-order-"));
    try {
      await writeFile(join(root, "index.html"), "<!doctype html><title>fixture</title>");
      await build({
        root,
        publicDir: false,
        logLevel: "silent",
        plugins: [
          {
            name: "delayed-assets",
            async closeBundle() {
              await new Promise((resolve) => setTimeout(resolve, 25));
              await writeFile(join(root, "dist", "late.bin"), "late asset");
            }
          },
          releaseAssets()
        ],
        build: {
          outDir: "dist",
          minify: false
        }
      });

      const manifest = await readReleaseAssetManifest(join(root, "dist"));
      expect(manifest.files.map((file) => file.path)).toContain("late.bin");
    } finally {
      await rm(root, { recursive: true });
    }
  });
});
