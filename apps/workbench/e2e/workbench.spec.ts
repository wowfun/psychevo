import { expect, test } from "@playwright/test";
import { startPevoWeb } from "./harness";
import {
  assertLeftNavigationSectionAlignment,
  assertNoHorizontalOverflow,
  assertNoPageVerticalOverflow,
  captureWorkbench,
  composerBoxMetrics,
  openPanel,
  sideConversationPanel
} from "./workbench.support";

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

  test("keeps the desktop app shell fixed in a short viewport", async ({ page, isMobile }) => {
    test.skip(isMobile, "desktop shell overflow is covered by the desktop viewport");
    await page.setViewportSize({ width: 1280, height: 420 });
    const server = await startPevoWeb({ live: false });
    try {
      await page.goto(server.url);
      await expect(page.getByPlaceholder("Ask Psychevo...")).toBeVisible();
      await assertNoPageVerticalOverflow(page);

      await page.getByRole("button", { name: "Settings" }).click();
      await expect(page.getByRole("region", { name: "Settings" })).toBeVisible();
      await assertNoPageVerticalOverflow(page);
    } finally {
      await server.stop();
    }
  });
});
