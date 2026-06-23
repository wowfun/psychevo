import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page, type TestInfo } from "@playwright/test";
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
      await expect(commandJson).toHaveValue("");
      await expect(commandJson).toHaveAttribute("placeholder", /"command": "opencode"/);
      await expect(commandJson).toHaveAttribute("placeholder", /"args": \["acp"\]/);
      await expect(form.getByLabel("Command", { exact: true })).toHaveCount(0);
      await expect(form.getByLabel("Args")).toHaveCount(0);
      await expect(form.getByLabel("Env")).toHaveCount(0);
      await expect(form.getByLabel("CWD")).toHaveValue("");
      await expect(form.locator("label").filter({ hasText: "Label" }).getByText("Optional")).toBeVisible();
      await expect(form.locator("label").filter({ hasText: "Description" }).getByText("Optional")).toBeVisible();
      await expect(form.getByText(/Resolves to /)).toBeVisible();
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
          localStorage.setItem("psychevo.workbench.v0.prefs", JSON.stringify({ appearance: value, debug: false }));
        }, appearance);
        await page.reload();
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

  test("secondary menus close on outside click", async ({ page, isMobile }) => {
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
      const sessionMenu = page.locator(".pevo-sessionMenu").first();
      const sessionTrigger = sessionMenu.locator("summary");
      await expect(sessionMenu).toHaveCount(1);
      await sessionTrigger.click();
      await expect(sessionMenu).toHaveJSProperty("open", true);
      await page.mouse.click(10, 10);
      await expect(sessionMenu).toHaveJSProperty("open", false);
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
