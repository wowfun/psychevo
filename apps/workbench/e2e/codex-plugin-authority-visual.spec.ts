import { expect, test, type Locator, type Page, type TestInfo } from "@playwright/test";
import { chmodSync, mkdirSync, mkdtempSync, rmSync, unlinkSync, writeFileSync } from "node:fs";
import path from "node:path";
import { repoRoot, startPevoWeb } from "./harness";
import { openPanel } from "./workbench.support";
import { visualScreenshotRoot } from "./visualArtifacts";

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
    writeFileSync(binaryPath, CODEX_APP_SERVER_FIXTURE);
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

      await toggleAuthority(card, "Enable Codex plugins");
      await expect(card.getByRole("switch", { name: "Disable Codex plugins" })).toBeVisible();
      await refreshAuthority(card);
      await expect(card).toContainText("ready · available");
      await expect(card).toContainText(/Ready · generation/);
      await captureAuthority(page, testInfo, "codex-plugin-authority-ready");

      unlinkSync(globalAuth);
      await toggleAuthority(card, "Disable Codex plugins");
      await expect(card).toContainText("disabled · unavailable");
      await toggleAuthority(card, "Enable Codex plugins");
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

async function toggleAuthority(card: Locator, name: "Enable Codex plugins" | "Disable Codex plugins") {
  const toggle = card.getByRole("switch", { name });
  await expect(toggle).toBeEnabled();
  await toggle.evaluate((element) => (element as HTMLButtonElement).click());
  await expect(card.getByRole("switch", {
    name: name === "Enable Codex plugins" ? "Disable Codex plugins" : "Enable Codex plugins"
  })).toBeEnabled();
}

function writeState(statePath: string, state: CodexFixtureState) {
  writeFileSync(statePath, `${JSON.stringify(state)}\n`);
}

const CODEX_APP_SERVER_FIXTURE = `#!/usr/bin/env node
import { readFileSync, writeFileSync } from "node:fs";
import readline from "node:readline";

const statePath = process.env.PSYCHEVO_CODEX_PLUGIN_FIXTURE_STATE;
if (!statePath) throw new Error("missing PSYCHEVO_CODEX_PLUGIN_FIXTURE_STATE");
const readState = () => JSON.parse(readFileSync(statePath, "utf8"));
const reply = (message) => process.stdout.write(JSON.stringify(message) + "\\n");
const catalog = (state) => ({
  marketplaces: [{
    name: "openai",
    path: null,
    plugins: [{
      id: "review@openai",
      name: "review",
      description: "Review changes with Codex Apps",
      installed: state.installed,
      enabled: state.installed,
      localVersion: state.installed ? "1.0.0" : null
    }]
  }],
  marketplaceLoadErrors: [],
  featuredPluginIds: ["review@openai"]
});
const plugin = (state) => ({
  marketplaceName: "openai",
  summary: {
    id: "review@openai",
    name: "review",
    description: "Review changes with Codex Apps",
    installed: state.installed,
    enabled: state.installed,
    localVersion: state.installed ? "1.0.0" : null
  },
  skills: [{ name: "review", path: "remote://review/SKILL.md", enabled: true }],
  hooks: [{ event: "after_tool", path: "remote://review/hook.json" }],
  mcpServers: [{ name: "review_remote", url: "https://plugins.example.test/mcp", remote: true }],
  apps: [{ id: "review-app", installUrl: "https://apps.example.test/install/review" }],
  appTemplates: [{ id: "review-template", appId: "review-app" }],
  scheduledTasks: [{ id: "weekly-review" }],
  browserExtensions: [{ id: "review-browser" }],
  futureField: { detected: true }
});

const input = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of input) {
  if (!line.trim()) continue;
  const message = JSON.parse(line);
  const method = message.method;
  if (method === "initialized") continue;
  const state = readState();
  if (method === "initialize") {
    reply({ jsonrpc: "2.0", id: message.id, result: {
      codexHome: process.env.CODEX_HOME,
      platformFamily: "unix",
      platformOs: "linux",
      userAgent: "visual-fixture/" + state.version
    } });
    continue;
  }
  if (message.params == null) {
    reply({ jsonrpc: "2.0", id: message.id, error: { code: -32602, message: "invalid params" } });
    continue;
  }
  if (method === "plugin/list" || method === "plugin/installed") {
    reply({ jsonrpc: "2.0", id: message.id, result: catalog(state) });
  } else if (method === "plugin/read") {
    if (state.installed && state.failReadAfterInstall) {
      reply({ jsonrpc: "2.0", id: message.id, error: { code: -32000, message: "deterministic detail reread failure" } });
    } else {
      reply({ jsonrpc: "2.0", id: message.id, result: { plugin: plugin(state) } });
    }
  } else if (method === "plugin/install") {
    writeFileSync(statePath, JSON.stringify({ ...state, installed: true }) + "\\n");
    reply({ jsonrpc: "2.0", id: message.id, result: { authPolicy: "ON_USE", appsNeedingAuth: [] } });
  } else if (method === "hooks/list") {
    reply({ jsonrpc: "2.0", id: message.id, result: { data: [] } });
  } else if (method === "app/list") {
    reply({ jsonrpc: "2.0", id: message.id, result: { data: [{ id: "review-app", isAccessible: false, installUrl: "https://apps.example.test/install/review" }] } });
  } else {
    reply({ jsonrpc: "2.0", id: message.id, error: { code: -32601, message: "method not found" } });
  }
}
`;
