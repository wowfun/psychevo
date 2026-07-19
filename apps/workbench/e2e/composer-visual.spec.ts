import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { startPevoWeb } from "./harness";
import { visualScreenshotRoot } from "./visualArtifacts";

const screenshotDir = visualScreenshotRoot();
const MANY_MODEL_CONFIG = `
[provider.lmstudio.models."alpha-1"]
[provider.lmstudio.models."beta-2"]
[provider.lmstudio.models."gamma-3"]
[provider.lmstudio.models."delta-4"]
[provider.lmstudio.models."epsilon-5"]
[provider.lmstudio.models."zeta-6"]

[provider.opencode-zen]
api = "https://opencode.ai/zen/v1"
no_auth = true

[provider.opencode-zen.models."big-pickle"]
cost = { input = 0, output = 0, cache_read = 0, cache_write = 0, request = 0 }
`;

test.describe("Workbench composer visual contract", () => {
  test("runs workspace HTML interactively without a trust prompt", async ({ page, isMobile }, testInfo) => {
    mkdirSync(screenshotDir, { recursive: true });
    const cwd = mkdtempSync(path.join(screenshotDir, "html-preview-cwd-"));
    writeFileSync(
      path.join(cwd, "dynamic-preview.html"),
      [
        "<!doctype html>",
        "<html><body>",
        "<div id=\"app\">pending</div>",
        "<button id=\"interaction-probe\" type=\"button\">Clicks: <span>0</span></button>",
        "<script>",
        "const rows = ['需求分析', 'UI/UX 设计', '部署上线'];",
        "document.getElementById('app').innerHTML = rows.map((row) => `<p class=\"gantt-row\">${row}</p>`).join('');",
        "let clicks = 0; document.getElementById('interaction-probe').addEventListener('click', () => { clicks += 1; document.querySelector('#interaction-probe span').textContent = String(clicks); });",
        "</script>",
        "</body></html>"
      ].join("")
    );
    writeFileSync(
      path.join(cwd, "wide-preview.md"),
      "# Wide preview\n\n| One | Two | Three |\n| --- | --- | --- |\n| alpha | beta | gamma |\n"
    );
    const server = await startPevoWeb({ cwd, live: false });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      await openStatusPanel(page, isMobile);
      const status = page.getByRole("region", { name: "Workspace status" });
      await status.getByRole("button", { name: "Refresh workspace" }).click();
      await status.getByRole("button", { name: "Files", exact: true }).click();
      const files = page.getByRole("region", { name: "Workspace files" });
      await expect(files).toBeVisible();
      await files.getByRole("treeitem", { name: /dynamic-preview\.html/ }).click();
      const inlineFrame = files.locator(".htmlStaticPreview iframe");
      await expect(inlineFrame).toHaveAttribute("sandbox", "allow-scripts");
      await expect(inlineFrame).not.toHaveAttribute("sandbox", /allow-forms/);
      await expect(inlineFrame).not.toHaveAttribute("sandbox", /allow-popups/);
      await expect(inlineFrame).not.toHaveAttribute("sandbox", /allow-same-origin/);
      await expect(inlineFrame).not.toHaveAttribute("inert", "");
      await expect(inlineFrame).toHaveAttribute("tabindex", "0");
      await expect(inlineFrame).toHaveCSS("pointer-events", "auto");
      await expect(page.locator(".htmlStaticPreview iframe")).toHaveCount(1);
      await expect(files.getByRole("button", { name: "Run interactive preview" })).toHaveCount(0);
      await expect(files.getByRole("button", { name: "Stop interactive preview" })).toHaveCount(0);
      const inlinePreview = files.frameLocator(".htmlStaticPreview iframe");
      await expect(inlinePreview.locator("#app .gantt-row")).toHaveCount(3);
      await expect(inlinePreview.locator("#app")).toContainText("部署上线");
      await inlinePreview.locator("#interaction-probe").click();
      await expect(inlinePreview.locator("#interaction-probe span")).toHaveText("1");

      const openPreviewAction = files.getByLabel("Open HTML preview for dynamic-preview.html");
      const editAction = files.getByLabel("Edit dynamic-preview.html");
      const hideFileTree = files.getByRole("button", { name: "Hide file tree" });
      const [openPreviewBox, editBox, treeToggleBox, previewWithTreeBox] = await Promise.all([
        openPreviewAction.boundingBox(),
        editAction.boundingBox(),
        hideFileTree.boundingBox(),
        files.locator(".htmlStaticPreview").boundingBox()
      ]);
      if (!openPreviewBox || !editBox || !treeToggleBox || !previewWithTreeBox) {
        throw new Error("missing Files action geometry");
      }
      expect(openPreviewBox.x).toBeLessThan(editBox.x);
      expect(Math.abs(editBox.x - openPreviewBox.x - openPreviewBox.width - 5)).toBeLessThanOrEqual(1);
      expect(Math.abs(openPreviewBox.y - editBox.y)).toBeLessThanOrEqual(1);
      expect(treeToggleBox.y).toBeLessThan(editBox.y);
      expect(Math.abs(treeToggleBox.x + treeToggleBox.width - editBox.x - editBox.width)).toBeLessThanOrEqual(6);
      await page.screenshot({
        path: path.join(screenshotDir, `html-files-controls-${testInfo.project.name}.png`)
      });

      await hideFileTree.click();
      await expect(files.getByRole("complementary", { name: "Workspace file tree" })).toHaveCount(0);
      await expect(files).not.toHaveClass(/has-fileTree/);
      const previewWithoutTreeBox = await files.locator(".htmlStaticPreview").boundingBox();
      if (!previewWithoutTreeBox) {
        throw new Error("missing expanded Files preview geometry");
      }
      expect(Math.abs(previewWithoutTreeBox.y - previewWithTreeBox.y)).toBeLessThanOrEqual(1);
      if (isMobile) {
        expect(previewWithoutTreeBox.height).toBeGreaterThan(previewWithTreeBox.height * 1.3);
      } else {
        expect(previewWithoutTreeBox.width).toBeGreaterThan(previewWithTreeBox.width * 1.3);
      }
      await page.screenshot({
        path: path.join(screenshotDir, `html-files-tree-hidden-${testInfo.project.name}.png`)
      });
      await files.getByRole("button", { name: "Show file tree" }).click();
      await expect(files.getByRole("complementary", { name: "Workspace file tree" })).toBeVisible();

      if (!isMobile) {
        await page.setViewportSize({ width: 1800, height: 960 });
        await page.locator(".workbench").evaluate((element) => {
          (element as HTMLElement).style.setProperty("--right-column-width", "820px");
        });
        await files.getByRole("treeitem", { name: /wide-preview\.md/ }).click();
        const markdown = files.locator(".fileMarkdownPreview .pevo-markdown");
        const markdownPane = files.locator(".fileMarkdownPreview");
        await expect(markdown).toBeVisible();
        const withTree = await markdown.boundingBox();
        await files.getByRole("button", { name: "Hide file tree" }).click();
        await expect(files.getByRole("complementary", { name: "Workspace file tree" })).toHaveCount(0);
        const [withoutTree, paneWithoutTree] = await Promise.all([
          markdown.boundingBox(),
          markdownPane.boundingBox()
        ]);
        if (!withTree || !withoutTree || !paneWithoutTree) {
          throw new Error("missing Markdown preview geometry");
        }
        expect(withoutTree.width).toBeGreaterThan(withTree.width * 1.3);
        expect(Math.abs(withoutTree.width - paneWithoutTree.width)).toBeLessThanOrEqual(2);
        await files.getByRole("button", { name: "Show file tree" }).click();
        await files.getByRole("treeitem", { name: /dynamic-preview\.html/ }).click();
      }

      await openPreviewAction.click();
      const preview = page.getByRole("region", { name: "Preview" });
      await expect(preview.getByRole("heading", { name: "dynamic-preview.html" })).toBeVisible();
      const previewIframe = preview.locator(".htmlStaticPreview iframe");
      const previewFrame = preview.frameLocator(".htmlStaticPreview iframe");
      await expect(page.locator(".htmlStaticPreview iframe")).toHaveCount(1);
      await expect(inlineFrame).toHaveCount(0);
      await expect(previewIframe).toHaveAttribute("sandbox", "allow-scripts");
      await expect(preview.getByRole("button", { name: "Run interactive preview" })).toHaveCount(0);
      await expect(preview.getByRole("button", { name: "Stop interactive preview" })).toHaveCount(0);
      await expect(previewFrame.locator("#app .gantt-row")).toHaveCount(3);
      await expect(previewFrame.locator("#app")).toContainText("UI/UX 设计");
      await previewFrame.locator("#interaction-probe").click();
      await expect(previewFrame.locator("#interaction-probe span")).toHaveText("1");
      await page.screenshot({
        path: path.join(screenshotDir, `html-preview-scripts-${testInfo.project.name}.png`)
      });
    } finally {
      await server.stop();
      rmSync(cwd, { force: true, recursive: true });
    }
  });

  test("renders descriptor-driven composer controls and interrupt state without overlap", async ({ page, isMobile }, testInfo) => {
    mkdirSync(screenshotDir, { recursive: true });
    const visualRoot = mkdtempSync(path.join(screenshotDir, "composer-home-"));
    const visualHome = path.join(visualRoot, "home");
    const visualCwd = path.join(visualHome, "Projects", "a-very-long-workspace-name-for-visual-proof");
    const agentDir = path.join(visualCwd, ".psychevo", "agents");
    mkdirSync(agentDir, { recursive: true });
    writeFileSync(path.join(agentDir, "translate.md"), "---\ndescription: Translate user messages.\n---\nTranslate the user's message.\n");
    const server = await startPevoWeb({
      configAppend: MANY_MODEL_CONFIG,
      cwd: visualCwd,
      home: visualHome,
      live: false,
      model: "opencode-zen/big-pickle",
      processEnv: {
        HOME: visualHome,
        CARGO_HOME: process.env.CARGO_HOME ?? path.join(homedir(), ".cargo"),
        RUSTUP_HOME: process.env.RUSTUP_HOME ?? path.join(homedir(), ".rustup")
      }
    });
    const websocketFrames = { received: [] as string[], sent: [] as string[] };
    page.on("websocket", (socket) => {
      socket.on("framesent", (event) => websocketFrames.sent.push(String(event.payload)));
      socket.on("framereceived", (event) => websocketFrames.received.push(String(event.payload)));
    });
    try {
      await page.goto(server.url);
      await expect(page).toHaveTitle("Psychevo");
      await expect(page.locator('link[rel="icon"][href="/favicon.svg"]')).toHaveCount(1);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      const initialWorkspaceControl = page.getByRole("button", { name: "Workspace", exact: true });
      await expect(initialWorkspaceControl).toHaveText("~/Projects/a-very-long-workspace-name-for-visual-proof");
      await expect(initialWorkspaceControl).toHaveAttribute("title", visualCwd);

      await openPanel(page, isMobile, "History");
      await page.getByRole("button", { name: "New Session" }).click();
      await openPanel(page, isMobile, "Transcript");

      const composer = page.locator(".pevo-composer");
      await expect(composer).toBeVisible();
      const promptInput = page.getByPlaceholder("Ask Psychevo...");
      await promptInput.focus();
      expect(await promptInput.evaluate((element) => ({
        borderWidth: getComputedStyle(element).borderTopWidth,
        outlineStyle: getComputedStyle(element).outlineStyle
      }))).toEqual({ borderWidth: "1px", outlineStyle: "none" });
      const agentControl = page.getByRole("button", { name: "Agent target", exact: true });
      await expect(agentControl).toBeVisible();
      await expect(agentControl).toContainText("Psychevo");
      await expect(agentControl).not.toContainText("Psychevo (Native)");
      await expect(agentControl).toHaveAttribute("title", "Psychevo · Psychevo (Native)");
      const permissionMode = page.getByRole("combobox", { name: "Permission mode" });
      await expect(permissionMode).toBeVisible();
      expect(await selectedOptionText(permissionMode)).toBe("Default Permission");
      await agentControl.click();
      const agentPopover = page.getByRole("dialog", { name: "Agent target" });
      const agentGroup = agentPopover.getByRole("radiogroup", { name: "Agent target" });
      await expect(agentGroup.getByRole("radio", { name: "Psychevo · Psychevo (Native)" })).toHaveAttribute("aria-checked", "true");
      await expect(agentGroup.getByRole("radio", { name: "translate · Psychevo (Native)" })).toBeVisible();
      await expect(agentPopover.getByText("Agent target", { exact: true })).toHaveCount(0);
      await expect(agentPopover.getByText("Manage Agent targets", { exact: true })).toHaveCount(0);
      await expect(agentPopover.getByText("Runtime Options", { exact: true })).toHaveCount(0);
      await expect(agentPopover.getByRole("combobox", { name: "Permission mode" })).toHaveCount(0);
      const controlPopoverSurface = await popoverSurfaceSignature(agentPopover);
      await expect(page.locator(".pevo-controlPopover:visible")).toHaveCount(1);
      await page.screenshot({
        path: path.join(screenshotDir, `composer-agent-runtime-${testInfo.project.name}.png`)
      });
      await agentControl.click();
      await expect(permissionMode).toBeVisible();

      const modeSelect = page.getByRole("combobox", { name: "Mode", exact: true });
      const modelButton = page.getByRole("button", { name: "Model", exact: true });
      await expect(modeSelect).toBeVisible();
      await expect(modeSelect).toHaveAttribute("aria-haspopup", "listbox");
      expect(await modeSelect.evaluate((element) => element.tagName)).toBe("BUTTON");
      await expect(modelButton).toBeVisible();
      expect(await selectedOptionText(modeSelect)).toBe("default");
      await expectTextFits(modeSelect.locator("span"));
      await expect(modelButton).toContainText("big-pickle Unavailable");
      await expect(modelButton).toHaveAttribute("title", "opencode-zen/big-pickle / Reasoning unavailable");
      await expectTextFits(modelButton.locator("span").first());
      for (const control of [agentControl, modeSelect, modelButton]) {
        expect(await control.evaluate((element) => getComputedStyle(element).borderTopWidth)).toBe("0px");
      }
      await expect(agentControl.locator("svg")).toHaveCount(0);
      await expect(modelButton.locator("svg")).toHaveCount(0);
      await expect(page.getByRole("combobox", { name: "Model" })).toHaveCount(0);
      await expect(page.getByRole("combobox", { name: "Reasoning" })).toHaveCount(0);
      await modelButton.click();
      const modelPopover = page.getByRole("dialog", { name: "Model and reasoning" });
      await expect(modelPopover).toBeVisible();
      await expect(modelPopover.getByRole("searchbox", { name: "Model filter" })).toBeVisible();
      const modelRows = modelPopover.getByRole("radiogroup", { name: "Model" }).getByRole("radio");
      await expect(modelRows).toHaveCount(7);
      await expect(modelRows.first()).toHaveAttribute("title", await modelRows.first().locator("strong").textContent() ?? "");
      await expect(modelPopover.getByRole("radiogroup", { name: "Reasoning" }).getByRole("radio", { name: "Default" })).toHaveAttribute("aria-checked", "false");
      await expect(page.getByRole("option", { name: "Select model" })).toHaveCount(0);
      expect(await popoverSurfaceSignature(modelPopover)).toEqual(controlPopoverSurface);
      await page.screenshot({
        path: path.join(screenshotDir, `composer-model-reasoning-${testInfo.project.name}.png`)
      });
      await page.keyboard.press("Escape");
      await expect(page.getByRole("button", { name: "Context usage" })).toBeVisible();
      await expect(page.getByRole("combobox", { name: "Psychevo mode" })).toHaveCount(0);
      await expect(page.getByRole("tablist", { name: "Turn mode" })).toHaveCount(0);
      await assertComposerGeometry(page, isMobile);
      await assertDraftComposerCentered(page);
      const defaultFooterBox = await page.locator(".pevo-composerFooter").boundingBox();
      expect(defaultFooterBox).not.toBeNull();
      await page.screenshot({
        path: path.join(screenshotDir, `composer-empty-${testInfo.project.name}.png`)
      });

      const workspaceControl = page.getByRole("button", { name: "Workspace", exact: true });
      await expect(workspaceControl).toContainText("~/Projects/a-very-long-workspace-name-for-visual-proof");
      await expect(workspaceControl).toHaveAttribute("title", visualCwd);
      if (!isMobile) {
        expect(await workspaceControl.evaluate((element) => Number.parseFloat(getComputedStyle(element).maxWidth)))
          .toBeGreaterThan(180);
      }
      expect(await permissionMode.evaluate((element) => element.tagName)).toBe("BUTTON");
      await permissionMode.click();
      const permissionListbox = page.getByRole("listbox", { name: "Permission mode" });
      await expect(permissionListbox.getByRole("option", { name: "Default Permission" })).toHaveAttribute("aria-selected", "true");
      expect(await popoverSurfaceSignature(permissionListbox)).toEqual(controlPopoverSurface);
      await expect(page.locator(".pevo-controlPopover:visible")).toHaveCount(1);
      await page.screenshot({
        path: path.join(screenshotDir, `composer-permission-${testInfo.project.name}.png`)
      });
      await workspaceControl.click();
      await expect(permissionListbox).toHaveCount(0);
      const workspaceMenu = page.getByRole("menu", { name: "Workspace" });
      await expect(workspaceMenu.getByRole("menuitem", { name: "Open workspace..." })).toBeVisible();
      expect(await popoverSurfaceSignature(workspaceMenu)).toEqual(controlPopoverSurface);
      await expect(page.locator(".pevo-controlPopover:visible")).toHaveCount(1);
      const workspaceMenuBox = await workspaceMenu.boundingBox();
      expect(workspaceMenuBox).not.toBeNull();
      const viewport = page.viewportSize();
      expect(viewport).not.toBeNull();
      expect(workspaceMenuBox!.width).toBeLessThanOrEqual(viewport!.width - 24);
      await page.screenshot({
        path: path.join(screenshotDir, `composer-workspace-menu-${testInfo.project.name}.png`)
      });
      await permissionMode.click();
      await expect(workspaceMenu).toHaveCount(0);
      await expect(permissionListbox).toBeVisible();
      await expect(page.locator(".pevo-controlPopover:visible")).toHaveCount(1);
      await workspaceControl.click();
      await expect(permissionListbox).toHaveCount(0);
      await expect(workspaceMenu).toBeVisible();
      await workspaceMenu.getByRole("menuitem", { name: "Open workspace..." }).click();
      const folderPicker = page.getByRole("dialog", { name: "Choose workspace folder" });
      await expect(folderPicker.getByRole("button", { name: "Open folder" })).toBeEnabled();
      await expect(folderPicker.getByRole("textbox")).toHaveCount(0);
      await page.screenshot({
        path: path.join(screenshotDir, `composer-folder-picker-${testInfo.project.name}.png`)
      });
      await folderPicker.getByRole("button", { name: "Close folder picker" }).click();

      const branchControl = page.getByRole("button", { name: "Git branch", exact: true });
      await branchControl.click();
      const branchMenu = page.getByRole("menu", { name: "Git branch" });
      await expect(branchMenu.getByRole("menuitem", { name: "New branch..." })).toBeVisible();
      expect(await popoverSurfaceSignature(branchMenu)).toEqual(controlPopoverSurface);
      const branchMenuBox = await branchMenu.boundingBox();
      expect(branchMenuBox).not.toBeNull();
      expect(branchMenuBox!.width).toBeLessThan(280);
      expect(branchMenuBox!.width).toBeLessThan(workspaceMenuBox!.width);
      await page.screenshot({
        path: path.join(screenshotDir, `composer-branch-menu-${testInfo.project.name}.png`)
      });
      await page.keyboard.press("Escape");

      if (!isMobile) {
        const contextTrigger = page.getByRole("button", { name: "Context usage" });
        await expect(contextTrigger).toHaveAttribute("aria-haspopup", "dialog");
        await contextTrigger.click();
        const contextPopover = page.getByRole("dialog", { name: "Context usage" });
        await expect(contextPopover).toBeVisible();
        const contextSummary = page.locator(".composerContextSummary strong");
        await contextSummary.evaluate((element) => {
          element.textContent = "16.7k/1.0M (1.6%)";
        });
        await expectElementInsideViewport(page, contextPopover);
        await expectTextNotClipped(contextSummary);
        expect(await popoverSurfaceSignature(contextPopover)).toEqual(controlPopoverSurface);
        await page.screenshot({
          path: path.join(screenshotDir, `composer-context-${testInfo.project.name}.png`)
        });
        await page.keyboard.press("Escape");
      }

      await promptInput.fill("/");
      const completionPopover = page.locator(".pevo-completionPopover");
      const completionOption = completionPopover.getByRole("option").first();
      await expect(completionOption).toBeVisible();
      await expect(completionOption).toHaveAttribute("title", /\S+/);
      expect(await popoverSurfaceSignature(completionPopover)).toEqual(controlPopoverSurface);
      const [completionBox, inputBox] = await Promise.all([
        completionPopover.boundingBox(),
        page.locator(".pevo-composerInput").boundingBox()
      ]);
      expect(completionBox).not.toBeNull();
      expect(inputBox).not.toBeNull();
      expect(Math.abs(completionBox!.x - inputBox!.x)).toBeLessThanOrEqual(0.5);
      expect(Math.abs(completionBox!.width - inputBox!.width)).toBeLessThanOrEqual(0.5);
      await page.screenshot({
        path: path.join(screenshotDir, `composer-completion-${testInfo.project.name}.png`)
      });

      const addTrigger = page.getByRole("button", { name: "Add attachments and options" });
      await expect(addTrigger).toHaveAttribute("aria-haspopup", "dialog");
      await addTrigger.click();
      await expect(completionPopover).toHaveCount(0);
      const addPopover = page.getByRole("dialog", { name: "Add options" });
      const addFiles = addPopover.getByRole("button", { name: "Add images and files" });
      const autoSpeak = page.getByRole("switch", { name: "Auto-speak" });
      const realtime = page.getByRole("switch", { name: "Realtime voice" });
      await expect(addFiles).toBeVisible();
      await expect(autoSpeak).toBeVisible();
      await expect(realtime).toBeVisible();
      await expect(addFiles.locator(".lucide-paperclip")).toBeVisible();
      await expect(autoSpeak.locator(".lucide-volume-2")).toBeVisible();
      await expect(realtime.locator(".lucide-radio")).toBeVisible();
      const [popoverBox, autoTrackBox, realtimeTrackBox, popoverPaddingRight, rowPaddingRight] = await Promise.all([
        addPopover.boundingBox(),
        autoSpeak.locator(".pevo-switchTrack").boundingBox(),
        realtime.locator(".pevo-switchTrack").boundingBox(),
        addPopover.evaluate((element) => Number.parseFloat(getComputedStyle(element).paddingRight)),
        autoSpeak.evaluate((element) => Number.parseFloat(getComputedStyle(element).paddingRight))
      ]);
      expect(popoverBox).not.toBeNull();
      expect(autoTrackBox).not.toBeNull();
      expect(realtimeTrackBox).not.toBeNull();
      expect(popoverBox!.width).toBeLessThan(260);
      expect(Math.abs(
        autoTrackBox!.x + autoTrackBox!.width - realtimeTrackBox!.x - realtimeTrackBox!.width
      )).toBeLessThanOrEqual(1);
      expect(Math.abs(
        autoTrackBox!.x + autoTrackBox!.width
        - (popoverBox!.x + popoverBox!.width - popoverPaddingRight - rowPaddingRight)
      )).toBeLessThanOrEqual(2);
      expect(await popoverSurfaceSignature(addPopover)).toEqual(controlPopoverSurface);
      await expect(page.getByRole("switch", { name: "Plan mode" })).toHaveCount(0);
      await page.screenshot({
        path: path.join(screenshotDir, `composer-menu-${testInfo.project.name}.png`)
      });
      await page.keyboard.press("Escape");
      const beforeControl = workbenchRpcResultsForMethod(websocketFrames, "thread/draft/open").at(-1)?.context as Record<string, unknown> | undefined;
      expect(beforeControl).toBeDefined();
      await modeSelect.click();
      const modeListbox = page.getByRole("listbox", { name: "Mode" });
      await expect(modeListbox.getByRole("option", { name: "plan" })).toBeVisible();
      expect(await popoverSurfaceSignature(modeListbox)).toEqual(controlPopoverSurface);
      await page.screenshot({
        path: path.join(screenshotDir, `composer-mode-${testInfo.project.name}.png`)
      });
      await modeListbox.getByRole("option", { name: "plan" }).click();
      await expect.poll(() => selectedOptionText(modeSelect)).toBe("plan");
      await expect.poll(() => workbenchRpcResultsForMethod(websocketFrames, "thread/control/set").length).toBe(1);
      const controlReceipt = workbenchRpcResultsForMethod(websocketFrames, "thread/control/set")[0]!;
      expect(controlReceipt.status).toBe("applied");
      expect((controlReceipt.context as Record<string, unknown>).selectedTargetId).toBe(beforeControl!.selectedTargetId);
      expect(controlReceipt.contextRevision).toBe(beforeControl!.contextRevision);
      expect(controlReceipt.controlRevision).not.toBe(beforeControl!.controlRevision);
      await expect(page.getByRole("alert")).toHaveCount(0);
      await expect(page.locator(".pevo-planChip")).toHaveCount(0);
      await assertComposerGeometry(page, isMobile);
      const planFooterBox = await page.locator(".pevo-composerFooter").boundingBox();
      expect(planFooterBox).not.toBeNull();
      expect(Math.abs(planFooterBox!.height - defaultFooterBox!.height)).toBeLessThanOrEqual(1);
      await page.screenshot({
        path: path.join(screenshotDir, `composer-plan-${testInfo.project.name}.png`)
      });

      const textarea = page.getByPlaceholder("Ask Psychevo...");
      const oneLineBox = await textarea.boundingBox();
      await textarea.fill("Review the composer layout.\nKeep the controls aligned.\nMake the input grow.");
      const multilineBox = await textarea.boundingBox();
      expect(oneLineBox).not.toBeNull();
      expect(multilineBox).not.toBeNull();
      expect(multilineBox!.height).toBeGreaterThan(oneLineBox!.height + 20);
      await assertComposerGeometry(page, isMobile);
      await page.screenshot({
        path: path.join(screenshotDir, `composer-multiline-${testInfo.project.name}.png`)
      });

      await forceInterruptVisualState(page);
      await expect(page.getByRole("button", { name: "Interrupt active turn" })).toBeVisible({ timeout: 10_000 });
      await expect(page.locator(".pevo-composerTurnStatus")).toContainText("1m05s");
      await expect(page.locator(".pevo-composerTurnSpinner")).toBeVisible();
      await expect(page.locator(".pevo-composerRightControls .pevo-composerTurnStatus")).toHaveCount(0);
      await assertComposerGeometry(page, isMobile);
      await page.screenshot({
        path: path.join(screenshotDir, `composer-interrupt-${testInfo.project.name}.png`)
      });
    } finally {
      await server.stop();
      rmSync(visualRoot, { force: true, recursive: true });
    }
  });

  test("fits compact model labels and hides Transcript scrollbars until active", async ({ page, isMobile }) => {
    const server = await startPevoWeb({ live: false, model: "lmstudio/mimo-v2.5-pro" });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      await expect(page.getByRole("button", { name: "Model", exact: true })).toBeVisible();

      await openPanel(page, isMobile, "History");
      await page.getByRole("button", { name: "New Session" }).click();
      await openPanel(page, isMobile, "Transcript");

      const modelButton = page.getByRole("button", { name: "Model", exact: true });
      await expect(modelButton).toBeVisible();
      await expect(modelButton).toContainText("mimo-v2.5-pro Unavailable");
      await expect(modelButton).toHaveAttribute("title", "lmstudio/mimo-v2.5-pro / Reasoning unavailable");
      await expectTextFits(modelButton.locator("span").first());
      await modelButton.click();
      const modelPicker = page.getByRole("dialog", { name: "Model and reasoning" });
      await expect(modelPicker.getByRole("radio", { name: "mimo-v2.5-pro" })).toHaveAttribute("data-model-value", "lmstudio/mimo-v2.5-pro");
      await page.keyboard.press("Escape");

      const threadItems = page.locator(".pevo-threadItems");
      await threadItems.evaluate((container) => {
        container.innerHTML = Array.from({ length: 36 }, (_, index) => (
          `<article class="pevo-messageFrame"><div class="pevo-message">Transcript row ${index + 1}</div></article>`
        )).join("") + `<div data-testid="reading-column-probe" style="height: 1px;"></div>`;
      });
      await assertComposerDockTracksTranscriptColumn(page, isMobile);
      if (!isMobile) {
        await page.mouse.move(1, 1);
      }
      const restingColor = await scrollbarColor(threadItems);
      expectTransparentScrollbar(restingColor);
      const restingBox = await threadItems.boundingBox();
      expect(restingBox).not.toBeNull();

      await threadItems.evaluate((element) => {
        element.scrollTop = element.scrollHeight;
        element.dispatchEvent(new Event("scroll", { bubbles: true }));
      });
      await expect.poll(() => threadItems.evaluate((element) => element.classList.contains("is-scrolling"))).toBe(true);
      const scrollingColor = await scrollbarColor(threadItems);
      expect(scrollingColor).not.toBe(restingColor);
      const scrollingBox = await threadItems.boundingBox();
      expect(scrollingBox).not.toBeNull();
      expect(Math.abs(scrollingBox!.width - restingBox!.width)).toBeLessThanOrEqual(1);

      if (!isMobile) {
        await expect.poll(() => threadItems.evaluate((element) => element.classList.contains("is-scrolling"))).toBe(false);
        await threadItems.hover({ position: { x: 1, y: 1 } });
        const hoverColor = await scrollbarColor(threadItems);
        expect(hoverColor).not.toBe(restingColor);
      }
    } finally {
      await server.stop();
    }
  });

  test("keeps transcript rows in a readable shared column", async ({ page, isMobile }, testInfo) => {
    const server = await startPevoWeb({ live: false });
    mkdirSync(screenshotDir, { recursive: true });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();

      await openPanel(page, isMobile, "History");
      const newSessionButton = page.getByRole("button", { name: "New Session" });
      const settingsButton = page.getByRole("button", { name: "Settings" });
      await expectCssPropertyMatchesRootVar(newSessionButton, "color", "--pevo-nav-text");
      await expectCssPropertyMatchesRootVar(settingsButton, "color", "--pevo-nav-text");
      await newSessionButton.click();
      await openPanel(page, isMobile, "Transcript");

      const threadItems = page.locator(".pevo-threadItems");
      await threadItems.evaluate((container) => {
        container.innerHTML = `
          <div class="pevo-messageFrame is-user" data-testid="user-frame">
            <article class="pevo-message is-user"><p>Improve the GUI contrast.</p></article>
          </div>
          <div class="pevo-messageFrame is-assistant" data-testid="assistant-frame">
            <article class="pevo-message is-assistant"><p>The sidebar and transcript can be tightened without changing data flow.</p></article>
          </div>
          <article class="pevo-reasoning is-running" data-testid="thinking-row">
            <button class="pevo-reasoningHeader" type="button">
              <svg width="15" height="15" aria-hidden="true"></svg>
              <span>Thinking</span>
              <em>running</em>
            </button>
          </article>
          <article class="pevo-evidence is-completed is-tool-update" data-testid="inline-diff-row">
            <button class="pevo-evidenceLine is-singleTitle" aria-expanded="true" type="button">
              <svg width="15" height="15" aria-hidden="true"></svg>
              <code>Edited primes.py (+1 -1)</code>
            </button>
            <div class="pevo-toolDetail">
              <section class="pevo-toolSection is-diff">
                <div class="pevo-inlineDiff" aria-label="Inline diff">
                  <article class="pevo-inlineDiffFile">
                    <header>
                      <span class="pevo-inlineDiffPath" title="primes.py">primes.py</span>
                      <span class="pevo-inlineDiffStats" aria-label="1 additions, 1 deletions">
                        <span class="pevo-inlineDiffAdd">+1</span>
                        <span class="pevo-inlineDiffDelete">-1</span>
                      </span>
                    </header>
                    <section class="pevo-inlineDiffHunk">
                      <div class="pevo-inlineDiffHunkHeader">@@ -1,3 +1,3 @@</div>
                      <div class="pevo-inlineDiffLines">
                        <div class="pevo-inlineDiffLine is-context">
                          <span class="pevo-inlineDiffNumber">1</span>
                          <span class="pevo-inlineDiffMarker"></span>
                          <code>def is_prime(n):</code>
                        </div>
                        <div class="pevo-inlineDiffLine is-delete">
                          <span class="pevo-inlineDiffNumber">2</span>
                          <span class="pevo-inlineDiffMarker">-</span>
                          <code>    return False</code>
                        </div>
                        <div class="pevo-inlineDiffLine is-add">
                          <span class="pevo-inlineDiffNumber">2</span>
                          <span class="pevo-inlineDiffMarker">+</span>
                          <code>    return n &gt; 1 and all(n % factor for factor in range(2, int(n ** 0.5) + 1))</code>
                        </div>
                      </div>
                    </section>
                  </article>
                </div>
              </section>
            </div>
          </article>
        `;
      });

      const userFrame = page.getByTestId("user-frame");
      const assistantFrame = page.getByTestId("assistant-frame");
      const thinkingRow = page.getByTestId("thinking-row");
      const inlineDiffRow = page.getByTestId("inline-diff-row");
      const userBubble = userFrame.locator(".pevo-message.is-user");
      const thinkingHeader = thinkingRow.locator(".pevo-reasoningHeader");
      const [threadBox, userBox, assistantBox, thinkingBox, inlineDiffBox] = await Promise.all([
        threadItems.boundingBox(),
        userFrame.boundingBox(),
        assistantFrame.boundingBox(),
        thinkingRow.boundingBox(),
        inlineDiffRow.boundingBox()
      ]);

      expect(threadBox).not.toBeNull();
      expect(userBox).not.toBeNull();
      expect(assistantBox).not.toBeNull();
      expect(thinkingBox).not.toBeNull();
      expect(inlineDiffBox).not.toBeNull();
      expect(assistantBox!.width).toBeLessThanOrEqual(762);
      expect(userBox!.x).toBeGreaterThan(assistantBox!.x);
      expect(userBox!.x + userBox!.width).toBeLessThanOrEqual(assistantBox!.x + 842);
      expect(Math.abs(thinkingBox!.x - assistantBox!.x)).toBeLessThanOrEqual(1);
      expect(Math.abs(inlineDiffBox!.x - assistantBox!.x)).toBeLessThanOrEqual(1);
      expect(inlineDiffBox!.width).toBeLessThanOrEqual(threadBox!.width);
      await expect(inlineDiffRow.locator('[aria-label="Inline diff"]')).toBeVisible();
      await expect(inlineDiffRow.locator(".pevo-inlineDiffLine")).toHaveCount(3);
      await expect(inlineDiffRow.locator(".diffLineNumber")).toHaveCount(0);
      await expect(inlineDiffRow.locator(".pevo-toolSection h4")).toHaveCount(0);
      await expect(inlineDiffRow.locator(".pevo-toolSection.is-kv")).toHaveCount(0);
      await expectTextFits(inlineDiffRow.locator(".pevo-evidenceLine code"));

      if (!isMobile) {
        expect(assistantBox!.x - threadBox!.x).toBeGreaterThan(16);
        expect(threadBox!.x + threadBox!.width - (userBox!.x + userBox!.width)).toBeGreaterThan(16);
      }

      await expectCssPropertyMatchesRootVar(userBubble, "background-color", "--pevo-user-bubble");
      await expectCssPropertyMatchesRootVar(userBubble, "border-top-color", "--pevo-user-bubble-border");
      await expectCssPropertyMatchesRootVar(thinkingHeader, "color", "--pevo-muted-strong");
      await page.screenshot({
        path: path.join(screenshotDir, `transcript-column-${testInfo.project.name}.png`)
      });
    } finally {
      await server.stop();
    }
  });
});

