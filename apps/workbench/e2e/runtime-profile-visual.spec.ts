import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, test, type Page, type TestInfo } from "@playwright/test";
import { startPevoWeb } from "./harness";
import { prepareDeterministicRuntime } from "./runtime-live.support";
import { visualScreenshotRoot } from "./visualArtifacts";

const screenshotDir = visualScreenshotRoot();

test.describe("Runtime Profile targeted visual contract", () => {
  test("shows readiness, structured editing, and native session ownership", async ({ page, isMobile }, testInfo) => {
    test.setTimeout(180_000);
    mkdirSync(screenshotDir, { recursive: true });
    const fixture = prepareDeterministicRuntime("codex", screenshotDir);
    const server = await startPevoWeb({ configAppend: fixture.configAppend, live: false });
    try {
      await page.goto(server.url);
      await selectRuntime(page, "Codex");
      await runTurn(page, "create a native session for visual inspection", fixture.expectedAnswer);

      const detail = await openRuntimeProfileDetail(page, "codex", isMobile);
      await expect(detail.getByRole("region", { name: "Runtime readiness" })).toContainText("Capabilities");
      await expect(detail).toContainText("Last checked");
      await capture(page, testInfo, "runtime-profile-detail");

      await detail.getByRole("button", { name: "Load sessions" }).click();
      await expect(detail.locator(".runtimeSessionRow").first()).toBeVisible({ timeout: 30_000 });
      await expect(detail).toContainText(/Read-write|Read-only|Runtime active/);
      await capture(page, testInfo, "runtime-native-sessions");

      const editProfile = detail.getByRole("button", { name: "Edit", exact: true });
      await expect(editProfile).toBeEnabled();
      await editProfile.click();
      const editor = detail.getByRole("form", { name: "Runtime Profile" });
      await expect(editor.getByLabel("Runtime Profile runtime")).toHaveValue("codex");
      await expect(editor.getByLabel("Runtime Profile approval mode")).toHaveValue("on-request");
      await capture(page, testInfo, "runtime-profile-editor");
    } finally {
      await server.stop();
    }
  });

  test("shows real public child provenance and Codex authorization lifetime in Shared Attention", async ({ page, isMobile }, testInfo) => {
    test.setTimeout(180_000);
    mkdirSync(screenshotDir, { recursive: true });
    const fixture = prepareDeterministicRuntime("codex", screenshotDir, "child_interaction");
    const server = await startPevoWeb({ configAppend: fixture.configAppend, live: false });
    try {
      await page.goto(server.url);
      await selectRuntime(page, "Codex");
      await runTurn(page, "establish a deterministic native child", "child ready");
      const childTab = page.getByRole("button", { name: "Codex child", exact: true });
      await expect(childTab).toBeVisible({ timeout: 30_000 });
      await childTab.click();
      const childPanel = page.getByRole("region", { name: "Codex child" });
      const childThreadId = await childPanel.locator(".threadPanelTitle p").textContent();
      const parentOrigin = await childPanel.locator(".threadPanelParent").textContent();
      expect(childThreadId).toMatch(
        /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/
      );
      expect(parentOrigin).toMatch(
        /^Parent [0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/
      );
      const parentThreadId = parentOrigin!.slice("Parent ".length);
      expect(childThreadId).not.toBe(parentThreadId);
      await expect(childPanel.getByRole("note")).toContainText("Read-only runtime child");
      if (isMobile) {
        await page.getByRole("button", { name: "Transcript", exact: true }).click();
      }
      const composer = page.getByPlaceholder("Ask Psychevo...");
      await expect(composer).toBeVisible();
      await composer.fill("request deterministic runtime authorization");
      await page.getByRole("button", { name: "Send message" }).click();

      const attention = page.getByLabel("Shared Attention context").first();
      await expect(attention).toBeVisible({ timeout: 30_000 });
      await expect(attention).toContainText("Codex · Codex (codex)");
      const origin = attention.locator("span").filter({
        hasText: /^Child [0-9a-f-]+ · Parent [0-9a-f-]+$/
      });
      await expect(origin).toHaveText(`Child ${childThreadId} · Parent ${parentThreadId}`);
      await expect(attention).toContainText("Once · this request only");
      await expect(attention).toContainText("Session · current Codex session");
      const attentionText = await attention.textContent();
      expect(attentionText).not.toContain("native-1");
      expect(attentionText).not.toContain("child-1");

      const request = page.getByLabel("Pending requests").locator(".composerRequest").first();
      await expect(request.getByRole("button", { name: "Once", exact: true })).toBeVisible();
      await expect(request.getByRole("button", { name: "Session", exact: true })).toBeVisible();
      await expect(request.getByRole("button", { name: "Always", exact: true })).toHaveCount(0);
      await expect(page.getByRole("button", { name: /^Bound Runtime Profile / })).toContainText("Direct");
      await capture(page, testInfo, "runtime-shared-attention");
    } finally {
      await server.stop();
    }
  });

  test("shows OpenCode plan and diff observations from typed timeline events", async ({ page, isMobile }, testInfo) => {
    test.setTimeout(180_000);
    mkdirSync(screenshotDir, { recursive: true });
    const fixture = prepareDeterministicRuntime("opencode", screenshotDir);
    const server = await startPevoWeb({ configAppend: fixture.configAppend, live: false });
    try {
      await page.goto(server.url);
      await selectRuntime(page, "OpenCode");
      await runTurn(page, "emit deterministic typed timeline state", fixture.expectedAnswer);

      await expect(
        page.locator(".pevo-message.is-user").filter({ hasText: "emit deterministic typed timeline state" })
      ).toHaveCount(1);
      await expect(page.locator(".pevo-evidence").filter({ hasText: "Plan" })).toContainText(
        "Validate direct runtime"
      );
      await expect(page.locator(".pevo-evidence").filter({ hasText: "Diff" })).toContainText(
        "runtime-live.ts"
      );
      await capture(page, testInfo, "runtime-opencode-timeline");

      const detail = await openRuntimeProfileDetail(page, "opencode", isMobile);
      await detail.getByRole("button", { name: "Load sessions" }).click();
      const loadRevisions = detail.getByRole("button", { name: "Load revisions" }).first();
      await expect(loadRevisions).toBeVisible({ timeout: 30_000 });
      await loadRevisions.click();
      await expect(detail.getByLabel(/Revert point for/).first()).toBeVisible();
      await expect(detail.getByRole("button", { name: "Revert" }).first()).toBeEnabled();
      await capture(page, testInfo, "runtime-opencode-revert");
    } finally {
      await server.stop();
    }
  });
});

async function selectRuntime(page: Page, label: "Codex" | "OpenCode") {
  await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
  await page.getByRole("button", { name: "Runtime Profile" }).click();
  await page
    .getByRole("dialog", { name: "Runtime Profile selection" })
    .getByRole("radio", { name: label, exact: true })
    .click();
}

async function runTurn(page: Page, prompt: string, answer: string) {
  await page.getByPlaceholder("Ask Psychevo...").fill(prompt);
  await page.getByRole("button", { name: "Send message" }).click();
  await expect(page.locator(".pevo-message.is-assistant").filter({ hasText: answer })).toHaveCount(1, {
    timeout: 60_000
  });
  await expect(page.locator(".pevo-composer").first()).not.toHaveClass(/is-running/, { timeout: 30_000 });
}

async function openRuntimeProfileDetail(page: Page, runtimeRef: string, isMobile: boolean) {
  if (isMobile) {
    await page.getByRole("button", { name: "History", exact: true }).click();
  }
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
