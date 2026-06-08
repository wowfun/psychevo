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

async function openPanel(page: Page, isMobile: boolean, name: "History" | "Status" | "Transcript") {
  if (name === "Status") {
    if (isMobile) {
      await page.getByRole("button", { name: "Transcript" }).click();
    }
    const expandInspector = page.getByRole("button", { name: "Show right inspector" });
    if (await expandInspector.isVisible().catch(() => false)) {
      await expandInspector.click();
    }
  }
  if (isMobile) {
    await page.getByRole("button", { name }).click();
  }
}
