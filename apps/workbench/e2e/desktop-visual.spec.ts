import { createServer, type Server } from "node:http";
import { existsSync, mkdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { repoRoot } from "./harness";
import { visualScreenshotRoot } from "./visualArtifacts";

const desktopDist = path.join(repoRoot, "apps/desktop/dist");
const screenshotDir = visualScreenshotRoot("desktop");

test.describe("Desktop Floating visual contract", () => {
  test("renders toolbar, running, expanded, parked, and capture-error states", async ({ page }, testInfo) => {
    const server = await startDesktopDist();
    mkdirSync(screenshotDir, { recursive: true });
    try {
      await page.goto(`${server.url}/?surface=floating&visual=1`);
      const capsule = page.locator(".pevo-floating-capsule");
      const askAction = page.getByLabel("Floating actions").getByRole("button", { name: "Ask" });
      await expect(capsule).toBeVisible();
      await expect(askAction).toBeVisible();
      await expect(page.locator(".pevo-floating-attachmentChip")).toContainText("Selected text");
      await assertNoHorizontalOverflow(page, capsule);
	      await capture(page, testInfo, "01-toolbar");

	      await askAction.click();
	      await expect(capsule).toHaveAttribute("data-mode", "running");
	      await expect(page.locator(".pevo-floating-runningRow")).toHaveCount(0);
	      await expect(page.locator('.pevo-floating-promptRow button[title="Interrupt"]')).toBeVisible();
	      const assistantTranscript = page.locator(".pevo-transcript .pevo-message.is-assistant");
	      await expect(assistantTranscript).toContainText("keep context visible");
	      await capture(page, testInfo, "02-running");

      await expect(assistantTranscript).toContainText("avoid stealing focus", { timeout: 5_000 });
      await expect(page.getByRole("button", { name: "Open in main window" })).toBeVisible();
      await assertNoHorizontalOverflow(page, capsule);
      await assertFullyContained(assistantTranscript, page.locator(".pevo-threadItems"));
      await capture(page, testInfo, "03-expanded-answer");

      await askAction.click();
      await page.getByRole("button", { name: "Park" }).click();
      await expect(page.locator(".pevo-floating-parkedButton")).toHaveAttribute("aria-label", "Floating is running");
      await capture(page, testInfo, "04-parked-running");
      await expect(page.locator(".pevo-floating-parkedButton")).toHaveAttribute("aria-label", "Floating answer ready", { timeout: 5_000 });
      await capture(page, testInfo, "05-parked-done");

      await page.locator(".pevo-floating-parkedButton").click();
	      await page.getByRole("button", { name: "Capture region" }).click();
	      await expect(page.getByRole("alert")).toContainText("unavailable");
	      await assertNoHorizontalOverflow(page, capsule);
	      await assertFullyContained(assistantTranscript.last(), page.locator(".pevo-threadItems"));
	      await capture(page, testInfo, "06-capture-error");

	      await page.locator('button[title="Close"]').click();
	      await expect(page.locator(".pevo-floating-capsule")).toHaveCount(0);
	      await expect(page.locator(".pevo-floating-parkedButton")).toHaveCount(0);
	    } finally {
	      await server.stop();
	    }
  });
});

async function startDesktopDist(): Promise<{ stop(): Promise<void>; url: string }> {
  if (!existsSync(path.join(desktopDist, "index.html"))) {
    throw new Error(`Desktop dist is missing: ${desktopDist}`);
  }
  const server = createServer((request, response) => {
    const url = new URL(request.url ?? "/", "http://127.0.0.1");
    const relativePath = decodeURIComponent(url.pathname === "/" ? "/index.html" : url.pathname);
    const filePath = path.normalize(path.join(desktopDist, relativePath));
    if (!filePath.startsWith(desktopDist) || !existsSync(filePath) || !statSync(filePath).isFile()) {
      response.writeHead(404);
      response.end("not found");
      return;
    }
    response.writeHead(200, { "content-type": contentType(filePath) });
    response.end(readFileSync(filePath));
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
  const address = server.address();
  if (!address || typeof address === "string") {
    throw new Error("Desktop visual server did not bind a TCP port");
  }
  return {
    stop: () => closeServer(server),
    url: `http://127.0.0.1:${address.port}`
  };
}

function closeServer(server: Server): Promise<void> {
  return new Promise((resolve) => server.close(() => resolve()));
}

function contentType(filePath: string): string {
  if (filePath.endsWith(".html")) {
    return "text/html; charset=utf-8";
  }
  if (filePath.endsWith(".css")) {
    return "text/css; charset=utf-8";
  }
  if (filePath.endsWith(".js")) {
    return "text/javascript; charset=utf-8";
  }
  if (filePath.endsWith(".svg")) {
    return "image/svg+xml";
  }
  return "application/octet-stream";
}

async function capture(page: Page, testInfo: { project: { name: string } }, label: string) {
  await page.screenshot({
    path: path.join(screenshotDir, `${label}-${testInfo.project.name}.png`)
  });
}

async function assertNoHorizontalOverflow(page: Page, locator = page.locator("body")) {
  const overflow = await locator.evaluate((element) => {
    const root = element instanceof HTMLElement ? element : document.documentElement;
    return Math.ceil(root.scrollWidth) - Math.ceil(root.clientWidth);
  });
  expect(overflow).toBeLessThanOrEqual(1);
}

async function assertFullyContained(content: Locator, viewport: Locator) {
  const [contentBox, viewportBox] = await Promise.all([content.boundingBox(), viewport.boundingBox()]);
  expect(contentBox).not.toBeNull();
  expect(viewportBox).not.toBeNull();
  expect(contentBox!.y).toBeGreaterThanOrEqual(viewportBox!.y - 1);
  expect(contentBox!.y + contentBox!.height).toBeLessThanOrEqual(viewportBox!.y + viewportBox!.height + 1);
}
