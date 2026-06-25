import { mkdirSync, writeFileSync } from "node:fs";
import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import path from "node:path";
import { expect, test, type Locator, type Page, type TestInfo } from "@playwright/test";
import { PREFS_APPEARANCE_VERSION, PREFS_KEY } from "../src/storage";
import { repoRoot, startPevoWeb } from "./harness";

test.describe("pevo Web Workbench", () => {
  test("connects to Gateway and manages a source thread", async ({ page, isMobile }) => {
    const server = await startPevoWeb({ live: false });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      if (!isMobile) {
        await expect(page.getByRole("heading", { name: "Psychevo" })).toBeVisible();
        await assertLeftNavigationSectionAlignment(page);
        await page.getByRole("button", { name: "Collapse left sidebar" }).click();
        const logoToggle = page.getByRole("button", { name: "Expand left sidebar" });
        const newSessionButton = page.getByRole("button", { name: "New Session" });
        const searchButton = page.getByRole("button", { name: "Search" });
        const settingsButton = page.getByRole("button", { name: "Settings" });
        await expect(logoToggle).toBeVisible();
        await expect(newSessionButton).toBeVisible();
        await expect(searchButton).toBeVisible();
        await expect(page.getByRole("button", { name: "Artifacts" })).toHaveCount(0);
        await expect(settingsButton).toBeVisible();
        const [railBox, logoBox, newSessionBox, searchBox, settingsBox] = await Promise.all([
          page.locator(".historyColumn").boundingBox(),
          logoToggle.boundingBox(),
          newSessionButton.boundingBox(),
          searchButton.boundingBox(),
          settingsButton.boundingBox()
        ]);
        expect(railBox).not.toBeNull();
        expect(logoBox).not.toBeNull();
        expect(newSessionBox).not.toBeNull();
        expect(searchBox).not.toBeNull();
        expect(settingsBox).not.toBeNull();
        expect(newSessionBox!.y).toBeGreaterThanOrEqual(logoBox!.y + logoBox!.height);
        expect(searchBox!.y).toBeGreaterThan(newSessionBox!.y);
        expect(settingsBox!.y).toBeGreaterThan(searchBox!.y + searchBox!.height);
        expect(railBox!.y + railBox!.height - (settingsBox!.y + settingsBox!.height)).toBeLessThanOrEqual(18);
        await logoToggle.click();
      }

      await openPanel(page, isMobile, "History");
      await page.getByRole("button", { name: "New Session" }).click();
      await expect(page.locator(".pevo-sessionRow")).toHaveCount(0);
      await expect(page.locator(".pevo-sessionRow.is-draft")).toHaveCount(0);

      await openPanel(page, isMobile, "Transcript");
      await expect(page.getByText("No messages yet")).toBeVisible();

      const composer = page.getByPlaceholder("Ask Psychevo...");
      await composer.fill("/");
      await expect(page.getByRole("option", { name: /\/new/ })).toBeVisible();
      await page.keyboard.press("Escape");

      await composer.fill("$rev");
      await expect(page.getByRole("option", { name: /\$reviewer/ })).toBeVisible();
      await page.keyboard.press("Enter");
      await expect(composer).toHaveValue("$reviewer ");

      await composer.fill("@src/ma");
      await expect(page.getByRole("option", { name: /@src\/main\.rs/ })).toBeVisible();
      await page.keyboard.press("Escape");

      await composer.fill("/new");
      await page.keyboard.press("Escape");
      await page.keyboard.press("Enter");
      await openPanel(page, isMobile, "History");
      await expect(page.locator(".pevo-sessionRow")).toHaveCount(0);
      await expect(page.locator(".pevo-sessionRow.is-draft")).toHaveCount(0);

      await openPanel(page, isMobile, "Status");
      const statusRegion = page.getByRole("region", { name: "Workspace status" });
      await expect(statusRegion.getByText("draft")).toBeVisible();
      await expect(statusRegion.locator(".rightStatusMetrics")).toHaveCount(0);
      const sessionValue = statusRegion.locator(".rightWorkspaceSessionId");
      const longSessionId = "019ebc20-1234-5678-9abc-def0123492dd";
      await sessionValue.evaluate((element, value) => {
        element.textContent = value;
        element.setAttribute("title", value);
      }, longSessionId);
      await expect(sessionValue).toHaveText(longSessionId);
      await expect(sessionValue).not.toHaveText("019ebc20...92dd");
      expect(await sessionValue.evaluate((element) => {
        const style = getComputedStyle(element);
        return {
          overflow: style.overflow,
          overflowWrap: style.overflowWrap,
          textOverflow: style.textOverflow,
          whiteSpace: style.whiteSpace
        };
      })).toEqual({
        overflow: "visible",
        overflowWrap: "anywhere",
        textOverflow: "clip",
        whiteSpace: "normal"
      });
    } finally {
      await server.stop();
    }
  });

  test("opens scoped side chats with visible first prompt", async ({ page, isMobile }, testInfo) => {
    const server = await startPevoWeb({ live: false });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      await openPanel(page, isMobile, "Transcript");

      const composer = page.getByPlaceholder("Ask Psychevo...");
      await composer.fill("Create a parent session for side chat validation.");
      await page.getByRole("button", { name: "Send message" }).click();
      await expect(page.locator(".pevo-message.is-user")).toContainText("Create a parent session");

      await openPanel(page, isMobile, "Status");
      const statusRegion = page.getByRole("region", { name: "Workspace status" });
      await expect(statusRegion.getByRole("button", { name: "Side chat" })).toBeVisible();
      await statusRegion.getByRole("button", { name: "Side chat" }).click();
      await expect(sideConversationPanel(page)).toBeVisible();
      await page.getByRole("button", { name: /^Close Side/ }).click();

      await openPanel(page, isMobile, "Transcript");
      const sidePrompt = "Inspect isolated side prompt visibility.";
      await composer.fill(`/btw ${sidePrompt}`);
      await page.keyboard.press("Enter");

      const sideConversation = sideConversationPanel(page);
      await expect(sideConversation).toBeVisible({ timeout: 30_000 });
      await expect(sideConversation.locator(".pevo-message.is-user")).toContainText(sidePrompt, { timeout: 30_000 });
      await assertNoHorizontalOverflow(page, sideConversation);

      const sideComposer = sideConversation.locator(".pevo-composer");
      const sideTextarea = sideComposer.locator("textarea");
      await expect(sideTextarea).toBeVisible();
      const sideMetrics = await composerBoxMetrics(sideComposer);
      expect(sideMetrics.textarea).toBeGreaterThanOrEqual(42);
      if (!isMobile) {
        const mainMetrics = await composerBoxMetrics(page.locator(".composerDock .pevo-composer"));
        const metrics = JSON.stringify({ main: mainMetrics, side: sideMetrics });
        expect(Math.abs(sideMetrics.textarea - mainMetrics.textarea), metrics).toBeLessThanOrEqual(1);
        expect(Math.abs(sideMetrics.input - mainMetrics.input), metrics).toBeLessThanOrEqual(1);
        expect(Math.abs(sideMetrics.composer - mainMetrics.composer), metrics).toBeLessThanOrEqual(1);
        expect(Math.abs(sideMetrics.inputTop - mainMetrics.inputTop), metrics).toBeLessThanOrEqual(8);
      }
      await captureWorkbench(page, testInfo, `side-conversation-${isMobile ? "mobile" : "desktop"}`);

      if (isMobile) {
        await openPanel(page, isMobile, "History");
      }
      await page.getByRole("button", { name: "New Session", exact: true }).click();
      await expect(page.locator(".threadPanel")).toHaveCount(0);
      await openPanel(page, isMobile, "Status");
      await expect(page.getByRole("region", { name: "Workspace status" }).getByRole("button", { name: "Side chat" })).toHaveCount(0);
    } finally {
      await server.stop();
    }
  });

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

  test("shows Settings as an app-level configuration center", async ({ page, isMobile }, testInfo) => {
    const server = await startPevoWeb({ live: false });
    try {
          await page.goto(server.url);
          await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
        await page.getByRole("button", { name: "Agent" }).click();
        await expect(page.getByRole("dialog", { name: "Agent and runtime" }).getByRole("radiogroup", { name: "Main agent" }).getByRole("radio", { name: "translate" })).toBeVisible();
        await page.getByRole("button", { name: "Agent" }).click();
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
      await expect(settings.getByRole("button", { name: "Agents" })).toBeVisible();
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

      await settings.getByRole("button", { name: "Agents" }).click();
      await expect(settings.getByRole("region", { name: "Agents" })).toBeVisible();
      await expect(settings.getByRole("button", { name: "Add ACP backend" })).toBeVisible();
      await expect(settings.getByText("translate")).toHaveCount(0);
      await expect(settings.getByText("Translate user messages")).toHaveCount(0);
      await expect(settings.getByText("Runs")).toHaveCount(0);
      await assertNoHorizontalOverflow(page, settings);
      await captureWorkbench(page, testInfo, `settings-agents-${isMobile ? "mobile" : "desktop"}`);

      await settings.getByRole("button", { name: "Models" }).click();
      const models = settings.getByRole("region", { name: "Models" });
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

      await settings.getByRole("button", { name: "Archived sessions" }).click();
      await expect(settings.getByRole("region", { name: "Archived sessions" })).toBeVisible();
      await assertNoHorizontalOverflow(page, settings);
      await captureWorkbench(page, testInfo, `settings-archived-${isMobile ? "mobile" : "desktop"}`);

      await settings.getByRole("button", { name: "Agents" }).click();
      await settings.getByRole("button", { name: "Add ACP backend" }).click();
      const form = settings.getByRole("form", { name: "Profile ACP backend" });
      await expect(form).toBeVisible();
      await expect(form.getByLabel("Target")).toHaveCount(0);
      await expect(form.getByLabel("ID")).toHaveValue("");
      const commandJson = form.getByLabel("Command JSON");
      await expect(commandJson).toHaveValue(/"command": "opencode"/);
      expect(JSON.parse(await commandJson.inputValue())).toEqual({
        command: "opencode",
        args: ["acp"],
        env: {}
      });
      expect(await commandJson.evaluate((element) => (element as HTMLTextAreaElement).placeholder)).toBe("");
      await expect(form.getByLabel("Command", { exact: true })).toHaveCount(0);
      await expect(form.getByLabel("Args")).toHaveCount(0);
      await expect(form.getByLabel("Env")).toHaveCount(0);
      await expect(form.getByLabel("CWD")).toHaveValue("");
      await expect(form.locator("label").filter({ hasText: "Label" }).getByText("Optional")).toBeVisible();
      await expect(form.locator("label").filter({ hasText: "Description" }).getByText("Optional")).toBeVisible();
      await expect(form.getByText(/Resolves to /)).toHaveCount(0);
      await expect(form.getByLabel("Enabled")).toHaveCount(0);
      await expect(form.getByText("Entrypoints")).toHaveCount(0);
      await assertNoHorizontalOverflow(page, form);
      await expectControlsFitHorizontally(form);
      await captureWorkbench(page, testInfo, `settings-backend-form-${isMobile ? "mobile" : "desktop"}`);
      await form.getByLabel("ID").fill("playwright-acp");
      await commandJson.fill(JSON.stringify({ command: "playwright-acp", args: ["acp"], env: {} }, null, 2));
      await expect(form.getByRole("button", { name: "Save" })).toBeEnabled();
      await form.getByRole("button", { name: "Save" }).click();
      await expect(settings.getByRole("switch", { name: "Disable playwright-acp" })).toBeVisible();
      await expect(settings.getByLabel("playwright-acp peer entrypoint")).toBeChecked();
      await expect(settings.getByLabel("playwright-acp subagent entrypoint")).toBeChecked();
      await assertNoHorizontalOverflow(page, settings);
      await captureWorkbench(page, testInfo, `settings-backend-row-controls-${isMobile ? "mobile" : "desktop"}`);

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

      const channels = settings.getByRole("region", { name: "Channels" });
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
      const listAgain = settings.getByRole("region", { name: "Channels" });
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

  test("keeps long tool headers inside transcript rows", async ({ page, isMobile }) => {
    const server = await startPevoWeb({ live: false });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      await openPanel(page, isMobile, "Transcript");

      await page.locator(".pevo-threadItems").evaluate((container) => {
        container.innerHTML = `
          <article class="pevo-evidence is-running" data-testid="long-tool-row">
            <button class="pevo-evidenceLine is-singleTitle" type="button">
              <svg width="15" height="15" aria-hidden="true"></svg>
              <code>exec_command python /home/kevin/Projects/feedgarden/.agents/skills/x-daily/scripts/fetch.py --project /home/kevin/Projects/feedgarden</code>
              <em>running</em>
            </button>
          </article>
        `;
      });

      const row = page.getByTestId("long-tool-row");
      const status = row.locator(".pevo-evidenceLine em");
      const title = row.locator(".pevo-evidenceLine code");
      const rowBox = await row.boundingBox();
      const statusBox = await status.boundingBox();
      const titleClipped = await title.evaluate((element) => element.scrollWidth > element.clientWidth);

      expect(rowBox).not.toBeNull();
      expect(statusBox).not.toBeNull();
      await expect(title).toContainText("exec_command python");
      await expect(row.locator(".pevo-evidenceLine span")).toHaveCount(0);
      expect(statusBox!.x + statusBox!.width).toBeLessThanOrEqual(rowBox!.x + rowBox!.width);
      expect(titleClipped).toBe(true);
    } finally {
      await server.stop();
    }
  });

  test("renders structured tool evidence rows without raw JSON", async ({ page, isMobile }, testInfo) => {
    const server = await startPevoWeb({ live: false });
    try {
      await page.goto(server.url);
      for (const appearance of ["dark", "light", "warm"] as const) {
        await page.evaluate((value) => {
          localStorage.setItem(
            value.key,
            JSON.stringify({ appearance: value.appearance, appearanceVersion: value.version, debug: false })
          );
        }, { appearance, key: PREFS_KEY, version: PREFS_APPEARANCE_VERSION });
        await page.reload();
        await expect(page.locator("html")).toHaveAttribute("data-pevo-appearance", appearance);
        await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
        await openPanel(page, isMobile, "Transcript");
        await injectStructuredToolRows(page);

        const toolText = await page.locator(".pevo-evidence").evaluateAll((rows) =>
          rows.map((row) => row.textContent ?? "").join("\n")
        );
        expect(toolText).toContain("exec_command python fetch.py");
        expect(toolText).toContain("Command");
        expect(toolText).toContain("Output");
        expect(toolText).not.toMatch(/\{.*"(args|result|bytes_written|exit_code|output|session_id)"/);

        await page.screenshot({
          fullPage: true,
          path: testInfo.outputPath(`tool-evidence-${appearance}-${isMobile ? "mobile" : "desktop"}.png`)
        });
      }
    } finally {
      await server.stop();
    }
  });

  test("secondary menus close on outside click", async ({ page, isMobile }, testInfo) => {
    const server = await startPevoWeb({ live: false });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();

      await openPanel(page, isMobile, "Transcript");
      const composer = page.getByPlaceholder("Ask Psychevo...");
      await composer.fill("Create a visible history row.");
      await page.getByRole("button", { name: "Send message" }).click();
      await expect(page.locator(".pevo-threadItems")).toContainText("Create a visible history row.");
      await openPanel(page, isMobile, "History");
      const sessionRow = page.locator(".pevo-sessionRow").first();
      await sessionRow.locator(".pevo-sessionTitle").evaluate((element) => {
        const longTitle = "A very long session title that must truncate before the recent update time and running status";
        element.textContent = longTitle;
        element.setAttribute("title", longTitle);
      });
      await sessionRow.locator(".pevo-sessionMeta").evaluate((element) => {
        const running = document.createElement("b");
        running.className = "pevo-sessionRunning";
        running.setAttribute("aria-label", "running");
        running.textContent = "running";
        element.appendChild(running);
      });
      await sessionRow.hover();
      const sessionList = page.locator(".pevo-sessionList");
      await expect.poll(() => sessionList.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
      const titleLayout = await sessionRow.evaluate((element) => {
        const title = element.querySelector(".pevo-sessionTitleAnchor")?.getBoundingClientRect();
        const meta = element.querySelector(".pevo-sessionMeta")?.getBoundingClientRect();
        return title && meta ? { titleRight: title.right, metaLeft: meta.left } : null;
      });
      expect(titleLayout).not.toBeNull();
      expect(titleLayout!.titleRight).toBeLessThanOrEqual(titleLayout!.metaLeft + 1);
      await captureWorkbench(page, testInfo, `history-long-session-${isMobile ? "mobile" : "desktop"}`);
      const sessionMenu = page.locator(".pevo-sessionMenu").first();
      const sessionTrigger = sessionMenu.locator("summary");
      await expect(sessionMenu).toHaveCount(1);
      await sessionTrigger.click();
      await expect(sessionMenu).toHaveJSProperty("open", true);
      await openPanel(page, isMobile, "Transcript");
      const viewport = page.viewportSize();
      expect(viewport).not.toBeNull();
      await page.mouse.click(
        viewport!.width - 24,
        viewport!.height - 24
      );
      await expect(sessionMenu).toHaveJSProperty("open", false);
      await openPanel(page, isMobile, "History");
      await sessionTrigger.click();
      await sessionMenu.getByRole("menuitem", { name: "Rename" }).click();
      await expect(page.locator(".pevo-sessionMenu[open]")).toHaveCount(0);
      await page.keyboard.press("Escape");

      await openPanel(page, isMobile, "Status");
      const home = page.getByRole("region", { name: "Workspace status" });
      await home.getByRole("button", { name: /Review/ }).click();
      await expect(page.getByRole("region", { name: "Review" })).toBeVisible();

      const addMenu = page.locator(".rightAddMenu");
      const addTrigger = addMenu.locator("summary");
      await addTrigger.click();
      await expect(addMenu).toHaveJSProperty("open", true);
      await page.mouse.click(10, 10);
      await expect(addMenu).toHaveJSProperty("open", false);

      await addTrigger.click();
      await page.getByRole("menuitem", { name: "Files" }).click();
      await expect(page.getByRole("region", { name: "Workspace files" })).toBeVisible();
      await expect(addMenu).toHaveJSProperty("open", false);

      await addTrigger.click();
      await page.getByRole("menuitem", { name: "Terminal" }).click();
      await expect(page.getByRole("region", { name: "Terminal" })).toBeVisible();
      await expect(addMenu).toHaveJSProperty("open", false);
    } finally {
      await server.stop();
    }
  });

  test("submits a real provider turn through the composer @live", async ({ page, isMobile }) => {
    test.skip(process.env.PSYCHEVO_PLAYWRIGHT_LIVE !== "1", "live provider validation is opt-in");
    test.skip(isMobile, "live provider validation runs once on the desktop project");
    const server = await startPevoWeb({ live: true });
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
    test.skip(process.env.PSYCHEVO_PLAYWRIGHT_LIVE !== "1", "live provider validation is opt-in");
    test.skip(isMobile, "live automation validation runs once on the desktop project");
    test.setTimeout(360_000);
    const server = await startPevoWeb({ live: true, workdir: ensureLiveAutomationWorkdir() });
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
    test.skip(process.env.PSYCHEVO_PLAYWRIGHT_LIVE !== "1", "live provider validation is opt-in");
    test.skip(isMobile, "live provider validation runs once on the desktop project");
    test.setTimeout(420_000);
    const server = await startPevoWeb({ live: true, workdir: ensureLiveSubagentWorkdir() });
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

const LIVE_TRANSLATE_SUBAGENT_PROMPT = "使用 translate agent 并发演示简单的中译英和英译中";
const CHANNELS_VISUAL_CONFIG = `

[[channels.connections]]
id = "wechat"
channel = "wechat"
enabled = true
label = "WeChat"
transport = "polling"
model = "lmstudio/noop"
credential_env = "WECHAT_BOT_TOKEN"
account_env = "WECHAT_ACCOUNT_ID"
allow_users = ["wx-user"]

[[channels.connections]]
id = "ops-lark"
channel = "lark"
enabled = false
label = "Ops Lark"
transport = "long_connection"
credential_env = "LARK_APP_SECRET"
app_id_env = "LARK_APP_ID"
allow_groups = []
`;
const CHANNELS_VISUAL_ENV = [
  "WECHAT_BOT_TOKEN=test-wechat-token",
  "WECHAT_ACCOUNT_ID=test-wechat-account",
  "LARK_APP_ID=test-lark-app"
].join("\n");
const CHANNELS_SCREENSHOT_DIR = path.join(repoRoot, ".local/playwright/screenshots/channels");

async function startAutomationToolMockProvider(): Promise<{
  baseUrl: string;
  close(): Promise<void>;
  requests: unknown[];
}> {
  const requests: unknown[] = [];
  const server = createServer(async (request, response) => {
    if (request.method !== "POST" || request.url !== "/v1/chat/completions") {
      response.writeHead(404);
      response.end();
      return;
    }

    const body = await readRequestBody(request);
    const payload = JSON.parse(body) as {
      messages?: Array<{ role?: string }>;
      tools?: Array<{ function?: { name?: string }; name?: string }>;
    };
    requests.push(payload);
    const hasAutomationTool = payload.tools?.some((tool) => tool.name === "automation" || tool.function?.name === "automation") ?? false;
    const hasToolResult = payload.messages?.some((message) => message.role === "tool") ?? false;

    if (hasAutomationTool && !hasToolResult) {
      sendSse(response, [
        {
          id: "mock-tool-call",
          model: "automation",
          choices: [
            {
              delta: {
                tool_calls: [
                  {
                    index: 0,
                    id: "call_automation_tip",
                    function: {
                      name: "automation",
                      arguments: JSON.stringify({
                        action: "create",
                        title: "pevo-deterministic-engineering-tip",
                        prompt: "每次发送一条最有价值的软件工程 tip。",
                        schedule: { kind: "interval", everyMinutes: 1 }
                      })
                    }
                  }
                ]
              },
              finish_reason: "tool_calls"
            }
          ]
        }
      ]);
      return;
    }

    if (hasToolResult) {
      sendSse(response, [
        {
          id: "mock-final",
          model: "automation",
          choices: [
            {
              delta: { content: "已创建 pevo-deterministic-engineering-tip。" },
              finish_reason: "stop"
            }
          ]
        }
      ]);
      return;
    }

    sendSse(response, [
      {
        id: "mock-title",
        model: "automation",
        choices: [
          {
            delta: { content: "Deterministic Automation Tip" },
            finish_reason: "stop"
          }
        ]
      }
    ]);
  });

  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address() as AddressInfo;
  return {
    baseUrl: `http://127.0.0.1:${address.port}/v1`,
    close: () => new Promise<void>((resolve) => server.close(() => resolve())),
    requests
  };
}

function readRequestBody(request: IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    request.on("data", (chunk: Buffer) => chunks.push(chunk));
    request.on("end", () => resolve(Buffer.concat(chunks).toString("utf8")));
    request.on("error", reject);
  });
}

function sendSse(response: ServerResponse, chunks: unknown[]) {
  response.writeHead(200, {
    "content-type": "text/event-stream",
    connection: "close"
  });
  for (const chunk of chunks) {
    response.write(`data: ${JSON.stringify(chunk)}\n\n`);
  }
  response.end("data: [DONE]\n\n");
}

function ensureLiveSubagentWorkdir(): string {
  const workdir = process.env.PSYCHEVO_PLAYWRIGHT_LIVE_SUBAGENT_WORKDIR
    ? path.resolve(process.env.PSYCHEVO_PLAYWRIGHT_LIVE_SUBAGENT_WORKDIR)
    : path.join(repoRoot, ".local/.psychevo-dev/live-validation/gui-workdir");
  const agentDir = path.join(workdir, ".psychevo", "agents");
  mkdirSync(agentDir, { recursive: true });
  writeFileSync(
    path.join(agentDir, "translate.md"),
    `---
description: Translate between Chinese and English.
---
Translate the assigned text between Chinese and English. Return only the translation and direction.
`
  );
  return workdir;
}

function ensureLiveAutomationWorkdir(): string {
  const workdir = process.env.PSYCHEVO_PLAYWRIGHT_LIVE_AUTOMATION_WORKDIR
    ? path.resolve(process.env.PSYCHEVO_PLAYWRIGHT_LIVE_AUTOMATION_WORKDIR)
    : path.join(repoRoot, ".local/.psychevo-dev/live-validation/gui-automation-workdir");
  mkdirSync(workdir, { recursive: true });
  writeFileSync(path.join(workdir, "README.md"), "Live GUI automation validation workspace.\n");
  return workdir;
}

async function assertLeftNavigationSectionAlignment(page: Page) {
  const actionIcon = page.locator(".leftActions button").first().locator("svg");
  const actionLabel = page.locator(".leftActions button").first().locator("span");
  const pinnedIcon = page.locator(".leftPinnedPanel header svg");
  const pinnedLabel = page.locator(".leftPinnedPanel header span");
  const sessionsIcon = page.locator(".pevo-sessionsHeader .pevo-titleLine svg");
  const sessionsLabel = page.locator(".pevo-sessionsHeader h2");
  const [actionIconBox, actionLabelBox, pinnedIconBox, pinnedLabelBox, sessionsIconBox, sessionsLabelBox] =
    await Promise.all([
      actionIcon.boundingBox(),
      actionLabel.boundingBox(),
      pinnedIcon.boundingBox(),
      pinnedLabel.boundingBox(),
      sessionsIcon.boundingBox(),
      sessionsLabel.boundingBox()
    ]);

  expect(actionIconBox).not.toBeNull();
  expect(actionLabelBox).not.toBeNull();
  expect(pinnedIconBox).not.toBeNull();
  expect(pinnedLabelBox).not.toBeNull();
  expect(sessionsIconBox).not.toBeNull();
  expect(sessionsLabelBox).not.toBeNull();
  expect(Math.abs(pinnedIconBox!.x - actionIconBox!.x)).toBeLessThanOrEqual(1);
  expect(Math.abs(sessionsIconBox!.x - actionIconBox!.x)).toBeLessThanOrEqual(1);
  expect(Math.abs(pinnedLabelBox!.x - actionLabelBox!.x)).toBeLessThanOrEqual(1);
  expect(Math.abs(sessionsLabelBox!.x - actionLabelBox!.x)).toBeLessThanOrEqual(1);

  const [actionFont, pinnedFont, sessionsFont] = await Promise.all([
    actionLabel.evaluate(fontSignature),
    pinnedLabel.evaluate(fontSignature),
    sessionsLabel.evaluate(fontSignature)
  ]);
  expect(pinnedFont).toEqual(actionFont);
  expect(sessionsFont).toEqual(actionFont);
}

function fontSignature(element: Element) {
  const style = getComputedStyle(element);
  return {
    fontSize: style.fontSize,
    fontWeight: style.fontWeight
  };
}

async function captureWorkbench(page: Page, testInfo: TestInfo, label: string) {
  await page.screenshot({
    fullPage: true,
    path: testInfo.outputPath(`${label}-${testInfo.project.name}.png`)
  });
}

async function expectNoTransientAssistantDuplicateDuring(
  page: Page,
  testInfo: TestInfo,
  rows: Locator,
  label: string,
  timeoutMs: number,
  stableAfterVisibleMs: number
) {
  const deadline = Date.now() + timeoutMs;
  let stableDeadline: number | null = null;
  const samples: Array<{
    allRows: TranscriptRowSample[];
    elapsedMs: number;
    matchingRows: TranscriptRowSample[];
  }> = [];
  while (Date.now() < deadline) {
    const currentRows = await transcriptRowSamples(rows);
    const allRows = await allTranscriptRowSamples(page);
    const elapsedMs = timeoutMs - Math.max(0, deadline - Date.now());
    samples.push({ allRows, elapsedMs, matchingRows: currentRows });
    if (allRows.some((row) => row.text.includes("current thread is not available"))) {
      await captureWorkbench(page, testInfo, `${label}-current-thread-unavailable`);
      await testInfo.attach(`${label}-current-thread-unavailable-rows.json`, {
        body: JSON.stringify(samples, null, 2),
        contentType: "application/json"
      });
      throw new Error(`${label}: automation tool reported missing current thread: ${JSON.stringify(allRows, null, 2)}`);
    }
    if (currentRows.length > 1) {
      await captureWorkbench(page, testInfo, `${label}-duplicate`);
      await testInfo.attach(`${label}-duplicate-rows.json`, {
        body: JSON.stringify(samples, null, 2),
        contentType: "application/json"
      });
      throw new Error(`${label}: duplicate assistant rows appeared transiently: ${JSON.stringify(currentRows, null, 2)}`);
    }
    if (currentRows.length === 1) {
      stableDeadline ??= Date.now() + stableAfterVisibleMs;
      if (Date.now() >= stableDeadline) {
        await expect(rows).toHaveCount(1);
        return;
      }
    } else {
      stableDeadline = null;
    }
    await page.waitForTimeout(250);
  }
  await testInfo.attach(`${label}-rows-timeout.json`, {
    body: JSON.stringify(samples, null, 2),
    contentType: "application/json"
  });
  throw new Error(`${label}: assistant row did not become visible within ${timeoutMs}ms`);
}

type TranscriptRowSample = {
  blockId: string | null;
  entryId: string | null;
  source: string | null;
  text: string;
  turnId: string | null;
};

async function transcriptRowSamples(rows: Locator): Promise<TranscriptRowSample[]> {
  return rows.evaluateAll((elements) => elements.map((element) => ({
    blockId: element.getAttribute("data-block-id"),
    entryId: element.getAttribute("data-entry-id"),
    source: element.getAttribute("data-source"),
    text: (element.textContent ?? "").replace(/\s+/g, " ").trim(),
    turnId: element.getAttribute("data-turn-id")
  })));
}

async function allTranscriptRowSamples(page: Page): Promise<TranscriptRowSample[]> {
  return transcriptRowSamples(
    page.locator(".pevo-threadItems .pevo-message, .pevo-threadItems .pevo-evidence")
  );
}

async function captureChannelsWorkbench(page: Page, testInfo: TestInfo, label: string) {
  await captureWorkbench(page, testInfo, label);
  mkdirSync(CHANNELS_SCREENSHOT_DIR, { recursive: true });
  await page.screenshot({
    fullPage: true,
    scale: "css",
    path: path.join(CHANNELS_SCREENSHOT_DIR, `${label}.png`)
  });
}

function sideConversationPanel(page: Page): Locator {
  return page.getByRole("region", { name: /^Side chat$/i });
}

async function composerBoxMetrics(composer: Locator) {
  return composer.evaluate((element) => {
    const input = element.querySelector(".pevo-composerInput");
    const textarea = element.querySelector("textarea");
    return {
      composer: element.getBoundingClientRect().height,
      composerTop: element.getBoundingClientRect().top,
      input: input?.getBoundingClientRect().height ?? 0,
      inputTop: input?.getBoundingClientRect().top ?? 0,
      textarea: textarea?.getBoundingClientRect().height ?? 0
    };
  });
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

async function expectSettingsGutterScrollsContent(page: Page, settings: Locator) {
  const content = settings.locator(".settingsContent");
  const inner = settings.locator(".settingsContentInner");
  const metrics = await content.evaluate((element) => ({
    clientHeight: element.clientHeight,
    scrollHeight: element.scrollHeight
  }));
  if (metrics.scrollHeight <= metrics.clientHeight + 1) {
    return;
  }
  await content.evaluate((element) => { element.scrollTop = 0; });
  const [contentBox, innerBox] = await Promise.all([
    content.boundingBox(),
    inner.boundingBox()
  ]);
  expect(contentBox).not.toBeNull();
  expect(innerBox).not.toBeNull();
  const contentRight = contentBox!.x + contentBox!.width;
  const contentBottom = contentBox!.y + contentBox!.height;
  const innerRight = innerBox!.x + innerBox!.width;
  const gutter = Math.max(8, contentRight - innerRight);
  const x = Math.min(contentRight - 12, innerRight + gutter / 2);
  const y = Math.min(contentBottom - 80, contentBox!.y + 220);
  await page.mouse.move(x, y);
  await page.mouse.wheel(0, 520);
  await expect.poll(() => content.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
  await content.evaluate((element) => { element.scrollTop = 0; });
}

async function expectControlsFitHorizontally(locator: Locator) {
  const clipped = await locator.locator("input, textarea, select, button").evaluateAll((controls) =>
    controls
      .map((control) => {
        const element = control as HTMLElement;
        return {
          label: element.getAttribute("aria-label") ?? element.textContent?.trim() ?? element.tagName,
          clippedX: element.scrollWidth > element.clientWidth + 2
        };
      })
      .filter((item) => item.clippedX)
  );
  expect(clipped).toEqual([]);
}

async function expectModelRowsFillPopover(popover: Locator) {
  const gaps = await popover.locator(".modelReasoningModelRows .modelReasoningRow").evaluateAll((rows) => (
    rows.map((row) => {
      const element = row as HTMLElement;
      const parent = element.parentElement as HTMLElement | null;
      return parent ? parent.clientWidth - element.clientWidth : 0;
    })
  ));
  expect(Math.max(...gaps)).toBeLessThanOrEqual(8);
}

async function openPanel(page: Page, isMobile: boolean, name: "History" | "Status" | "Transcript") {
  if (name === "Status") {
    if (isMobile) {
      await page.getByRole("button", { name: "Transcript" }).click();
    }
    const expandInspector = page.getByRole("button", { name: "Show right inspector" });
    const collapseInspector = page.getByRole("button", { name: "Collapse right inspector" });
    if (await collapseInspector.count() === 0) {
      await expect(expandInspector).toBeVisible();
      await expandInspector.click();
      await expect(collapseInspector).toBeVisible();
    }
  }
  if (isMobile) {
    await page.getByRole("button", { name, exact: true }).click();
  }
  if (name === "Status") {
    await expect(page.getByRole("region", { name: "Workspace status" })).toBeVisible();
  }
}

async function injectStructuredToolRows(page: Page) {
  await page.locator(".pevo-threadItems").evaluate((container) => {
    container.innerHTML = `
      <article class="pevo-evidence is-completed is-tool-run" data-block-kind="shell" data-testid="structured-exec-row">
        <button class="pevo-evidenceLine is-singleTitle" type="button">
          <svg width="15" height="15" aria-hidden="true"></svg>
          <code>exec_command python fetch.py</code>
        </button>
        <div class="pevo-toolDetail">
          <section class="pevo-toolSection is-text is-code">
            <h4>Command</h4>
            <pre>python fetch.py</pre>
          </section>
          <section class="pevo-toolSection is-kv">
            <h4>Input</h4>
            <dl><div><dt>workdir</dt><dd>/tmp/project</dd></div></dl>
          </section>
          <section class="pevo-toolSection is-text is-code">
            <h4>Output</h4>
            <pre>first
second</pre>
          </section>
          <section class="pevo-toolSection is-kv">
            <h4>Status</h4>
            <dl><div><dt>exit</dt><dd>0</dd></div></dl>
          </section>
        </div>
      </article>
      <article class="pevo-evidence is-completed is-tool-update" data-block-kind="file" data-testid="structured-write-row">
        <button class="pevo-evidenceLine" type="button">
          <svg width="15" height="15" aria-hidden="true"></svg>
          <code>write feeds/report.md</code>
          <span>34,093 bytes / ok</span>
        </button>
        <div class="pevo-toolDetail">
          <section class="pevo-toolSection is-kv">
            <h4>Input</h4>
            <dl><div><dt>path</dt><dd>feeds/report.md</dd></div></dl>
          </section>
          <section class="pevo-toolSection is-kv">
            <h4>Change</h4>
            <dl><div><dt>bytes</dt><dd>34093</dd></div><div><dt>status</dt><dd>ok</dd></div></dl>
          </section>
        </div>
      </article>
    `;
  });
}
