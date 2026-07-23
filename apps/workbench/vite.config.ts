import react from "@vitejs/plugin-react";
import { fileViewerRenderers } from "@file-viewer/vite-plugin";
import { createRequire } from "node:module";
import { configDefaults, defineConfig } from "vitest/config";
import { excalidrawAssets } from "../excalidraw-assets-vite-plugin";
import { sharedViteBuildConfig } from "../shared-vite-config";

const configRequire = createRequire(import.meta.url);
const jszipBrowserEntry = configRequire.resolve("jszip/dist/jszip.min.js");

export default defineConfig({
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
  build: sharedViteBuildConfig({ includePreloadHelper: true, includeYaml: true }),
  server: {
    host: "127.0.0.1",
    port: 5173
  },
  test: {
    exclude: [...configDefaults.exclude, "e2e/**"]
  }
});
