import react from "@vitejs/plugin-react";
import { configDefaults, defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  build: {
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
