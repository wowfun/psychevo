import { existsSync, mkdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page, type TestInfo } from "@playwright/test";
import { repoRoot, startPevoWeb } from "./harness";
import { prepareDeterministicAcpAgent } from "./runtime-live.support";
import { visualScreenshotRoot } from "./visualArtifacts";

const screenshotDir = visualScreenshotRoot("acp-peer-visual");

test.describe("Workbench stable ACP v1 Agent visual streaming", () => {
  test("configures one ACP backend and renders the common standard event stream", async ({ page, isMobile }, testInfo) => {
    test.setTimeout(180_000);
    mkdirSync(screenshotDir, { recursive: true });
    const fixture = prepareDeterministicAcpAgent("codex", screenshotDir);
    const server = await startPevoWeb({ live: false });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();

      if (isMobile) await openPanel(page, isMobile, "History");
      let agentsPanel = await openCapabilityBackendPanel(page);
      await agentsPanel.getByRole("button", { name: "Add ACP backend" }).click();

      const form = agentsPanel.getByRole("form", { name: "Profile ACP backend" });
      await expect(form).toBeVisible();
      await form.getByLabel("ID").fill("visual-acp");
      const commandJson = form.getByLabel("Command JSON");
      await commandJson.fill(JSON.stringify({
        command: fixture.command,
        args: fixture.args,
        env: {}
      }, null, 2));
      await expect(form.getByRole("button", { name: "Save" })).toBeEnabled();
      await commandJson.blur();
      await commandJson.evaluate((element) => { element.scrollTop = 0; });
      expect(await commandJson.evaluate((element) => element.scrollTop)).toBe(0);
      await capture(page, testInfo, `01-stable-v1-backend-form-${projectSuffix(isMobile)}`);
      await form.getByRole("button", { name: "Save" }).click();
      await expect(form).toBeHidden({ timeout: 30_000 });

      agentsPanel = await openCapabilityBackendPanel(page);
      await expect(agentsPanel.getByRole("switch", { name: "Disable visual-acp" })).toBeVisible();
      await expect(agentsPanel.getByLabel("visual-acp peer entrypoint")).toBeChecked();
      await expect(agentsPanel.getByLabel("visual-acp subagent entrypoint")).toBeChecked();
      await capture(page, testInfo, `02-stable-v1-backend-configured-${projectSuffix(isMobile)}`);

      if (isMobile) await openPanel(page, isMobile, "History");
      await page.getByRole("button", { name: "New Session", exact: true }).click();
      await openPanel(page, isMobile, "Transcript");
      const targetControl = page.getByRole("button", { name: "Agent target", exact: true });
      await expect(targetControl).toContainText("Psychevo");
      await expect(targetControl).not.toContainText("Psychevo (Native)");
      await expect(targetControl).toHaveAttribute("title", "Psychevo · Psychevo (Native)");
      await targetControl.click();
      const popover = page.getByRole("dialog", { name: "Agent target" });
      const targetGroup = popover.getByRole("radiogroup", { name: "Agent target" });
      await expect(targetGroup.getByRole("radio", { name: "Psychevo · Psychevo (Native)" })).toHaveAttribute("aria-checked", "true");
      const acpTarget = targetGroup.getByRole("radio", { name: "visual-acp · visual-acp (ACP)" });
      await expect(acpTarget).toBeVisible();
      await acpTarget.click();
      await expect(targetControl).toContainText("visual-acp");
      await expect(targetControl).toContainText("visual-acp (ACP)");
      await expect(page.getByText("Runtime default", { exact: true })).toHaveCount(0);

      const prompt = "Exercise the stable ACP v1 standard event stream.";
      await page.getByPlaceholder("Ask Psychevo...").fill(prompt);
      await page.getByRole("button", { name: "Send message" }).click();

      const assistantMessage = page.locator(".pevo-message.is-assistant").last();
      await expect(assistantMessage).toContainText("Codex ACP response");
      await expect(assistantMessage).toContainText("model=fixture/default");
      await expect(assistantMessage).toContainText("mode=build");
      const reasoning = page.locator(".pevo-reasoning").last();
      const reasoningHeader = reasoning.getByRole("button", { name: "Thinking", exact: true });
      await expect(reasoningHeader).toHaveAttribute("aria-expanded", "false");
      await expect(page.locator(".pevo-evidence").filter({ hasText: "Inspect ACP fixture" })).toBeVisible();
      await expect(page.locator(".pevo-evidence").filter({ hasText: "Plan" })).toContainText(
        "Project through the common application path"
      );
      await assertNoHorizontalOverflow(page, page.getByRole("region", { name: "Transcript" }));
      await assertTranscriptRowsFit(page);
      await capture(page, testInfo, `03-stable-v1-stream-${projectSuffix(isMobile)}`);
      await reasoningHeader.click();
      await expect(reasoning).toContainText("stable v1 reasoning");

      const initialize = await expect.poll(() => traceEvents(fixture.logPath).find((event) => event.type === "initialize"), {
        timeout: 30_000
      }).not.toBeUndefined();
      void initialize;
      expect(traceEvents(fixture.logPath).find((event) => event.type === "initialize")?.requestedProtocolVersion).toBe(1);

      await openPanel(page, isMobile, "Status");
      const statusRegion = page.getByRole("region", { name: "Workspace status" });
      await expect(statusRegion.getByRole("region", { name: "Session observability" })).toContainText("exact");
      await expect(statusRegion).not.toContainText("reported by ACP peer");
      await expect(statusRegion).toContainText("Session tokens");
      await expect(statusRegion).toContainText("129");
      await assertNoWorkbenchRenderError(page);
      await assertNoHorizontalOverflow(page, statusRegion);
      await capture(page, testInfo, `04-stable-v1-final-${projectSuffix(isMobile)}`);

      if (isMobile) await openPanel(page, isMobile, "History");
      await page.getByRole("button", { name: "New Session", exact: true }).click();
      await openPanel(page, isMobile, "Status");
      const draftStatusRegion = page.getByRole("region", { name: "Workspace status" });
      await expect(draftStatusRegion.getByText("draft")).toBeVisible();
      await expect(draftStatusRegion).toContainText(/No active (session|context)/);
      await expect(draftStatusRegion).toContainText("No session usage yet.");
      await assertNoHorizontalOverflow(page, draftStatusRegion);
      await capture(page, testInfo, `05-new-draft-status-${projectSuffix(isMobile)}`);
    } finally {
      await server.stop();
    }
  });
});

