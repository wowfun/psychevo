import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { expect, test, type BrowserContext, type Page } from "@playwright/test";
import {
  beginBrowserJourneySample,
  installJourneyWebSocketProbe,
  readBrowserJourneyMarks,
  waitForBrowserJourneyMark,
  waitForBrowserJourneyRequestsSettled,
  waitForBrowserJourneyRunnerMark
} from "./journey-websocket-probe";
import type { BrowserJourneyMark } from "./journey-websocket-probe";
import { repoRoot, startPevoWeb } from "./harness";
import {
  startDeterministicNativeModel,
  type DeterministicJourneyControl,
  type DeterministicJourneyEvent,
  type DeterministicJourneyRequestSelector,
  type DeterministicNativeModelFixture
} from "./runtime-live.support";

const enabled = process.env.PSYCHEVO_SURFACE_PROFILE === "1";
const measuredSampleCount = positiveInteger(process.env.PSYCHEVO_SURFACE_PROFILE_SAMPLES, 20);
const trackedDirtyFileCount = nonNegativeInteger(
  process.env.PSYCHEVO_SURFACE_PROFILE_DIRTY_FILES,
  200
);
const artifactRoot = path.resolve(
  process.env.PSYCHEVO_SURFACE_PROFILE_ROOT
    ?? path.join(repoRoot, ".local/playwright/surface-comparison")
);
const pevoBin = path.resolve(
  process.env.PSYCHEVO_PEVO_BIN ?? path.join(repoRoot, "target/debug/pevo")
);
const FIXED_INPUT = "Describe the deterministic surface journey in one sentence.";
const MODEL = "journey-native/default";
const CORE_METRICS = [
  "sendToFeedbackCommitMs",
  "sendToRequestMs",
  "requestToFirstSurfaceCommitMs",
  "firstSurfaceCommitToSettledCommitMs",
  "sendToSettledCommitMs"
] as const;
const GATEWAY_METRICS = [
  "gatewayEntryToThreadMaterializedMs",
  "threadMaterializedToTurnStartedMs",
  "turnStartedToAdapterMs",
  "adapterToUserEntryProjectedMs",
  "userEntryProjectedToFirstAssistantMs",
  "firstAssistantToGatewayCompletedMs"
] as const;
const SURFACE_METRICS = [
  "assistantReceivedToControllerAppliedMs",
  "assistantAppliedToSurfaceCommitMs",
  "completionReceivedToControllerAppliedMs",
  "completionAppliedToSettledCommitMs"
] as const;

type CoreMetric = typeof CORE_METRICS[number];
type GatewayMetric = typeof GATEWAY_METRICS[number];
type SurfaceMetric = typeof SURFACE_METRICS[number];
type SamplePhase = "cold" | "measured" | "trace-diagnostic" | "warmup";

interface SurfaceSample extends Record<Exclude<CoreMetric, "sendToFeedbackCommitMs">, number> {
  clockDomains: {
    runner: "node:hrtime";
    surface: string;
  };
  diagnostics: Array<{
    data: Record<string, unknown>;
    id: string;
    monotonicMs: number;
    sequence: number;
  }>;
  firstEmitToCompletionMs: number;
  gatewayStructure: {
    reviewScans: number;
    turnStarted: number;
  };
  gatewaySpans: Record<GatewayMetric, number>;
  gatewayTurnId: string;
  index: number;
  mainRequestSequence: number;
  phase: SamplePhase;
  providerRequestToFirstEmitMs: number;
  requestIndex: number;
  longTaskCount: number;
  longTaskDurationMs: number;
  postSettleDrainMs: number;
  sendToFeedbackCommitMs: number | null;
  sendToFirstSurfaceCommitMs: number;
  surfaceSpans: Record<SurfaceMetric, number>;
}

interface MetricSummary {
  missingSamples: number;
  observedSamples: number;
  p50: number | null;
  p95: number | null;
}

interface SurfaceProfile {
  cold: SurfaceSample;
  samples: SurfaceSample[];
  startup: Record<string, number | string>;
  gatewaySummary: Record<GatewayMetric, MetricSummary>;
  summary: Record<CoreMetric, MetricSummary>;
  surfaceSummary: Record<SurfaceMetric, MetricSummary>;
  traceDiagnostic: SurfaceSample;
  transport: string;
  warmup: SurfaceSample;
}

interface ComparisonManifest {
  artifacts: Record<string, string>;
  contract: {
    adapter: "native";
    inputBytes: number;
    inputSha256: string;
    measuredSamples: number;
    responseStageDelayMs: 32;
    scenario: "ready-send";
    traceDiagnosticSamples: 1;
    trackedDirtyFiles: number;
    warmupSamples: 1;
  };
  delta: MetricDelta<CoreMetric>;
  gatewayDelta: MetricDelta<GatewayMetric>;
  environment: {
    platform: string;
    playwrightProject: string;
  };
  error?: { message: string; name: string };
  outcome: "failed" | "passed";
  schemaVersion: 2;
  surfaceDelta: MetricDelta<SurfaceMetric>;
  surfaces?: {
    tui: SurfaceProfile;
    workbench: SurfaceProfile;
  };
}

type MetricDelta<Metric extends string> = Record<Metric, {
  p50Ms: number | null;
  p95Ms: number | null;
  ratioP50: number | null;
  ratioP95: number | null;
}>;

interface TuiTraceRecord {
  clockDomainId: string;
  epochUnixMs: number;
  event: string;
  monotonicNs: number;
  sampleIndex: number | null;
  schemaVersion: 1;
  seq: number;
  surface: "tui";
}

interface GatewayTraceRecord {
  clockDomainId: string;
  event: string;
  eventType?: string;
  hasVisibleAssistantText?: boolean;
  monotonicNs: string;
  runtimeSource?: string;
  schemaVersion: 1;
  sequence: number;
  surface: "gateway";
  threadId?: string;
  turnId?: string;
}

test.use({ trace: "off", video: "off" });
test.describe.configure({ mode: "serial" });
test.skip(!enabled, "run through cargo xtask ci run --profile surface-profile");
test.skip(process.platform === "win32", "fullscreen PTY profiling is not supported on Windows");

