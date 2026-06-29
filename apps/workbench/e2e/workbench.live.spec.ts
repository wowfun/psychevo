import { expect, test } from "@playwright/test";
import { startPevoWeb } from "./harness";
import { liveContextFor } from "./liveContext";
import {
  LIVE_TRANSLATE_SUBAGENT_PROMPT,
  assertNoHorizontalOverflow,
  captureWorkbench,
  ensureLiveAutomationCwd,
  ensureLiveSubagentCwd,
  expectNoTransientAssistantDuplicateDuring
} from "./workbench.support";

test.describe("pevo Web Workbench", () => {
  test("submits a real provider turn through the composer @live", async ({ page, isMobile }) => {
    const context = liveContextFor("web-composer-live");
    if (!context) {
      test.skip(true, "run through cargo xtask live");
      return;
    }
    test.skip(isMobile, "live provider validation runs once on the desktop project");
    const server = await startPevoWeb({
      live: true,
      model: context.model,
      configPath: context.configPath,
      dbPath: context.dbPath,
      home: context.home,
      pevoBin: context.pevoBin
    });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();

      await page.getByPlaceholder("Ask Psychevo...").fill(
        "Reply with exactly this text and nothing else: psychevo web live ok"
      );
      await page.getByRole("button", { name: "Send message" }).click();

      await expect(
        page.locator(".pevo-message.is-assistant").getByText(/psychevo web live ok/i)
      ).toBeVisible({ timeout: 240_000 });
    } finally {
      await server.stop();
    }
  });

  test("creates an automation through the live GUI without duplicating the final answer @live", async ({ page, isMobile }, testInfo) => {
    const context = liveContextFor("web-automation-live");
    if (!context) {
      test.skip(true, "run through cargo xtask live");
      return;
    }
    test.skip(isMobile, "live automation validation runs once on the desktop project");
    test.setTimeout(context.timeoutMs);
    const server = await startPevoWeb({
      live: true,
      model: context.model,
      configPath: context.configPath,
      dbPath: context.dbPath,
      home: context.home,
      pevoBin: context.pevoBin,
      cwd: ensureLiveAutomationCwd(context.cwd)
    });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();

      await page.getByPlaceholder("Ask Psychevo...").fill(
        "请使用 automation 工具创建一个自动化。标题必须是 pevo-live-engineering-tip，schedule 必须是 interval everyMinutes=1（当前系统不支持秒级，使用最快支持间隔），prompt 是：每次发送一条最有价值的软件工程 tip。不要显式指定 project 或 currentThread；按当前对话的默认目标创建。请实际创建，不要等待触发，不要只说明。"
      );
      await page.getByRole("button", { name: "Send message" }).click();

      const finalRows = page.locator(".pevo-message.is-assistant").filter({ hasText: /pevo-live-engineering-tip/ });
      await expectNoTransientAssistantDuplicateDuring(page, testInfo, finalRows, "live-automation", 240_000, 8_000);
      await assertNoHorizontalOverflow(page, page.getByRole("region", { name: "Transcript" }));
      await captureWorkbench(page, testInfo, "live-automation-transcript");

      await page.getByRole("button", { name: "Automations" }).click();
      const automations = page.getByRole("region", { name: "Automations" });
      await expect(automations).toBeVisible();
      await expect(automations.getByText("pevo-live-engineering-tip").first()).toBeVisible({ timeout: 30_000 });
      await expect(automations.getByText("every 1m").first()).toBeVisible();
      await assertNoHorizontalOverflow(page, automations);
      await captureWorkbench(page, testInfo, "live-automation-list");
    } finally {
      await server.stop();
    }
  });

  test("opens live translate subagent sessions from the GUI @live", async ({ page, isMobile }, testInfo) => {
    const context = liveContextFor("web-subagent-live");
    if (!context) {
      test.skip(true, "run through cargo xtask live");
      return;
    }
    test.skip(isMobile, "live provider validation runs once on the desktop project");
    test.setTimeout(context.timeoutMs);
    const server = await startPevoWeb({
      live: true,
      model: context.model,
      configPath: context.configPath,
      dbPath: context.dbPath,
      home: context.home,
      pevoBin: context.pevoBin,
      cwd: ensureLiveSubagentCwd(context.cwd)
    });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();

      await page.getByPlaceholder("Ask Psychevo...").fill(LIVE_TRANSLATE_SUBAGENT_PROMPT);
      await page.getByRole("button", { name: "Send message" }).click();

      const openAgentButtons = page.getByRole("button", { name: /Open .*agent session/i });
      await expect.poll(async () => openAgentButtons.count(), { timeout: 240_000 }).toBeGreaterThanOrEqual(2);
      await captureWorkbench(page, testInfo, "live-translate-agent-rows");

      await openAgentButtons.first().click();
      await expect(page.locator(".threadPanel")).toBeVisible({ timeout: 30_000 });
      await expect(page.locator(".threadPanel")).toContainText(/Parent/);
      await expect(page.locator(".threadPanel .pevo-message").first()).toBeVisible({ timeout: 120_000 });
      await captureWorkbench(page, testInfo, "live-translate-agent-session");
    } finally {
      await server.stop();
    }
  });
});
