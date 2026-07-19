import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { browser, expect } from "@wdio/globals";

const artifactRoot = path.resolve(
  process.env.PSYCHEVO_WDIO_ARTIFACT_ROOT
    ?? path.join(process.cwd(), "../../.local/.psychevo-dev/wdio/desktop-native-smoke")
);
const screenshotRoot = path.join(artifactRoot, "screenshots");
const startupRustTracePath = path.join(artifactRoot, "desktop-startup-rust.jsonl");
const startupManifestPath = path.join(artifactRoot, "desktop-startup-journey.json");
const providerLive = process.env.PSYCHEVO_DESKTOP_PROVIDER_LIVE === "1";
const providerToken = process.env.PSYCHEVO_FLOATING_PROVIDER_TOKEN ?? "PEVO_DESKTOP_FLOATING_PROVIDER_LIVE_OK";
const compactFloatingHeightLimit = 220;
const compositorFloatingHeightLimit = 280;
const desktopStartupIds = [
  "process_start",
  "window_ready",
  "managed_gateway_ready",
  "bridge_connected",
  "gui_ready",
  "draft_context_ready"
] as const;
const rustStartupIds: DesktopStartupId[] = [
  "process_start",
  "window_ready",
  "managed_gateway_ready",
  "bridge_connected"
];

type DesktopStartupId = typeof desktopStartupIds[number];

interface DesktopStartupMark {
  epochMs: number;
  id: DesktopStartupId;
  monotonicOffsetMs: number;
  sequence: number;
  sourceClock: "desktop-rust-monotonic" | "workbench-browser-performance";
}

interface StartupScreenshot {
  captureEndEpochMs: number;
  captureLagMs: number;
  captureStartEpochMs: number;
  path: string;
}

interface DesktopStartupCheckpoint {
  deltaFromPreviousSameClockMs: number | null;
  epochMs: number | null;
  id: DesktopStartupId;
  monotonicOffsetMs: number | null;
  sequence: number | null;
  screenshot: StartupScreenshot | null;
  sourceClock: DesktopStartupMark["sourceClock"] | null;
  status: "complete" | "missing";
}

interface DesktopStartupManifest {
  schemaVersion: 1;
  run: {
    outcome: "failed" | "passed";
    platform: NodeJS.Platform;
    surface: "desktop-native";
  };
  checkpoints: DesktopStartupCheckpoint[];
  failure: string | null;
  sourceArtifacts: {
    rustJsonl: string;
  };
}

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
    await captureDesktopStartupJourney();
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

async function captureDesktopStartupJourney(): Promise<void> {
  const browserMarks: DesktopStartupMark[] = [];
  const screenshots = new Map<DesktopStartupId, StartupScreenshot>();
  try {
    await browser.waitUntil(async () => {
      const shell = await browser.$('.appShell[data-gateway-status="connected"]');
      const composer = await browser.$('textarea[placeholder="Ask Psychevo..."]');
      return (await shell.isDisplayed()) && (await composer.isDisplayed()) && (await composer.isEnabled());
    }, {
      timeoutMsg: "Workbench GUI did not become connected and editable in the native window"
    });
    browserMarks.push(await waitForRetainedBrowserTiming("psychevo:gui_ready", "gui_ready", 1));
    screenshots.set(
      "gui_ready",
      await captureStartupScreenshot("00-workbench-gui-ready.png")
    );

    await browser.waitUntil(async () => (
      await browser.$('.appShell[data-composer-state="ready"]')
    ).isDisplayed(), {
      timeoutMsg: "Workbench draft context did not become ready in the native window"
    });
    browserMarks.push(await waitForRetainedBrowserTiming(
      "psychevo:draft_context_ready",
      "draft_context_ready",
      2
    ));
    await assertNoPageVerticalOverflow();
    screenshots.set(
      "draft_context_ready",
      await captureStartupScreenshot("01-workbench-short-window.png")
    );

    const rustMarks = readRustStartupMarks(true);
    const manifest = createDesktopStartupManifest(rustMarks, browserMarks, screenshots, null);
    validateDesktopStartupManifest(manifest);
    writeDesktopStartupManifest(manifest);
  } catch (error) {
    const failure = boundedFailure(error);
    const retainedBrowserMarks = await readAvailableBrowserStartupMarks();
    const rustMarks = readAvailableRustStartupMarks();
    const manifest = createDesktopStartupManifest(
      rustMarks,
      mergeStartupMarks(browserMarks, retainedBrowserMarks),
      screenshots,
      failure
    );
    writeDesktopStartupManifest(manifest);
    throw error;
  }
}

