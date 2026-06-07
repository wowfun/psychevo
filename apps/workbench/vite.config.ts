import react from "@vitejs/plugin-react";
import { configDefaults, defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          const normalized = id.replace(/\\/g, "/");
          if (normalized.includes("/node_modules/lucide-react/")) {
            return "vendor-icons";
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