test("profiles the same Native journey through fullscreen TUI and Workbench", async ({
  context,
  page
}, testInfo) => {
  test.setTimeout(900_000);
  rmSync(artifactRoot, { force: true, recursive: true });
  mkdirSync(artifactRoot, { recursive: true });
  const artifacts = comparisonArtifactPaths(artifactRoot);
  for (const directory of [artifacts.providerDir, artifacts.tuiDir, artifacts.workbenchDir]) {
    mkdirSync(directory, { recursive: true });
  }
  const scratch = mkdtempSync(path.join(tmpdir(), "psychevo-surface-comparison-"));
  const manifestBase: Omit<
    ComparisonManifest,
    "delta" | "gatewayDelta" | "outcome" | "surfaceDelta"
  > = {
    artifacts: {
      providerEvents: relativeArtifact(artifactRoot, artifacts.providerEvents),
      report: relativeArtifact(artifactRoot, artifacts.report),
      tuiGatewayTrace: relativeArtifact(artifactRoot, artifacts.tuiGatewayTrace),
      tuiTrace: relativeArtifact(artifactRoot, artifacts.tuiTrace),
      workbenchBrowserMarks: relativeArtifact(artifactRoot, artifacts.workbenchBrowserMarks),
      workbenchGatewayTrace: relativeArtifact(artifactRoot, artifacts.workbenchGatewayTrace),
      workbenchTrace: relativeArtifact(artifactRoot, artifacts.workbenchTrace)
    },
    contract: {
      adapter: "native",
      inputBytes: Buffer.byteLength(FIXED_INPUT),
      inputSha256: createHash("sha256").update(FIXED_INPUT).digest("hex"),
      measuredSamples: measuredSampleCount,
      responseStageDelayMs: 32,
      scenario: "ready-send",
      traceDiagnosticSamples: 1,
      trackedDirtyFiles: trackedDirtyFileCount,
      warmupSamples: 1
    },
    environment: {
      platform: process.platform,
      playwrightProject: testInfo.project.name
    },
    schemaVersion: 2
  };
  let fixture: DeterministicNativeModelFixture | null = null;
  let tui: TuiPtyDriver | null = null;
  let server: Awaited<ReturnType<typeof startPevoWeb>> | null = null;
  let tracing = false;
  try {
    expect(existsSync(pevoBin), `pevo binary is missing: ${pevoBin}`).toBe(true);
    fixture = await startDeterministicNativeModel({ journeyMode: "profile" });
    if (!fixture.journey) throw new Error("deterministic Native profile control is unavailable");
    const workspace = path.join(scratch, "workspace");
    prepareSyntheticGitWorkspace(workspace, trackedDirtyFileCount);

    const tuiRuntime = prepareTuiRuntime({
      fixture,
      gatewayTrace: artifacts.tuiGatewayTrace,
      scratch,
      trace: artifacts.tuiTrace,
      workspace
    });
    tui = await TuiPtyDriver.start(tuiRuntime);
    const tuiInputReady = await waitForTuiTrace(artifacts.tuiTrace, (record) => (
      record.event === "input_ready"
    ));
    const tuiProcessStarted = await waitForTuiTrace(artifacts.tuiTrace, (record) => (
      record.event === "process_started"
    ));
    const tuiProfile = await runTuiProfile({
      control: fixture.journey,
      driver: tui,
      gatewayTracePath: artifacts.tuiGatewayTrace,
      inputReady: tuiInputReady.record,
      processStarted: tuiProcessStarted.record,
      tracePath: artifacts.tuiTrace
    });
    await tui.stop();
    tui = null;

    server = await startPevoWeb({
      configAppend: nativeProviderConfig(fixture.baseUrl),
      cwd: workspace,
      live: false,
      model: MODEL,
      pevoBin,
      processEnv: {
        PSYCHEVO_GATEWAY_PROFILE_PATH: artifacts.workbenchGatewayTrace
      }
    });
    await installJourneyWebSocketProbe(page);
    await page.goto(server.url, { waitUntil: "domcontentloaded" });
    await waitForWorkbenchGuiReady(page);
    const navigationMark = await waitForBrowserJourneyMark(
      page,
      "navigation_started",
      60_000,
      0
    );
    const composerShellMark = await waitForBrowserJourneyMark(
      page,
      "composer_shell_dom_committed",
      60_000,
      0
    );
    const inputReadyMark = await waitForBrowserJourneyMark(
      page,
      "gui_ready_dom_committed",
      60_000,
      0
    );
    await waitForWorkbenchDraftReady(page);
    const draftReadyMark = await waitForBrowserJourneyMark(
      page,
      "draft_context_ready_dom_committed",
      60_000,
      0
    );
    const workbenchProfile = await runWorkbenchProfile({
      context,
      control: fixture.journey,
      gatewayTracePath: artifacts.workbenchGatewayTrace,
      page,
      tracePath: artifacts.workbenchTrace,
      traceState: {
        get active() { return tracing; },
        set active(value: boolean) { tracing = value; }
      }
    });
    const browserMarks = await readBrowserJourneyMarks(page);
    const startupDurations = browserStartupDurations(browserMarks, navigationMark);
    writeJsonLines(artifacts.workbenchBrowserMarks, browserMarks.map((mark) => ({
      clock: mark.clock,
      data: mark.data,
      epochMs: mark.epochMs,
      event: mark.id,
      monotonicMs: mark.monotonicMs,
      sampleIndex: mark.sampleIndex,
      sequence: mark.sequence
    })));

    const providerEvents = contentFreeProviderEvents(fixture.journey.events());
    writeJsonLines(artifacts.providerEvents, providerEvents);
    const delta = compareSummaries(tuiProfile.summary, workbenchProfile.summary);
    const manifest: ComparisonManifest = {
      ...manifestBase,
      delta,
      gatewayDelta: compareMetricSummaries(
        tuiProfile.gatewaySummary,
        workbenchProfile.gatewaySummary,
        GATEWAY_METRICS
      ),
      outcome: "passed",
      surfaceDelta: compareMetricSummaries(
        tuiProfile.surfaceSummary,
        workbenchProfile.surfaceSummary,
        SURFACE_METRICS
      ),
      surfaces: {
        tui: tuiProfile,
        workbench: {
          ...workbenchProfile,
          startup: {
            ...workbenchProfile.startup,
            clockDomainId: "browser:performance",
            navigationToComposerShellCommitMs: composerShellMark.monotonicMs - navigationMark.monotonicMs,
            navigationToGuiReadyMs: inputReadyMark.monotonicMs - navigationMark.monotonicMs,
            navigationToDraftReadyMs: draftReadyMark.monotonicMs - navigationMark.monotonicMs,
            ...startupDurations
          }
        }
      }
    };
    writeFileSync(artifacts.report, renderReport(manifest));
    validateComparison(manifest, artifactRoot);
    writeFileSync(artifacts.manifest, `${JSON.stringify(manifest, null, 2)}\n`);
  } catch (error) {
    if (tracing) {
      await context.tracing.stop({ path: artifacts.workbenchTrace }).catch(() => undefined);
      tracing = false;
    }
    if (fixture?.journey) {
      writeJsonLines(
        artifacts.providerEvents,
        contentFreeProviderEvents(fixture.journey.events())
      );
    }
    const partialBrowserMarks = await readBrowserJourneyMarks(page).catch(() => []);
    if (partialBrowserMarks.length > 0) {
      writeJsonLines(artifacts.workbenchBrowserMarks, partialBrowserMarks.map((mark) => ({
        clock: mark.clock,
        data: mark.data,
        epochMs: mark.epochMs,
        event: mark.id,
        monotonicMs: mark.monotonicMs,
        sampleIndex: mark.sampleIndex,
        sequence: mark.sequence
      })));
    }
    const safeError = sanitizeError(error, fixture?.expectedAnswer);
    writeFileSync(
      artifacts.report,
      `# TUI vs Workbench surface profile\n\nFailed: ${safeError.name}: ${safeError.message}\n`
    );
    writeFileSync(artifacts.manifest, `${JSON.stringify({
      ...manifestBase,
      delta: emptyDelta(),
      gatewayDelta: emptyMetricDelta(GATEWAY_METRICS),
      error: safeError,
      outcome: "failed",
      surfaceDelta: emptyMetricDelta(SURFACE_METRICS)
    } satisfies ComparisonManifest, null, 2)}\n`);
    throw error;
  } finally {
    await tui?.stop().catch(() => undefined);
    await server?.stop().catch(() => undefined);
    await fixture?.stop().catch(() => undefined);
    rmSync(scratch, { force: true, recursive: true });
  }
});

