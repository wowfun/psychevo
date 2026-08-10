import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator } from "@playwright/test";
import { startPevoWeb } from "./harness";
import { captureWorkbench, openPanel } from "./workbench.support";

test.describe("workspace navigation", () => {
  test("keeps pin styling shared and exercises multi-directory workspace navigation", async ({ page, isMobile }, testInfo) => {
    const server = await startPevoWeb({ live: false });
    const secondaryRoot = path.join(server.root, "secondary-root");
    mkdirSync(secondaryRoot, { recursive: true });
    writeFileSync(path.join(secondaryRoot, "SECONDARY.md"), "Secondary workspace root.\n");
    writeFileSync(path.join(server.cwd, "FIRST.md"), "First primary-root file.\n");
    writeFileSync(path.join(server.cwd, "SECOND.md"), "Second primary-root file.\n");

    try {
      await page.goto(server.url);
      const composer = page.getByPlaceholder("Ask Psychevo...");
      await expect(composer).toBeEnabled({ timeout: 60_000 });
      await openPanel(page, isMobile, "Transcript");
      await composer.fill("Workspace navigation visual validation.");
      await page.getByRole("button", { name: "Send message" }).click();
      await expect(page.locator(".pevo-message.is-user")).toContainText("Workspace navigation visual validation");

      await openPanel(page, isMobile, "History");
      const sessionsPanel = page.getByRole("region", { name: "Sessions", exact: true });
      const ordinaryRow = sessionsPanel.locator(".pevo-sessionRow").first();
      await expect(ordinaryRow).toBeVisible();
      const ordinarySignature = await rowVisualSignature(ordinaryRow);
      await ordinaryRow.locator('summary[aria-label="Session actions"]').click();
      await page.getByRole("menuitem", { name: "Pin", exact: true }).click();

      const pinnedPanel = page.getByRole("region", { name: "Pinned sessions" });
      const pinnedRow = pinnedPanel.locator(".pevo-sessionRow").first();
      await expect(pinnedRow).toBeVisible();
      await expect(sessionsPanel.locator(".pevo-sessionRow")).toHaveCount(0);
      expect(await rowVisualSignature(pinnedRow)).toEqual(ordinarySignature);
      await pinnedRow.locator('summary[aria-label="Session actions"]').click();
      for (const label of ["Unpin", "Rename", "Export", "Share", "Archive", "Delete"]) {
        await expect(page.getByRole("menuitem", { name: label, exact: true })).toBeVisible();
      }
      await captureWorkbench(page, testInfo, `workspace-thread-pinned-${isMobile ? "mobile" : "desktop"}`);
      await page.getByRole("menuitem", { name: "Unpin", exact: true }).click();
      await expect(sessionsPanel.locator(".pevo-sessionRow").first()).toBeVisible();

      const ordinaryWorkspaceHeader = sessionsPanel.locator(".pevo-sessionGroupHeader").first();
      await ordinaryWorkspaceHeader.locator('summary[aria-label="Workspace actions"]').click();
      await page.getByRole("menuitem", { name: "Edit workspace", exact: true }).click();
      const editor = page.getByRole("dialog", { name: /Edit workspace/ });
      await expect(editor).toBeVisible();
      await editor.getByLabel("Name").fill("Workspace visual validation");
      await editor.getByRole("button", { name: "Add directory" }).click();
      await editor.getByLabel("Working directory 2").fill(secondaryRoot);
      await expect(editor.getByText("Primary", { exact: true })).toBeVisible();
      await expect(editor.getByRole("button", { name: `Make ${secondaryRoot} primary` })).toBeVisible();
      await captureWorkbench(page, testInfo, `workspace-editor-${isMobile ? "mobile" : "desktop"}`);
      await editor.getByRole("button", { name: "Save workspace" }).click();
      await expect(editor).toBeHidden();
      await expect(sessionsPanel.getByText("Workspace visual validation", { exact: true })).toBeVisible();

      const renamedWorkspaceHeader = sessionsPanel.locator(".pevo-sessionGroupHeader").first();
      await renamedWorkspaceHeader.locator('summary[aria-label="Workspace actions"]').click();
      await page.getByRole("menuitem", { name: "Pin", exact: true }).click();
      const pinnedWorkspaceHeader = pinnedPanel.locator(".pevo-sessionGroupHeader").first();
      await expect(pinnedWorkspaceHeader.getByText("Workspace visual validation", { exact: true })).toBeVisible();
      await expect(pinnedPanel.locator(".pevo-sessionRow").first()).toBeVisible();
      await pinnedWorkspaceHeader.locator('summary[aria-label="Workspace actions"]').click();
      await expect(page.getByRole("menuitem", { name: "Unpin", exact: true })).toBeVisible();
      await expect(page.getByRole("menuitem", { name: "Edit workspace", exact: true })).toBeVisible();
      await page.keyboard.press("Escape");
      await captureWorkbench(page, testInfo, `workspace-pinned-${isMobile ? "mobile" : "desktop"}`);

      await pinnedWorkspaceHeader.getByRole("button", {
        name: "New session in Workspace visual validation",
        exact: true
      }).click();
      await openPanel(page, isMobile, "Transcript");
      await expect(composer).toBeEnabled({ timeout: 60_000 });
      await composer.fill("Validate the explicit multi-directory workspace.");
      await page.getByRole("button", { name: "Send message" }).click();
      await expect(page.locator(".pevo-message.is-user").last())
        .toContainText("explicit multi-directory workspace");

      await openPanel(page, isMobile, "Status");
      const statusRegion = page.getByRole("region", { name: "Workspace status" });
      await statusRegion.getByRole("button", { name: "Files", exact: true }).click();
      const filesRegion = page.getByRole("region", { name: "Workspace files" });
      await expect(filesRegion).toBeVisible();
      const rootSelector = filesRegion.getByRole("button", { name: "Workspace directory" });
      await rootSelector.click();
      const rootMenu = filesRegion.getByRole("menu", { name: "Workspace directory" });
      await expect(rootMenu.getByRole("menuitemradio")).toHaveCount(2);
      await expect(rootMenu.locator('[role="menuitemradio"][aria-checked="true"] svg')).toBeVisible();
      const rootMenuBox = await rootMenu.boundingBox();
      const viewport = page.viewportSize();
      expect(rootMenuBox).not.toBeNull();
      expect(viewport).not.toBeNull();
      expect(rootMenuBox!.x).toBeGreaterThanOrEqual(0);
      expect(rootMenuBox!.x + rootMenuBox!.width).toBeLessThanOrEqual(viewport!.width + 1);
      await captureWorkbench(page, testInfo, `workspace-root-selector-${isMobile ? "mobile" : "desktop"}`);
      await rootMenu.getByRole("menuitemradio", { name: secondaryRoot, exact: true }).click();
      await expect(rootSelector).toHaveAttribute("title", secondaryRoot);
      await expect(filesRegion.getByText("SECONDARY.md", { exact: true })).toBeVisible();
      await captureWorkbench(page, testInfo, `workspace-secondary-root-${isMobile ? "mobile" : "desktop"}`);

      await filesRegion.getByRole("treeitem", { name: /SECONDARY\.md/ }).click();
      await filesRegion.getByRole("button", { name: /^Edit .*SECONDARY\.md$/ }).click();
      const fileEditor = filesRegion.getByRole("textbox", { name: /^Edit .*SECONDARY\.md$/ });
      await fileEditor.fill("Unsaved secondary-root edit.\n");
      await rootSelector.click();
      await rootMenu.getByRole("menuitemradio", { name: server.cwd, exact: true }).click();
      const discardDialog = page.getByRole("dialog", { name: "Discard unsaved file edits?" });
      await expect(discardDialog).toBeVisible();
      await captureWorkbench(page, testInfo, `workspace-root-dirty-confirm-${isMobile ? "mobile" : "desktop"}`);
      await discardDialog.getByRole("button", { name: "Cancel", exact: true }).click();
      await expect(rootSelector).toHaveAttribute("title", secondaryRoot);
      await expect(fileEditor).toHaveValue("Unsaved secondary-root edit.\n");

      await rootSelector.click();
      await rootMenu.getByRole("menuitemradio", { name: server.cwd, exact: true }).click();
      await page.getByRole("dialog", { name: "Discard unsaved file edits?" })
        .getByRole("button", { name: "Discard edits", exact: true })
        .click();
      await expect(rootSelector).toHaveAttribute("title", server.cwd);
      await expect(filesRegion.getByRole("treeitem", { name: "src" })).toBeVisible();

      await filesRegion.getByRole("treeitem", { name: /FIRST\.md/ }).click();
      await filesRegion.getByRole("button", { name: /^Edit .*FIRST\.md$/ }).click();
      const primaryEditor = filesRegion.getByRole("textbox", { name: /^Edit .*FIRST\.md$/ });
      await primaryEditor.fill("Unsaved same-root edit.\n");
      await filesRegion.getByRole("treeitem", { name: /SECOND\.md/ }).click();
      const sameRootDiscard = page.getByRole("dialog", { name: "Discard unsaved file edits?" });
      await expect(sameRootDiscard).toBeVisible();
      await captureWorkbench(page, testInfo, `workspace-same-root-dirty-confirm-${isMobile ? "mobile" : "desktop"}`);
      await sameRootDiscard.getByRole("button", { name: "Cancel", exact: true }).click();
      await expect(primaryEditor).toHaveValue("Unsaved same-root edit.\n");
    } finally {
      await server.stop();
    }
  });
});