async function selectedOptionText(select: Locator): Promise<string> {
  return select.evaluate((element) => {
    if (element instanceof HTMLSelectElement) {
      return element.selectedOptions[0]?.textContent?.trim() ?? "";
    }
    return element.textContent?.trim() ?? "";
  });
}

async function popoverSurfaceSignature(popover: Locator) {
  return popover.evaluate((element) => {
    const style = getComputedStyle(element);
    return {
      backgroundColor: style.backgroundColor,
      borderBottomColor: style.borderBottomColor,
      borderBottomStyle: style.borderBottomStyle,
      borderBottomWidth: style.borderBottomWidth,
      borderRadius: style.borderRadius,
      boxShadow: style.boxShadow,
      color: style.color,
      gap: style.gap,
      paddingBottom: style.paddingBottom,
      paddingLeft: style.paddingLeft,
      paddingRight: style.paddingRight,
      paddingTop: style.paddingTop
    };
  });
}

function workbenchRpcResultsForMethod(
  capture: { received: string[]; sent: string[] },
  method: string
): Array<Record<string, unknown>> {
  const requestIds = new Set(capture.sent.flatMap((payload) => {
    try {
      const message = JSON.parse(payload) as { id?: unknown; method?: string };
      const id = message.id == null ? "" : String(message.id);
      return message.method === method && /^\d+$/.test(id) ? [id] : [];
    } catch {
      return [];
    }
  }));
  return capture.received.flatMap((payload) => {
    try {
      const message = JSON.parse(payload) as { id?: unknown; result?: unknown };
      return message.id != null
        && requestIds.has(String(message.id))
        && typeof message.result === "object"
        && message.result !== null
        ? [message.result as Record<string, unknown>]
        : [];
    } catch {
      return [];
    }
  });
}

