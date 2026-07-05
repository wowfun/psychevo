import react from "@vitejs/plugin-react";
import { configDefaults, defineConfig } from "vitest/config";

export default defineConfig({
  clearScreen: false,
  plugins: [react()],
  build: {
    sourcemap: true
  },
  server: {
    host: "127.0.0.1",
    port: 5175,
    strictPort: true
  },
  test: {
    exclude: [...configDefaults.exclude, "src-tauri/**", "wdio/**"]
  }
});