async function openPanel(page: Page, isMobile: boolean, name: "History" | "Status" | "Transcript") {
  if (name === "Status") {
    if (isMobile) await page.getByRole("button", { name: "Transcript" }).click();
    const expandInspector = page.getByRole("button", { name: "Show right inspector" });
    const collapseInspector = page.getByRole("button", { name: "Collapse right inspector" });
    if (await collapseInspector.count() === 0) {
      await expect(expandInspector).toBeVisible();
      await expandInspector.click();
      await expect(collapseInspector).toBeVisible();
    }
  }
  if (isMobile) await page.getByRole("button", { name, exact: true }).click();
  if (name === "Status") {
    await expect(page.getByRole("region", { name: "Workspace status" })).toBeVisible();
  }
}

async function openCapabilityBackendPanel(page: Page): Promise<Locator> {
  for (let attempt = 0; attempt < 3; attempt += 1) {
    try {
      let capabilities = page.getByRole("region", { name: "Capabilities" });
      if (!(await capabilities.count()) || !(await capabilities.isVisible().catch(() => false))) {
        await page.getByRole("button", { name: "Capabilities" }).click();
        capabilities = page.getByRole("region", { name: "Capabilities" });
      }
      await expect(capabilities).toBeVisible();
      const agentsTopTab = capabilities.getByRole("tab", { name: "Agents" });
      if ((await agentsTopTab.getAttribute("aria-selected")) !== "true") await agentsTopTab.click();
      const backendsTab = capabilities.getByRole("tab", { name: "ACP Backends" });
      if ((await backendsTab.getAttribute("aria-selected")) !== "true") await backendsTab.click();
      const agentsPanel = capabilities.getByRole("region", { name: "Agents" });
      await expect(agentsPanel).toBeVisible();
      return agentsPanel;
    } catch (error) {
      if (attempt === 2) throw error;
      await page.waitForTimeout(100);
    }
  }
  throw new Error("unreachable");
}

function traceEvents(logPath: string): Array<Record<string, unknown>> {
  if (!existsSync(logPath)) return [];
  return readFileSync(logPath, "utf8")
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line) => JSON.parse(line) as Record<string, unknown>);
}

async function capture(page: Page, testInfo: TestInfo, label: string) {
  const fileName = `${label}-${testInfo.project.name}.png`;
  const stablePath = path.join(screenshotDir, fileName);
  await page.screenshot({ fullPage: true, path: stablePath });
  await testInfo.attach(fileName, { path: stablePath, contentType: "image/png" });
  process.stdout.write(`[acp-peer-visual] screenshot ${path.relative(repoRoot, stablePath)}\n`);
}

function projectSuffix(isMobile: boolean) {
  return isMobile ? "mobile" : "desktop";
}

async function assertNoHorizontalOverflow(page: Page, locator: Locator) {
  const [viewport, result] = await Promise.all([
    page.viewportSize(),
    locator.evaluate((element) => {
      const box = element.getBoundingClientRect();
      return {
        clientWidth: element.clientWidth,
        left: box.left,
        right: box.right,
        scrollWidth: element.scrollWidth
      };
    })
  ]);
  expect(viewport).not.toBeNull();
  expect(result.left).toBeGreaterThanOrEqual(-1);
  expect(result.right).toBeLessThanOrEqual(viewport!.width + 1);
  expect(result.scrollWidth).toBeLessThanOrEqual(result.clientWidth + 1);
}

async function assertTranscriptRowsFit(page: Page) {
  const violations = await page.locator(".pevo-threadItems > article, .pevo-messageFrame").evaluateAll((rows) =>
    rows.flatMap((row, index) => {
      const element = row as HTMLElement;
      const rowBox = element.getBoundingClientRect();
      const measured = Array.from(element.querySelectorAll<HTMLElement>(".pevo-message, .pevo-evidenceLine, .pevo-reasoningHeader"));
      return measured.flatMap((child) => {
        const childBox = child.getBoundingClientRect();
        const inside = childBox.left >= rowBox.left - 1 && childBox.right <= rowBox.right + 1;
        return inside ? [] : [{ index, text: child.textContent?.trim() ?? child.className }];
      });
    })
  );
  expect(violations).toEqual([]);
}

async function assertNoWorkbenchRenderError(page: Page) {
  const alert = page.getByRole("alert");
  const alertText = await alert.textContent().catch(() => null);
  if (alertText?.includes("Workbench render failed")) throw new Error(alertText);
}
