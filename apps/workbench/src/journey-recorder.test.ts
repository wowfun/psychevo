import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { JourneyRecorder, type JourneyRun } from "./journey-recorder";

const roots: string[] = [];

afterEach(() => {
  for (const root of roots.splice(0)) {
    rmSync(root, { force: true, recursive: true });
  }
});

describe("JourneyRecorder", () => {
  it("writes one ordered ready-send manifest without leaking model content", () => {
    const root = temporaryRoot();
    const recorder = new JourneyRecorder(run(root, "ready-send"), {
      now: sequenceClock(),
      sourceClock: "node:test"
    });

    recorder.mark("navigation_started", {
      data: {
        requestId: "request-1",
        prompt: "secret prompt",
        providerToken: "secret provider token",
        nested: {
          apiKey: "secret API key",
          contextRevision: "context-safe",
          generatedText: "secret generated text"
        }
      }
    });
    recordReadyCheckpoints(recorder, root);
    const manifestPath = recorder.finalize();

    expect(existsSync(manifestPath)).toBe(true);
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    expect(manifest.schemaVersion).toBe(1);
    expect(manifest.outcome).toBe("passed");
    expect(manifest.checkpoints.map((checkpoint: { id: string }) => checkpoint.id)).toEqual([
      "gui_ready",
      "draft_context_ready",
      "send_clicked",
      "runtime_request_dispatched",
      "first_output_visible",
      "turn_settled"
    ]);
    expect(manifest.checkpoints[0].screenshot.path).toBe("01-gui-ready.png");
    expect(manifest.checkpoints[1].deltaMs).toBe(1);
    expect(manifest.marks[0].data).toEqual({
      requestId: "request-1",
      nested: { contextRevision: "context-safe" }
    });
    expect(JSON.stringify(manifest)).not.toContain("secret prompt");
    expect(JSON.stringify(manifest)).not.toContain("secret response");
    expect(JSON.stringify(manifest)).not.toContain("secret provider token");
    expect(JSON.stringify(manifest)).not.toContain("secret API key");
    expect(JSON.stringify(manifest)).not.toContain("secret generated text");
  });

  it("accepts the pending-draft checkpoint order and preserves external clock domains", () => {
    const root = temporaryRoot();
    const recorder = new JourneyRecorder(run(root, "pending-draft-send"), {
      now: sequenceClock(),
      sourceClock: "node:runner"
    });
    recorder.mark("runtime_request_received", {
      clock: { epochMs: 5_000, monotonicMs: 12, source: "node:fixture" },
      correlation: { requestId: "request-2" }
    });
    recordPendingCheckpoints(recorder, root);

    const manifest = JSON.parse(readFileSync(recorder.finalize(), "utf8"));
    expect(manifest.checkpoints.map((checkpoint: { id: string }) => checkpoint.id)).toEqual([
      "gui_ready",
      "send_clicked",
      "draft_context_ready",
      "runtime_request_dispatched",
      "first_output_visible",
      "turn_settled"
    ]);
    expect(manifest.marks[0].clock).toMatchObject({
      epochMs: 5_000,
      monotonicMs: 12,
      source: "node:fixture"
    });
    expect(manifest.marks[0]).not.toHaveProperty("deltaMs");
  });

  it("writes a bounded partial failure manifest after an ordering error", () => {
    const root = temporaryRoot();
    const recorder = new JourneyRecorder(run(root, "ready-send"));
    const screenshot = fixtureScreenshot(root, 1, "gui-ready");
    recorder.checkpoint("gui_ready", { screenshot });

    let failure: unknown;
    try {
      recorder.checkpoint("send_clicked");
    } catch (error) {
      failure = error;
    }
    const manifest = JSON.parse(readFileSync(recorder.finalize({ error: failure }), "utf8"));
    expect(manifest.outcome).toBe("failed");
    expect(manifest.checkpoints).toHaveLength(1);
    expect(manifest.failure.message).toContain("expected draft_context_ready");
  });

  it("does not persist arbitrary exception text in a failed manifest", () => {
    const root = temporaryRoot();
    const recorder = new JourneyRecorder(run(root, "ready-send"));
    const manifest = JSON.parse(readFileSync(recorder.finalize({
      error: new Error("provider failed while handling private prompt words")
    }), "utf8"));

    expect(manifest.failure.name).toBe("Error");
    expect(manifest.failure.message).toBe("Journey failed; inspect the automation trace and logs.");
    expect(JSON.stringify(manifest)).not.toContain("private prompt words");
  });

  it("rejects a passed visual manifest when any checkpoint lacks a screenshot", () => {
    const root = temporaryRoot();
    const recorder = new JourneyRecorder(run(root, "ready-send"));
    recorder.checkpoint("gui_ready", { screenshot: fixtureScreenshot(root, 1, "gui-ready") });
    recorder.checkpoint("draft_context_ready");
    recorder.checkpoint("send_clicked");
    recorder.checkpoint("runtime_request_dispatched");
    recorder.checkpoint("first_output_visible");
    recorder.checkpoint("turn_settled");

    expect(() => recorder.finalize()).toThrow(/visual journey checkpoint .* screenshot/);
  });

  it("accepts a complete screenshot-free profile and rejects visual capture contamination", () => {
    const root = temporaryRoot();
    const recorder = new JourneyRecorder({ ...run(root, "ready-send"), pass: "profile" });
    for (const id of [
      "gui_ready",
      "draft_context_ready",
      "send_clicked",
      "runtime_request_dispatched",
      "first_output_visible",
      "turn_settled"
    ] as const) {
      recorder.checkpoint(id);
    }
    const manifest = JSON.parse(readFileSync(recorder.finalize(), "utf8"));
    expect(manifest.run.pass).toBe("profile");
    expect(manifest.checkpoints.every((checkpoint: { screenshot?: unknown }) => !checkpoint.screenshot))
      .toBe(true);

    const contaminatedRoot = temporaryRoot();
    const contaminated = new JourneyRecorder({
      ...run(contaminatedRoot, "ready-send"),
      pass: "profile"
    });
    contaminated.checkpoint("gui_ready", {
      screenshot: fixtureScreenshot(contaminatedRoot, 1, "gui-ready")
    });
    contaminated.checkpoint("draft_context_ready");
    contaminated.checkpoint("send_clicked");
    contaminated.checkpoint("runtime_request_dispatched");
    contaminated.checkpoint("first_output_visible");
    contaminated.checkpoint("turn_settled");
    expect(() => contaminated.finalize()).toThrow(/profile journey checkpoint .* screenshot/);
  });

  it("requires trace artifacts to exist below the journey root", () => {
    const root = temporaryRoot();
    const outside = temporaryRoot();
    const recorder = completeProfileRecorder(root);
    const outsideTrace = path.join(outside, "trace.zip");
    writeFileSync(outsideTrace, "trace");

    expect(() => recorder.finalize({ trace: outsideTrace })).toThrow(/below/);

    const valid = completeProfileRecorder(root);
    const trace = path.join(root, "trace.zip");
    writeFileSync(trace, "trace");
    const manifest = JSON.parse(readFileSync(valid.finalize({ trace }), "utf8"));
    expect(manifest.trace.path).toBe("trace.zip");
  });
});

