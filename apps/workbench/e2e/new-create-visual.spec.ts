import { expect, test, type Locator, type Page } from "@playwright/test";
import { startPevoWeb } from "./harness";
import { assertNoHorizontalOverflow, captureWorkbench, openPanel } from "./workbench.support";

test.describe("Workbench New/Create visual contract", () => {
  test("keeps New/Add/Install/Connect panels within the visible Workbench viewport", async ({ page, isMobile }, testInfo) => {
    await page.setViewportSize(isMobile ? { width: 390, height: 900 } : { width: 1440, height: 960 });
    const server = await startPevoWeb({ live: false });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();

      if (isMobile) {
        await openPanel(page, isMobile, "History");
      }
      await page.getByRole("button", { name: "New Workspace" }).click();
      const workspaceDialog = page.getByRole("dialog", { name: "New Workspace" });
      await expectPanelInViewport(page, workspaceDialog, "workspace dialog");
      await captureWorkbench(page, testInfo, `new-create-workspace-${isMobile ? "mobile" : "desktop"}`);
      await workspaceDialog.getByRole("button", { name: "Cancel" }).click();

      await openTopLevelView(page, isMobile, "Automations");
      const automations = page.getByRole("region", { name: "Automations" });
      await expect(automations).toBeVisible();
      await automations.getByRole("button", { name: "New" }).click();
      await expectPanelInViewport(page, automations.getByRole("group", { name: "New automation" }), "automation create panel");
      await captureWorkbench(page, testInfo, `new-create-automations-${isMobile ? "mobile" : "desktop"}`);

      await openTopLevelView(page, isMobile, "Settings");
      let settings = page.getByRole("region", { name: "Settings", exact: true });
      await expect(settings).toBeVisible();
      await openSettingsSection(settings, "Models");
      const models = settings.getByRole("region", { name: "Models", exact: true });
      await models.getByRole("button", { name: "Connect provider" }).click();
      await expectPanelInViewport(page, models.getByRole("group", { name: "Connect provider" }), "connect provider panel");
      await captureWorkbench(page, testInfo, `new-create-models-connect-${isMobile ? "mobile" : "desktop"}`);
      await models.getByRole("group", { name: "Connect provider" }).getByRole("button", { name: "Close" }).click();

      const editProvider = models.locator(".modelProviderRow", { hasText: "OpenCode Zen" }).getByRole("button", { name: "Edit" });
      await editProvider.click();
      const editProviderPanel = models.getByRole("group", { name: "Edit OpenCode Zen" });
      await editProviderPanel.scrollIntoViewIfNeeded();
      await expectPanelInViewport(page, editProviderPanel, "edit provider panel");
      await captureWorkbench(page, testInfo, `new-create-models-edit-${isMobile ? "mobile" : "desktop"}`);

      await openTopLevelView(page, isMobile, "Capabilities");
      let capabilities = page.getByRole("region", { name: "Capabilities" });
      await expect(capabilities).toBeVisible();
      await capabilities.getByRole("tab", { name: "Agents" }).click();
      await capabilities.getByRole("tab", { name: "ACP Backends" }).click();
      await capabilities.getByRole("button", { name: "Add ACP backend" }).click();
      await expectPanelInViewport(page, capabilities.getByRole("group", { name: "Add backend" }), "add backend panel");
      await captureWorkbench(page, testInfo, `new-create-capabilities-agents-backend-${isMobile ? "mobile" : "desktop"}`);

      await openTopLevelView(page, isMobile, "Settings");
      settings = page.getByRole("region", { name: "Settings", exact: true });
      await expect(settings).toBeVisible();
      await openSettingsSection(settings, "Channels");
      const channels = settings.getByRole("region", { name: "Channels", exact: true });
      await channels.getByRole("button", { name: "Set up channel" }).click();
      await expectPanelInViewport(page, channels.getByRole("group", { name: "Set up channel" }), "set up channel panel");
      await captureWorkbench(page, testInfo, `new-create-channels-setup-${isMobile ? "mobile" : "desktop"}`);

      await openTopLevelView(page, isMobile, "Capabilities");
      capabilities = page.getByRole("region", { name: "Capabilities" });
      await expect(capabilities).toBeVisible();
      await capabilities.getByRole("tab", { name: "Skills" }).click();
      await capabilities.getByRole("button", { name: "Install skill" }).click();
      await expectPanelInViewport(page, capabilities.getByRole("group", { name: "Install skill" }), "install skill panel");
      await captureWorkbench(page, testInfo, `new-create-capabilities-skills-${isMobile ? "mobile" : "desktop"}`);

      await openCapabilityPanel(page, capabilities, "Plugins", "Install plugin");
      await expectPanelInViewport(page, capabilities.getByRole("group", { name: "Install plugin" }), "install plugin panel");
      await captureWorkbench(page, testInfo, `new-create-capabilities-plugins-${isMobile ? "mobile" : "desktop"}`);

      await openCapabilityPanel(page, capabilities, "MCP", "Add MCP server");
      await expectPanelInViewport(page, capabilities.getByRole("group", { name: "Add MCP server" }), "add MCP server panel");
      await captureWorkbench(page, testInfo, `new-create-capabilities-mcp-${isMobile ? "mobile" : "desktop"}`);

      await openCapabilityPanel(page, capabilities, "Tools", "Create toolset");
      await expectPanelInViewport(page, capabilities.getByRole("group", { name: "Create toolset" }), "create toolset panel");
      await captureWorkbench(page, testInfo, `new-create-capabilities-tools-${isMobile ? "mobile" : "desktop"}`);
    } finally {
      await server.stop();
    }
  });
});