async function runTuiProfile(options: {
  control: DeterministicJourneyControl;
  driver: TuiPtyDriver;
  gatewayTracePath: string;
  inputReady: TuiTraceRecord;
  processStarted: TuiTraceRecord;
  tracePath: string;
}): Promise<SurfaceProfile> {
  const samples: SurfaceSample[] = [];
  let cold: SurfaceSample | null = null;
  let warmup: SurfaceSample | null = null;
  let traceDiagnostic: SurfaceSample | null = null;
  const firstMainSequence = nextPurposeSequence(options.control, "main_turn");
  const firstTitleSequence = nextPurposeSequence(options.control, "async_title");
  const total = measuredSampleCount + 3;
  for (let index = 0; index < total; index += 1) {
    const phase = phaseForIndex(index);
    const sample = await runTuiSample({
      ...options,
      index,
      mainRequestSequence: firstMainSequence + index,
      phase
    });
    if (phase === "cold") cold = sample;
    else if (phase === "warmup") warmup = sample;
    else if (phase === "trace-diagnostic") traceDiagnostic = sample;
    else samples.push(sample);
    if (phase === "cold") {
      await drainFirstTitleRequest(options.control, firstTitleSequence);
    }
  }
  if (!cold || !warmup || !traceDiagnostic) throw new Error("TUI profile phases are incomplete");
  return {
    cold,
    gatewaySummary: summarizeSampleSpans(samples, "gatewaySpans", GATEWAY_METRICS),
    samples,
    startup: {
      clockDomainId: options.inputReady.clockDomainId,
      processToInputReadyMs: tuiDuration(options.processStarted, options.inputReady)
    },
    summary: summarizeSamples(samples),
    surfaceSummary: summarizeSampleSpans(samples, "surfaceSpans", SURFACE_METRICS),
    traceDiagnostic,
    transport: "in-process-gateway",
    warmup
  };
}

async function runTuiSample(options: {
  control: DeterministicJourneyControl;
  driver: TuiPtyDriver;
  gatewayTracePath: string;
  index: number;
  mainRequestSequence: number;
  phase: SamplePhase;
  tracePath: string;
}): Promise<SurfaceSample> {
  assertMainRequestCount(options.control, options.mainRequestSequence - 1);
  const written = await options.driver.type(FIXED_INPUT);
  const send = await waitForTuiTrace(options.tracePath, (record) => (
    record.event === "send_committed" && record.sampleIndex === options.index
  ));
  const feedback = await waitForTuiTrace(options.tracePath, (record) => (
    record.event === "send_feedback_surface_committed" && record.sampleIndex === options.index
  ));
  const selector = mainTurn(options.mainRequestSequence);
  const request = await options.control.waitFor("request_received", selector, 60_000);
  const firstEmit = await options.control.waitFor("first_output_emitted", selector, 60_000);
  const firstVisible = await waitForTuiTrace(options.tracePath, (record) => (
    record.event === "first_output_surface_committed" && record.sampleIndex === options.index
  ));
  const completion = await options.control.waitFor("completion_emitted", selector, 60_000);
  const settled = await waitForTuiTrace(options.tracePath, (record) => (
    record.event === "turn_settled_surface_committed" && record.sampleIndex === options.index
  ));
  await assertMainRequestCountSettled(options.control, options.mainRequestSequence);
  const diagnostics = tuiSampleDiagnostics(options.tracePath, options.index);
  const gateway = gatewayTurnBreakdown(options.gatewayTracePath, options.index);
  return {
    clockDomains: {
      runner: "node:hrtime",
      surface: send.record.clockDomainId
    },
    diagnostics,
    firstEmitToCompletionMs: providerDuration(firstEmit, completion),
    firstSurfaceCommitToSettledCommitMs: tuiDuration(firstVisible.record, settled.record),
    gatewayStructure: gateway.structure,
    gatewaySpans: gateway.spans,
    gatewayTurnId: gateway.turnId,
    index: options.index,
    longTaskCount: 0,
    longTaskDurationMs: 0,
    mainRequestSequence: options.mainRequestSequence,
    phase: options.phase,
    postSettleDrainMs: 0,
    providerRequestToFirstEmitMs: providerDuration(request, firstEmit),
    requestIndex: request.requestIndex,
    requestToFirstSurfaceCommitMs: firstVisible.observedRunnerMs - providerMonotonicMs(request),
    sendToFeedbackCommitMs: tuiDuration(send.record, feedback.record),
    sendToFirstSurfaceCommitMs: tuiDuration(send.record, firstVisible.record),
    sendToRequestMs: providerMonotonicMs(request) - written.sentRunnerMonotonicMs,
    sendToSettledCommitMs: tuiDuration(send.record, settled.record),
    surfaceSpans: tuiSurfaceBreakdown(diagnostics)
  };
}

async function runWorkbenchProfile(options: {
  context: BrowserContext;
  control: DeterministicJourneyControl;
  gatewayTracePath: string;
  page: Page;
  tracePath: string;
  traceState: { active: boolean };
}): Promise<SurfaceProfile> {
  const samples: SurfaceSample[] = [];
  let cold: SurfaceSample | null = null;
  let warmup: SurfaceSample | null = null;
  let traceDiagnostic: SurfaceSample | null = null;
  const firstMainSequence = nextPurposeSequence(options.control, "main_turn");
  const firstTitleSequence = nextPurposeSequence(options.control, "async_title");
  const total = measuredSampleCount + 3;
  for (let index = 0; index < total; index += 1) {
    const phase = phaseForIndex(index);
    const sample = await runWorkbenchSample({
      ...options,
      index,
      mainRequestSequence: firstMainSequence + index,
      phase,
      traced: phase === "trace-diagnostic"
    });
    if (phase === "cold") cold = sample;
    else if (phase === "warmup") warmup = sample;
    else if (phase === "trace-diagnostic") traceDiagnostic = sample;
    else samples.push(sample);
    if (phase === "cold") {
      await drainFirstTitleRequest(options.control, firstTitleSequence);
    }
  }
  if (!cold || !warmup || !traceDiagnostic) {
    throw new Error("Workbench profile phases are incomplete");
  }
  return {
    cold,
    gatewaySummary: summarizeSampleSpans(samples, "gatewaySpans", GATEWAY_METRICS),
    samples,
    startup: {},
    summary: summarizeSamples(samples),
    surfaceSummary: summarizeSampleSpans(samples, "surfaceSpans", SURFACE_METRICS),
    traceDiagnostic,
    transport: "managed-gateway-websocket",
    warmup
  };
}