async function expectTextNotClipped(locator: Locator) {
  const result = await locator.evaluate((element) => ({
    clippedX: element.scrollWidth > element.clientWidth + 1,
    clippedY: element.scrollHeight > element.clientHeight + 1,
    overflow: getComputedStyle(element).overflow
  }));
  expect(result.overflow).not.toBe("hidden");
  expect(result.clippedX).toBe(false);
  expect(result.clippedY).toBe(false);
}

async function expectTextFits(locator: Locator) {
  const result = await locator.evaluate((element) => ({
    clientWidth: element.clientWidth,
    scrollWidth: element.scrollWidth,
    text: element.textContent?.trim() ?? ""
  }));
  expect(result.text.length).toBeGreaterThan(0);
  expect(result.scrollWidth).toBeLessThanOrEqual(result.clientWidth + 1);
}

async function expectElementInsideViewport(page: Page, locator: Locator) {
  const [box, viewport] = await Promise.all([locator.boundingBox(), page.viewportSize()]);
  expect(box).not.toBeNull();
  expect(viewport).not.toBeNull();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(viewport!.width);
}

async function scrollbarColor(locator: Locator): Promise<string> {
  return locator.evaluate((element) => getComputedStyle(element).scrollbarColor);
}

function expectTransparentScrollbar(value: string) {
  expect(value.toLowerCase()).toMatch(/transparent|rgba\(0, 0, 0, 0\)|color\(srgb 0 0 0 \/ 0\)/);
}

