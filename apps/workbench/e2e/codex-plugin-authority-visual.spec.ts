import { expect, test, type Locator, type Page, type TestInfo } from "@playwright/test";
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, rmSync, unlinkSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { repoRoot, startPevoWeb } from "./harness";
import { openPanel } from "./workbench.support";
import { visualScreenshotRoot } from "./visualArtifacts";

const CODEX_PLUGIN_APP_SERVER_PATH = fileURLToPath(
  new URL("./fixtures/codex-plugin-app-server.mjs", import.meta.url)
);

interface CodexFixtureState {
  failReadAfterInstall: boolean;
  installed: boolean;
  version: string;
}

test.describe("Codex plugin authority visual contract", () => {
  test("shows isolated authority lifecycle, repair, and compatibility states", async ({ page, isMobile }, testInfo) => {
    test.skip(isMobile, "Codex authority state matrix is captured once on desktop");
    await page.setViewportSize({ width: 1440, height: 960 });
    const artifactRoot = process.env.PSYCHEVO_PLAYWRIGHT_SCREENSHOTS ?? testInfo.outputDir;
    mkdirSync(artifactRoot, { recursive: true });
    const fixtureRoot = mkdtempSync(path.join(artifactRoot, "codex-plugin-authority-"));
    const statePath = path.join(fixtureRoot, "state.json");
    const binaryPath = path.join(fixtureRoot, "codex-fixture.mjs");
    const userHome = path.join(fixtureRoot, "user-home");
    const globalAuth = path.join(userHome, ".codex", "auth.json");
    mkdirSync(path.dirname(globalAuth), { recursive: true });
    writeFileSync(globalAuth, "fixture-auth-never-read\n");
    writeState(statePath, { failReadAfterInstall: false, installed: false, version: "0.144.1" });
    copyFileSync(CODEX_PLUGIN_APP_SERVER_PATH, binaryPath);
    chmodSync(binaryPath, 0o755);

    const configAppend = [
      "[codex_plugins]",
      "enabled = false",
      `binary = ${JSON.stringify(binaryPath)}`,
      "",
      '[plugins."codex:review@openai"]',
      "enabled = true"
    ].join("\n");
    const server = await startPevoWeb({
      configAppend,
      live: false,
      pevoBin: path.join(repoRoot, "target", "debug", process.platform === "win32" ? "pevo.exe" : "pevo"),
      processEnv: {
        HOME: userHome,
        PSYCHEVO_CODEX_PLUGIN_FIXTURE_STATE: statePath
      }
    });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      await openCapabilitiesPlugins(page);
      const capabilities = page.getByRole("region", { name: "Capabilities" });
      const card = capabilities.getByRole("region", { name: "Codex plugin compatibility" });

      await expect(card).toContainText("disabled · unavailable");
      await captureAuthority(page, testInfo, "codex-plugin-authority-disabled");

      await setAuthorityEnabled(card, true);
      await refreshAuthority(card);
      await expect(card).toContainText("ready · available");
      await expect(card).toContainText(/Ready · generation/);
      await captureAuthority(page, testInfo, "codex-plugin-authority-ready");

      unlinkSync(globalAuth);
      await setAuthorityEnabled(card, false);
      await expect(card).toContainText("disabled · unavailable");
      await setAuthorityEnabled(card, true);
      await refreshAuthority(card);
      await expect(card).toContainText("ready · unavailable");
      await captureAuthority(page, testInfo, "codex-plugin-authority-needs-auth");

      writeState(statePath, { failReadAfterInstall: false, installed: true, version: "0.144.1" });
      await refreshAuthority(card);
      const review = capabilities.getByRole("button", { name: "Plugin review" });
      await expect(review).toBeVisible();
      await review.click();
      await expect(capabilities.getByRole("button", { name: "Trust" })).toBeVisible();
      await captureAuthority(page, testInfo, "codex-plugin-authority-needs-trust");

      writeState(statePath, { failReadAfterInstall: true, installed: false, version: "0.144.1" });
      await refreshAuthority(card);
      await expect(review).toContainText("Available");
      await review.click();
      await capabilities.getByRole("button", { name: "Install", exact: true }).click();
      const partial = capabilities.getByRole("status", { name: "Plugin operation result" });
      await expect(partial).toContainText(/Partial install · (Disabled|Needs trust)/);
      await expect(partial).toContainText("Failed at detail reread");
      await captureAuthority(page, testInfo, "codex-plugin-authority-partial");

      writeState(statePath, { failReadAfterInstall: false, installed: false, version: "0.145.0" });
      await refreshAuthority(card);
      await refreshCapabilities(capabilities);
      await expect(card).toContainText("incompatible · unavailable");
      await expect(card).toContainText(/reviewed `0\.144\.1`|resolved `0\.145\.0`/);
      await captureAuthority(page, testInfo, "codex-plugin-authority-incompatible");
    } finally {
      await server.stop();
      rmSync(fixtureRoot, { force: true, recursive: true });
    }
  });
});

async function openCapabilitiesPlugins(page: Page) {
  if (await page.getByRole("button", { name: "Capabilities", exact: true }).count() === 0) {
    await openPanel(page, false, "History");
  }
  await page.getByRole("button", { name: "Capabilities", exact: true }).click();
  const capabilities = page.getByRole("region", { name: "Capabilities" });
  await expect(capabilities).toBeVisible();
  await capabilities.getByRole("tab", { name: "Plugins" }).click();
  await expect(capabilities.getByRole("region", { name: "Codex plugin compatibility" })).toBeVisible();
}

async function captureAuthority(page: Page, testInfo: TestInfo, label: string) {
  await page.screenshot({
    fullPage: true,
    path: visualScreenshotRoot(`${label}-${testInfo.project.name}.png`)
  });
}

async function refreshCapabilities(capabilities: Locator) {
  await capabilities.locator(".capabilitiesHeader").getByRole("button", { name: "Refresh" }).click();
}

async function refreshAuthority(card: Locator) {
  const refresh = card.getByRole("button", { name: "Refresh" });
  await expect(refresh).toBeEnabled();
  await refresh.evaluate((element) => (element as HTMLButtonElement).click());
  await expect(refresh).toBeEnabled();
}

async function setAuthorityEnabled(card: Locator, enabled: boolean) {
  const toggle = card.getByRole("switch", { name: "Codex plugin compatibility" });
  await expect(toggle).toBeEnabled();
  if ((await toggle.getAttribute("aria-checked")) !== String(enabled)) {
    await toggle.click();
  }
  await expect(toggle).toHaveAttribute("aria-checked", String(enabled));
}

function writeState(statePath: string, state: CodexFixtureState) {
  writeFileSync(statePath, `${JSON.stringify(state)}\n`);
}