async function runWorkbenchSample(options: {
  context: BrowserContext;
  control: DeterministicJourneyControl;
  gatewayTracePath: string;
  index: number;
  mainRequestSequence: number;
  page: Page;
  phase: SamplePhase;
  tracePath: string;
  traceState: { active: boolean };
  traced: boolean;
}): Promise<SurfaceSample> {
  assertMainRequestCount(options.control, options.mainRequestSequence - 1);
  await beginBrowserJourneySample(options.page, options.index);
  await options.page.getByPlaceholder("Ask Psychevo...").fill(FIXED_INPUT);
  if (options.traced) {
    await options.context.tracing.start({ screenshots: false, snapshots: false, sources: false });
    options.traceState.active = true;
  }
  await options.page.getByRole("button", { name: "Send message" }).click();
  const sendRunner = await waitForBrowserJourneyRunnerMark(
    options.page,
    "send_clicked",
    options.index
  );
  const send = await waitForBrowserJourneyMark(
    options.page,
    "send_clicked",
    60_000,
    options.index
  );
  const selector = mainTurn(options.mainRequestSequence);
  const request = await options.control.waitFor("request_received", selector, 60_000);
  await options.control.waitFor("first_output_emitted", selector, 60_000);
  const firstVisibleRunner = await waitForBrowserJourneyRunnerMark(
    options.page,
    "first_output_surface_committed",
    options.index,
    60_000
  );
  const firstVisible = await waitForBrowserJourneyMark(
    options.page,
    "first_output_surface_committed",
    60_000,
    options.index
  );
  const firstEmit = await options.control.waitFor("first_output_emitted", selector, 60_000);
  const completion = await options.control.waitFor("completion_emitted", selector, 60_000);
  const settledRunner = await waitForBrowserJourneyRunnerMark(
    options.page,
    "turn_settled_surface_committed",
    options.index,
    60_000
  );
  const settled = await waitForBrowserJourneyMark(
    options.page,
    "turn_settled_surface_committed",
    60_000,
    options.index
  );
  await expect(options.page.locator('.appShell[data-turn-state="idle"]')).toBeVisible();
  await expect(options.page.getByPlaceholder("Ask Psychevo...")).toBeEditable();
  if (options.traced) {
    await options.context.tracing.stop({ path: options.tracePath });
    options.traceState.active = false;
  }
  await assertMainRequestCountSettled(options.control, options.mainRequestSequence);
  await waitForBrowserJourneyRequestsSettled(options.page, options.index, 60_000);
  const postSettleDrainMs = monotonicNow() - settledRunner.runnerMonotonicMs;
  const browserMarks = (await readBrowserJourneyMarks(options.page))
    .filter((mark) => mark.sampleIndex === options.index)
    .map((mark) => ({
      data: mark.data,
      id: mark.id,
      monotonicMs: mark.monotonicMs,
      sequence: mark.sequence
    }));
  const feedback = browserMarks.find((mark) => mark.id === "send_feedback_surface_committed");
  const longTasks = browserMarks.filter((mark) => mark.id === "browser_long_task");
  const gateway = gatewayTurnBreakdown(options.gatewayTracePath, options.index);
  return {
    clockDomains: {
      runner: "node:hrtime",
      surface: "browser:performance"
    },
    diagnostics: browserMarks,
    firstEmitToCompletionMs: providerDuration(firstEmit, completion),
    firstSurfaceCommitToSettledCommitMs: settled.monotonicMs - firstVisible.monotonicMs,
    gatewayStructure: gateway.structure,
    gatewaySpans: gateway.spans,
    gatewayTurnId: gateway.turnId,
    index: options.index,
    longTaskCount: longTasks.length,
    longTaskDurationMs: longTasks.reduce((total, mark) => (
      total + (typeof mark.data.durationMs === "number" ? mark.data.durationMs : 0)
    ), 0),
    mainRequestSequence: options.mainRequestSequence,
    phase: options.phase,
    postSettleDrainMs,
    providerRequestToFirstEmitMs: providerDuration(request, firstEmit),
    requestIndex: request.requestIndex,
    requestToFirstSurfaceCommitMs: firstVisibleRunner.runnerMonotonicMs - providerMonotonicMs(request),
    sendToFeedbackCommitMs: feedback
      ? feedback.monotonicMs - send.monotonicMs
      : null,
    sendToFirstSurfaceCommitMs: firstVisible.monotonicMs - send.monotonicMs,
    sendToRequestMs: providerMonotonicMs(request) - sendRunner.runnerMonotonicMs,
    sendToSettledCommitMs: settled.monotonicMs - send.monotonicMs,
    surfaceSpans: workbenchSurfaceBreakdown(browserMarks)
  };
}

function prepareTuiRuntime(options: {
  fixture: DeterministicNativeModelFixture;
  gatewayTrace: string;
  scratch: string;
  trace: string;
  workspace: string;
}): TuiRuntimeOptions {
  const root = path.join(options.scratch, "tui");
  const home = path.join(root, "psychevo-home");
  const osHome = path.join(root, "os-home");
  const config = path.join(root, "config.toml");
  const db = path.join(root, "state.db");
  mkdirSync(home, { recursive: true });
  mkdirSync(osHome, { recursive: true });
  const baseEnv: NodeJS.ProcessEnv = {
    ...process.env,
    HOME: osHome,
    NO_COLOR: "1",
    PSYCHEVO_HOME: home,
    TERM: "xterm-256color"
  };
  const initialized = spawnSync(pevoBin, ["init"], {
    cwd: repoRoot,
    encoding: "utf8",
    env: baseEnv,
    timeout: 60_000
  });
  if (initialized.status !== 0) {
    throw new Error(`pevo init failed for surface profile: ${initialized.stderr.trim()}`);
  }
  writeFileSync(config, `model = ${JSON.stringify(MODEL)}\n${nativeProviderConfig(options.fixture.baseUrl)}\n`);
  writeFileSync(path.join(home, "config.toml"), readFileSync(config));
  return {
    cwd: options.workspace,
    env: {
      ...baseEnv,
      PSYCHEVO_CONFIG: config,
      PSYCHEVO_DB: db,
      PSYCHEVO_GATEWAY_PROFILE_PATH: options.gatewayTrace,
      PSYCHEVO_TUI_PROFILE_PATH: options.trace
    },
    model: MODEL,
    pevo: pevoBin
  };
}

function nativeProviderConfig(baseUrl: string): string {
  return [
    "[provider.journey-native]",
    `api = ${JSON.stringify(baseUrl)}`,
    "no_auth = true",
    "",
    "[provider.journey-native.models.default]",
    ""
  ].join("\n");
}

async function waitForWorkbenchGuiReady(page: Page): Promise<void> {
  await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible({ timeout: 60_000 });
  await expect(page.locator('.appShell[data-gateway-status="connected"]')).toBeVisible();
  await expect(page.getByPlaceholder("Ask Psychevo...")).toBeEditable();
}

async function waitForWorkbenchDraftReady(page: Page): Promise<void> {
  await expect(page.locator('.appShell[data-composer-state="ready"]')).toBeVisible({ timeout: 60_000 });
}

function phaseForIndex(index: number): SamplePhase {
  if (index === 0) return "cold";
  if (index === 1) return "warmup";
  if (index === 2) return "trace-diagnostic";
  return "measured";
}

function mainTurn(sequence: number): DeterministicJourneyRequestSelector {
  return { purpose: "main_turn", sequence };
}

function nextPurposeSequence(
  control: DeterministicJourneyControl,
  purpose: "async_title" | "main_turn"
): number {
  return Math.max(0, ...control.events()
    .filter((event) => event.event === "request_received" && event.purpose === purpose)
    .map((event) => event.purposeSequence)) + 1;
}

async function drainFirstTitleRequest(
  control: DeterministicJourneyControl,
  sequence: number
): Promise<void> {
  const selector = { purpose: "async_title" as const, sequence };
  await control.waitFor("request_received", selector, 60_000);
  await control.waitFor("completion_emitted", selector, 60_000);
}

function mainRequestCount(control: DeterministicJourneyControl): number {
  return control.events().filter((event) => (
    event.event === "request_received" && event.purpose === "main_turn"
  )).length;
}

function assertMainRequestCount(
  control: DeterministicJourneyControl,
  expected: number
): void {
  expect(
    mainRequestCount(control),
    `main provider request count must be ${expected} before the next sample`
  ).toBe(expected);
}

async function assertMainRequestCountSettled(
  control: DeterministicJourneyControl,
  expected: number
): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 25));
  assertMainRequestCount(control, expected);
}

function providerMonotonicMs(event: DeterministicJourneyEvent): number {
  return Number(BigInt(event.monotonicNs)) / 1_000_000;
}

function providerDuration(
  start: DeterministicJourneyEvent,
  end: DeterministicJourneyEvent
): number {
  return providerMonotonicMs(end) - providerMonotonicMs(start);
}

function tuiDuration(start: TuiTraceRecord, end: TuiTraceRecord): number {
  if (start.clockDomainId !== end.clockDomainId) {
    throw new Error("TUI duration endpoints use different clock domains");
  }
  return (end.monotonicNs - start.monotonicNs) / 1_000_000;
}

function tuiSampleDiagnostics(tracePath: string, sampleIndex: number): SurfaceSample["diagnostics"] {
  return readTuiTrace(tracePath)
    .filter((record) => record.sampleIndex === sampleIndex)
    .map((record) => ({
      data: {},
      id: record.event,
      monotonicMs: record.monotonicNs / 1_000_000,
      sequence: record.seq
    }));
}

