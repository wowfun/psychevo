import { expect, test, type Page } from "@playwright/test";
import { startPevoWeb } from "./harness";

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
        const artifactsButton = page.getByRole("button", { name: "Artifacts" });
        const settingsButton = page.getByRole("button", { name: "Settings" });
        await expect(logoToggle).toBeVisible();
        await expect(newSessionButton).toBeVisible();
        await expect(searchButton).toBeVisible();
        await expect(artifactsButton).toBeVisible();
        await expect(settingsButton).toBeVisible();
        const [railBox, logoBox, newSessionBox, searchBox, artifactsBox, settingsBox] = await Promise.all([
          page.locator(".historyColumn").boundingBox(),
          logoToggle.boundingBox(),
          newSessionButton.boundingBox(),
          searchButton.boundingBox(),
          artifactsButton.boundingBox(),
          settingsButton.boundingBox()
        ]);
        expect(railBox).not.toBeNull();
        expect(logoBox).not.toBeNull();
        expect(newSessionBox).not.toBeNull();
        expect(searchBox).not.toBeNull();
        expect(artifactsBox).not.toBeNull();
        expect(settingsBox).not.toBeNull();
        expect(newSessionBox!.y).toBeGreaterThanOrEqual(logoBox!.y + logoBox!.height);
        expect(searchBox!.y).toBeGreaterThan(newSessionBox!.y);
        expect(artifactsBox!.y).toBeGreaterThan(searchBox!.y);
        expect(settingsBox!.y).toBeGreaterThan(artifactsBox!.y + artifactsBox!.height);
        expect(railBox!.y + railBox!.height - (settingsBox!.y + settingsBox!.height)).toBeLessThanOrEqual(18);
        await logoToggle.click();
      }

      await openPanel(page, isMobile, "History");
      await page.getByRole("button", { name: "New Session" }).click();
      await expect(page.locator(".pevo-sessionRow")).toHaveCount(1);
      await expect(page.locator(".pevo-sessionRow.is-draft")).toHaveCount(1);

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
      await expect(page.locator(".pevo-sessionRow")).toHaveCount(1);
      await expect(page.locator(".pevo-sessionRow.is-draft")).toHaveCount(1);

      await openPanel(page, isMobile, "Status");
      const statusRegion = page.getByRole("region", { name: "Status" });
      await expect(statusRegion.getByText("idle")).toBeVisible();
      await expect(statusRegion.getByText("No active session")).toBeVisible();
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
});

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

async function openPanel(page: Page, isMobile: boolean, name: "History" | "Status" | "Transcript") {
  if (name === "Status") {
    if (isMobile) {
      await page.getByRole("button", { name: "Transcript" }).click();
    }
    const expandInspector = page.getByRole("button", { name: "Show right inspector" });
    if (await expandInspector.count()) {
      await expect(expandInspector).toBeVisible();
      await expandInspector.click();
    }
  }
  if (isMobile) {
    await page.getByRole("button", { name }).click();
  }
  if (name === "Status") {
    await expect(page.getByRole("region", { name: "Status" })).toBeVisible();
  }
}
