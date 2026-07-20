import { existsSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import path from "node:path";
import { expect, test } from "@playwright/test";
import { startPevoWeb } from "./harness";
import { writeWorkspacePreviewFixtures } from "./fixtures/workspace-preview-fixtures";
import { openPanel } from "./workbench.support";

test("renders generated workspace preview fixtures without external document requests", async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== "chromium-desktop", "one deterministic browser covers the format engines");
  mkdirSync(testInfo.outputDir, { recursive: true });
  const cwd = mkdtempSync(path.join(testInfo.outputDir, "workspace-preview-"));
  const manifest = await writeWorkspacePreviewFixtures(cwd);
  if (manifest.tools.ffmpeg === null) {
    throw new Error("workspace preview media fixtures require ffmpeg with H.264, AAC, VP9, Opus, and MP3 encoders");
  }
  const externalRequests: string[] = [];
  const browserErrors: string[] = [];
  let allowedOrigin: string | null = null;
  const fileViewerResponses: { status: number; url: string }[] = [];
  const parseWorkerResponses: { status: number; url: string }[] = [];
  const previewRequests: { range: string | undefined; url: string }[] = [];
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__PSYCHEVO_PREVIEW_PWNED__ = false;
  });
  page.on("request", (request) => {
    const requestUrl = new URL(request.url());
    if (allowedOrigin && /^https?:$/.test(requestUrl.protocol) && requestUrl.origin !== allowedOrigin) {
      externalRequests.push(requestUrl.href);
    }
    if (request.url().includes("/_gateway/workspace-preview/")) {
      previewRequests.push({ range: request.headers().range, url: request.url() });
    }
  });
  page.on("response", (response) => {
    if (response.url().includes("/file-viewer/")) {
      fileViewerResponses.push({ status: response.status(), url: response.url() });
    }
    if (response.url().includes("workspace-file-parse.worker-")) {
      parseWorkerResponses.push({ status: response.status(), url: response.url() });
    }
  });
  page.on("console", (message) => {
    if (message.type() === "error") browserErrors.push(message.text());
  });
  page.on("pageerror", (error) => browserErrors.push(error.message));
  const server = await startPevoWeb({ cwd, live: false });
  allowedOrigin = new URL(server.url).origin;
  try {
    await page.goto(server.url);
    await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
    const initialScripts = await page.evaluate(() => performance.getEntriesByType("resource")
      .filter((entry): entry is PerformanceResourceTiming => (
        entry instanceof PerformanceResourceTiming && new URL(entry.name).pathname.endsWith(".js")
      ))
      .map((entry) => new URL(entry.name).pathname));
    expect(initialScripts.filter((name) => /(?:workspace-file-|_virtual_file-viewer|file-viewer-renderer)/i.test(name))).toEqual([]);
    await openPanel(page, false, "Status");
    const status = page.getByRole("region", { name: "Workspace status" });
    await status.getByRole("button", { name: "Refresh workspace" }).click();
    await status.getByRole("button", { name: "Files", exact: true }).click();
    const files = page.getByRole("region", { name: "Workspace files" });

    await files.getByRole("treeitem", { name: /fixture\.png/ }).click();
    const png = files.getByRole("img", { name: "Preview fixture.png" });
    await expect(png).toBeVisible();
    await expect.poll(() => png.evaluate((image: HTMLImageElement) => image.naturalWidth))
      .toBeGreaterThan(0);

    await files.getByRole("treeitem", { name: /hostile\.svg/ }).click();
    const svg = files.getByRole("img", { name: "Preview hostile.svg" });
    await expect(svg).toBeVisible();
    await expect.poll(() => svg.evaluate((image: HTMLImageElement) => image.naturalWidth))
      .toBeGreaterThan(0);

    await files.getByRole("treeitem", { name: /fixture\.csv/ }).click();
    const csv = files.getByRole("table", { name: "Preview fixture.csv" });
    await expect(csv.getByRole("columnheader", { name: "Name" })).toBeVisible();
    await expect(csv.getByRole("cell", { name: "Ada" })).toBeVisible();
    await expect(csv.getByRole("cell", { name: "99" })).toBeVisible();

    await files.getByRole("treeitem", { name: /fixture\.tsv/ }).click();
    const tsv = files.getByRole("table", { name: "Preview fixture.tsv" });
    await expect(tsv.getByRole("columnheader", { name: "Language" })).toBeVisible();
    await expect(tsv.getByRole("cell", { name: "TypeScript" })).toBeVisible();

    await files.getByRole("treeitem", { name: /fixture\.excalidraw/ }).click();
    const drawing = files.getByRole("img", { name: "Preview fixture.excalidraw" });
    await expect(drawing).toBeVisible();
    await expect(drawing).toContainText("Excalidraw fixture visible");

    await files.getByRole("treeitem", { name: /hostile\.zip/ }).click();
    const zip = files.getByRole("region", { name: "Preview hostile.zip", exact: true });
    await expect(zip).toContainText("safe/readme.txt");
    const zipPaths = await zip.locator("code").allTextContents();
    expect(zipPaths).toContain("safe/readme.txt");
    expect(zipPaths.every((entry) => (
      !entry.includes("..")
      && !entry.includes("\\")
      && !entry.startsWith("/")
      && !/^[A-Za-z]:/.test(entry)
    ))).toBe(true);

    previewRequests.length = 0;
    await files.getByRole("treeitem", { name: /fixture\.pdf/ }).click();
    const pdf = files.getByRole("region", { name: "File preview fixture.pdf", exact: true });
    await expect(pdf).toHaveAttribute("data-preview-state", "ready", { timeout: 30_000 });
    await expect(pdf).toContainText("1 pages", { timeout: 30_000 });
    await expect.poll(() => pdf.locator(".pdf-wrapper").evaluate((element) => element.clientHeight))
      .toBeGreaterThan(0);
    const pdfCanvas = pdf.locator("canvas").first();
    await expect(pdfCanvas).toBeVisible({ timeout: 30_000 });
    await expect.poll(() => pdfCanvas.evaluate((canvas: HTMLCanvasElement) => {
      const context = canvas.getContext("2d");
      if (!context || canvas.width === 0 || canvas.height === 0) return 0;
      const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
      let ink = 0;
      for (let index = 0; index < pixels.length; index += 16) {
        if (pixels[index + 3] && (pixels[index] < 245 || pixels[index + 1] < 245 || pixels[index + 2] < 245)) {
          ink += 1;
        }
      }
      return ink;
    }), { timeout: 30_000 }).toBeGreaterThan(20);
    const pdfResourceUrl = previewRequests.find((request) => request.url.includes("/_gateway/workspace-preview/"))?.url;
    expect(pdfResourceUrl).toBeTruthy();
    const pdfRange = await page.evaluate(async (url) => {
      const response = await fetch(url, {
        cache: "no-store",
        headers: { Range: "bytes=0-1023" }
      });
      return {
        bytes: (await response.arrayBuffer()).byteLength,
        contentRange: response.headers.get("content-range"),
        status: response.status
      };
    }, pdfResourceUrl!);
    expect(pdfRange).toEqual({
      bytes: 1024,
      contentRange: expect.stringMatching(/^bytes 0-1023\//),
      status: 206
    });
    await expect.poll(() => fileViewerResponses.some((response) => (
      response.url.includes("/file-viewer/vendor/pdf/pdf.worker.mjs") && response.status < 400
    ))).toBe(true);

    for (const [filename, sentinel, workerPath] of [
      ["fixture.docx", "DOCX fixture visible", "/file-viewer/vendor/docx/docx.worker.js"],
      ["fixture.xlsx", "XLSX fixture visible", "/file-viewer/vendor/xlsx/sheet.worker.js"],
      ["fixture.pptx", "PPTX fixture visible", "/file-viewer/vendor/pptx/pptx.worker.js"],
      ["fixture.rtf", "RTF fixture visible", null],
      ["fixture.odt", "ODT fixture visible", null],
      ["fixture.ods", "ODS fixture visible", null],
      ["fixture.odp", "ODP fixture visible", null],
      ["fixture.ofd", "OFD fixture visible", null]
    ] as const) {
      await files.getByRole("treeitem", { name: new RegExp(filename.replace(".", "\\.")) }).click();
      const office = files.getByRole("region", { name: `File preview ${filename}`, exact: true });
      if (filename === "fixture.xlsx" || filename === "fixture.ods") {
        const rowSummary = filename === "fixture.xlsx" ? "2 rows, 2 columns" : "1 rows, 2 columns";
        await expect(office).toContainText(rowSummary, { timeout: 30_000 });
        const canvas = office.locator(".e-virt-table-canvas");
        await expect(canvas).toBeVisible();
        await expect.poll(() => canvas.evaluate((element: HTMLCanvasElement) => {
          const pixels = element.getContext("2d")?.getImageData(0, 0, element.width, element.height).data;
          let ink = 0;
          if (pixels) {
            for (let offset = 0; offset < pixels.length; offset += 4) {
              if (pixels[offset + 3] > 0 && (
                pixels[offset] < 245 || pixels[offset + 1] < 245 || pixels[offset + 2] < 245
              )) ink += 1;
            }
          }
          return ink;
        }), { timeout: 30_000 }).toBeGreaterThan(1_000);
        await page.context().grantPermissions(["clipboard-read", "clipboard-write"], {
          origin: allowedOrigin ?? undefined
        });
        await page.evaluate(() => navigator.clipboard.writeText(""));
        await canvas.click({ position: { x: 120, y: 45 } });
        await page.keyboard.press("Control+c");
        await expect.poll(() => page.evaluate(() => navigator.clipboard.readText()))
          .toContain(sentinel);
      } else {
        await expect(office).toContainText(sentinel, { timeout: 30_000 });
      }
      if (workerPath) {
        await expect.poll(() => fileViewerResponses.some((response) => (
          response.url.includes(workerPath) && response.status < 400
        )), { timeout: 30_000 }).toBe(true);
      }
    }

    for (const filename of ["fixture.mp4", "fixture.webm"] as const) {
      await files.getByRole("treeitem", { name: new RegExp(filename.replace(".", "\\.")) }).click();
      const video = files.getByLabel(`Preview ${filename}`, { exact: true });
      await expect(video).toBeVisible();
      await expect.poll(() => video.evaluate((element: HTMLVideoElement) => element.readyState))
        .toBeGreaterThanOrEqual(1);
      expect(await video.evaluate((element: HTMLVideoElement) => element.duration)).toBeGreaterThan(3);
      await video.evaluate(async (element: HTMLVideoElement) => {
        element.muted = true;
        await element.play();
      });
      await expect.poll(() => video.evaluate((element: HTMLVideoElement) => element.currentTime)).toBeGreaterThan(0.1);
      await video.evaluate((element: HTMLVideoElement) => {
        element.currentTime = 1.5;
      });
      await expect.poll(() => video.evaluate((element: HTMLVideoElement) => element.currentTime)).toBeGreaterThan(1.4);
      await video.evaluate((element: HTMLVideoElement) => element.pause());
    }

    await files.getByRole("treeitem", { name: /fixture\.mp3/ }).click();
    const audio = files.getByLabel("Preview fixture.mp3", { exact: true });
    await expect(audio).toBeVisible();
    await expect.poll(() => audio.evaluate((element: HTMLAudioElement) => element.readyState)).toBeGreaterThanOrEqual(1);
    expect(await audio.evaluate((element: HTMLAudioElement) => element.duration)).toBeGreaterThan(3);

    await files.getByRole("treeitem", { name: /fixture\.heic/ }).click();
    const heicSurface = files.getByRole("region", { name: "File preview fixture.heic", exact: true });
    const heic = heicSurface.getByRole("img", { name: "Image", exact: true });
    await expect.poll(async () => {
      const alert = heicSurface.getByRole("alert");
      if (await alert.count()) {
        return `error:${await alert.textContent()}:${browserErrors.join(" | ")}`;
      }
      if (await heic.count()) {
        return await heic.evaluate((image: HTMLImageElement) => image.naturalWidth) > 0
          ? "ready"
          : "loading";
      }
      return "loading";
    }, { timeout: 30_000 }).toBe("ready");

    for (const viewport of [
      { height: 960, width: 1440 },
      { height: 720, width: 900 }
    ]) {
      await page.setViewportSize(viewport);
      await expect(heicSurface).toBeVisible();
      const layout = await heicSurface.evaluate((surface) => {
        const vendor = surface.querySelector<HTMLElement>(".workspaceFileVendor");
        return {
          surfaceWidth: surface.clientWidth,
          vendorScrollWidth: vendor?.scrollWidth ?? 0,
          vendorWidth: vendor?.clientWidth ?? 0
        };
      });
      expect(layout.surfaceWidth).toBeGreaterThan(0);
      expect(layout.vendorWidth).toBeGreaterThan(0);
      expect(layout.vendorWidth).toBeLessThanOrEqual(layout.surfaceWidth);
      expect(layout.vendorScrollWidth).toBeLessThanOrEqual(layout.vendorWidth + 1);
    }

    expect(fileViewerResponses.filter((response) => response.status >= 400)).toEqual([]);
    expect(parseWorkerResponses.length).toBeGreaterThan(0);
    expect(parseWorkerResponses.every((response) => response.status < 400)).toBe(true);

    expect(manifest.fixtures.map((fixture) => fixture.path)).toEqual(expect.arrayContaining([
      "fixture.docx", "fixture.heic", "fixture.odp", "fixture.ods", "fixture.odt", "fixture.ofd",
      "fixture.pdf", "fixture.png", "fixture.pptx", "fixture.rtf", "fixture.xlsx"
    ]));
    expect(manifest.fixtures.map((fixture) => fixture.path)).toEqual(expect.arrayContaining([
      "fixture.mp3", "fixture.mp4", "fixture.webm"
    ]));
    expect(manifest.unavailable).not.toContain("heic:no-local-encoder");
    expect(await page.evaluate(() => (
      (window as unknown as Record<string, unknown>).__PSYCHEVO_PREVIEW_PWNED__
    ))).toBe(false);
    expect(existsSync(path.join(cwd, "escape.txt"))).toBe(false);
    expect(existsSync(path.join(cwd, "escape2.txt"))).toBe(false);
    expect(existsSync(path.resolve(cwd, "..", "escape.txt"))).toBe(false);
    expect(existsSync(path.resolve(cwd, "..", "escape2.txt"))).toBe(false);
    expect(externalRequests).toEqual([]);
  } finally {
    await server.stop();
    rmSync(cwd, { force: true, recursive: true });
  }
});