function gatewayTurnBreakdown(
  tracePath: string,
  turnIndex: number
): {
  structure: SurfaceSample["gatewayStructure"];
  spans: Record<GatewayMetric, number>;
  turnId: string;
} {
  const records = readGatewayTrace(tracePath);
  const entered = records.filter((record) => record.event === "gateway_run_turn_entered")[turnIndex];
  if (!entered?.turnId) {
    throw new Error(`missing Gateway turn ${turnIndex} in ${tracePath}`);
  }
  const turn = records.filter((record) => record.turnId === entered.turnId);
  const materialized = requireGatewayMark(turn, "gateway_thread_materialized");
  const adapter = requireGatewayMark(turn, "native_adapter_submitted");
  const turnStarted = turn.filter((record) => (
    record.event === "gateway_event_emitted" && record.eventType === "turnStarted"
  ));
  if (turnStarted.length !== 1) {
    throw new Error(
      `Gateway turn ${entered.turnId} must emit exactly one public turnStarted event`
    );
  }
  const reviewScans = turn.filter((record) => (
    record.event === "workspace_review_capture_started"
    || record.event === "workspace_review_capture_finished"
  ));
  if (reviewScans.length !== 0) {
    throw new Error(`Gateway turn ${entered.turnId} performed a synchronous workspace review scan`);
  }
  const promptProjected = turn.find((record) => (
    record.event === "gateway_event_emitted"
    && record.eventType === "entryCompleted"
    && record.hasVisibleAssistantText !== true
  ));
  const firstAssistant = turn.find((record) => (
    record.event === "gateway_event_emitted" && record.hasVisibleAssistantText === true
  ));
  const completed = requireGatewayMark(turn, "gateway_turn_completed");
  if (!promptProjected || !firstAssistant) {
    throw new Error(`Gateway turn ${entered.turnId} has an incomplete prompt/output projection`);
  }
  assertOneClockDomain([
    entered,
    materialized,
    turnStarted[0]!,
    adapter,
    promptProjected,
    firstAssistant,
    completed
  ]);
  return {
    structure: {
      reviewScans: reviewScans.length,
      turnStarted: turnStarted.length
    },
    spans: {
      adapterToUserEntryProjectedMs: gatewayDuration(adapter, promptProjected),
      firstAssistantToGatewayCompletedMs: gatewayDuration(firstAssistant, completed),
      gatewayEntryToThreadMaterializedMs: gatewayDuration(entered, materialized),
      threadMaterializedToTurnStartedMs: gatewayDuration(materialized, turnStarted[0]!),
      turnStartedToAdapterMs: gatewayDuration(turnStarted[0]!, adapter),
      userEntryProjectedToFirstAssistantMs: gatewayDuration(promptProjected, firstAssistant),
    },
    turnId: entered.turnId
  };
}

function readGatewayTrace(tracePath: string): GatewayTraceRecord[] {
  if (!existsSync(tracePath)) return [];
  return readFileSync(tracePath, "utf8").split("\n").flatMap((line) => {
    if (!line.trim()) return [];
    try {
      const value = JSON.parse(line) as Partial<GatewayTraceRecord>;
      return value.schemaVersion === 1
        && value.surface === "gateway"
        && typeof value.clockDomainId === "string"
        && typeof value.event === "string"
        && typeof value.monotonicNs === "string"
        && typeof value.sequence === "number"
        ? [value as GatewayTraceRecord]
        : [];
    } catch {
      return [];
    }
  });
}

function requireGatewayMark(
  records: GatewayTraceRecord[],
  event: string
): GatewayTraceRecord {
  const record = records.find((candidate) => candidate.event === event);
  if (!record) throw new Error(`Gateway turn is missing ${event}`);
  return record;
}

function assertOneClockDomain(records: GatewayTraceRecord[]): void {
  const domains = new Set(records.map((record) => record.clockDomainId));
  if (domains.size !== 1) throw new Error("Gateway span mixes clock domains");
}

function gatewayDuration(start: GatewayTraceRecord, end: GatewayTraceRecord): number {
  if (start.clockDomainId !== end.clockDomainId) {
    throw new Error("Gateway duration endpoints use different clock domains");
  }
  const duration = Number(BigInt(end.monotonicNs) - BigInt(start.monotonicNs)) / 1_000_000;
  if (duration < 0) throw new Error(`Gateway duration ${start.event} -> ${end.event} is negative`);
  return duration;
}

function browserStartupDurations(
  marks: BrowserJourneyMark[],
  navigation: BrowserJourneyMark
): Record<string, number> {
  const durationForRpc = (method: string): number | null => {
    const request = marks.find((mark) => (
      mark.id === "rpc_request_sent"
      && mark.data.method === method
    ));
    if (!request || typeof request.data.requestId !== "string") return null;
    const response = marks.find((mark) => (
      mark.id === "rpc_response_arrived"
      && mark.data.method === method
      && mark.data.requestId === request.data.requestId
    ));
    return response ? response.monotonicMs - request.monotonicMs : null;
  };
  const values: Record<string, number> = {};
  for (const [name, id] of [
    ["navigationToComposerShell", "composer_shell_dom_committed"],
    ["navigationToGuiReady", "gui_ready_dom_committed"],
    ["navigationToFirstToken", "gateway_first_nonempty_assistant_received"]
  ] as const) {
    const mark = marks.find((candidate) => candidate.id === id);
    if (mark) values[name] = mark.monotonicMs - navigation.monotonicMs;
  }
  const draftOpenDuration = durationForRpc("thread/draft/open");
  if (draftOpenDuration !== null) values.draftOpenDuration = draftOpenDuration;
  const branchReadDuration = durationForRpc("workspace/git/branches");
  if (branchReadDuration !== null) values.branchReadDuration = branchReadDuration;
  return values;
}

function tuiSurfaceBreakdown(
  diagnostics: SurfaceSample["diagnostics"]
): Record<SurfaceMetric, number> {
  const received = requireDiagnostic(diagnostics, "gateway_first_assistant_event_received");
  const applied = requireDiagnostic(diagnostics, "gateway_first_assistant_event_applied");
  const visible = requireDiagnostic(diagnostics, "first_output_surface_committed");
  const completionReceived = requireDiagnostic(diagnostics, "turn_completed_received");
  const completionApplied = requireDiagnostic(diagnostics, "turn_completed_applied");
  const settled = requireDiagnostic(diagnostics, "turn_settled_surface_committed");
  return surfaceDurations(received, applied, visible, completionReceived, completionApplied, settled);
}

function workbenchSurfaceBreakdown(
  diagnostics: SurfaceSample["diagnostics"]
): Record<SurfaceMetric, number> {
  const received = requireDiagnostic(diagnostics, "gateway_first_nonempty_assistant_received");
  const applied = requireDiagnostic(diagnostics, "controller_first_nonempty_assistant_applied");
  const visible = requireDiagnostic(diagnostics, "first_output_surface_committed");
  const completionReceived = requireDiagnostic(diagnostics, "turn_completed_received");
  const completionApplied = requireDiagnostic(diagnostics, "turn_completed_applied");
  const settled = requireDiagnostic(diagnostics, "turn_settled_surface_committed");
  return surfaceDurations(received, applied, visible, completionReceived, completionApplied, settled);
}

function requireDiagnostic(
  diagnostics: SurfaceSample["diagnostics"],
  id: string,
  eventType?: string
): SurfaceSample["diagnostics"][number] {
  const mark = diagnostics.find((candidate) => (
    candidate.id === id
    && (eventType === undefined || candidate.data.eventType === eventType)
  ));
  if (!mark) throw new Error(`surface sample is missing ${id}${eventType ? `:${eventType}` : ""}`);
  return mark;
}

