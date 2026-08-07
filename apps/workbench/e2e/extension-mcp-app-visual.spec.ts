import { execFileSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync
} from "node:fs";
import path from "node:path";
import { expect, test, type Page, type TestInfo } from "@playwright/test";
import { repoRoot, startPevoWeb } from "./harness";
import { openPanel } from "./workbench.support";
import { visualScreenshotRoot } from "./visualArtifacts";

test.describe("Extension and MCP App visual contract", () => {
  test("manages a lazy Extension and bounds its display-only App lease", async ({ page, isMobile }, testInfo) => {
    test.skip(process.platform === "win32", "the fake executable fixture requires a Unix shebang");
    await page.setViewportSize(isMobile ? { width: 390, height: 900 } : { width: 1440, height: 960 });
    const artifactRoot = process.env.PSYCHEVO_PLAYWRIGHT_SCREENSHOTS ?? testInfo.outputDir;
    mkdirSync(artifactRoot, { recursive: true });
    const fixtureRoot = mkdtempSync(path.join(artifactRoot, "extension-mcp-app-"));
    const home = path.join(fixtureRoot, "home");
    const cwd = path.join(fixtureRoot, "workspace");
    const extension = path.join(fixtureRoot, "extension");
    mkdirSync(home, { recursive: true });
    mkdirSync(cwd, { recursive: true });
    installExtensionFixture(home, cwd, extension);

    await page.route("https://apps.example.test/dashboard.html", async (route) => {
      await route.fulfill({
        body: [
          "<!doctype html><html><head><style>",
          "body{margin:0;background:#111820;color:#d9e2ec;font:14px system-ui}",
          ".app{padding:28px}.eyebrow{color:#7dd3fc;font-size:11px;letter-spacing:.14em;text-transform:uppercase}",
          "h1{font-size:24px;margin:8px 0}.status{border-left:2px solid #34d399;padding:12px 14px;background:#17212b}",
          "</style></head><body><main class=\"app\"><div class=\"eyebrow\">Extension MCP App</div>",
          "<h1>Deployment dashboard</h1><div class=\"status\">Display lease ready · no tool authority</div></main></body></html>"
        ].join(""),
        headers: {
          "access-control-allow-origin": "*",
          "content-type": "text/html; charset=utf-8"
        },
        status: 200
      });
    });

    const pevoBin = path.join(repoRoot, "target", "debug", process.platform === "win32" ? "pevo.exe" : "pevo");
    const server = await startPevoWeb({ home, cwd, live: false, pevoBin });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      await openExtensions(page, isMobile);
      const capabilities = page.getByRole("region", { name: "Capabilities" });
      const extensionRow = capabilities.getByRole("button", { name: "Extension Deployment dashboard" });
      await expect(extensionRow).toBeVisible();
      await extensionRow.click();
      await expect(capabilities.getByRole("region", { name: "Extension runtime evidence" }))
        .toContainText("Trusted fingerprint");
      const app = capabilities.getByRole("region", { name: "Extension MCP App" });
      await expect(app).toContainText("Use the deployment dashboard text fallback.");
      await capture(page, testInfo, `extension-static-${isMobile ? "mobile" : "desktop"}`);

      await app.getByRole("button", { name: "Open App" }).click();
      const frame = page.frameLocator('iframe[title="dashboard"]');
      await expect(frame.getByRole("heading", { name: "Deployment dashboard" })).toBeVisible();
      await expect(frame.getByText("Display lease ready · no tool authority")).toBeVisible();
      await capture(page, testInfo, `extension-mcp-app-open-${isMobile ? "mobile" : "desktop"}`);

      await capabilities.getByRole("switch", { name: "Deployment dashboard enabled" }).click();
      await expect(capabilities.locator(".capabilityBanner.is-error"))
        .toContainText("active MCP App lease");
      await capture(page, testInfo, `extension-active-lease-${isMobile ? "mobile" : "desktop"}`);

      await app.getByRole("button", { name: "Close App" }).click();
      await expect(page.locator('iframe[title="dashboard"]')).toHaveCount(0);
      await capabilities.getByRole("switch", { name: "Deployment dashboard enabled" }).click();
      await expect(capabilities.getByRole("switch", { name: "Deployment dashboard enabled" }))
        .toHaveAttribute("aria-checked", "false");
    } finally {
      await server.stop();
      rmSync(fixtureRoot, { force: true, recursive: true });
    }
  });
});

function installExtensionFixture(home: string, cwd: string, extension: string): void {
  mkdirSync(extension, { recursive: true });
  const sidecar = path.join(extension, "sidecar");
  copyFileSync(
    path.join(repoRoot, "crates", "psychevo", "tests", "fixtures", "extension_echo_sidecar.py"),
    sidecar
  );
  chmodSync(sidecar, 0o755);
  writeFileSync(path.join(extension, "psychevo.extension.json"), `${JSON.stringify({
    schemaVersion: 1,
    id: "example.deployment-dashboard",
    version: "local",
    displayName: "Deployment dashboard",
    description: "A deterministic display-only operational view.",
    runtime: {
      protocol: "psychevo-extension/1",
      executable: "./sidecar"
    },
    contributions: {
      mcpApps: [{
        id: "dashboard",
        resourceUri: "ui://example/dashboard.html",
        fallback: "Use the deployment dashboard text fallback.",
        resourceUrl: "https://apps.example.test/dashboard.html",
        resourceDomains: ["https://apps.example.test"]
      }]
    }
  }, null, 2)}\n`);
  const pevoBin = path.join(repoRoot, "target", "debug", process.platform === "win32" ? "pevo.exe" : "pevo");
  const env = {
    ...process.env,
    HOME: home,
    PSYCHEVO_HOME: home,
    USERPROFILE: home
  };
  execFileSync(pevoBin, ["init"], { cwd, env, stdio: "pipe" });
  execFileSync(pevoBin, ["install", extension, "--json"], {
    cwd,
    env,
    stdio: "pipe"
  });
}

async function openExtensions(page: Page, isMobile: boolean): Promise<void> {
  if (isMobile) await openPanel(page, isMobile, "History");
  await page.getByRole("button", { name: "Capabilities", exact: true }).click();
  const capabilities = page.getByRole("region", { name: "Capabilities" });
  await expect(capabilities).toBeVisible();
  await capabilities.getByRole("tab", { name: "Extensions" }).click();
}

async function capture(page: Page, testInfo: TestInfo, label: string): Promise<void> {
  await page.screenshot({
    fullPage: true,
    path: visualScreenshotRoot(`${label}-${testInfo.project.name}.png`)
  });
}
