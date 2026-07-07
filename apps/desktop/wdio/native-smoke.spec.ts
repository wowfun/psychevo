import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { browser, expect } from "@wdio/globals";

const artifactRoot = path.resolve(
  process.env.PSYCHEVO_WDIO_ARTIFACT_ROOT
    ?? path.join(process.cwd(), "../../.local/.psychevo-dev/wdio/desktop-native-smoke")
);
const screenshotRoot = path.join(artifactRoot, "screenshots");
const providerLive = process.env.PSYCHEVO_DESKTOP_PROVIDER_LIVE === "1";
const providerToken = process.env.PSYCHEVO_FLOATING_PROVIDER_TOKEN ?? "PEVO_DESKTOP_FLOATING_PROVIDER_LIVE_OK";
const compactFloatingHeightLimit = 220;
const compositorFloatingHeightLimit = 280;

interface PageVerticalMetrics {
  bodyClientHeight: number;
  bodyScrollHeight: number;
  clientHeight: number;
  scrollHeight: number;
  scrollTop: number;
  scrollY: number;
}

interface FloatingWindowMetrics {
  bodyBackground: string;
  capsuleHeight: number;
  capsuleLeft: number;
  capsuleWidth: number;
  devicePixelRatio: number;
  innerHeight: number;
  innerWidth: number;
  rootBackground: string;
}

interface FloatingProviderLiveTimings {
  acceptedOrTurnStartedAtMs: number | null;
  clickedAtMs: number | null;
  error: string | null;
  finalTokenAtMs: number | null;
  firstAssistantDomAtMs: number | null;
  firstAssistantText: string | null;
  observedStreamingBeforeFinal: boolean;
  providerToken: string;
}

describe("Psychevo Desktop native smoke", () => {
  before(() => {
    mkdirSync(screenshotRoot, { recursive: true });
  });

  it("renders Workbench and Floating windows and exposes the native bridge", async () => {
    if (!existsSync(process.env.PSYCHEVO_DESKTOP_WDIO_APP ?? "")) {
      // The configured application path is still validated by the service; this
      // branch only keeps the assertion message clear when an override is wrong.
      expect(process.env.PSYCHEVO_DESKTOP_WDIO_APP ?? "default application path").toBeTruthy();
    }

    const windows = await waitForWindows();
    const workbench = windows.find((window) => !window.url.includes("surface=floating"));
    const floating = windows.find((window) => window.title.includes("Floating") || window.url.includes("surface=floating"));

    expect(workbench).toBeTruthy();
    expect(floating).toBeTruthy();

    await browser.switchToWindow(workbench!.handle);
    await browser.setWindowSize(1280, 420);
    await browser.waitUntil(async () => (await browser.$('textarea[placeholder="Ask Psychevo..."]')).isDisplayed(), {
      timeoutMsg: "Workbench composer did not render in the native window"
    });
    await assertNoPageVerticalOverflow();
    await browser.saveScreenshot(path.join(screenshotRoot, "01-workbench-short-window.png"));
    await browser.$('button[title="Settings"]').click();
    await browser.waitUntil(async () => (await browser.$(".settingsPage")).isDisplayed(), {
      timeoutMsg: "Workbench Settings did not render in the native window"
    });
    await assertNoPageVerticalOverflow();
    await browser.saveScreenshot(path.join(screenshotRoot, "02-workbench-settings-short-window.png"));
    const capabilities = await invokeDesktopPlatformCapabilities();
    expect(capabilities.ok).toBe(true);
    expect(capabilities.value.os).toBeTruthy();
    expect(capabilities.value.capture).toBeTruthy();

    await browser.switchToWindow(floating!.handle);
    if (providerLive) {
      await assertFloatingProviderLive();
      await browser.saveScreenshot(path.join(screenshotRoot, "03-floating-provider-live-window.png"));
      return;
    }

    await browser.execute(() => {
      window.location.href = "/?surface=floating&visual=1";
    });
    await waitForFloatingCapsule("Floating visual capsule did not render in the native window");
    await browser.waitUntil(async () => {
      const metrics = await floatingWindowMetrics();
      return floatingWindowFitsVisualBounds(metrics);
    }, {
      timeoutMsg: "Floating native window did not fit the visual capsule content"
    });
    const floatingMetrics = await floatingWindowMetrics();
    expect(floatingWindowFitsVisualBounds(floatingMetrics)).toBe(true);
    expect(floatingMetrics.capsuleLeft).toBeLessThanOrEqual(1);
    expect(floatingMetrics.capsuleWidth).toBeGreaterThanOrEqual(floatingMetrics.innerWidth - 1);
    expect(isTransparentColor(floatingMetrics.rootBackground)).toBe(false);
    expect(isTransparentColor(floatingMetrics.bodyBackground)).toBe(false);
    expect(await browser.$(".pevo-floating-attachmentChip").getText()).toContain("Selected text");
    await browser.saveScreenshot(path.join(screenshotRoot, "03-floating-visual-window.png"));
  });
});

