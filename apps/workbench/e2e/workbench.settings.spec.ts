import { expect, test } from "@playwright/test";
import { startPevoWeb } from "./harness";
import {
  CHANNELS_VISUAL_CONFIG,
  CHANNELS_VISUAL_ENV,
  assertNoHorizontalOverflow,
  captureChannelsWorkbench,
  captureWorkbench,
  expectControlsFitHorizontally,
  expectModelRowsFillPopover,
  expectSettingsGutterScrollsContent,
  openPanel
} from "./workbench.support";

test.describe("pevo Web Workbench", () => {
  test("shows Settings as an app-level configuration center", async ({ page, isMobile }, testInfo) => {
    const server = await startPevoWeb({ live: false });
    try {
          await page.goto(server.url);
          await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
        await page.getByRole("button", { name: "Agent Definition", exact: true }).click();
        await expect(page.getByRole("dialog", { name: "Agent Definition and Runtime Profile" }).getByRole("radiogroup", { name: "Main agent" }).getByRole("radio", { name: "translate" })).toBeVisible();
        await page.getByRole("button", { name: "Agent Definition", exact: true }).click();
        if (isMobile) {
          await openPanel(page, isMobile, "History");
        }
      await page.getByRole("button", { name: "Settings" }).click();

      const settings = page.getByRole("region", { name: "Settings", exact: true });
      await expect(settings).toBeVisible();
      await expect(settings.locator(".centerPageTitle p")).toHaveCount(0);
      await expect(settings.getByRole("heading", { name: "Settings" })).toHaveCount(0);
      await expect(settings.getByRole("button", { name: "Back to transcript" })).toHaveCount(0);
      await expect(settings.getByRole("button", { name: "Back to app" })).toBeVisible();
      await expect(settings.getByRole("searchbox", { name: "Search settings" })).toBeVisible();
      await expect(settings.getByRole("button", { name: "Appearance" })).toBeVisible();
      await expect(settings.getByRole("button", { name: "Debug" })).toBeVisible();
      await expect(settings.getByRole("button", { name: "Agents" })).toHaveCount(0);
      await expect(settings.getByRole("button", { name: "Models" })).toBeVisible();
      await expect(settings.getByRole("button", { name: "Archived sessions" })).toBeVisible();
      await expect(settings.getByRole("button", { name: "General", exact: true })).toHaveCount(0);
      await expect(settings.getByRole("button", { name: "Session", exact: true })).toHaveCount(0);
      await expect(settings.getByRole("button", { name: "Session history", exact: true })).toHaveCount(0);
      await expect(settings.getByRole("button", { name: "Commands", exact: true })).toHaveCount(0);
      await expect(settings.getByRole("button", { name: "Artifacts", exact: true })).toHaveCount(0);
      await expect(page.locator(".historyColumn")).toBeHidden();
      await expect(page.locator(".statusColumn")).toBeHidden();
      await expect(page.locator(".composerDock")).toHaveCount(0);
      await expect(page.locator(".mobileTabs")).toBeHidden();

      if (isMobile) {
        await expect(settings.locator(".settingsNavGroups")).toHaveCSS("display", "flex");
      } else {
        const [navBox, contentBox] = await Promise.all([
          settings.locator(".settingsNav").boundingBox(),
          settings.locator(".settingsContent").boundingBox()
        ]);
        expect(navBox).not.toBeNull();
        expect(contentBox).not.toBeNull();
        expect(navBox!.x + navBox!.width).toBeLessThanOrEqual(contentBox!.x);
        expect(navBox!.y).toBeLessThan(70);
        expect(contentBox!.y).toBeLessThan(120);
      }
      await assertNoHorizontalOverflow(page, settings);
      await captureWorkbench(page, testInfo, `settings-appearance-${isMobile ? "mobile" : "desktop"}`);

      await settings.getByRole("button", { name: "Light" }).click();
      await expect(page.locator("html")).toHaveAttribute("data-pevo-appearance", "light");
      await settings.getByRole("button", { name: "Usage" }).click();
      const usage = settings.getByRole("region", { name: "Usage" });
      await expect(usage).toBeVisible();
      const heatmap = usage.getByRole("region", { name: "Token activity" });
      await expect(heatmap).toBeVisible();
      await heatmap.locator(".usageHeatmapGrid").evaluate((grid) => {
        const cells = Array.from(grid.querySelectorAll<HTMLElement>(".usageHeatmapCell:not(.is-empty)")).slice(0, 5);
        cells.forEach((cell, index) => {
          cell.dataset.level = String(index);
          cell.title = index === 0 ? "No tokens" : `Level ${index}`;
        });
      });
      const heatmapColors = await heatmap.locator(".usageHeatmapCell:not(.is-empty)").evaluateAll((cells) => (
        Array.from(new Set(cells.slice(0, 5).map((cell) => getComputedStyle(cell).backgroundColor)))
      ));
      expect(heatmapColors.length).toBeGreaterThanOrEqual(5);
      await assertNoHorizontalOverflow(page, settings);
      await captureWorkbench(page, testInfo, `settings-usage-light-${isMobile ? "mobile" : "desktop"}`);
      await settings.getByRole("button", { name: "Appearance" }).click();
      await settings.getByRole("button", { name: "Dark" }).click();
      await expect(page.locator("html")).toHaveAttribute("data-pevo-appearance", "dark");

      await settings.getByRole("button", { name: "Debug" }).click();
      await expect(settings.getByRole("heading", { name: "Debug" })).toBeVisible();
      await assertNoHorizontalOverflow(page, settings);
      await captureWorkbench(page, testInfo, `settings-debug-${isMobile ? "mobile" : "desktop"}`);

      await settings.getByRole("button", { name: "Models" }).click();
      const models = settings.getByRole("region", { name: "Models", exact: true });
      await expect(models).toBeVisible();
      await expect(models.getByRole("button", { name: "Default model" })).toBeVisible();
      await expect(models.getByRole("button", { name: "Title generation" })).toBeVisible();
      await expect(models.getByRole("button", { name: "Context compression" })).toBeVisible();
      await expect(models.getByRole("combobox", { name: /model|reasoning/i })).toHaveCount(0);
      await expect(models.locator('input[placeholder="provider/model"]')).toHaveCount(0);
      await expect(models.getByText("OpenCode Zen")).toBeVisible();
      await expect(models.getByText("Default reasoning")).toHaveCount(0);
      await models.getByRole("button", { name: "Default model" }).click();
      const defaultPicker = models.getByRole("dialog", { name: "Default model and reasoning" });
      await expect(defaultPicker.getByRole("searchbox", { name: "Default model filter" })).toBeVisible();
      await expect(defaultPicker.getByText("Explicit reasoning effort")).toHaveCount(0);
      await expect(defaultPicker.locator(".modelReasoningProviderHeading", { hasText: "LM Studio" })).toBeVisible();
      await expect(defaultPicker.getByRole("radio", { name: /noop/ })).toHaveAttribute("data-model-value", "lmstudio/noop");
      await expect(defaultPicker.getByRole("radio", { name: /noop/ })).not.toHaveAttribute("title", /.+/);
      await expectModelRowsFillPopover(defaultPicker);
      await captureWorkbench(page, testInfo, `settings-models-picker-${isMobile ? "mobile" : "desktop"}`);
      await page.keyboard.press("Escape");
      await expect(models.locator(".modelAssignmentRow > div:first-child span")).toHaveCount(0);
      await expect(models.getByText(/missing OPENROUTER_API_KEY/)).toHaveCount(0);
      await expect(models.getByText("no key required")).toHaveCount(0);
      await expect(models.locator(".modelProviderIdentity > span").filter({ hasText: /models|OPENROUTER_API_KEY|no key required|Available/ })).toHaveCount(0);
      await assertNoHorizontalOverflow(page, settings);
      await expectControlsFitHorizontally(models.locator(".modelAssignmentPanel"));
      await captureWorkbench(page, testInfo, `settings-models-${isMobile ? "mobile" : "desktop"}`);
      const addProviderButton = models.getByRole("button", { name: "Connect provider" });
      await addProviderButton.click();
      const addProviderEditor = models.getByRole("group", { name: "Connect provider" });
      await expect(addProviderEditor).toBeVisible();
      await expect(addProviderButton).toHaveAttribute("aria-expanded", "true");
      await addProviderButton.click();
      await expect(addProviderEditor).toHaveCount(0);
      await expect(addProviderButton).toHaveAttribute("aria-expanded", "false");
      const firstEditButton = models.getByRole("button", { name: "Edit" }).first();
      await firstEditButton.click();
      const providerEditor = models.getByRole("group", { name: "Edit OpenCode Zen" });
      await expect(providerEditor.getByLabel("Provider id")).toBeVisible();
      await expect(providerEditor.getByLabel("API key env")).toHaveValue("OPENCODE_ZEN_API_KEY");
      await expect(providerEditor.getByRole("button", { name: "Fetch models" })).toBeVisible();
      await expect(firstEditButton).toHaveAttribute("aria-expanded", "true");
      await providerEditor.scrollIntoViewIfNeeded();
      await assertNoHorizontalOverflow(page, settings);
      await captureWorkbench(page, testInfo, `settings-models-editor-${isMobile ? "mobile" : "desktop"}`);
      await providerEditor.getByLabel("Advanced Metadata").scrollIntoViewIfNeeded();
      await assertNoHorizontalOverflow(page, settings);
      await captureWorkbench(page, testInfo, `settings-models-editor-advanced-${isMobile ? "mobile" : "desktop"}`);
      await firstEditButton.click();
      await expect(providerEditor).toHaveCount(0);
      await expect(firstEditButton).toHaveAttribute("aria-expanded", "false");

      await settings.getByRole("button", { name: "Archived sessions" }).click();
      await expect(settings.getByRole("region", { name: "Archived sessions" })).toBeVisible();
      await assertNoHorizontalOverflow(page, settings);
      await captureWorkbench(page, testInfo, `settings-archived-${isMobile ? "mobile" : "desktop"}`);

      await settings.getByRole("button", { name: "Back to app" }).click();
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
    } finally {
      await server.stop();
    }
  });

  test("renders Channels settings with compact detail and QR-first setup", async ({ page, isMobile }, testInfo) => {
    await page.setViewportSize(isMobile ? { width: 390, height: 900 } : { width: 1440, height: 1000 });
    const server = await startPevoWeb({
      live: false,
      configAppend: CHANNELS_VISUAL_CONFIG,
      envFile: CHANNELS_VISUAL_ENV
    });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      if (isMobile) {
        await openPanel(page, isMobile, "History");
      }
      await page.getByRole("button", { name: "Settings" }).click();
      const settings = page.getByRole("region", { name: "Settings", exact: true });
      await settings.getByRole("button", { name: "Channels" }).click();

      const channels = settings.getByRole("region", { name: "Channels", exact: true });
      await expect(channels.getByText("Connected Channels")).toBeVisible();
      await expect(channels.getByText("WeChat · wechat · polling")).toBeVisible();
      await expect(channels.getByText("ready")).toBeVisible();
      await expect(channels.getByLabel("wechat status").getByText("Runner stopped")).toBeVisible();
      await expect(channels.getByRole("switch", { name: "Disable wechat" })).toBeVisible();
      await expect(channels.getByRole("button", { name: "All" })).toHaveCount(0);
      await assertNoHorizontalOverflow(page, settings);
      await expectControlsFitHorizontally(settings);
      await captureChannelsWorkbench(page, testInfo, `settings-channels-list-${isMobile ? "mobile" : "desktop"}`);

      await channels.getByRole("button", { name: "Settings wechat" }).click();
      const detail = settings.getByRole("region", { name: "Channel settings" });
      await expect(detail.getByText("Config", { exact: true })).toBeVisible();
      await expect(detail.getByText("Runner", { exact: true })).toBeVisible();
      await expect(detail.getByText("Credential", { exact: true })).toBeVisible();
      await expect(detail.getByText("Allowlist", { exact: true })).toBeVisible();
      await expect(detail.getByText("Runtime", { exact: true })).toBeVisible();
      await expect(detail.getByRole("heading", { name: "Runtime settings" })).toBeVisible();
      await expect(detail.getByRole("combobox", { name: "Channel model" })).toBeVisible();
      const workspacePreset = detail.getByRole("combobox", { name: "Channel workspace preset" });
      await expect(workspacePreset).toBeVisible();
      await expect(workspacePreset.locator("option", { hasText: "Profile default" })).toHaveCount(1);
      await expect(detail.getByRole("textbox", { name: "Channel workspace" })).toBeVisible();
      await expect(detail.getByText("Changing workspace starts a fresh channel thread on the next message. Current running work is not interrupted.")).toBeVisible();
      await expect(detail.getByRole("textbox", { name: "Allowed direct users" })).toBeVisible();
      await expect(detail.getByText("Advanced diagnostics")).toBeVisible();
      await expect(detail.getByText("Runner activity")).toBeHidden();
      await detail.getByText("Advanced diagnostics").click();
      await expect(detail.getByText("Runner activity")).toBeVisible();
      await expect(detail.getByText("Remote lanes", { exact: true })).toBeVisible();
      await expect(detail.getByText("No remote lanes have started a local thread yet.")).toBeVisible();
      await assertNoHorizontalOverflow(page, settings);
      await expectControlsFitHorizontally(settings);
      await detail.getByText("Advanced diagnostics").click();
      await expect(detail.getByText("Runner activity")).toBeHidden();
      await expect(detail.getByText("Account env")).toHaveCount(0);
      await expect(detail.getByText("Base URL env")).toHaveCount(0);
      await expect(detail.getByText("WECHAT_ACCOUNT_ID")).toHaveCount(0);
      await expect(detail.getByText("WECHAT_ILINK_BASE_URL")).toHaveCount(0);
      await expect(detail.getByLabel("wechat doctor checks")).toHaveCount(0);
      await expect(detail.getByRole("switch", { name: "Disable wechat on save" })).toHaveCount(0);
      await expect(detail.getByRole("switch", { name: "Enable wechat on save" })).toHaveCount(0);
      await expect(detail.getByRole("button", { name: "Test wechat" })).toHaveCount(0);
      await expect(detail.getByRole("button", { name: "Cancel" })).toHaveCount(0);
      await expect(detail.getByRole("button", { name: "Save" }).first()).toBeDisabled();
      await detail.getByRole("textbox", { name: "Channel label" }).fill("WeChat Ops");
      await expect(detail.getByText("Unsaved changes")).toHaveCount(0);
      await expect(detail.getByRole("button", { name: "Cancel" })).toHaveCount(1);
      await expect(detail.getByRole("button", { name: "Save" })).toHaveCount(1);
      await expect(detail.getByRole("button", { name: "Save" }).first()).toBeEnabled();
      await assertNoHorizontalOverflow(page, settings);
      await expectControlsFitHorizontally(settings);
      if (!isMobile) {
        await expectSettingsGutterScrollsContent(page, settings);
      }
      await captureChannelsWorkbench(page, testInfo, `settings-channel-detail-${isMobile ? "mobile" : "desktop"}`);

      await detail.getByRole("button", { name: "Back to Channels" }).click();
      await expect(detail.getByText("Discard unsaved changes?")).toBeVisible();
      await detail.getByRole("button", { name: "Discard changes" }).click();
      const listAgain = settings.getByRole("region", { name: "Channels", exact: true });
      await listAgain.getByRole("button", { name: "Set up channel" }).click();
      await listAgain.getByRole("tab", { name: "WeChat" }).click();
      await expect(listAgain.getByText("WeChat connected")).toBeVisible();
      await expect(listAgain.getByRole("button", { name: "Reconnect QR" })).toBeVisible();
      await assertNoHorizontalOverflow(page, settings);
      await expectControlsFitHorizontally(settings);
      await listAgain.getByText("WeChat connected").scrollIntoViewIfNeeded();
      await captureChannelsWorkbench(page, testInfo, `settings-channel-wechat-setup-${isMobile ? "mobile" : "desktop"}`);
    } finally {
      await server.stop();
    }
  });
});
