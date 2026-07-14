import { expect, test } from "@playwright/test";
import { PREFS_APPEARANCE_VERSION, PREFS_KEY } from "../src/storage";
import { startPevoWeb } from "./harness";
import {
  captureWorkbench,
  injectStructuredToolRows,
  openPanel
} from "./workbench.support";

test.describe("pevo Web Workbench", () => {
  test("keeps parallel live web search headers inside transcript rows", async ({ page, isMobile }) => {
    const server = await startPevoWeb({ live: false });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      await openPanel(page, isMobile, "Transcript");

      const longTitles = [
        "Searching the web trending Rust projects July 2026 GitHub repositories with sustained community growth and recent releases",
        "Searching the web 2026 年 7 月最流行的 Rust 开源项目、近期发布、社区增长趋势与 GitHub 活跃度排行"
      ];
      await page.locator(".pevo-threadItems").evaluate((container, titles) => {
        container.innerHTML = titles.map((_, index) => `
          <article class="pevo-evidence is-running" data-testid="long-tool-row-${index}">
            <button class="pevo-evidenceLine is-singleTitle is-runningTool" type="button">
              <span class="pevo-evidenceSpinner" aria-hidden="true">◌</span>
              <code></code>
              <em>${index + 3}s</em>
            </button>
          </article>
        `).join("");
        container.querySelectorAll("code").forEach((code, index) => {
          code.textContent = titles[index] ?? "";
          code.title = titles[index] ?? "";
        });
      }, longTitles);

      await expect(page.locator('[data-testid^="long-tool-row-"]')).toHaveCount(2);
      for (const [index, longTitle] of longTitles.entries()) {
        const row = page.getByTestId(`long-tool-row-${index}`);
        const line = row.locator(".pevo-evidenceLine");
        const status = line.locator("em");
        const title = line.locator("code");
        const rowBox = await line.boundingBox();
        const statusBox = await status.boundingBox();
        const titleClipped = await title.evaluate((element) => element.scrollWidth > element.clientWidth);

        expect(rowBox).not.toBeNull();
        expect(statusBox).not.toBeNull();
        await expect(title).toContainText("Searching the web");
        await expect(title).toHaveAttribute("title", longTitle);
        await expect(line.locator("span:not(.pevo-evidenceSpinner)")).toHaveCount(0);
        expect(statusBox!.x + statusBox!.width).toBeLessThanOrEqual(rowBox!.x + rowBox!.width);
        expect(titleClipped).toBe(true);
      }
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
});