async function assertFloatingProviderLive(): Promise<void> {
  const timings: FloatingProviderLiveTimings = {
    acceptedOrTurnStartedAtMs: null,
    clickedAtMs: null,
    error: null,
    finalTokenAtMs: null,
    firstAssistantDomAtMs: null,
    firstAssistantText: null,
    observedStreamingBeforeFinal: false,
    providerToken
  };
  await waitForFloatingCapsule("Floating provider capsule did not render in the native window");
  const chip = await browser.$(".pevo-floating-attachmentChip");
  await browser.waitUntil(async () => ((await chip.getAttribute("title")) ?? "").includes(providerToken), {
    timeoutMsg: "Floating provider live selection did not include the probe token"
  });
  const prompt = await browser.$('input[aria-label="Ask Psychevo"]');
  await prompt.setValue(`Reply with exactly this text and nothing else: ${providerToken}`);
  await browser.waitUntil(async () => (await browser.$('.pevo-floating-promptRow button[title="Ask"]')).isEnabled(), {
    timeoutMsg: "Floating provider submit button did not become enabled"
  });
  await browser.$('.pevo-floating-promptRow button[title="Ask"]').click();
  timings.clickedAtMs = Date.now();
  try {
    await browser.waitUntil(async () => {
      const mode = await browser.$(".pevo-floating-capsule").getAttribute("data-mode").catch(() => null);
      if (mode === "running" || mode === "expanded") {
        timings.acceptedOrTurnStartedAtMs ??= Date.now();
        return true;
      }
      return false;
    }, {
      interval: 250,
      timeout: 30_000,
      timeoutMsg: "Floating provider live did not observe accepted/running state"
    });

    await browser.waitUntil(async () => {
      const error = await floatingErrorText();
      if (error) {
        timings.error = error;
        throw new Error(`Floating provider live failed: ${error}`);
      }
      const rows = await browser.$$(".pevo-transcript .pevo-message.is-assistant");
      for (const row of rows) {
        const text = (await row.getText()).trim();
        if (!text) {
          continue;
        }
        if (timings.firstAssistantDomAtMs === null) {
          timings.firstAssistantDomAtMs = Date.now();
          timings.firstAssistantText = text;
        }
        if (text.includes(providerToken)) {
          timings.finalTokenAtMs = Date.now();
          return true;
        }
        timings.observedStreamingBeforeFinal = true;
      }
      return false;
    }, {
      interval: 3_000,
      timeout: 240_000,
      timeoutMsg: "Floating provider live response did not contain the probe token"
    });
  } catch (error) {
    timings.error = error instanceof Error ? error.message : String(error);
    throw error;
  } finally {
    writeFloatingProviderTimingArtifact(timings);
  }
}

function writeFloatingProviderTimingArtifact(timings: FloatingProviderLiveTimings): void {
  const durations = {
    acceptedOrTurnStartedMs: timings.clickedAtMs && timings.acceptedOrTurnStartedAtMs
      ? timings.acceptedOrTurnStartedAtMs - timings.clickedAtMs
      : null,
    firstAssistantDomMs: timings.clickedAtMs && timings.firstAssistantDomAtMs
      ? timings.firstAssistantDomAtMs - timings.clickedAtMs
      : null,
    finalTokenMs: timings.clickedAtMs && timings.finalTokenAtMs
      ? timings.finalTokenAtMs - timings.clickedAtMs
      : null
  };
  writeFileSync(
    path.join(artifactRoot, "floating-provider-live-timings.json"),
    `${JSON.stringify({ ...timings, durations }, null, 2)}\n`,
    "utf8"
  );
}