async function rowVisualSignature(row: Locator) {
  return row.evaluate((element) => {
    const rowStyle = getComputedStyle(element);
    const main = element.querySelector<HTMLElement>(".pevo-sessionMain");
    const title = element.querySelector<HTMLElement>(".pevo-sessionTitle");
    const time = element.querySelector<HTMLElement>(".pevo-sessionTime");
    const menu = element.querySelector<HTMLElement>(".pevo-sessionMenu summary");
    if (!main || !title || !time || !menu) throw new Error("shared session row is incomplete");
    const rect = element.getBoundingClientRect();
    const mainStyle = getComputedStyle(main);
    const titleStyle = getComputedStyle(title);
    const timeStyle = getComputedStyle(time);
    const menuStyle = getComputedStyle(menu);
    return {
      backgroundColor: rowStyle.backgroundColor,
      borderColor: rowStyle.borderColor,
      borderRadius: rowStyle.borderRadius,
      display: rowStyle.display,
      gridTemplateColumns: rowStyle.gridTemplateColumns,
      height: rect.height,
      mainColor: mainStyle.color,
      menuColor: menuStyle.color,
      padding: rowStyle.padding,
      timeColor: timeStyle.color,
      titleColor: titleStyle.color,
      titleFontSize: titleStyle.fontSize,
      titleFontWeight: titleStyle.fontWeight
    };
  });
}
