import { expect, test } from "@playwright/test";
import { startPevoWeb } from "./harness";
import {
  assertNoHorizontalOverflow,
  captureWorkbench,
  expectNoTransientAssistantDuplicateDuring,
  openPanel,
  startAutomationToolMockProvider
} from "./workbench.support";

test.describe("pevo Web Workbench", () => {
  test("renders Automations as a compact app-level surface", async ({ page, isMobile }, testInfo) => {
    const server = await startPevoWeb({ live: false });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      if (isMobile) {
        await openPanel(page, isMobile, "History");
      }
      await page.getByRole("button", { name: "Automations" }).click();

      const automations = page.getByRole("region", { name: "Automations" });
      await expect(automations).toBeVisible();
      await expect(page.locator(".composerDock")).toHaveCount(0);
      await expect(automations.locator(".automationTitleBlock p")).toHaveCount(0);
      await expect(automations.getByRole("button", { name: "Refresh" })).toBeVisible();
      await expect(automations.getByLabel("Workspace")).toBeVisible();
      await expect(automations.getByLabel("Automation description")).toBeVisible();
      await expect(automations.getByRole("button", { name: "Draft" })).toBeVisible();
      const projectTemplateButton = automations.getByRole("button", { name: "Project check" });
      await expect(projectTemplateButton).toHaveCount(1);
      await expect(automations.getByRole("button", { name: "Thread heartbeat" })).toHaveCount(1);
      await expect(automations.getByRole("form", { name: "Automation draft" })).toHaveCount(0);
      await expect(automations.locator(".automationDraftPlaceholder")).toHaveCount(0);
      await assertNoHorizontalOverflow(page, automations);
      const emptyPaneCenter = await automations.locator(".automationListPane").evaluate((element) => {
        const page = element.closest(".automationsPage");
        const paneBox = element.getBoundingClientRect();
        const pageBox = page?.getBoundingClientRect();
        if (!pageBox) {
          return null;
        }
        return Math.abs((paneBox.left + paneBox.width / 2) - (pageBox.left + pageBox.width / 2));
      });
      expect(emptyPaneCenter).not.toBeNull();
      expect(emptyPaneCenter!).toBeLessThanOrEqual(isMobile ? 2 : 12);
      await captureWorkbench(page, testInfo, `automations-empty-${isMobile ? "mobile" : "desktop"}`);

      await projectTemplateButton.click();
      const draft = automations.getByRole("form", { name: "Automation draft" });
      await expect(draft).toBeVisible();
      await expect(draft.getByLabel("Bind to")).toBeVisible();
      await expect(draft.getByLabel("Draft workspace")).toBeVisible();
      const draftCenter = await draft.evaluate((element) => {
        const page = element.closest(".automationsPage");
        const draftBox = element.getBoundingClientRect();
        const pageBox = page?.getBoundingClientRect();
        if (!pageBox) {
          return null;
        }
        return Math.abs((draftBox.left + draftBox.width / 2) - (pageBox.left + pageBox.width / 2));
      });
      expect(draftCenter).not.toBeNull();
      expect(draftCenter!).toBeLessThanOrEqual(isMobile ? 2 : 12);
      await draft.getByLabel("Title").fill("Morning repository check");
      await draft.getByLabel("Prompt").fill("Review current repository state and summarize risky work.");
      await expect(draft.getByRole("button", { name: "Auto in sandbox" })).toHaveClass(/is-selected/);
      await expect(draft.getByRole("button", { name: "Ask first" })).toBeVisible();
      await captureWorkbench(page, testInfo, `automations-draft-${isMobile ? "mobile" : "desktop"}`);
      await draft.getByRole("button", { name: "Save" }).click();

      await expect(automations.getByText("Morning repository check")).toBeVisible();
      await expect(automations.getByText("every 60m")).toBeVisible();
      await expect(automations.locator(".automationMeta").getByText("project", { exact: true })).toBeVisible();
      await expect(automations.getByRole("button", { name: "Run" })).toBeVisible();
      await assertNoHorizontalOverflow(page, automations);
      await captureWorkbench(page, testInfo, `automations-project-${isMobile ? "mobile" : "desktop"}`);
    } finally {
      await server.stop();
    }
  });

  test("starts a new session from Automations", async ({ page, isMobile }) => {
    const server = await startPevoWeb({ live: false });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      if (isMobile) {
        await openPanel(page, isMobile, "History");
      }
      await page.getByRole("button", { name: "Automations" }).click();
      await expect(page.getByRole("region", { name: "Automations" })).toBeVisible();

      if (isMobile) {
        await openPanel(page, isMobile, "History");
      }
      await page.getByRole("button", { name: "New Session", exact: true }).click();

      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      await expect(page.getByRole("region", { name: "Automations" })).toHaveCount(0);
    } finally {
      await server.stop();
    }
  });

  test("creates a first-turn Automations task through deterministic composer without current thread failure", async ({ page, isMobile }, testInfo) => {
    test.skip(isMobile, "deterministic automation composer validation runs once on desktop");
    const mockProvider = await startAutomationToolMockProvider();
    const server = await startPevoWeb({
      live: false,
      model: "mock/automation",
      configAppend: `
[provider.mock.options]
base_url = "${mockProvider.baseUrl}"
api_key_env = "MOCK_PROVIDER_KEY"

[provider.mock.models.automation]
`,
      envFile: "MOCK_PROVIDER_KEY=test-key\n"
    });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();

      await page.getByPlaceholder("Ask Psychevo...").fill(
        "请创建一个自动化：每分钟发送一条最有价值的软件工程 tip。标题必须是 pevo-deterministic-engineering-tip。不要显式指定 target。"
      );
      await page.getByRole("button", { name: "Send message" }).click();

      const finalRows = page.locator(".pevo-message.is-assistant").filter({ hasText: /pevo-deterministic-engineering-tip/ });
      await expectNoTransientAssistantDuplicateDuring(page, testInfo, finalRows, "deterministic-automation", 60_000, 1_000);
      await assertNoHorizontalOverflow(page, page.getByRole("region", { name: "Transcript" }));
      await captureWorkbench(page, testInfo, "deterministic-automation-transcript");

      await page.getByRole("button", { name: "Automations" }).click();
      const automations = page.getByRole("region", { name: "Automations" });
      await expect(automations.getByText("pevo-deterministic-engineering-tip").first()).toBeVisible();
      await expect(automations.getByText("every 1m").first()).toBeVisible();
      await expect(automations.locator(".automationMeta").getByText("thread", { exact: true }).first()).toBeVisible();
      await assertNoHorizontalOverflow(page, automations);
      await captureWorkbench(page, testInfo, "deterministic-automation-list");
      expect(mockProvider.requests.length).toBeGreaterThanOrEqual(2);
    } finally {
      await server.stop();
      await mockProvider.close();
    }
  });
});