async function expectCssPropertyMatchesRootVar(locator: Locator, property: string, variableName: string) {
  const values = await locator.evaluate((element, [cssProperty, cssVariable]) => {
    const style = getComputedStyle(element);
    const probe = document.createElement("span");
    probe.style.setProperty(cssProperty, `var(${cssVariable})`);
    document.documentElement.appendChild(probe);
    const expected = getComputedStyle(probe).getPropertyValue(cssProperty);
    probe.remove();
    return {
      actual: style.getPropertyValue(cssProperty).trim().replace(/\s+/g, " "),
      expected: expected.trim().replace(/\s+/g, " ")
    };
  }, [property, variableName]);
  expect(values.actual).toBe(values.expected);
}

async function assertComposerDockTracksTranscriptColumn(page: Page, isMobile: boolean) {
  if (isMobile) {
    return;
  }
  const composerDock = page.locator(".composerDock");
  const readingProbe = page.getByTestId("reading-column-probe");
  const [dockBox, probeBox] = await Promise.all([
    composerDock.boundingBox(),
    readingProbe.boundingBox()
  ]);
  expect(dockBox).not.toBeNull();
  expect(probeBox).not.toBeNull();
  expect(Math.abs(dockBox!.x - probeBox!.x)).toBeLessThanOrEqual(12);
  expect(Math.abs(dockBox!.width - probeBox!.width)).toBeLessThanOrEqual(16);
}