async function waitForRetainedBrowserTiming(
  name: string,
  id: DesktopStartupId,
  sequence: number
): Promise<DesktopStartupMark> {
  await browser.waitUntil(async () => (
    await readBrowserPerformanceEntries(name)
  ).offsets.length > 0, {
    timeoutMsg: `Workbench did not retain the ${name} browser timing`
  });
  const observation = await readBrowserPerformanceEntries(name);
  if (observation.offsets.length !== 1) {
    throw new Error(`Workbench retained ${observation.offsets.length} ${name} performance marks; expected exactly one`);
  }
  const monotonicOffsetMs = observation.offsets[0]!;
  return {
    epochMs: observation.timeOrigin + monotonicOffsetMs,
    id,
    monotonicOffsetMs,
    sequence,
    sourceClock: "workbench-browser-performance"
  };
}

async function readAvailableBrowserStartupMarks(): Promise<DesktopStartupMark[]> {
  const specs = [
    { id: "gui_ready" as const, name: "psychevo:gui_ready", sequence: 1 },
    { id: "draft_context_ready" as const, name: "psychevo:draft_context_ready", sequence: 2 }
  ];
  const marks: DesktopStartupMark[] = [];
  for (const spec of specs) {
    try {
      const observation = await readBrowserPerformanceEntries(spec.name);
      if (observation.offsets.length !== 1) {
        continue;
      }
      const monotonicOffsetMs = observation.offsets[0]!;
      marks.push({
        epochMs: observation.timeOrigin + monotonicOffsetMs,
        id: spec.id,
        monotonicOffsetMs,
        sequence: spec.sequence,
        sourceClock: "workbench-browser-performance"
      });
    } catch {
      // The Workbench window may no longer be reachable. Preserve the other
      // process evidence and let the manifest show these marks as missing.
    }
  }
  return marks;
}

async function readBrowserPerformanceEntries(name: string): Promise<{
  offsets: number[];
  timeOrigin: number;
}> {
  return browser.execute((requestedName) => {
    const retained = (window as Window & {
      __psychevoJourneyTiming?: Record<string, { monotonicMs?: unknown }>;
    }).__psychevoJourneyTiming?.[requestedName];
    const retainedOffset = typeof retained?.monotonicMs === "number"
      ? retained.monotonicMs
      : null;
    return {
      offsets: retainedOffset === null
        ? performance.getEntriesByName(requestedName, "mark").map((entry) => entry.startTime)
        : [retainedOffset],
      timeOrigin: performance.timeOrigin
    };
  }, name) as Promise<{ offsets: number[]; timeOrigin: number }>;
}

async function captureStartupScreenshot(filename: string): Promise<StartupScreenshot> {
  const captureStartEpochMs = Date.now();
  await browser.saveScreenshot(path.join(screenshotRoot, filename));
  const captureEndEpochMs = Date.now();
  return {
    captureEndEpochMs,
    captureLagMs: captureEndEpochMs - captureStartEpochMs,
    captureStartEpochMs,
    path: path.posix.join("screenshots", filename)
  };
}

function readRustStartupMarks(required: boolean): DesktopStartupMark[] {
  if (!existsSync(startupRustTracePath)) {
    if (required) {
      throw new Error(`Desktop Rust startup trace is missing: ${startupRustTracePath}`);
    }
    return [];
  }
  const raw = readFileSync(startupRustTracePath, "utf8");
  if (/authorization|baseUrl|bearer|token|wsUrl/i.test(raw)) {
    throw new Error("Desktop Rust startup trace contains a forbidden endpoint or credential field");
  }
  const marks: DesktopStartupMark[] = [];
  for (const line of raw.split(/\r?\n/).filter((value) => value.trim())) {
    const parsed = JSON.parse(line) as Partial<DesktopStartupMark> & { schemaVersion?: number };
    if (
      parsed.schemaVersion !== 1
      || !isDesktopStartupId(parsed.id)
      || !rustStartupIds.includes(parsed.id)
    ) {
      throw new Error("Desktop Rust startup trace contains an unsupported record");
    }
    if (
      parsed.sourceClock !== "desktop-rust-monotonic"
      || typeof parsed.epochMs !== "number"
      || typeof parsed.monotonicOffsetMs !== "number"
      || typeof parsed.sequence !== "number"
    ) {
      throw new Error(`Desktop Rust startup trace mark ${parsed.id} is incomplete`);
    }
    marks.push(parsed as DesktopStartupMark);
  }
  const duplicateIds = rustStartupIds.filter((id) => (
    marks.filter((mark) => mark.id === id).length > 1
  ));
  if (duplicateIds.length > 0) {
    throw new Error(`Desktop Rust startup trace contains duplicate marks: ${duplicateIds.join(", ")}`);
  }
  return marks;
}

function readAvailableRustStartupMarks(): DesktopStartupMark[] {
  try {
    return readRustStartupMarks(false);
  } catch {
    return [];
  }
}

