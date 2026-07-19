import { existsSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import path from "node:path";
import { expect, test, type Page } from "@playwright/test";
import {
  JourneyRecorder,
  type JourneyCheckpointId,
  type JourneyClockObservation,
  type JourneyPass,
  type JourneyScenario
} from "../src/journey-recorder";
import { repoRoot, startPevoWeb } from "./harness";
import {
  afterTwoPaints,
  beginBrowserJourneySample,
  holdNextDraftOpen,
  holdNextTurnStart,
  installJourneyWebSocketProbe,
  readBrowserJourneyMarks,
  releaseDraftOpen,
  releaseTurnStart,
  waitForBrowserJourneyRunnerMark,
  type BrowserJourneyMark
} from "./journey-websocket-probe";
import {
  prepareDeterministicAcpAgent,
  startDeterministicNativeModel,
  type DeterministicAcpAgentFixture,
  type DeterministicJourneyControl,
  type DeterministicJourneyEvent,
  type DeterministicNativeModelFixture
} from "./runtime-live.support";

const journeyPass = parseJourneyPass(process.env.PSYCHEVO_JOURNEY_PASS);
const journeyRoot = path.resolve(
  process.env.PSYCHEVO_PLAYWRIGHT_JOURNEY_ROOT
    ?? path.join(repoRoot, ".local/playwright/journeys/first-turn")
);
const measuredSampleCount = positiveInteger(
  process.env.PSYCHEVO_JOURNEY_PROFILE_SAMPLES,
  20
);
const CRITICAL_JOURNEY_PROMPT = "critical journey deterministic prompt";

test.use({ trace: "off", video: "off" });
test.describe.configure({ mode: "serial" });
test.skip(!journeyPass, "run through the Workbench critical-journey profile or visual pass");

for (const adapter of ["native", "acp"] as const) {
  for (const scenario of ["ready-send", "pending-draft-send"] as const) {
    test(`${adapter} ${scenario} records the critical first-turn journey`, async ({
      context,
      isMobile,
      page
    }, testInfo) => {
      test.skip(isMobile, "the critical journey owns one desktop Chromium viewport");
      test.setTimeout(journeyPass === "profile" ? 600_000 : 240_000);
      const pass = journeyPass as JourneyPass;
      const artifactRoot = path.join(journeyRoot, adapter, scenario, pass);
      rmSync(artifactRoot, { force: true, recursive: true });
      mkdirSync(artifactRoot, { recursive: true });
      const recorder = new JourneyRecorder({
        adapter,
        artifactRoot,
        environment: {
          platform: process.platform,
          project: testInfo.project.name,
          viewport: "1440x960"
        },
        pass,
        runId: `${adapter}-${scenario}-${pass}`,
        scenario,
        surface: "workbench"
      });
      const fixtureParent = path.join(repoRoot, ".local/playwright");
      mkdirSync(fixtureParent, { recursive: true });
      const fixtureScratch = mkdtempSync(path.join(fixtureParent, `critical-journey-${adapter}-`));
      let runtime: JourneyRuntime | null = null;
      let server: Awaited<ReturnType<typeof startPevoWeb>> | null = null;
      const tracePath = path.join(artifactRoot, "trace.zip");
      let tracing = false;
      let finalized = false;
      try {
        runtime = await startJourneyRuntime(adapter, pass, fixtureScratch);
        server = await startJourneyServer(runtime, fixtureScratch);
        await installJourneyWebSocketProbe(page);
        const url = new URL(server.url);
        const navigationStartedAt = monotonicNow();
        await page.goto(url.toString(), { waitUntil: "domcontentloaded" });

        await waitForGuiReady(page);
        if (pass === "visual") await afterTwoPaints(page);
        await recordCheckpoint(recorder, page, artifactRoot, "gui_ready", pass);
        const coldGuiReadyMs = monotonicNow() - navigationStartedAt;

        const primaryDraft = await prepareJourneyDraft({
          adapter,
          initial: true,
          page,
          runtime,
          scenario
        });
        if (primaryDraft.ready) {
          if (pass === "visual") await afterTwoPaints(page);
          await recordCheckpoint(recorder, page, artifactRoot, "draft_context_ready", pass);
        }
        let coldDraftReadyMs = primaryDraft.ready ? monotonicNow() - navigationStartedAt : null;

        const primary = await runPrimaryJourney({
          artifactRoot,
          page,
          pass,
          recorder,
          mainRequestSequence: 1,
          runtime,
          scenario
        });
        coldDraftReadyMs ??= primary.draftReadyAtMs === null
          ? null
          : primary.draftReadyAtMs - navigationStartedAt;
        const primaryProbeMarks = await readBrowserJourneyMarks(page);
        for (const mark of primaryProbeMarks) {
          recorder.mark(`browser_${mark.id}`, {
            clock: {
              source: mark.clock,
              epochMs: mark.epochMs,
              monotonicMs: mark.monotonicMs
            },
            data: mark.data
          });
        }

        let profile: Record<string, unknown> = {};
        if (pass === "profile") {
          await prepareJourneyDraft({
            adapter,
            initial: false,
            page,
            runtime,
            scenario
          });
          await beginBrowserJourneySample(page, -2);
          await page.getByPlaceholder("Ask Psychevo...").fill(CRITICAL_JOURNEY_PROMPT);
          const warmup = await runMeasuredJourney({
            mainRequestSequence: 2,
            page,
            runtime,
            sampleIndex: -2,
            scenario
          });

          await prepareJourneyDraft({
            adapter,
            initial: false,
            page,
            runtime,
            scenario
          });
          await beginBrowserJourneySample(page, -1);
          await page.getByPlaceholder("Ask Psychevo...").fill(CRITICAL_JOURNEY_PROMPT);
          await context.tracing.start({ screenshots: false, snapshots: false, sources: false });
          tracing = true;
          const traceDiagnostic = await runMeasuredJourney({
            mainRequestSequence: 3,
            page,
            runtime,
            sampleIndex: -1,
            scenario
          });
          await context.tracing.stop({ path: tracePath });
          tracing = false;

          const samples: SendPathSample[] = [];
          for (let sampleIndex = 1; sampleIndex <= measuredSampleCount; sampleIndex += 1) {
            await prepareJourneyDraft({
              adapter,
              initial: false,
              page,
              runtime,
              scenario
            });
            await beginBrowserJourneySample(page, sampleIndex);
            await page.getByPlaceholder("Ask Psychevo...").fill(CRITICAL_JOURNEY_PROMPT);
            const sample = await runMeasuredJourney({
              mainRequestSequence: sampleIndex + 3,
              page,
              runtime,
              sampleIndex,
              scenario
            });
            samples.push(sample);
          }
          profile = {
            warmupExcluded: true,
            traceDiagnosticExcluded: true,
            warmup,
            traceDiagnostic,
            measuredSamples: measuredSampleCount,
            cold: {
              navigationToGuiReadyMs: coldGuiReadyMs,
              navigationToDraftReadyMs: coldDraftReadyMs
            },
            samples,
            summary: summarizeSamples(samples)
          };
        }

        recorder.finalize({
          correlations: { adapter, scenario },
          profile,
          ...(pass === "profile" ? { trace: tracePath } : {})
        });
        finalized = true;
      } catch (error) {
        if (!page.isClosed()) {
          const failureProbeMarks = await readBrowserJourneyMarks(page).catch(() => []);
          for (const mark of failureProbeMarks) {
            recorder.mark(`browser_${mark.id}`, {
              clock: {
                source: mark.clock,
                epochMs: mark.epochMs,
                monotonicMs: mark.monotonicMs
              },
              data: mark.data
            });
          }
        }
        if (tracing) {
          await context.tracing.stop({ path: tracePath }).catch(() => undefined);
          tracing = false;
        }
        if (!finalized) {
          recorder.finalize({
            error,
            ...(existsSync(tracePath) ? { trace: tracePath } : {})
          });
          finalized = true;
        }
        throw error;
      } finally {
        await server?.stop();
        await runtime?.stop();
        rmSync(fixtureScratch, { force: true, recursive: true });
      }
    });
  }
}

interface JourneyRuntime {
  adapter: "acp" | "native";
  configAppend: string;
  control: DeterministicJourneyControl;
  expectedAnswer: string;
  fixture: DeterministicAcpAgentFixture | DeterministicNativeModelFixture;
  model: string | undefined;
  runtimeRef: string;
  stop(): Promise<void>;
}

interface SendPathSample {
  frontendMarks: Array<{
    data: Record<string, unknown>;
    id: string;
    monotonicMs: number;
    sequence: number;
  }>;
  mainRequestSequence: number;
  requestIndex: number;
  requestToFirstOutputMs: number;
  firstOutputToSettledMs: number;
  sendToFirstOutputMs: number;
  sendToRequestMs: number;
  sendToSettledMs: number;
}

async function startJourneyRuntime(
  adapter: JourneyRuntime["adapter"],
  pass: JourneyPass,
  scratch: string
): Promise<JourneyRuntime> {
  if (adapter === "native") {
    const fixture = await startDeterministicNativeModel({ journeyMode: pass });
    if (!fixture.journey) throw new Error("Native critical journey control is unavailable");
    return {
      adapter,
      configAppend: [
        "[provider.journey-native]",
        `api = ${JSON.stringify(fixture.baseUrl)}`,
        "no_auth = true",
        "",
        "[provider.journey-native.models.default]",
        ""
      ].join("\n"),
      control: fixture.journey,
      expectedAnswer: fixture.expectedAnswer,
      fixture,
      model: "journey-native/default",
      runtimeRef: "native",
      stop: () => fixture.stop()
    };
  }
  const fixture = prepareDeterministicAcpAgent(
    "codex",
    scratch,
    "critical_journey",
    { journeyMode: pass, runtimeRef: "journey-acp", profileLabel: "Journey ACP" }
  );
  if (!fixture.journey) throw new Error("ACP critical journey control is unavailable");
  return {
    adapter,
    configAppend: fixture.configAppend,
    control: fixture.journey,
    expectedAnswer: fixture.expectedAnswer,
    fixture,
    model: undefined,
    runtimeRef: fixture.runtimeRef,
    stop: async () => undefined
  };
}

async function startJourneyServer(runtime: JourneyRuntime, scratch: string) {
  const cwd = path.join(scratch, "cwd");
  mkdirSync(cwd, { recursive: true });
  return startPevoWeb({
    configAppend: runtime.configAppend,
    cwd,
    live: false,
    model: runtime.model
  });
}

async function prepareJourneyDraft(options: {
  adapter: JourneyRuntime["adapter"];
  initial: boolean;
  page: Page;
  runtime: JourneyRuntime;
  scenario: JourneyScenario;
}): Promise<{ ready: boolean }> {
  const { adapter, initial, page, runtime, scenario } = options;
  if (initial) {
    await waitForDraftReady(page);
    if (adapter === "acp") {
      await selectAcpTarget(page, runtime.runtimeRef);
      await waitForDraftReady(page);
    }
  }

  if (scenario === "pending-draft-send") {
    const heldBefore = await browserMarkCount(page, "draft_response_held");
    await holdNextDraftOpen(page);
    await page.getByRole("button", { name: "New Session", exact: true }).click();
    await waitForBrowserMarkAfter(page, "draft_response_held", heldBefore);
    return { ready: false };
  }

  if (!initial) {
    await page.getByRole("button", { name: "New Session", exact: true }).click();
    await waitForDraftReady(page);
  }
  return { ready: true };
}

async function runPrimaryJourney(options: {
  artifactRoot: string;
  page: Page;
  pass: JourneyPass;
  recorder: JourneyRecorder;
  mainRequestSequence: number;
  runtime: JourneyRuntime;
  scenario: JourneyScenario;
}): Promise<{ draftReadyAtMs: number | null }> {
  const { artifactRoot, mainRequestSequence, page, pass, recorder, runtime, scenario } = options;
  let draftReadyAtMs: number | null = null;
  const input = page.getByPlaceholder("Ask Psychevo...");
  const environment = page.locator('[aria-label="Composer environment"]');
  const committedEnvironment = scenario === "pending-draft-send"
    ? await environment.textContent()
    : null;
  await beginBrowserJourneySample(page, 0);
  await input.fill(CRITICAL_JOURNEY_PROMPT);
  const turnRequestsBefore = await turnStartRequestCount(page);
  const draftAppliedBefore = await browserMarkCount(page, "draft_context_applied");
  await page.getByRole("button", { name: "Send message" }).click();
  if (pass === "visual") await afterTwoPaints(page);
  if (committedEnvironment !== null) {
    await expect(environment).toBeVisible();
    expect(await environment.textContent()).toBe(committedEnvironment);
  }
  await recordCheckpoint(recorder, page, artifactRoot, "send_clicked", pass);
  if (scenario === "pending-draft-send") {
    await expect(input).toHaveValue(CRITICAL_JOURNEY_PROMPT);
    expect(await turnStartRequestCount(page)).toBe(turnRequestsBefore);
    if (pass === "visual") await holdNextTurnStart(page);
    await releaseDraftOpen(page);
    await waitForBrowserMarkAfter(page, "draft_context_applied", draftAppliedBefore);
    draftReadyAtMs = monotonicNow();
    if (pass === "visual") await afterTwoPaints(page);
    await recordCheckpoint(recorder, page, artifactRoot, "draft_context_ready", pass);
    if (pass === "visual") await releaseTurnStart(page);
  }

  const request = await runtime.control.waitFor(
    "request_received",
    mainTurn(mainRequestSequence),
    60_000
  );
  recordRuntimeEvent(recorder, request);
  await recordCheckpoint(recorder, page, artifactRoot, "runtime_request_dispatched", pass);
  if (pass === "visual") runtime.control.releaseFirstOutput(request.requestIndex);
  const firstOutput = await runtime.control.waitFor("first_output_emitted", request.requestIndex, 60_000);
  recordRuntimeEvent(recorder, firstOutput);
  await waitForNewAssistantOutput(page, runtime.expectedAnswer);
  if (pass === "visual") await afterTwoPaints(page);
  await recordCheckpoint(recorder, page, artifactRoot, "first_output_visible", pass);
  if (pass === "visual") {
    await expect(page.locator(".appShell")).toHaveAttribute("data-turn-state", "running");
    runtime.control.releaseCompletion(request.requestIndex);
  }
  const completion = await runtime.control.waitFor("completion_emitted", request.requestIndex, 60_000);
  recordRuntimeEvent(recorder, completion);
  await waitForTurnSettled(page, runtime.expectedAnswer);
  if (pass === "visual") await afterTwoPaints(page);
  await recordCheckpoint(recorder, page, artifactRoot, "turn_settled", pass);
  expect(await turnStartRequestCount(page)).toBe(turnRequestsBefore + 1);
  expect(mainRequestEvents(runtime, mainRequestSequence)).toHaveLength(1);
  return { draftReadyAtMs };
}

async function runMeasuredJourney(options: {
  mainRequestSequence: number;
  page: Page;
  runtime: JourneyRuntime;
  sampleIndex: number;
  scenario: JourneyScenario;
}): Promise<SendPathSample> {
  const { mainRequestSequence, page, runtime, sampleIndex, scenario } = options;
  const before = await turnStartRequestCount(page);
  const draftAppliedBefore = await browserMarkCount(page, "draft_context_applied");
  await page.getByRole("button", { name: "Send message" }).click();
  const send = await waitForBrowserJourneyRunnerMark(page, "send_clicked", sampleIndex);
  if (scenario === "pending-draft-send") {
    expect(await turnStartRequestCount(page)).toBe(before);
    await releaseDraftOpen(page);
    await waitForBrowserMarkAfter(page, "draft_context_applied", draftAppliedBefore);
  }
  const request = await runtime.control.waitFor(
    "request_received",
    mainTurn(mainRequestSequence),
    60_000
  );
  const requestAt = monotonicNow();
  await runtime.control.waitFor("first_output_emitted", request.requestIndex, 60_000);
  await waitForNewAssistantOutput(page, runtime.expectedAnswer);
  const firstOutput = await waitForBrowserJourneyRunnerMark(
    page,
    "first_output_visible",
    sampleIndex
  );
  await runtime.control.waitFor("completion_emitted", request.requestIndex, 60_000);
  await waitForTurnSettled(page, runtime.expectedAnswer);
  const settled = await waitForBrowserJourneyRunnerMark(
    page,
    "turn_settled_visible",
    sampleIndex
  );
  expect(await turnStartRequestCount(page)).toBe(before + 1);
  expect(mainRequestEvents(runtime, mainRequestSequence)).toHaveLength(1);
  const frontendMarks = (await readBrowserJourneyMarks(page))
    .filter((mark) => mark.sampleIndex === sampleIndex)
    .map(contentFreeFrontendMark);
  return {
    frontendMarks,
    mainRequestSequence,
    requestIndex: request.requestIndex,
    sendToRequestMs: requestAt - send.runnerMonotonicMs,
    requestToFirstOutputMs: firstOutput.runnerMonotonicMs - requestAt,
    firstOutputToSettledMs: settled.runnerMonotonicMs - firstOutput.runnerMonotonicMs,
    sendToFirstOutputMs: firstOutput.runnerMonotonicMs - send.runnerMonotonicMs,
    sendToSettledMs: settled.runnerMonotonicMs - send.runnerMonotonicMs
  };
}

async function recordCheckpoint(
  recorder: JourneyRecorder,
  page: Page,
  artifactRoot: string,
  id: JourneyCheckpointId,
  pass: JourneyPass
): Promise<void> {
  if (pass === "profile") {
    recorder.checkpoint(id);
    return;
  }
  const screenshot = path.join(
    artifactRoot,
    `${String(checkpointSequence(id)).padStart(2, "0")}-${id.replaceAll("_", "-")}.png`
  );
  const captureStart = clockNow();
  await page.screenshot({ fullPage: true, path: screenshot });
  const captureEnd = clockNow();
  recorder.checkpoint(id, { captureEnd, captureStart, screenshot });
}

function checkpointSequence(id: JourneyCheckpointId): number {
  return ({
    gui_ready: 1,
    draft_context_ready: 2,
    send_clicked: 3,
    runtime_request_dispatched: 4,
    first_output_visible: 5,
    turn_settled: 6
  })[id];
}

function recordRuntimeEvent(recorder: JourneyRecorder, event: DeterministicJourneyEvent): void {
  recorder.mark(`runtime_${event.event}`, {
    clock: {
      source: event.clock,
      epochMs: event.epochMs,
      monotonicMs: Number(BigInt(event.monotonicNs)) / 1_000_000
    },
    correlation: {
      purpose: event.purpose,
      purposeSequence: event.purposeSequence,
      requestIndex: event.requestIndex,
      sessionId: event.sessionId ?? null
    },
    data: {
      adapter: event.adapter,
      plannedDelayMs: event.plannedDelayMs,
      sequence: event.sequence
    }
  });
}

function mainTurn(sequence: number) {
  return { purpose: "main_turn" as const, sequence };
}

function mainRequestEvents(runtime: JourneyRuntime, sequence: number): DeterministicJourneyEvent[] {
  return runtime.control.events().filter((event) => (
    event.event === "request_received"
    && event.purpose === "main_turn"
    && event.purposeSequence === sequence
  ));
}

function contentFreeFrontendMark(mark: BrowserJourneyMark): {
  data: Record<string, unknown>;
  id: string;
  monotonicMs: number;
  sequence: number;
} {
  return {
    data: mark.data,
    id: mark.id,
    monotonicMs: mark.monotonicMs,
    sequence: mark.sequence
  };
}

async function waitForGuiReady(page: Page): Promise<void> {
  await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
  await expect(page.getByPlaceholder("Ask Psychevo...")).toBeVisible();
  await expect(page.locator('.appShell[data-gateway-status="connected"]')).toBeVisible();
  await expect(page.getByPlaceholder("Ask Psychevo...")).toBeEditable();
}

async function waitForDraftReady(page: Page): Promise<void> {
  await expect(page.locator('.appShell[data-composer-state="ready"]')).toBeVisible({ timeout: 60_000 });
  await expect(page.getByPlaceholder("Ask Psychevo...")).toBeEditable();
  await expect(page.getByRole("button", { name: "Agent target" })).not.toContainText("Select Agent");
  await expect(page.getByRole("button", { name: "Workspace", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Git branch" })).not.toHaveText("Git branch");
}

async function waitForNewAssistantOutput(page: Page, expectedAnswer: string): Promise<void> {
  await expect(page.locator(".pevo-message.is-assistant").filter({
    hasText: new RegExp(escapeRegExp(expectedAnswer.slice(0, Math.ceil(expectedAnswer.length / 2))))
  })).toHaveCount(1, { timeout: 60_000 });
}

async function waitForTurnSettled(page: Page, expectedAnswer: string): Promise<void> {
  await expect(page.locator(".pevo-message.is-assistant").filter({
    hasText: new RegExp(escapeRegExp(expectedAnswer))
  })).toHaveCount(1, { timeout: 60_000 });
  await expect(page.locator('.appShell[data-turn-state="idle"]')).toBeVisible({ timeout: 60_000 });
  await expect(page.getByPlaceholder("Ask Psychevo...")).toBeEditable();
}

async function selectAcpTarget(page: Page, runtimeRef: string): Promise<void> {
  const catalog = await gatewayRequest(page, "thread/context/read", {
    scope: webScope(await activeCwd(page)),
    target: null,
    threadId: null
  }) as {
    compatibleTargets?: Array<{
      agentRef?: string | null;
      label?: string;
      runtimeProfileRef?: string;
    }>;
  };
  const target = catalog.compatibleTargets?.find((candidate) => (
    candidate.runtimeProfileRef === runtimeRef && candidate.agentRef === runtimeRef
  ));
  if (!target?.label) throw new Error(`missing ACP journey target for ${runtimeRef}`);
  await page.getByRole("button", { name: "Agent target", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Agent target" });
  const choice = dialog.getByRole("radio", {
    name: new RegExp(`^(?:Start a new thread with )?${escapeRegExp(target.label)}$`)
  });
  await expect(choice).toBeEnabled({ timeout: 30_000 });
  await choice.click();
}

async function activeCwd(page: Page): Promise<string> {
  const initialized = await gatewayRequest(page, "initialize", {} as Record<string, never>) as {
    scope?: { cwd?: string };
  };
  const cwd = initialized.scope?.cwd;
  if (!cwd) throw new Error("Gateway initialize result did not include cwd");
  return cwd;
}

async function turnStartRequestCount(page: Page): Promise<number> {
  return (await readBrowserJourneyMarks(page)).filter((mark) => (
    mark.id === "rpc_request_sent" && mark.data.method === "turn/start"
  )).length;
}

async function waitForBrowserMarkAfter(page: Page, id: string, before: number): Promise<void> {
  await expect.poll(async () => (
    await readBrowserJourneyMarks(page)
  ).filter((mark) => mark.id === id).length, { timeout: 15_000 }).toBeGreaterThan(before);
}

async function browserMarkCount(page: Page, id: string): Promise<number> {
  return (await readBrowserJourneyMarks(page)).filter((mark) => mark.id === id).length;
}

async function gatewayRequest(page: Page, method: string, params: unknown): Promise<unknown> {
  return page.evaluate(async ({ method, params }) => new Promise((resolve, reject) => {
    const url = new URL("/ws", window.location.origin);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    const socket = new WebSocket(url);
    const id = `critical-journey-${method}-${Date.now()}`;
    const timeout = window.setTimeout(() => {
      socket.close();
      reject(new Error(`${method} timed out`));
    }, 30_000);
    socket.addEventListener("open", () => {
      socket.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
    });
    socket.addEventListener("message", (event) => {
      const message = JSON.parse(String(event.data)) as { error?: unknown; id?: string; result?: unknown };
      if (message.id !== id) return;
      window.clearTimeout(timeout);
      socket.close();
      if (message.error) reject(new Error(`${method} failed`));
      else resolve(message.result);
    });
    socket.addEventListener("error", () => {
      window.clearTimeout(timeout);
      reject(new Error(`${method} WebSocket failed`));
    });
  }), { method, params });
}

function webScope(cwd: string) {
  return {
    cwd,
    source: {
      kind: "web",
      lifetime: "persistent",
      rawId: null,
      rawIdentity: null,
      visibleName: null
    }
  };
}

function summarizeSamples(samples: SendPathSample[]): Record<string, { p50: number; p95: number }> {
  const metrics: Array<
    | "sendToRequestMs"
    | "requestToFirstOutputMs"
    | "firstOutputToSettledMs"
    | "sendToFirstOutputMs"
    | "sendToSettledMs"
  > = [
    "sendToRequestMs",
    "requestToFirstOutputMs",
    "firstOutputToSettledMs",
    "sendToFirstOutputMs",
    "sendToSettledMs"
  ];
  return Object.fromEntries(metrics.map((metric) => {
    const values = samples.map((sample) => sample[metric]);
    return [metric, { p50: percentile(values, 0.5), p95: percentile(values, 0.95) }];
  }));
}

function percentile(values: number[], quantile: number): number {
  if (values.length === 0) throw new Error("cannot summarize an empty journey sample set");
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.max(0, Math.ceil(sorted.length * quantile) - 1)]!;
}

function clockNow(): JourneyClockObservation {
  return { epochMs: Date.now(), monotonicMs: monotonicNow() };
}

function monotonicNow(): number {
  return Number(process.hrtime.bigint()) / 1_000_000;
}

function parseJourneyPass(value: string | undefined): JourneyPass | null {
  return value === "profile" || value === "visual" ? value : null;
}

function positiveInteger(value: string | undefined, fallback: number): number {
  const parsed = Number(value ?? fallback);
  return Number.isInteger(parsed) && parsed > 0 ? parsed : fallback;
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