function surfaceDurations(
  received: SurfaceSample["diagnostics"][number],
  applied: SurfaceSample["diagnostics"][number],
  visible: SurfaceSample["diagnostics"][number],
  completionReceived: SurfaceSample["diagnostics"][number],
  completionApplied: SurfaceSample["diagnostics"][number],
  settled: SurfaceSample["diagnostics"][number]
): Record<SurfaceMetric, number> {
  const duration = (
    start: SurfaceSample["diagnostics"][number],
    end: SurfaceSample["diagnostics"][number]
  ) => {
    const value = end.monotonicMs - start.monotonicMs;
    if (value < 0) throw new Error(`surface duration ${start.id} -> ${end.id} is negative`);
    return value;
  };
  return {
    assistantAppliedToSurfaceCommitMs: duration(applied, visible),
    assistantReceivedToControllerAppliedMs: duration(received, applied),
    completionAppliedToSettledCommitMs: duration(completionApplied, settled),
    completionReceivedToControllerAppliedMs: duration(completionReceived, completionApplied)
  };
}

function readTuiTrace(tracePath: string): TuiTraceRecord[] {
  if (!existsSync(tracePath)) return [];
  return readFileSync(tracePath, "utf8").split("\n").flatMap((line) => {
    if (!line.trim()) return [];
    try {
      const value = JSON.parse(line) as Partial<TuiTraceRecord>;
      return value.schemaVersion === 1
        && value.surface === "tui"
        && typeof value.clockDomainId === "string"
        && typeof value.event === "string"
        && typeof value.monotonicNs === "number"
        && typeof value.seq === "number"
        ? [value as TuiTraceRecord]
        : [];
    } catch {
      return [];
    }
  });
}

async function waitForTuiTrace(
  tracePath: string,
  predicate: (record: TuiTraceRecord) => boolean,
  timeoutMs = 60_000
): Promise<{ observedRunnerMs: number; record: TuiTraceRecord }> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const record = readTuiTrace(tracePath).find(predicate);
    if (record) return { observedRunnerMs: monotonicNow(), record };
    await new Promise((resolve) => setTimeout(resolve, 2));
  }
  throw new Error(`timed out waiting for content-free TUI profile event in ${tracePath}`);
}

function summarizeSamples(samples: SurfaceSample[]): Record<CoreMetric, MetricSummary> {
  if (samples.length === 0) throw new Error("cannot summarize an empty surface sample set");
  return summarizeMetricValues(samples, CORE_METRICS, (sample, metric) => sample[metric]);
}

function summarizeSampleSpans<Metric extends GatewayMetric | SurfaceMetric>(
  samples: SurfaceSample[],
  field: "gatewaySpans" | "surfaceSpans",
  metrics: readonly Metric[]
): Record<Metric, MetricSummary> {
  if (samples.length === 0) throw new Error("cannot summarize an empty span sample set");
  return summarizeMetricValues(
    samples,
    metrics,
    (sample, metric) => (sample[field] as Record<Metric, number>)[metric]
  );
}

function summarizeMetricValues<Item, Metric extends string>(
  items: Item[],
  metrics: readonly Metric[],
  value: (item: Item, metric: Metric) => number | null
): Record<Metric, MetricSummary> {
  return Object.fromEntries(metrics.map((metric) => {
    const observed = items.map((item) => value(item, metric)).filter((candidate): candidate is number => (
      typeof candidate === "number" && Number.isFinite(candidate)
    ));
    return [metric, {
      missingSamples: items.length - observed.length,
      observedSamples: observed.length,
      p50: observed.length > 0 ? percentile(observed, 0.5) : null,
      p95: observed.length > 0 ? percentile(observed, 0.95) : null
    }];
  })) as Record<Metric, MetricSummary>;
}

function compareSummaries(
  tui: Record<CoreMetric, MetricSummary>,
  workbench: Record<CoreMetric, MetricSummary>
): ComparisonManifest["delta"] {
  return compareMetricSummaries(tui, workbench, CORE_METRICS);
}

function compareMetricSummaries<Metric extends string>(
  tui: Record<Metric, MetricSummary>,
  workbench: Record<Metric, MetricSummary>,
  metrics: readonly Metric[]
): MetricDelta<Metric> {
  return Object.fromEntries(metrics.map((metric) => [metric, {
    p50Ms: subtractOptional(workbench[metric].p50, tui[metric].p50),
    p95Ms: subtractOptional(workbench[metric].p95, tui[metric].p95),
    ratioP50: safeRatio(workbench[metric].p50, tui[metric].p50),
    ratioP95: safeRatio(workbench[metric].p95, tui[metric].p95)
  }])) as MetricDelta<Metric>;
}

function emptyDelta(): ComparisonManifest["delta"] {
  return emptyMetricDelta(CORE_METRICS);
}

function emptyMetricDelta<Metric extends string>(metrics: readonly Metric[]): MetricDelta<Metric> {
  return Object.fromEntries(metrics.map((metric) => [metric, {
    p50Ms: 0,
    p95Ms: 0,
    ratioP50: null,
    ratioP95: null
  }])) as MetricDelta<Metric>;
}

function safeRatio(numerator: number | null, denominator: number | null): number | null {
  return numerator !== null && denominator !== null && denominator > 0
    ? numerator / denominator
    : null;
}

function subtractOptional(left: number | null, right: number | null): number | null {
  return left === null || right === null ? null : left - right;
}

function percentile(values: number[], quantile: number): number {
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.max(0, Math.ceil(sorted.length * quantile) - 1)]!;
}

function validateComparison(manifest: ComparisonManifest, root: string): void {
  expect(manifest.schemaVersion).toBe(2);
  if (manifest.outcome !== "passed" || !manifest.surfaces) {
    throw new Error("surface comparison did not produce both passed surfaces");
  }
  expect(manifest.surfaces.tui.samples).toHaveLength(measuredSampleCount);
  expect(manifest.surfaces.workbench.samples).toHaveLength(measuredSampleCount);
  for (const [surfaceName, surface] of Object.entries(manifest.surfaces)) {
    for (const sample of surface.samples) {
      for (const metric of CORE_METRICS) {
        const value = sample[metric];
        if (metric === "sendToFeedbackCommitMs" && value === null) continue;
        expect(Number.isFinite(value), `${surfaceName}.${metric} must be finite`).toBe(true);
        expect(value, `${surfaceName}.${metric} must be non-negative`).toBeGreaterThanOrEqual(0);
      }
      for (const [group, metrics] of [
        ["gatewaySpans", GATEWAY_METRICS],
        ["surfaceSpans", SURFACE_METRICS]
      ] as const) {
        for (const metric of metrics) {
          const value = (sample[group] as Record<string, number>)[metric];
          expect(Number.isFinite(value), `${surfaceName}.${group}.${metric} must be finite`).toBe(true);
          expect(value, `${surfaceName}.${group}.${metric} must be non-negative`).toBeGreaterThanOrEqual(0);
        }
      }
      expect(sample.gatewayTurnId, `${surfaceName} sample must correlate one Gateway Turn`).not.toBe("");
      expect(sample.gatewayStructure).toEqual({ reviewScans: 0, turnStarted: 1 });
      if (surfaceName === "workbench") {
        expect(sample.diagnostics.filter((mark) => (
          mark.id === "rpc_response_arrived"
          && mark.data.observedSampleIndex !== mark.data.originSampleIndex
        ))).toEqual([]);
      }
    }
    expect(surface.summary).toEqual(summarizeSamples(surface.samples));
    expect(surface.gatewaySummary).toEqual(
      summarizeSampleSpans(surface.samples, "gatewaySpans", GATEWAY_METRICS)
    );
    expect(surface.surfaceSummary).toEqual(
      summarizeSampleSpans(surface.samples, "surfaceSpans", SURFACE_METRICS)
    );
  }
  expect(manifest.delta).toEqual(compareSummaries(
    manifest.surfaces.tui.summary,
    manifest.surfaces.workbench.summary
  ));
  expect(manifest.gatewayDelta).toEqual(compareMetricSummaries(
    manifest.surfaces.tui.gatewaySummary,
    manifest.surfaces.workbench.gatewaySummary,
    GATEWAY_METRICS
  ));
  expect(manifest.surfaceDelta).toEqual(compareMetricSummaries(
    manifest.surfaces.tui.surfaceSummary,
    manifest.surfaces.workbench.surfaceSummary,
    SURFACE_METRICS
  ));
  for (const relative of Object.values(manifest.artifacts)) {
    expect(existsSync(path.join(root, relative)), `missing surface artifact ${relative}`).toBe(true);
  }
  assertContentFree(manifest);
}

