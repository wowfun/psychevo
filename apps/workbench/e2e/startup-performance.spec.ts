import { expect, test } from "@playwright/test";
import { startPevoWeb } from "./harness";
import { openPanel } from "./workbench.support";

const INITIAL_JAVASCRIPT_BUDGET_BYTES = 1_800_000;
const DEFERRED_CHUNK_PATTERN = /(mermaid|terminal|settings-panels|capabilities-page|automations-panel|search-|right-workspace)/i;

test("keeps off-screen features outside the production startup graph", async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== "chromium-desktop", "one deterministic desktop budget is sufficient");
  const server = await startPevoWeb({ live: false });
  try {
    await page.goto(server.url);
    await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();

    const initial = await javascriptResources(page);
    expect(initial.map((entry) => entry.name).filter((name) => DEFERRED_CHUNK_PATTERN.test(name))).toEqual([]);
    expect(initial.reduce((total, entry) => total + entry.encodedBodySize, 0))
      .toBeLessThanOrEqual(INITIAL_JAVASCRIPT_BUDGET_BYTES);

    await page.reload();
    await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
    const reload = await javascriptResources(page);
    expect(reload.length).toBeGreaterThan(0);
    expect(reload.every((entry) => entry.transferSize === 0)).toBe(true);

    await openPanel(page, false, "Status");
    const terminalChunk = page.waitForResponse((response) => (
      response.url().includes("/assets/vendor-terminal-") && response.url().endsWith(".js")
    ));
    await page.getByRole("button", { name: "Terminal", exact: true }).click();
    expect((await terminalChunk).ok()).toBe(true);
  } finally {
    await server.stop();
  }
});

async function javascriptResources(page: import("@playwright/test").Page) {
  await page.waitForLoadState("load");
  return page.evaluate(() => performance.getEntriesByType("resource")
    .filter((entry): entry is PerformanceResourceTiming => (
      entry instanceof PerformanceResourceTiming && new URL(entry.name).pathname.endsWith(".js")
    ))
    .map((entry) => ({
      encodedBodySize: entry.encodedBodySize,
      name: new URL(entry.name).pathname,
      transferSize: entry.transferSize
    })));
}
