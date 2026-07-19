import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";

export type JourneyAdapter = "acp" | "native";
export type JourneyPass = "profile" | "visual";
export type JourneyScenario = "pending-draft-send" | "ready-send";
export type JourneySurface = "desktop" | "workbench";

export type JourneyCheckpointId =
  | "gui_ready"
  | "draft_context_ready"
  | "send_clicked"
  | "runtime_request_dispatched"
  | "first_output_visible"
  | "turn_settled";

export interface JourneyRun {
  adapter: JourneyAdapter;
  artifactRoot: string;
  environment: Record<string, string | number | boolean | null>;
  pass: JourneyPass;
  runId: string;
  scenario: JourneyScenario;
  surface: JourneySurface;
}

export interface JourneyClockObservation {
  epochMs: number;
  monotonicMs: number;
}

export interface JourneyRecorderOptions {
  now?: () => JourneyClockObservation;
  sourceClock?: string;
}

export interface JourneyEventOptions {
  captureEnd?: JourneyClockObservation;
  captureStart?: JourneyClockObservation;
  correlation?: Record<string, unknown>;
  screenshot?: string;
  [key: string]: unknown;
}

export interface JourneyMarkOptions {
  clock?: JourneyClockObservation & { source: string };
  correlation?: Record<string, unknown>;
  data?: Record<string, unknown>;
}

export interface JourneyFinalizeOptions {
  correlations?: Record<string, unknown>;
  error?: unknown;
  outcome?: "failed" | "passed";
  profile?: Record<string, unknown>;
  trace?: string;
}

const readyOrder: JourneyCheckpointId[] = [
  "gui_ready",
  "draft_context_ready",
  "send_clicked",
  "runtime_request_dispatched",
  "first_output_visible",
  "turn_settled"
];
const pendingOrder: JourneyCheckpointId[] = [
  "gui_ready",
  "send_clicked",
  "draft_context_ready",
  "runtime_request_dispatched",
  "first_output_visible",
  "turn_settled"
];
const blockedKeyWords = new Set([
  "authorization",
  "content",
  "credential",
  "prompt",
  "response",
  "secret",
  "text",
  "token"
]);

export class JourneyRecorder {
  readonly #run: JourneyRun;
  readonly #now: () => JourneyClockObservation;
  readonly #sourceClock: string;
  readonly #startedAt: JourneyClockObservation;
  readonly #marks: Array<Record<string, unknown>> = [];
  readonly #checkpoints: Array<Record<string, unknown>> = [];
  #finalized = false;

  constructor(run: JourneyRun, options: JourneyRecorderOptions = {}) {
    this.#run = { ...run, artifactRoot: path.resolve(run.artifactRoot) };
    this.#now = options.now ?? defaultClock;
    this.#sourceClock = options.sourceClock ?? "node:process";
    this.#startedAt = this.#now();
  }

