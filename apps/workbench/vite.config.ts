import react from "@vitejs/plugin-react";
import { configDefaults, defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          const normalized = id.replace(/\\/g, "/");
          if (
            normalized.includes("/node_modules/react/") ||
            normalized.includes("/node_modules/react-dom/") ||
            normalized.includes("/node_modules/scheduler/")
          ) {
            return "vendor-react";
          }
          if (normalized.includes("/node_modules/highlight.js/")) {
            return "vendor-highlight";
          }
          if (
            normalized.includes("/node_modules/react-markdown/") ||
            normalized.includes("/node_modules/remark-") ||
            normalized.includes("/node_modules/unified/") ||
            normalized.includes("/node_modules/micromark") ||
            normalized.includes("/node_modules/mdast-") ||
            normalized.includes("/node_modules/hast-") ||
            normalized.includes("/node_modules/unist-") ||
            normalized.includes("/node_modules/vfile") ||
            normalized.includes("/node_modules/markdown-table/") ||
            normalized.includes("/node_modules/property-information/")
          ) {
            return "vendor-markdown";
          }
          if (
            normalized.includes("/node_modules/ajv/") ||
            normalized.includes("/node_modules/fast-uri/") ||
            normalized.includes("/node_modules/json-schema-traverse/")
          ) {
            return "vendor-validation";
          }
          if (normalized.includes("/node_modules/lucide-react/")) {
            return "vendor-icons";
          }
          if (normalized.includes("/node_modules/@xterm/")) {
            return "vendor-terminal";
          }
          if (normalized.includes("/node_modules/")) {
            return "vendor";
          }
          const schemaMatch = normalized.match(/\/packages\/protocol\/src\/generated\/schemas\/([^/.]+)\.ts$/);
          if (schemaMatch?.[1]) {
            return `protocol-schema-${schemaMatch[1]}`;
          }
          if (normalized.includes("/packages/protocol/")) {
            return "protocol-runtime";
          }
          if (normalized.includes("/packages/components/")) {
            return "ui-components";
          }
          if (normalized.includes("/packages/client/")) {
            return "client-runtime";
          }
          if (normalized.includes("/packages/host/")) {
            return "host-runtime";
          }
          if (normalized.includes("/packages/assets/")) {
            return "assets";
          }
          return undefined;
        }
      }
    },
    sourcemap: true
  },
  server: {
    host: "127.0.0.1",
    port: 5173
  },
  test: {
    exclude: [...configDefaults.exclude, "e2e/**"]
  }
});