function temporaryRoot(): string {
  const root = mkdtempSync(path.join(tmpdir(), "psychevo-journey-recorder-"));
  roots.push(root);
  return root;
}

function run(artifactRoot: string, scenario: JourneyRun["scenario"]): JourneyRun {
  return {
    adapter: "native",
    artifactRoot,
    environment: { platform: "test", viewport: "1440x960" },
    pass: "visual",
    runId: "native-ready-send-visual",
    scenario,
    surface: "workbench"
  };
}

function sequenceClock(): () => { epochMs: number; monotonicMs: number } {
  let value = 0;
  return () => ({ epochMs: 1_000 + value, monotonicMs: value++ });
}

function completeProfileRecorder(root: string): JourneyRecorder {
  const recorder = new JourneyRecorder({ ...run(root, "ready-send"), pass: "profile" });
  for (const id of [
    "gui_ready",
    "draft_context_ready",
    "send_clicked",
    "runtime_request_dispatched",
    "first_output_visible",
    "turn_settled"
  ] as const) {
    recorder.checkpoint(id);
  }
  return recorder;
}

function recordReadyCheckpoints(recorder: JourneyRecorder, root: string): void {
  recorder.checkpoint("gui_ready", { screenshot: fixtureScreenshot(root, 1, "gui-ready") });
  recorder.checkpoint("draft_context_ready", { screenshot: fixtureScreenshot(root, 2, "draft-context-ready") });
  recorder.checkpoint("send_clicked", { screenshot: fixtureScreenshot(root, 3, "send-clicked") });
  recorder.checkpoint("runtime_request_dispatched", {
    correlation: { turnId: "turn-1" },
    screenshot: fixtureScreenshot(root, 4, "runtime-request-dispatched")
  });
  recorder.checkpoint("first_output_visible", {
    response: "secret response",
    screenshot: fixtureScreenshot(root, 5, "first-output-visible")
  });
  recorder.checkpoint("turn_settled", { screenshot: fixtureScreenshot(root, 6, "turn-settled") });
}

function recordPendingCheckpoints(recorder: JourneyRecorder, root: string): void {
  recorder.checkpoint("gui_ready", { screenshot: fixtureScreenshot(root, 1, "gui-ready") });
  recorder.checkpoint("send_clicked", { screenshot: fixtureScreenshot(root, 2, "send-clicked") });
  recorder.checkpoint("draft_context_ready", { screenshot: fixtureScreenshot(root, 3, "draft-context-ready") });
  recorder.checkpoint("runtime_request_dispatched", {
    screenshot: fixtureScreenshot(root, 4, "runtime-request-dispatched")
  });
  recorder.checkpoint("first_output_visible", {
    screenshot: fixtureScreenshot(root, 5, "first-output-visible")
  });
  recorder.checkpoint("turn_settled", { screenshot: fixtureScreenshot(root, 6, "turn-settled") });
}

function fixtureScreenshot(root: string, sequence: number, id: string): string {
  const screenshot = path.join(root, `${String(sequence).padStart(2, "0")}-${id}.png`);
  writeFileSync(screenshot, "fixture");
  return screenshot;
}