async function assertComposerGeometry(page: Page, isMobile: boolean) {
  const add = page.getByRole("button", { name: "Add attachments and options" });
  const input = page.locator(".pevo-composerInput");
  const footer = page.locator(".pevo-composerFooter");
  const action = page.locator(".pevo-sendButton");
  const dictation = page.getByRole("button", { name: /dictation/ });
  const agent = page.getByRole("button", { name: "Agent target", exact: true });
  const permission = page.getByRole("combobox", { name: "Permission mode" });
  const workspace = page.getByRole("button", { name: "Workspace", exact: true });
  const branch = page.getByRole("button", { name: "Git branch" });
  const mode = page.getByRole("combobox", { name: "Mode", exact: true });
  const model = page.getByRole("button", { name: "Model", exact: true });
  const context = page.getByRole("button", { name: "Context usage" });
  const [addBox, inputBox, footerBox, actionBox, dictationBox, permissionBox, workspaceBox, branchBox, agentBox, modeBox, modelBox, contextBox] = await Promise.all([
    add.boundingBox(),
    input.boundingBox(),
    footer.boundingBox(),
    action.boundingBox(),
    dictation.boundingBox(),
    permission.boundingBox(),
    workspace.boundingBox(),
    branch.boundingBox(),
    agent.boundingBox(),
    mode.boundingBox(),
    model.boundingBox(),
    context.boundingBox()
  ]);

  expect(addBox).not.toBeNull();
  expect(inputBox).not.toBeNull();
  expect(footerBox).not.toBeNull();
  expect(actionBox).not.toBeNull();
  expect(dictationBox).not.toBeNull();
  expect(permissionBox).not.toBeNull();
  expect(workspaceBox).not.toBeNull();
  expect(branchBox).not.toBeNull();
  expect(agentBox).not.toBeNull();
  expect(modeBox).not.toBeNull();
  expect(modelBox).not.toBeNull();
  expect(contextBox).not.toBeNull();
  expect(agentBox!.width).toBeLessThanOrEqual(210);
  expect(Math.abs(dictationBox!.width - actionBox!.width)).toBeLessThanOrEqual(1);
  expect(Math.abs(dictationBox!.height - actionBox!.height)).toBeLessThanOrEqual(1);

  const actionCenterY = actionBox!.y + actionBox!.height / 2;
  const dictationCenterY = dictationBox!.y + dictationBox!.height / 2;
  const addCenterY = addBox!.y + addBox!.height / 2;
  const agentCenterY = agentBox!.y + agentBox!.height / 2;
  const modeCenterY = modeBox!.y + modeBox!.height / 2;
  const modelCenterY = modelBox!.y + modelBox!.height / 2;
  const contextCenterY = contextBox!.y + contextBox!.height / 2;
  if (isMobile) {
    const viewport = await page.viewportSize();
    expect(viewport).not.toBeNull();
    const visibleBoxes = [addBox!, permissionBox!, workspaceBox!, branchBox!, agentBox!, modeBox!, modelBox!, contextBox!, dictationBox!, actionBox!];
    for (const box of visibleBoxes) {
      expect(box.x).toBeGreaterThanOrEqual(0);
      expect(box.x + box.width).toBeLessThanOrEqual(viewport!.width);
    }
    expect(Math.abs(addCenterY - agentCenterY)).toBeLessThanOrEqual(5);
    expect(Math.abs(agentCenterY - modeCenterY)).toBeLessThanOrEqual(5);
    expect(Math.abs(
      (permissionBox!.y + permissionBox!.height / 2)
      - (workspaceBox!.y + workspaceBox!.height / 2)
    )).toBeLessThanOrEqual(5);
    expect(Math.abs(actionCenterY - dictationCenterY)).toBeLessThanOrEqual(5);
    expect(contextBox!.y).toBeGreaterThanOrEqual(modelBox!.y - 1);
    await expectNoBoxOverlap({ action: actionBox!, context: contextBox!, dictation: dictationBox!, model: modelBox! });
    expect(await footer.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
    expect(inputBox!.y + inputBox!.height).toBeLessThanOrEqual(footerBox!.y + 2);
    expect(addBox!.x).toBeLessThan(agentBox!.x);
    expect(workspaceBox!.x).toBeLessThan(branchBox!.x);
    expect(branchBox!.x).toBeLessThan(permissionBox!.x);
    expect(dictationBox!.x).toBeLessThan(actionBox!.x);
    return;
  }
  expect(Math.abs(actionCenterY - dictationCenterY)).toBeLessThanOrEqual(4);
  expect(Math.abs(actionCenterY - agentCenterY)).toBeLessThanOrEqual(4);
  expect(Math.abs(actionCenterY - modeCenterY)).toBeLessThanOrEqual(4);
  expect(Math.abs(actionCenterY - modelCenterY)).toBeLessThanOrEqual(4);
  expect(Math.abs(actionCenterY - contextCenterY)).toBeLessThanOrEqual(4);

  expect(inputBox!.y + inputBox!.height).toBeLessThanOrEqual(footerBox!.y + 2);
  expect(addBox!.x).toBeLessThan(agentBox!.x);
  expect(Math.abs(
    (permissionBox!.y + permissionBox!.height / 2)
    - (workspaceBox!.y + workspaceBox!.height / 2)
  )).toBeLessThanOrEqual(4);
  expect(workspaceBox!.x).toBeLessThan(branchBox!.x);
  expect(branchBox!.x).toBeLessThan(permissionBox!.x);
  expect(modeBox!.x).toBeGreaterThan(agentBox!.x);
  expect(modelBox!.x).toBeGreaterThan(agentBox!.x);
  expect(modelBox!.x).toBeLessThan(dictationBox!.x);
  expect(contextBox!.x).toBeGreaterThan(modelBox!.x);
  expect(contextBox!.x).toBeLessThan(dictationBox!.x);
  expect(dictationBox!.x).toBeLessThan(actionBox!.x);
}

async function assertDraftComposerCentered(page: Page) {
  const conversation = page.locator(".conversationColumn.is-draftSession");
  await expect(conversation).toHaveCount(1);
  const [workspaceBox, dockBox] = await Promise.all([
    conversation.locator(".centerWorkspace").boundingBox(),
    conversation.locator(".composerDock").boundingBox()
  ]);
  expect(workspaceBox).not.toBeNull();
  expect(dockBox).not.toBeNull();
  const workspaceCenter = workspaceBox!.y + workspaceBox!.height / 2;
  const dockCenter = dockBox!.y + dockBox!.height / 2;
  expect(Math.abs(workspaceCenter - dockCenter)).toBeLessThanOrEqual(2);
}

async function expectNoBoxOverlap(boxes: Record<string, { x: number; y: number; width: number; height: number }>) {
  const entries = Object.entries(boxes);
  for (let leftIndex = 0; leftIndex < entries.length; leftIndex += 1) {
    for (let rightIndex = leftIndex + 1; rightIndex < entries.length; rightIndex += 1) {
      const [leftName, left] = entries[leftIndex]!;
      const [rightName, right] = entries[rightIndex]!;
      const overlapX = Math.min(left.x + left.width, right.x + right.width) - Math.max(left.x, right.x);
      const overlapY = Math.min(left.y + left.height, right.y + right.height) - Math.max(left.y, right.y);
      expect(overlapX > 1 && overlapY > 1, `${leftName} overlaps ${rightName}`).toBe(false);
    }
  }
}

async function forceInterruptVisualState(page: Page) {
  await page.locator(".pevo-composer").evaluate((composer) => {
    composer.classList.add("is-running");
    const button = composer.querySelector<HTMLButtonElement>(".pevo-sendButton");
    const footer = composer.querySelector<HTMLElement>(".pevo-composerFooter");
    const rightControls = composer.querySelector<HTMLElement>(".pevo-composerRightControls");
    if (!button) {
      throw new Error("send button not found");
    }
    if (!footer || !rightControls) {
      throw new Error("composer footer controls not found");
    }
    button.disabled = false;
    button.type = "button";
    button.classList.add("is-interrupt");
    button.setAttribute("aria-label", "Interrupt active turn");
    button.innerHTML = `<span class="pevo-stopGlyph" aria-hidden="true"></span>`;
    if (!composer.querySelector(".pevo-composerTurnStatus")) {
      const status = document.createElement("span");
      status.className = "pevo-composerTurnStatus";
      status.setAttribute("aria-label", "Active turn elapsed");
      status.innerHTML = `<span class="pevo-composerTurnSpinner" aria-hidden="true">⠼</span><span>1m05s</span>`;
      footer.insertBefore(status, rightControls);
    }
  });
}

async function openPanel(page: Page, isMobile: boolean, name: "History" | "Status" | "Transcript") {
  if (isMobile) {
    await page.getByRole("button", { name }).click();
  }
}

async function openStatusPanel(page: Page, isMobile: boolean) {
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
  if (isMobile) {
    await page.getByRole("button", { name: "Status", exact: true }).click();
  }
  await expect(page.getByRole("region", { name: "Workspace status" })).toBeVisible();
}