function assertContentFree(value: unknown, pathParts: string[] = []): void {
  if (Array.isArray(value)) {
    value.forEach((item, index) => assertContentFree(item, [...pathParts, String(index)]));
    return;
  }
  if (!value || typeof value !== "object") return;
  for (const [key, child] of Object.entries(value as Record<string, unknown>)) {
    const normalized = key.toLowerCase();
    const forbidden = new Set([
      "authorization",
      "credential",
      "apikey",
      "api_key",
      "prompt",
      "prompttext",
      "requestbody",
      "response",
      "responsebody",
      "responsetext",
      "token",
      "tokens"
    ]);
    if (forbidden.has(normalized)) {
      throw new Error(`unsafe comparison field ${[...pathParts, key].join(".")}`);
    }
    assertContentFree(child, [...pathParts, key]);
  }
}

function contentFreeProviderEvents(events: DeterministicJourneyEvent[]): Array<Record<string, unknown>> {
  return events.map((event) => ({
    adapter: event.adapter,
    clock: event.clock,
    epochMs: event.epochMs,
    event: event.event,
    monotonicNs: event.monotonicNs,
    plannedDelayMs: event.plannedDelayMs,
    purpose: event.purpose,
    purposeSequence: event.purposeSequence,
    requestIndex: event.requestIndex,
    schemaVersion: event.schemaVersion,
    sequence: event.sequence
  }));
}

function renderReport(manifest: ComparisonManifest): string {
  if (!manifest.surfaces) return "# TUI vs Workbench surface profile\n\nIncomplete.\n";
  const lines = [
    "# TUI vs Workbench surface profile",
    "",
    `Measured samples: ${manifest.contract.measuredSamples} (cold, warmup, and trace diagnostic excluded).`,
    `Synthetic workspace: isolated Git repository with ${manifest.contract.trackedDirtyFiles} deterministically modified tracked files.`,
    "",
    "| Metric | TUI p50 | Workbench p50 | Delta | Ratio | TUI p95 | Workbench p95 |",
    "|---|---:|---:|---:|---:|---:|---:|"
  ];
  for (const metric of CORE_METRICS) {
    const tui = manifest.surfaces.tui.summary[metric];
    const workbench = manifest.surfaces.workbench.summary[metric];
    const delta = manifest.delta[metric];
    lines.push(
      `| ${metric} | ${formatMs(tui.p50)} | ${formatMs(workbench.p50)} | ${formatMs(delta.p50Ms)} | ${formatRatio(delta.ratioP50)} | ${formatMs(tui.p95)} | ${formatMs(workbench.p95)} |`
    );
  }
  lines.push(
    "",
    `Missing send-feedback commits: TUI ${manifest.surfaces.tui.summary.sendToFeedbackCommitMs.missingSamples}/${manifest.contract.measuredSamples}; Workbench ${manifest.surfaces.workbench.summary.sendToFeedbackCommitMs.missingSamples}/${manifest.contract.measuredSamples}.`,
    "",
    "Interpretation:",
    "",
    "- sendToRequest isolates surface admission before the shared provider boundary.",
    "- requestToFirstSurfaceCommit includes Gateway projection, surface delivery, reconciliation, and DOM/terminal commit.",
    "- sendToFeedbackCommit measures optimistic running commit independently from model output.",
    "- Browser post-frame and Long Task observations are diagnostics, not substitutes for paint evidence.",
    "- gatewayStructure requires one turnStarted and zero synchronous workspace review scans per turn.",
    "- Detailed content-free marks remain in the JSONL and manifest diagnostics.",
    ""
  );
  appendComparisonTable(
    lines,
    "Shared Gateway/runtime stages",
    GATEWAY_METRICS,
    manifest.surfaces.tui.gatewaySummary,
    manifest.surfaces.workbench.gatewaySummary,
    manifest.gatewayDelta
  );
  appendComparisonTable(
    lines,
    "Surface receipt/application/commit stages",
    SURFACE_METRICS,
    manifest.surfaces.tui.surfaceSummary,
    manifest.surfaces.workbench.surfaceSummary,
    manifest.surfaceDelta
  );
  appendWorkbenchRequestActivity(lines, manifest.surfaces.workbench.samples);
  return lines.join("\n");
}

function appendWorkbenchRequestActivity(lines: string[], samples: SurfaceSample[]): void {
  const methodCounts = new Map<string, number>();
  let crossSampleRpcOverlaps = 0;
  let settleRefreshFired = 0;
  let settleRefreshScheduled = 0;
  for (const sample of samples) {
    for (const mark of sample.diagnostics) {
      if (mark.id === "rpc_request_sent" && typeof mark.data.method === "string") {
        methodCounts.set(mark.data.method, (methodCounts.get(mark.data.method) ?? 0) + 1);
      } else if (
        mark.id === "rpc_response_arrived"
        && mark.data.observedSampleIndex !== mark.data.originSampleIndex
      ) {
        crossSampleRpcOverlaps += 1;
      } else if (mark.id === "settle_refresh_scheduled") {
        settleRefreshScheduled += 1;
      } else if (mark.id === "settle_refresh_fired") {
        settleRefreshFired += 1;
      }
    }
  }
  lines.push(
    "## Workbench request amplification",
    "",
    "| RPC method | Total requests | Mean per measured turn |",
    "|---|---:|---:|"
  );
  for (const [method, count] of [...methodCounts.entries()].sort((left, right) => (
    right[1] - left[1] || left[0].localeCompare(right[0])
  ))) {
    lines.push(`| ${method} | ${count} | ${(count / samples.length).toFixed(2)} |`);
  }
  lines.push(
    "",
    `Settle refreshes: ${settleRefreshScheduled} scheduled and ${settleRefreshFired} fired across ${samples.length} measured turns.`,
    `Cross-sample RPC overlaps: ${crossSampleRpcOverlaps}.`,
    `Browser Long Tasks: ${samples.reduce((total, sample) => total + sample.longTaskCount, 0)} (${formatMs(samples.reduce((total, sample) => total + sample.longTaskDurationMs, 0))} total).`,
    `Post-settle auxiliary drain: p50 ${formatMs(percentile(samples.map((sample) => sample.postSettleDrainMs), 0.5))}, p95 ${formatMs(percentile(samples.map((sample) => sample.postSettleDrainMs), 0.95))}.`,
    ""
  );
}

function appendComparisonTable<Metric extends string>(
  lines: string[],
  title: string,
  metrics: readonly Metric[],
  tuiSummary: Record<Metric, MetricSummary>,
  workbenchSummary: Record<Metric, MetricSummary>,
  deltaSummary: MetricDelta<Metric>
): void {
  lines.push(
    `## ${title}`,
    "",
    "| Metric | TUI p50 | Workbench p50 | Delta | Ratio | TUI p95 | Workbench p95 |",
    "|---|---:|---:|---:|---:|---:|---:|"
  );
  for (const metric of metrics) {
    const tui = tuiSummary[metric];
    const workbench = workbenchSummary[metric];
    const delta = deltaSummary[metric];
    lines.push(
      `| ${metric} | ${formatMs(tui.p50)} | ${formatMs(workbench.p50)} | ${formatMs(delta.p50Ms)} | ${formatRatio(delta.ratioP50)} | ${formatMs(tui.p95)} | ${formatMs(workbench.p95)} |`
    );
  }
  lines.push("");
}

