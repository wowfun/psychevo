import react from "@vitejs/plugin-react";
import { fileViewerRenderers } from "@file-viewer/vite-plugin";
import { createRequire } from "node:module";
import { configDefaults, defineConfig } from "vitest/config";
import { excalidrawAssets } from "../excalidraw-assets-vite-plugin";
import { releaseAssets } from "../release-assets-vite-plugin";
import { sharedViteBuildConfig } from "../shared-vite-config";
import { FILE_VIEWER_ASSET_FORMATS } from "./src/right-workspace/workspace-file-formats";

const configRequire = createRequire(import.meta.url);
const jszipBrowserEntry = configRequire.resolve("jszip/dist/jszip.min.js");
const testExecArgv = process.allowedNodeEnvironmentFlags.has("--no-experimental-webstorage")
  ? ["--no-experimental-webstorage"]
  : [];

export default defineConfig({
  publicDir: "static",
  resolve: {
    alias: [{ find: /^jszip$/, replacement: jszipBrowserEntry }]
  },
  plugins: [
    react(),
    fileViewerRenderers({
      copyAssets: process.env.VITEST
        ? false
        : {
            baseDir: "file-viewer",
            mode: "both",
            publicDir: "../../node_modules/.cache/psychevo-file-viewer/workbench"
          },
      formats: FILE_VIEWER_ASSET_FORMATS,
      inject: false,
      chunkStrategy: "none"
    }),
    excalidrawAssets({
      packageEntry: configRequire.resolve("@excalidraw/excalidraw")
    }),
    releaseAssets()
  ],
  build: sharedViteBuildConfig({ includePreloadHelper: true, includeYaml: true }),
  server: {
    host: "127.0.0.1",
    port: 5173
  },
  test: {
    execArgv: testExecArgv,
    exclude: [...configDefaults.exclude, "e2e/**"]
  }
});