function createDesktopStartupManifest(
  rustMarks: DesktopStartupMark[],
  browserMarks: DesktopStartupMark[],
  screenshots: Map<DesktopStartupId, StartupScreenshot>,
  failure: string | null
): DesktopStartupManifest {
  const marks = mergeStartupMarks(rustMarks, browserMarks);
  const checkpoints = desktopStartupIds.map((id, index): DesktopStartupCheckpoint => {
    const mark = marks.find((candidate) => candidate.id === id);
    if (!mark) {
      return {
        deltaFromPreviousSameClockMs: null,
        epochMs: null,
        id,
        monotonicOffsetMs: null,
        screenshot: screenshots.get(id) ?? null,
        sequence: null,
        sourceClock: null,
        status: "missing"
      };
    }
    const previous = index > 0
      ? marks.find((candidate) => candidate.id === desktopStartupIds[index - 1])
      : undefined;
    return {
      ...mark,
      deltaFromPreviousSameClockMs: previous?.sourceClock === mark.sourceClock
        ? mark.monotonicOffsetMs - previous.monotonicOffsetMs
        : null,
      screenshot: screenshots.get(id) ?? null,
      status: "complete"
    };
  });
  return {
    schemaVersion: 1,
    run: {
      outcome: failure ? "failed" : "passed",
      platform: process.platform,
      surface: "desktop-native"
    },
    checkpoints,
    failure,
    sourceArtifacts: {
      rustJsonl: path.basename(startupRustTracePath)
    }
  };
}

function validateDesktopStartupManifest(manifest: DesktopStartupManifest): void {
  const missing = manifest.checkpoints.filter((checkpoint) => checkpoint.status !== "complete");
  if (missing.length > 0) {
    throw new Error(`Desktop startup evidence is missing: ${missing.map((checkpoint) => checkpoint.id).join(", ")}`);
  }
  const duplicateIds = desktopStartupIds.filter((id) => (
    manifest.checkpoints.filter((checkpoint) => checkpoint.id === id).length !== 1
  ));
  if (duplicateIds.length > 0) {
    throw new Error(`Desktop startup evidence contains duplicate checkpoints: ${duplicateIds.join(", ")}`);
  }
  const rustMarks = manifest.checkpoints.filter((checkpoint) => (
    checkpoint.sourceClock === "desktop-rust-monotonic"
  ));
  const browserMarks = manifest.checkpoints.filter((checkpoint) => (
    checkpoint.sourceClock === "workbench-browser-performance"
  ));
  validateClockOrder(rustMarks, rustStartupIds);
  validateClockOrder(browserMarks, ["gui_ready", "draft_context_ready"]);
  for (const id of ["gui_ready", "draft_context_ready"] as const) {
    const screenshot = manifest.checkpoints.find((checkpoint) => checkpoint.id === id)?.screenshot;
    if (!screenshot || !existsSync(path.join(artifactRoot, screenshot.path))) {
      throw new Error(`Desktop startup screenshot is missing for ${id}`);
    }
  }
}

function validateClockOrder(
  marks: DesktopStartupCheckpoint[],
  expectedIds: DesktopStartupId[]
): void {
  if (marks.map((mark) => mark.id).join("|") !== expectedIds.join("|")) {
    throw new Error(`Desktop startup clock order is invalid for ${expectedIds.join(", ")}`);
  }
  for (let index = 1; index < marks.length; index += 1) {
    const previous = marks[index - 1]!;
    const current = marks[index]!;
    if (
      previous.monotonicOffsetMs === null
      || current.monotonicOffsetMs === null
      || previous.sequence === null
      || current.sequence === null
      || current.sequence <= previous.sequence
      || current.monotonicOffsetMs < previous.monotonicOffsetMs
    ) {
      throw new Error(`Desktop startup clock moved backwards at ${current.id}`);
    }
  }
}

function mergeStartupMarks(...groups: DesktopStartupMark[][]): DesktopStartupMark[] {
  const merged = new Map<DesktopStartupId, DesktopStartupMark>();
  for (const mark of groups.flat()) {
    merged.set(mark.id, mark);
  }
  return [...merged.values()];
}

function writeDesktopStartupManifest(manifest: DesktopStartupManifest): void {
  writeFileSync(startupManifestPath, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
}

function isDesktopStartupId(value: unknown): value is DesktopStartupId {
  return typeof value === "string" && desktopStartupIds.includes(value as DesktopStartupId);
}

function boundedFailure(error: unknown): string {
  return (error instanceof Error ? error.message : String(error))
    .replace(/Bearer\s+\S+/gi, "Bearer [redacted]")
    .replace(/([?&](?:token|key|secret)=)[^&\s]+/gi, "$1[redacted]")
    .slice(0, 500);
}

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
      .then((value) => done({
        ok: true,
        value: value as { capture?: unknown; os?: string }
      }))
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
