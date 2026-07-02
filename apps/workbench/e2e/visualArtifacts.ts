import path from "node:path";
import { repoRoot } from "./harness";

export function visualScreenshotRoot(name?: string): string {
  const base = process.env.PSYCHEVO_PLAYWRIGHT_SCREENSHOT_ROOT
    ? path.resolve(process.env.PSYCHEVO_PLAYWRIGHT_SCREENSHOT_ROOT)
    : path.join(repoRoot, ".local/playwright/screenshots");
  return name ? path.join(base, name) : base;
}
