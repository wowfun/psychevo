import path from "node:path";
import { rmSync } from "node:fs";
import type { Options } from "@wdio/types";
import { Agent, setGlobalDispatcher } from "undici";

const artifactRoot = path.resolve(
  process.env.PSYCHEVO_WDIO_ARTIFACT_ROOT
    ?? path.join(process.cwd(), "../../.local/.psychevo-dev/wdio/desktop-native-smoke")
);
// The embedded service launches the Tauri process from this Node process. Keep
// the resolved default visible to the test-only Rust startup recorder too.
process.env.PSYCHEVO_WDIO_ARTIFACT_ROOT = artifactRoot;
const application = process.env.PSYCHEVO_DESKTOP_WDIO_APP ?? defaultApplicationPath();
const providerLive = process.env.PSYCHEVO_DESKTOP_PROVIDER_LIVE === "1";

export const config: Options.Testrunner = {
  capabilities: [
    {
      browserName: "tauri",
      "tauri:options": {
        application
      }
    }
  ],
  beforeSession() {
    setGlobalDispatcher(new Agent());
  },
  framework: "mocha",
  logLevel: "info",
  maxInstances: 1,
  mochaOpts: {
    timeout: providerLive ? 300_000 : 60_000
  },
  onPrepare() {
    for (const relativePath of [
      "desktop-startup-rust.jsonl",
      "desktop-startup-journey.json",
      "screenshots/00-workbench-gui-ready.png",
      "screenshots/01-workbench-short-window.png"
    ]) {
      rmSync(path.join(artifactRoot, relativePath), { force: true });
    }
  },
  outputDir: path.join(artifactRoot, "logs"),
  reporters: ["spec"],
  runner: "local",
  services: [
    [
      "@wdio/tauri-service",
      {
        driverProvider: "embedded"
      }
    ]
  ],
  specs: ["./wdio/**/*.spec.ts"],
  waitforTimeout: providerLive ? 30_000 : 15_000
};

function defaultApplicationPath(): string {
  if (process.platform === "darwin") {
    return path.resolve(process.cwd(), "src-tauri/target/release/bundle/macos/Psychevo Desktop.app");
  }
  return path.resolve(
    process.cwd(),
    "src-tauri/target/release",
    process.platform === "win32" ? "psychevo-desktop.exe" : "psychevo-desktop"
  );
}