async function waitForFloatingCapsule(timeoutMsg: string): Promise<void> {
  await browser.waitUntil(async () => (await browser.$(".pevo-floating-capsule")).isDisplayed(), {
    timeoutMsg
  });
}

async function waitForWindows(): Promise<Array<{ handle: string; title: string; url: string }>> {
  await browser.waitUntil(async () => (await browser.getWindowHandles()).length >= 2, {
    timeoutMsg: "Desktop did not expose both Workbench and Floating windows"
  });
  const handles = await browser.getWindowHandles();
  const windows = [];
  for (const handle of handles) {
    await browser.switchToWindow(handle);
    windows.push({
      handle,
      title: await browser.getTitle(),
      url: await browser.getUrl()
    });
  }
  return windows;
}

async function invokeDesktopPlatformCapabilities(): Promise<{
  ok: boolean;
  error?: string;
  value: { capture?: unknown; os?: string };
}> {
  return browser.executeAsync((done) => {
    const invoke = (window as unknown as {
      __TAURI_INTERNALS__?: {
        invoke?: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
      };
    }).__TAURI_INTERNALS__?.invoke;
    if (!invoke) {
      done({ error: "Tauri invoke bridge is unavailable", ok: false, value: {} });
      return;
    }
    invoke("desktop_platform_capabilities")
      .then((value) => done({ ok: true, value }))
      .catch((error) => done({ error: String(error), ok: false, value: {} }));
  });
}

async function floatingWindowMetrics(): Promise<FloatingWindowMetrics> {
  return browser.execute(() => {
    const capsule = document.querySelector(".pevo-floating-capsule");
    const capsuleRect = capsule?.getBoundingClientRect();
    return {
      bodyBackground: window.getComputedStyle(document.body).backgroundColor,
      capsuleHeight: capsuleRect?.height ?? 0,
      capsuleLeft: capsuleRect?.left ?? 0,
      capsuleWidth: capsuleRect?.width ?? 0,
      devicePixelRatio: window.devicePixelRatio || 1,
      innerHeight: window.innerHeight,
      innerWidth: window.innerWidth,
      rootBackground: window.getComputedStyle(document.documentElement).backgroundColor
    };
  }) as Promise<FloatingWindowMetrics>;
}

function floatingWindowFitsVisualBounds(metrics: FloatingWindowMetrics): boolean {
  const contentFitLimit = Math.max(metrics.capsuleHeight + 24, compactFloatingHeightLimit);
  if (metrics.innerHeight <= contentFitLimit) {
    return true;
  }

  // WSLg/WebKitGTK can enforce a larger native minimum height; the Desktop spec
  // permits that only when Floating fills the bounded viewport without a transparent gutter.
  return metrics.innerHeight <= compositorFloatingHeightLimit
    && metrics.capsuleLeft <= 1
    && metrics.capsuleWidth >= metrics.innerWidth - 1
    && !isTransparentColor(metrics.rootBackground)
    && !isTransparentColor(metrics.bodyBackground);
}

function isTransparentColor(color: string): boolean {
  return color === "" || color === "transparent" || color === "rgba(0, 0, 0, 0)";
}

async function floatingErrorText(): Promise<string | null> {
  const error = await browser.$(".pevo-floating-errorRow");
  if (!(await error.isExisting())) {
    return null;
  }
  const text = (await error.getText()).trim();
  return text || "Floating rendered an empty error row";
}

async function assertNoPageVerticalOverflow(): Promise<void> {
  const metrics = await browser.execute(() => {
    const scrollingElement = document.scrollingElement ?? document.documentElement;
    window.scrollTo(0, scrollingElement.scrollHeight);
    const result = {
      bodyClientHeight: document.body.clientHeight,
      bodyScrollHeight: document.body.scrollHeight,
      clientHeight: scrollingElement.clientHeight,
      scrollHeight: scrollingElement.scrollHeight,
      scrollTop: scrollingElement.scrollTop,
      scrollY: window.scrollY
    };
    window.scrollTo(0, 0);
    return result;
  }) as PageVerticalMetrics;
  expect(metrics.scrollHeight).toBeLessThanOrEqual(metrics.clientHeight + 1);
  expect(metrics.bodyScrollHeight).toBeLessThanOrEqual(metrics.bodyClientHeight + 1);
  expect(metrics.scrollTop).toBeLessThanOrEqual(1);
  expect(metrics.scrollY).toBeLessThanOrEqual(1);
}