async function openTopLevelView(page: Page, isMobile: boolean, name: "Automations" | "Capabilities" | "Settings") {
  const settings = page.getByRole("region", { name: "Settings", exact: true });
  if (await settings.count()) {
    const back = settings.getByRole("button", { name: "Back to app" });
    if (await back.count()) {
      await back.click();
      await expect(settings).toHaveCount(0);
    }
  }
  if (isMobile) {
    await openPanel(page, isMobile, "History");
  }
  await page.getByRole("button", { name, exact: true }).click();
}

async function openSettingsSection(settings: Locator, name: string) {
  await settings.getByRole("button", { name }).click();
  await expect(settings.getByRole("region", { name, exact: true })).toBeVisible();
}

async function openCapabilityPanel(page: Page, capabilities: Locator, tabName: string, actionName: string) {
  await capabilities.getByRole("tab", { name: tabName }).click();
  await expect(page).toHaveURL(/./);
  await capabilities.getByRole("button", { name: actionName }).click();
}

async function expectPanelInViewport(page: Page, panel: Locator, label: string) {
  await expect(panel, label).toBeVisible();
  await assertNoHorizontalOverflow(page, panel);
  const [viewport, box] = await Promise.all([
    page.viewportSize(),
    panel.boundingBox()
  ]);
  expect(viewport, `${label}: viewport`).not.toBeNull();
  expect(box, `${label}: panel box`).not.toBeNull();
  expect(box!.x, `${label}: left`).toBeGreaterThanOrEqual(0);
  expect(box!.y, `${label}: top`).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width, `${label}: right`).toBeLessThanOrEqual(viewport!.width);
  if (box!.y + box!.height > viewport!.height) {
    const metrics = await panel.evaluate((element) => {
      const sample = (node: Element | null) => {
        if (!node) return null;
        const rect = node.getBoundingClientRect();
        const style = getComputedStyle(node);
        return {
          className: node.getAttribute("class"),
          bottom: rect.bottom,
          height: rect.height,
          maxHeight: style.maxHeight,
          minHeight: style.minHeight,
          overflow: style.overflow,
          overflowY: style.overflowY,
          top: rect.top
        };
      };
      return {
        panel: sample(element),
        form: sample(element.closest("form")),
        surface: sample(element.closest(".automationSurface, .capabilitiesPage, .settingsContent")),
        page: sample(element.closest(".automationsPage, .capabilitiesPage, .settingsPage")),
        centerPage: sample(element.closest(".centerPage")),
        centerWorkspace: sample(element.closest(".centerWorkspace")),
        main: sample(document.querySelector(".appShell"))
      };
    });
    throw new Error(`${label}: panel bottom ${box!.y + box!.height} exceeds viewport ${viewport!.height}\n${JSON.stringify(metrics, null, 2)}`);
  }

  const clippedChromeControls = await panel.locator(".pevo-createPanelHeader button, .pevo-createPanelFooter button").evaluateAll((controls) => {
    const viewportWidth = window.innerWidth;
    const viewportHeight = window.innerHeight;
    return controls
      .filter((control) => {
        const style = getComputedStyle(control);
        return style.visibility !== "hidden" && style.display !== "none";
      })
      .map((control) => {
        const rect = control.getBoundingClientRect();
        return {
          label: control.getAttribute("aria-label") ?? control.textContent?.replace(/\s+/g, " ").trim() ?? control.tagName,
          left: rect.left,
          top: rect.top,
          right: rect.right,
          bottom: rect.bottom,
          clipped: rect.left < -1 || rect.top < -1 || rect.right > viewportWidth + 1 || rect.bottom > viewportHeight + 1
        };
      })
      .filter((control) => control.clipped);
  });
  expect(clippedChromeControls, `${label}: panel actions inside viewport`).toEqual([]);
}
