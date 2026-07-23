import react from "@vitejs/plugin-react";
import { fileViewerRenderers } from "@file-viewer/vite-plugin";
import { createRequire } from "node:module";
import { configDefaults, defineConfig } from "vitest/config";
import { excalidrawAssets } from "../excalidraw-assets-vite-plugin";
import { sharedViteBuildConfig } from "../shared-vite-config";

const configRequire = createRequire(import.meta.url);
const workbenchRequire = createRequire(
  new URL("../workbench/package.json", import.meta.url)
);
const jszipBrowserEntry = workbenchRequire.resolve("jszip/dist/jszip.min.js");

export default defineConfig({
  clearScreen: false,
  resolve: {
    alias: [{ find: /^jszip$/, replacement: jszipBrowserEntry }]
  },
  plugins: [
    react(),
    fileViewerRenderers({
      copyAssets: { baseDir: "file-viewer", mode: "both" },
      formats: [
        "pdf",
        "docx", "docm", "dotx", "dotm", "rtf", "odt",
        "xlsx", "xlsm", "xlsb", "xltx", "xltm", "ods",
        "pptx", "pptm", "potx", "potm", "ppsx", "ppsm", "odp",
        "ofd", "heic", "heif"
      ],
      inject: false,
      chunkStrategy: "none"
    }),
    excalidrawAssets({
      packageEntry: configRequire.resolve("@excalidraw/excalidraw")
    })
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
    exclude: [...configDefaults.exclude, "src-tauri/**", "wdio/**"]
  }
});
