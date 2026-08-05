import path from "node:path";
import { defineConfig, devices } from "@playwright/test";

const artifactRoot = process.env.PSYCHEVO_PLAYWRIGHT_OUTPUT_ROOT;
const captureEveryTest = process.env.PSYCHEVO_PLAYWRIGHT_CAPTURE_EVERY_TEST === "1";

export default defineConfig({
  testDir: "apps/workbench/e2e",
  ...(artifactRoot ? { outputDir: path.resolve(artifactRoot) } : {}),
  fullyParallel: false,
  grepInvert: process.env.PSYCHEVO_XTASK_LIVE_CONTEXT ? undefined : /@live/,
  retries: 0,
  timeout: 180_000,
  expect: {
    timeout: 10_000
  },
  reporter: artifactRoot
    ? [["list"], ["json", { outputFile: path.join(path.resolve(artifactRoot), "report.json") }]]
    : [["list"]],
  use: {
    actionTimeout: 15_000,
    baseURL: "http://127.0.0.1",
    screenshot: captureEveryTest ? "on" : "off",
    trace: "retain-on-failure",
    video: "retain-on-failure"
  },
  projects: [
    {
      name: "chromium-desktop",
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 1440, height: 960 }
      }
    },
    {
      name: "chromium-mobile",
      use: {
        ...devices["Pixel 7"],
        isMobile: true
      }
    }
  ]
});
