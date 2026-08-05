import { expect, test } from "@playwright/test";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { startPevoWeb } from "./harness";
import { openPanel } from "./workbench.support";

const WORKBENCH_BUDGET = loadWorkbenchBudget();
const INITIAL_JAVASCRIPT_BUDGET_BYTES = WORKBENCH_BUDGET.maximum.initialJavascriptBytes;
const DEFERRED_CHUNK_PATTERN = /(mermaid|terminal|settings-panels|capabilities-page|automations-panel|search-|right-workspace)/i;

test("keeps off-screen features outside the production startup graph", async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== "chromium-desktop", "one deterministic desktop budget is sufficient");
  const server = await startPevoWeb({ live: false });
  try {
    await page.goto(server.url);
    await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();

    const initial = await javascriptResources(page);
    const initialEncodedBodySize = initial.reduce(
      (total, entry) => total + entry.encodedBodySize,
      0
    );
    const initialOptionalPreviewResources = await resourcePaths(page, "/file-viewer/");
    writeStartupProof(testInfo.project.name, {
      baselineBytes: WORKBENCH_BUDGET.baseline.initialJavascriptBytes,
      budgetBytes: INITIAL_JAVASCRIPT_BUDGET_BYTES,
      initial,
      initialEncodedBodySize,
      initialOptionalPreviewResources
    });
    expect(initial.map((entry) => entry.name).filter((name) => DEFERRED_CHUNK_PATTERN.test(name))).toEqual([]);
    expect(initialOptionalPreviewResources).toEqual([]);
    expect(initialEncodedBodySize).toBeLessThanOrEqual(INITIAL_JAVASCRIPT_BUDGET_BYTES);

    await page.reload();
    await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
    const reload = await javascriptResources(page);
    writeStartupProof(testInfo.project.name, {
      baselineBytes: WORKBENCH_BUDGET.baseline.initialJavascriptBytes,
      budgetBytes: INITIAL_JAVASCRIPT_BUDGET_BYTES,
      initial,
      initialEncodedBodySize,
      initialOptionalPreviewResources,
      reload
    });
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

function loadWorkbenchBudget(): {
  baseline: { initialJavascriptBytes: number | null };
  maximum: { initialJavascriptBytes: number };
} {
  const source = readFileSync(path.resolve(process.cwd(), "non-functional-budgets.json"), "utf8");
  const parsed = JSON.parse(source) as {
    schemaVersion?: unknown;
    workbench?: {
      baseline?: { initialJavascriptBytes?: unknown };
      maximum?: { initialJavascriptBytes?: unknown };
    };
  };
  const baseline = parsed.workbench?.baseline?.initialJavascriptBytes;
  const maximum = parsed.workbench?.maximum?.initialJavascriptBytes;
  if (
    parsed.schemaVersion !== 1
    || (baseline !== null && (
      typeof baseline !== "number"
      || !Number.isSafeInteger(baseline)
      || baseline < 0
    ))
    || typeof maximum !== "number"
    || !Number.isSafeInteger(maximum)
    || maximum < 0
    || (typeof baseline === "number" && maximum < baseline)
  ) {
    throw new Error("non-functional-budgets.json has an invalid Workbench budget");
  }
  return {
    baseline: { initialJavascriptBytes: baseline },
    maximum: { initialJavascriptBytes: maximum }
  };
}

function writeStartupProof(projectName: string, proof: Record<string, unknown>): void {
  const artifactRoot = process.env.PSYCHEVO_PLAYWRIGHT_SCREENSHOT_ROOT;
  if (!artifactRoot) {
    return;
  }
  mkdirSync(artifactRoot, { recursive: true });
  writeFileSync(
    path.join(artifactRoot, `startup-resources-${projectName}.json`),
    `${JSON.stringify(proof, null, 2)}\n`
  );
}

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

async function resourcePaths(
  page: import("@playwright/test").Page,
  prefix: string
): Promise<string[]> {
  return page.evaluate((resourcePrefix) => performance.getEntriesByType("resource")
    .map((entry) => new URL(entry.name).pathname)
    .filter((name) => name.startsWith(resourcePrefix)), prefix);
}