function formatMs(value: number | null): string {
  return value === null ? "n/a" : `${value.toFixed(2)} ms`;
}

function formatRatio(value: number | null): string {
  return value === null ? "n/a" : `${value.toFixed(2)}x`;
}

function writeJsonLines(target: string, values: unknown[]): void {
  writeFileSync(target, `${values.map((value) => JSON.stringify(value)).join("\n")}\n`);
}

function relativeArtifact(root: string, target: string): string {
  return path.relative(root, target).split(path.sep).join("/");
}

function comparisonArtifactPaths(root: string) {
  const providerDir = path.join(root, "provider");
  const tuiDir = path.join(root, "tui");
  const workbenchDir = path.join(root, "workbench");
  return {
    manifest: path.join(root, "comparison.json"),
    providerDir,
    providerEvents: path.join(providerDir, "events.jsonl"),
    report: path.join(root, "report.md"),
    tuiDir,
    tuiGatewayTrace: path.join(tuiDir, "gateway-marks.jsonl"),
    tuiTrace: path.join(tuiDir, "marks.jsonl"),
    workbenchBrowserMarks: path.join(workbenchDir, "browser-marks.jsonl"),
    workbenchDir,
    workbenchGatewayTrace: path.join(workbenchDir, "gateway-marks.jsonl"),
    workbenchTrace: path.join(workbenchDir, "trace.zip")
  };
}

function sanitizeError(error: unknown, expectedAnswer?: string): { message: string; name: string } {
  const source = error instanceof Error ? error : new Error(String(error));
  let message = source.message.replaceAll(FIXED_INPUT, "[redacted-input]");
  if (expectedAnswer) message = message.replaceAll(expectedAnswer, "[redacted-output]");
  return { message, name: source.name };
}

function monotonicNow(): number {
  return Number(process.hrtime.bigint()) / 1_000_000;
}

function positiveInteger(value: string | undefined, fallback: number): number {
  const parsed = Number(value ?? fallback);
  return Number.isInteger(parsed) && parsed > 0 ? parsed : fallback;
}

function nonNegativeInteger(value: string | undefined, fallback: number): number {
  const parsed = Number(value ?? fallback);
  return Number.isInteger(parsed) && parsed >= 0 ? parsed : fallback;
}

function prepareSyntheticGitWorkspace(workspace: string, dirtyFileCount: number): void {
  const tracked = path.join(workspace, "tracked");
  mkdirSync(tracked, { recursive: true });
  writeFileSync(path.join(workspace, "surface-profile.txt"), "deterministic surface profile\n");
  for (let index = 0; index < dirtyFileCount; index += 1) {
    writeFileSync(
      path.join(tracked, `file-${String(index).padStart(4, "0")}.txt`),
      `baseline ${index}\n`
    );
  }
  runGit(workspace, ["init", "--quiet"]);
  runGit(workspace, ["add", "."]);
  runGit(workspace, [
    "-c",
    "user.name=Psychevo Journey",
    "-c",
    "user.email=journey@invalid",
    "commit",
    "--quiet",
    "-m",
    "deterministic baseline"
  ]);
  for (let index = 0; index < dirtyFileCount; index += 1) {
    writeFileSync(
      path.join(tracked, `file-${String(index).padStart(4, "0")}.txt`),
      `changed ${index}\n`
    );
  }
}

function runGit(cwd: string, args: string[]): void {
  const result = spawnSync("git", args, {
    cwd,
    encoding: "utf8",
    timeout: 60_000
  });
  if (result.status !== 0) {
    const detail = result.stderr.trim() || result.stdout.trim() || `status ${result.status}`;
    throw new Error(`surface profile Git fixture failed: ${detail}`);
  }
}

interface TuiRuntimeOptions {
  cwd: string;
  env: NodeJS.ProcessEnv;
  model: string;
  pevo: string;
}

class TuiPtyDriver {
  private buffer = "";
  private nextCommandId = 1;
  private readonly pending = new Map<number, {
    reject(error: Error): void;
    resolve(value: {
      acknowledgedRunnerMonotonicMs: number;
      sentRunnerMonotonicMs: number;
    }): void;
    sentRunnerMonotonicMs: number;
  }>();
  private stderr = "";

  private constructor(private readonly child: ChildProcessWithoutNullStreams) {
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => this.consume(chunk));
    child.stderr.on("data", (chunk: string) => {
      this.stderr = `${this.stderr}${chunk}`.slice(-8_000);
    });
    child.once("exit", (code, signal) => {
      const error = new Error(`TUI PTY driver exited code=${code} signal=${signal}: ${this.stderr}`);
      for (const pending of this.pending.values()) pending.reject(error);
      this.pending.clear();
    });
  }

  static async start(options: TuiRuntimeOptions): Promise<TuiPtyDriver> {
    const script = path.join(
      repoRoot,
      "apps/workbench/e2e/fixtures/surface-profile-pty.py"
    );
    const child = spawn("python3", [
      script,
      "--pevo",
      options.pevo,
      "--cwd",
      options.cwd,
      "--model",
      options.model
    ], {
      cwd: repoRoot,
      env: options.env,
      stdio: "pipe"
    });
    const driver = new TuiPtyDriver(child);
    await driver.waitForControlEvent("started");
    return driver;
  }

  type(text: string): Promise<{
    acknowledgedRunnerMonotonicMs: number;
    sentRunnerMonotonicMs: number;
  }> {
    return this.command("type", { text });
  }

  async stop(): Promise<void> {
    if (this.child.exitCode !== null) return;
    await this.command("quit", {}).catch(() => undefined);
    await Promise.race([
      new Promise<void>((resolve) => this.child.once("exit", () => resolve())),
      new Promise<void>((resolve) => setTimeout(resolve, 5_000))
    ]);
    if (this.child.exitCode === null) this.child.kill("SIGTERM");
  }

  private command(
    command: "quit" | "type",
    fields: Record<string, unknown>
  ): Promise<{
    acknowledgedRunnerMonotonicMs: number;
    sentRunnerMonotonicMs: number;
  }> {
    const id = this.nextCommandId++;
    return new Promise((resolve, reject) => {
      const sentRunnerMonotonicMs = monotonicNow();
      const timeout = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`TUI PTY command ${command} timed out`));
      }, 30_000);
      this.pending.set(id, {
        reject: (error) => {
          clearTimeout(timeout);
          reject(error);
        },
        resolve: (value) => {
          clearTimeout(timeout);
          resolve(value);
        },
        sentRunnerMonotonicMs
      });
      this.child.stdin.write(`${JSON.stringify({ command, id, ...fields })}\n`);
    });
  }

  private consume(chunk: string): void {
    this.buffer += chunk;
    while (this.buffer.includes("\n")) {
      const newline = this.buffer.indexOf("\n");
      const line = this.buffer.slice(0, newline).trim();
      this.buffer = this.buffer.slice(newline + 1);
      if (!line) continue;
      const event = JSON.parse(line) as { event?: string; id?: number };
      if (typeof event.id === "number") {
        const pending = this.pending.get(event.id);
        pending?.resolve({
          acknowledgedRunnerMonotonicMs: monotonicNow(),
          sentRunnerMonotonicMs: pending.sentRunnerMonotonicMs
        });
        this.pending.delete(event.id);
      }
      this.controlEvents.push(event.event ?? "unknown");
    }
  }

  private readonly controlEvents: string[] = [];

  private async waitForControlEvent(event: string, timeoutMs = 30_000): Promise<void> {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      if (this.controlEvents.includes(event)) return;
      if (this.child.exitCode !== null) {
        throw new Error(`TUI PTY driver exited before ${event}: ${this.stderr}`);
      }
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
    throw new Error(`TUI PTY driver did not emit ${event}: ${this.stderr}`);
  }
}
