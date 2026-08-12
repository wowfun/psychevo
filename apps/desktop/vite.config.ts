import react from "@vitejs/plugin-react";
import { fileViewerRenderers } from "@file-viewer/vite-plugin";
import { createRequire } from "node:module";
import { configDefaults, defineConfig } from "vitest/config";
import { excalidrawAssets } from "../excalidraw-assets-vite-plugin";
import { releaseAssets } from "../release-assets-vite-plugin";
import { sharedViteBuildConfig } from "../shared-vite-config";
import { FILE_VIEWER_ASSET_FORMATS } from "../workbench/src/right-workspace/workspace-file-formats";

const configRequire = createRequire(import.meta.url);
const workbenchRequire = createRequire(
  new URL("../workbench/package.json", import.meta.url)
);
const jszipBrowserEntry = workbenchRequire.resolve("jszip/dist/jszip.min.js");
const testExecArgv = process.allowedNodeEnvironmentFlags.has("--no-experimental-webstorage")
  ? ["--no-experimental-webstorage"]
  : [];

export default defineConfig({
  clearScreen: false,
  publicDir: false,
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
            publicDir: "../../node_modules/.cache/psychevo-file-viewer/desktop"
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
  build: sharedViteBuildConfig({ includeFloatingApp: true }),
  server: {
    host: "127.0.0.1",
    port: 5175,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/target/**"]
    }
  },
  test: {
    execArgv: testExecArgv,
    exclude: [...configDefaults.exclude, "src-tauri/**", "wdio/**"]
  }
});