  mark(id: string, options: JourneyMarkOptions = {}): void {
    this.#ensureOpen();
    const observed = options.clock ?? this.#now();
    const source = options.clock?.source ?? this.#sourceClock;
    this.#marks.push({
      id,
      sequence: this.#marks.length + 1,
      clock: source === this.#sourceClock
        ? clockRecord(source, observed, this.#startedAt)
        : externalClockRecord(source, observed),
      correlation: sanitizeRecord(options.correlation ?? {}),
      data: sanitizeRecord(options.data ?? {})
    });
  }

  checkpoint(id: JourneyCheckpointId, options: JourneyEventOptions = {}): void {
    this.#ensureOpen();
    const expected = this.#expectedOrder()[this.#checkpoints.length];
    if (id !== expected) {
      throw new Error(`journey checkpoint ${id} is out of order; expected ${expected ?? "no further checkpoint"}`);
    }
    const observed = this.#now();
    const previous = this.#checkpoints.at(-1)?.clock as { monotonicMs?: number; source?: string } | undefined;
    const screenshot = options.screenshot ? this.#screenshot(options.screenshot, options) : undefined;
    const data = { ...options };
    delete data.screenshot;
    delete data.captureStart;
    delete data.captureEnd;
    delete data.correlation;
    this.#checkpoints.push({
      id,
      sequence: this.#checkpoints.length + 1,
      clock: clockRecord(this.#sourceClock, observed, this.#startedAt),
      deltaMs: previous?.source === this.#sourceClock && typeof previous.monotonicMs === "number"
        ? observed.monotonicMs - previous.monotonicMs
        : null,
      correlation: sanitizeRecord(options.correlation ?? {}),
      data: sanitizeRecord(data),
      ...(screenshot ? { screenshot } : {})
    });
  }

  finalize(options: JourneyFinalizeOptions = {}): string {
    this.#ensureOpen();
    const outcome = options.outcome ?? (options.error ? "failed" : "passed");
    if (outcome === "passed") {
      this.#validatePassedJourney();
    }
    this.#finalized = true;
    const completedAt = this.#now();
    const artifactRoot = this.#run.artifactRoot;
    mkdirSync(artifactRoot, { recursive: true });
    const manifestPath = path.join(artifactRoot, "journey.json");
    const trace = options.trace ? verifiedArtifactPath(artifactRoot, options.trace, "trace") : undefined;
    const manifest = {
      schemaVersion: 1,
      run: {
        adapter: this.#run.adapter,
        environment: sanitizeRecord(this.#run.environment),
        pass: this.#run.pass,
        runId: this.#run.runId,
        scenario: this.#run.scenario,
        surface: this.#run.surface
      },
      outcome,
      clock: {
        completed: clockRecord(this.#sourceClock, completedAt, this.#startedAt),
        started: clockRecord(this.#sourceClock, this.#startedAt, this.#startedAt)
      },
      correlations: sanitizeRecord(options.correlations ?? {}),
      checkpoints: this.#checkpoints,
      marks: this.#marks,
      profile: sanitizeRecord(options.profile ?? {}),
      ...(trace ? { trace: { path: trace } } : {}),
      ...(options.error ? { failure: boundedError(options.error) } : {})
    };
    writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
    return manifestPath;
  }

  #ensureOpen(): void {
    if (this.#finalized) {
      throw new Error("journey recorder has already been finalized");
    }
  }

  #expectedOrder(): JourneyCheckpointId[] {
    return this.#run.scenario === "pending-draft-send" ? pendingOrder : readyOrder;
  }

  #validatePassedJourney(): void {
    if (this.#checkpoints.length !== this.#expectedOrder().length) {
      throw new Error(
        `passed journey has ${this.#checkpoints.length} checkpoints; expected ${this.#expectedOrder().length}`
      );
    }
    for (const checkpoint of this.#checkpoints) {
      if (this.#run.pass === "visual" && !checkpoint.screenshot) {
        throw new Error(`visual journey checkpoint ${String(checkpoint.id)} is missing a screenshot`);
      }
      if (this.#run.pass === "profile" && checkpoint.screenshot) {
        throw new Error(`profile journey checkpoint ${String(checkpoint.id)} must not include a screenshot`);
      }
    }
  }

  #screenshot(screenshotPath: string, options: JourneyEventOptions): Record<string, unknown> {
    const relativePath = verifiedArtifactPath(this.#run.artifactRoot, screenshotPath, "screenshot");
    const start = options.captureStart;
    const end = options.captureEnd;
    return {
      path: relativePath,
      captureStart: start ? clockRecord(this.#sourceClock, start, this.#startedAt) : null,
      captureEnd: end ? clockRecord(this.#sourceClock, end, this.#startedAt) : null,
      captureLagMs: start && end ? end.monotonicMs - start.monotonicMs : null
    };
  }
}

function verifiedArtifactPath(root: string, candidate: string, kind: string): string {
  const relative = relativeArtifactPath(root, candidate, true);
  if (!existsSync(path.join(root, relative))) {
    throw new Error(`journey ${kind} does not exist: ${relative}`);
  }
  return relative;
}

function defaultClock(): JourneyClockObservation {
  return {
    epochMs: Date.now(),
    monotonicMs: Number(process.hrtime.bigint()) / 1_000_000
  };
}

function clockRecord(source: string, value: JourneyClockObservation, origin: JourneyClockObservation) {
  return {
    source,
    epochMs: value.epochMs,
    monotonicMs: value.monotonicMs,
    offsetMs: value.monotonicMs - origin.monotonicMs
  };
}

function externalClockRecord(
  source: string,
  value: JourneyClockObservation
): { epochMs: number; monotonicMs: number; offsetMs: null; source: string } {
  return {
    source,
    epochMs: value.epochMs,
    monotonicMs: value.monotonicMs,
    offsetMs: null
  };
}

function relativeArtifactPath(root: string, candidate: string, requireWithinRoot: boolean): string {
  const absolute = path.resolve(candidate);
  const relative = path.relative(root, absolute);
  if (requireWithinRoot && (relative === "" || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative))) {
    throw new Error(`journey artifact must be a file below ${root}`);
  }
  return relative.replaceAll(path.sep, "/");
}

function sanitizeRecord(record: Record<string, unknown>): Record<string, unknown> {
  return sanitizeValue(record) as Record<string, unknown>;
}

function sanitizeValue(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(sanitizeValue);
  }
  if (!value || typeof value !== "object") {
    return value;
  }
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>)
      .filter(([key]) => !sensitiveKey(key))
      .map(([key, nested]) => [key, sanitizeValue(nested)])
  );
}

function sensitiveKey(key: string): boolean {
  const words = key
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter(Boolean);
  return words.some((word) => blockedKeyWords.has(word))
    || (words.includes("api") && words.includes("key"));
}

function boundedError(error: unknown): { message: string; name: string } {
  const value = error instanceof Error ? error : new Error(String(error));
  const internalMessage = /^(?:journey |passed journey|profile journey|visual journey)/.test(
    value.message
  );
  return {
    name: value.name.slice(0, 80),
    message: internalMessage
      ? value.message.slice(0, 500)
      : "Journey failed; inspect the automation trace and logs."
  };
}
