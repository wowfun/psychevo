import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page, type TestInfo } from "@playwright/test";
import { startPevoWeb } from "./harness";
import { prepareDeterministicAcpAgent } from "./runtime-live.support";
import { visualScreenshotRoot } from "./visualArtifacts";

const screenshotDir = visualScreenshotRoot();

test.describe("ACP Runtime Profile targeted visual contract", () => {
  test("shows ACP readiness, backend ownership, and backend doctor", async ({ page, isMobile }, testInfo) => {
    test.setTimeout(180_000);
    mkdirSync(screenshotDir, { recursive: true });
    const fixture = prepareDeterministicAcpAgent("codex", screenshotDir, "history");
    const server = await startPevoWeb({ configAppend: fixture.configAppend, live: false });
    try {
      await page.goto(server.url);
      await selectRuntime(page, fixture.profileLabel);
      await runTurn(page, "Create an agent-owned ACP session", /Codex ACP response/i);

      const detail = await openRuntimeProfileDetail(page, fixture.runtimeRef, isMobile);
      await expect(detail.getByRole("region", { name: "Runtime readiness" })).toBeVisible();
      await expect(detail).toContainText(/ACP/);
      await expect(detail).toContainText(/codex/);
      await capture(page, testInfo, "acp-profile-detail");

      await detail.getByRole("button", { name: "Doctor backend" }).click();
      await expect(detail.getByRole("region", { name: "ACP backend doctor" })).toBeVisible({ timeout: 30_000 });
      await capture(page, testInfo, "acp-backend-doctor");

      const editProfile = detail.getByRole("button", { name: "Edit", exact: true });
      await expect(editProfile).toBeEnabled();
      await editProfile.click();
      const editor = detail.getByRole("form", { name: "Runtime Profile" });
      await expect(editor.getByLabel("Runtime Profile runtime")).toHaveValue("acp");
      await expect(editor.getByLabel("Runtime Profile ACP backend ref")).toHaveValue(fixture.runtimeRef);
      await capture(page, testInfo, "acp-profile-editor");
    } finally {
      await server.stop();
    }
  });

  test("renders OpenCode through standard ACP facts without direct-only actions", async ({ page, isMobile }, testInfo) => {
    test.setTimeout(180_000);
    mkdirSync(screenshotDir, { recursive: true });
    const fixture = prepareDeterministicAcpAgent("opencode", screenshotDir);
    const server = await startPevoWeb({ configAppend: fixture.configAppend, live: false });
    try {
      await page.goto(server.url);
      await selectRuntime(page, fixture.profileLabel);
      await runTurn(page, "Render standard OpenCode ACP facts", /OpenCode ACP response/i);

      const reasoning = page.locator(".pevo-reasoning").last();
      await expect(reasoning).toBeVisible();
      await reasoning.getByRole("button", { name: "Thinking" }).click();
      await expect(reasoning).toContainText("stable v1 reasoning");
      await expect(page.locator(".pevo-evidence").filter({ hasText: "Inspect ACP fixture" })).toBeVisible();
      await expect(page.locator(".pevo-evidence").filter({ hasText: "Plan" })).toContainText(
        "Project through the common application path"
      );
      await expect(page.getByRole("button", { name: /Codex child|OpenCode child/ })).toHaveCount(0);
      await capture(page, testInfo, "opencode-acp-standard-timeline");

      const detail = await openRuntimeProfileDetail(page, "opencode", isMobile);
      await expect(detail).toContainText("opencode");
      await expect(detail.getByRole("button", { name: "Load sessions" })).toHaveCount(0);
      await expect(detail.getByRole("button", { name: "Load revisions" })).toHaveCount(0);
      await expect(detail.getByRole("button", { name: "Revert", exact: true })).toHaveCount(0);
      await expect(detail.getByRole("button", { name: /auth|login|repair/i })).toHaveCount(0);
      await capture(page, testInfo, "acp-unsupported-direct-actions");
    } finally {
      await server.stop();
    }
  });
});

async function selectRuntime(page: Page, profileLabel: string) {
  await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
  await page.getByRole("button", { name: "Agent target", exact: true }).click();
  const targets = page.getByRole("dialog", { name: "Agent target" })
    .getByRole("radiogroup", { name: "Agent target" });
  await targets.getByRole("radio", {
    name: new RegExp(` · ${escapeRegExp(profileLabel)}$`)
  }).click();
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

async function runTurn(page: Page, prompt: string, answer: RegExp) {
  await page.getByPlaceholder("Ask Psychevo...").fill(prompt);
  await page.getByRole("button", { name: "Send message" }).click();
  await expect(page.locator(".pevo-message.is-assistant").filter({ hasText: answer })).toHaveCount(1, {
    timeout: 60_000
  });
  await expect(page.locator(".pevo-composer").first()).not.toHaveClass(/is-running/, { timeout: 30_000 });
}

async function openRuntimeProfileDetail(page: Page, runtimeRef: string, isMobile: boolean): Promise<Locator> {
  if (isMobile) await page.getByRole("button", { name: "History", exact: true }).click();
  const utilities = page.getByRole("navigation", { name: "Workbench utilities" });
  await expect(utilities).toBeVisible();
  await utilities.getByRole("button", { name: "Capabilities", exact: true }).click();
  const capabilities = page.getByRole("region", { name: "Capabilities" });
  await expect(capabilities).toBeVisible();
  await capabilities.getByRole("tab", { name: "Agents" }).click();
  await capabilities.getByRole("tab", { name: "Runtime Profiles" }).click();
  await capabilities.getByRole("button", { name: `Runtime Profile ${runtimeRef}` }).click();
  return capabilities.getByRole("complementary", { name: "Runtime Profile detail" });
}

async function capture(page: Page, testInfo: TestInfo, name: string) {
  await page.screenshot({
    fullPage: true,
    path: path.join(screenshotDir, `${name}-${testInfo.project.name}.png`)
  });
}
