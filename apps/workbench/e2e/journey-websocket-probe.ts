import type { Page } from "@playwright/test";

export interface BrowserJourneyMark {
  clock: "browser:performance";
  data: Record<string, unknown>;
  epochMs: number;
  id: string;
  monotonicMs: number;
  sampleIndex: number;
  sequence: number;
}

export interface BrowserJourneyRunnerMark {
  browserSequence: number;
  id: string;
  runnerMonotonicMs: number;
  sampleIndex: number;
}

type BrowserJourneyProbe = {
  beginSample(sampleIndex: number): void;
  holdNextDraftOpen(): void;
  holdNextTurnStart(): void;
  marks: BrowserJourneyMark[];
  pendingRequestCount(sampleIndex: number): number;
  record(id: string, data?: Record<string, unknown>): BrowserJourneyMark;
  releaseDraftOpen(): void;
  releaseTurnStart(): void;
  resetSample(): void;
};

declare global {
  interface Window {
    __psychevoJourneyDiagnosticsEnabled?: boolean;
    __psychevoJourneyProbe?: BrowserJourneyProbe;
    __psychevoJourneyRunnerMark?: (mark: {
      browserSequence: number;
      id: string;
      sampleIndex: number;
    }) => Promise<void>;
  }
}

const runnerMarksByPage = new WeakMap<Page, BrowserJourneyRunnerMark[]>();

/** Installs a content-free, browser-clock probe before the Workbench bundle runs. */
export async function installJourneyWebSocketProbe(
  page: Page,
  options: { holdInitialDraft?: boolean } = {}
): Promise<void> {
  const runnerMarks: BrowserJourneyRunnerMark[] = [];
  runnerMarksByPage.set(page, runnerMarks);
  await page.exposeBinding("__psychevoJourneyRunnerMark", ({ page: sourcePage }, mark: {
    browserSequence: number;
    id: string;
    sampleIndex: number;
  }) => {
    const target = runnerMarksByPage.get(sourcePage);
    target?.push({
      ...mark,
      runnerMonotonicMs: Number(process.hrtime.bigint()) / 1_000_000
    });
  });
  await page.addInitScript(({ holdInitialDraft }) => {
    let sampleIndex = Number(new URL(window.location.href).searchParams.get("journeySample") ?? "0");
    let draftResponsesToHold = holdInitialDraft ? 1 : 0;
    let sequence = 0;
    let turnStartsToHold = 0;
    const marks: BrowserJourneyMark[] = [];
    const draftRequestIds = new Set<string>();
    const requestMethods = new Map<string, string>();
    const requestSamples = new Map<string, number>();
    const pendingRequestIds = new Set<string>();
    const refreshSamples = new Map<number, number>();
    const firstNonEmptyAssistantTurns = new Set<string>();
    const turnSamples = new Map<string, number>();
    const heldDeliveries: Array<{ deliver(): void; requestId: string }> = [];
    const heldTurnStarts: Array<{ data: string; requestId: string; socket: WebSocket }> = [];

    function record(
      id: string,
      data: Record<string, unknown> = {},
      targetSampleIndex = sampleIndex
    ): BrowserJourneyMark {
      const monotonicMs = performance.now();
      const mark: BrowserJourneyMark = {
        clock: "browser:performance",
        data,
        epochMs: performance.timeOrigin + monotonicMs,
        id,
        monotonicMs,
        sampleIndex: Number.isFinite(targetSampleIndex) ? targetSampleIndex : 0,
        sequence: ++sequence
      };
      marks.push(mark);
      void window.__psychevoJourneyRunnerMark?.({
        browserSequence: mark.sequence,
        id: mark.id,
        sampleIndex: mark.sampleIndex
      });
      return mark;
    }

    function parseMessage(value: unknown): Record<string, unknown> | null {
      if (typeof value !== "string") return null;
      try {
        const parsed = JSON.parse(value) as unknown;
        return parsed && typeof parsed === "object" ? parsed as Record<string, unknown> : null;
      } catch {
        return null;
      }
    }

    const observedPerformanceMarks = new Set<string>();
    function retainPerformanceMark(name: string, startTime: number): void {
      if (observedPerformanceMarks.has(name)) return;
      observedPerformanceMarks.add(name);
      marks.push({
        clock: "browser:performance",
        data: { performanceMark: name },
        epochMs: performance.timeOrigin + startTime,
        id: name.replace(/^psychevo:/, ""),
        monotonicMs: startTime,
        sampleIndex: Number.isFinite(sampleIndex) ? sampleIndex : 0,
        sequence: ++sequence
      });
    }

    const performanceObserver = new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        if (entry.entryType === "mark" && (
          entry.name === "psychevo:gui_ready"
          || entry.name === "psychevo:draft_context_ready"
        )) {
          retainPerformanceMark(entry.name, entry.startTime);
        }
      }
    });
    performanceObserver.observe({ entryTypes: ["mark"] });

    try {
      const longTaskObserver = new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          record("browser_long_task", {
            durationMs: entry.duration,
            startTimeMs: entry.startTime
          });
        }
      });
      longTaskObserver.observe({ entryTypes: ["longtask"] });
    } catch {
      // Chromium exposes Long Tasks; unsupported engines keep the field empty.
    }

    document.addEventListener("submit", (event) => {
      const target = event.target;
      if (target instanceof Element && target.matches("form.pevo-composer")) {
        record("send_clicked");
      }
    }, true);

    let firstOutputScheduled = false;
    let optimisticPromptScheduled = false;
    let sendFeedbackScheduled = false;
    let turnSettledScheduled = false;
    let assistantCountAtSampleStart = 0;
    let userCountAtSampleStart = 0;
    let lastComposerState: string | null = null;
    function nonEmptyMessageCount(selector: string): number {
      return Array.from(document.querySelectorAll(selector))
        .filter((element) => Boolean(element.textContent?.trim())).length;
    }
    function assistantTextCount(): number {
      return nonEmptyMessageCount('.pevo-message.is-assistant[data-block-kind="text"]');
    }
    function entryHasNonEmptyAssistantText(entry: Record<string, unknown> | null): boolean {
      if (entry?.role !== "assistant" || !Array.isArray(entry.blocks)) return false;
      return entry.blocks.some((value) => {
        if (!value || typeof value !== "object" || Array.isArray(value)) return false;
        const block = value as Record<string, unknown>;
        return block.kind === "text"
          && [block.body, block.preview, block.detail].some((text) => (
            typeof text === "string" && Boolean(text.trim())
          ));
      });
    }
    function inspectPaintedState(): void {
      const shell = document.querySelector(".appShell");
      const composer = document.querySelector<HTMLTextAreaElement>('.pevo-composer textarea');
      if (composer && !marks.some((mark) => mark.id === "composer_shell_dom_committed")) {
        record("composer_shell_dom_committed");
      }
      if (
        composer
        && !composer.disabled
        && shell?.getAttribute("data-gateway-status") === "connected"
        && !marks.some((mark) => mark.id === "gui_ready_dom_committed")
      ) {
        record("gui_ready_dom_committed");
      }
      const composerState = document.querySelector(".appShell")?.getAttribute("data-composer-state") ?? null;
      if (lastComposerState === "opening" && composerState && composerState !== "opening") {
        record("draft_context_applied", { composerState });
      }
      if (
        composerState === "ready"
        && !marks.some((mark) => mark.id === "draft_context_ready_dom_committed")
      ) {
        const branchLabel = document.querySelector<HTMLButtonElement>(
          'button[aria-label="Git branch"]'
        )?.textContent?.trim();
        record("draft_context_ready_dom_committed", {
          currentBranchVisible: Boolean(branchLabel && branchLabel !== "Git branch")
        });
      }
      lastComposerState = composerState;
      const assistantCount = assistantTextCount();
      if (assistantCount > assistantCountAtSampleStart && !firstOutputScheduled) {
        firstOutputScheduled = true;
        const data = {
          turnState: document.querySelector(".appShell")?.getAttribute("data-turn-state") ?? null
        };
        record("first_output_surface_committed", data);
        record("first_output_visible", data);
      }
      const optimistic = nonEmptyMessageCount(".pevo-message.is-user") > userCountAtSampleStart;
      if (optimistic && !optimisticPromptScheduled) {
        optimisticPromptScheduled = true;
        record("optimistic_prompt_surface_committed");
      }
      const turnState = document.querySelector(".appShell")?.getAttribute("data-turn-state") ?? null;
      const elapsedVisible = Boolean(document.querySelector(".pevo-composerTurnStatus"));
      if (optimistic && turnState === "running" && elapsedVisible && !sendFeedbackScheduled) {
        sendFeedbackScheduled = true;
        record("send_feedback_surface_committed");
      }
      const completionReceived = marks.some((mark) => (
        mark.id === "turn_completed_received" && mark.sampleIndex === sampleIndex
      ));
      if (turnState === "idle" && completionReceived && !turnSettledScheduled) {
        turnSettledScheduled = true;
        record("turn_settled_surface_committed");
        record("turn_settled_visible");
      }
    }
    new MutationObserver(inspectPaintedState).observe(document, {
      attributes: true,
      childList: true,
      subtree: true
    });

    window.addEventListener("psychevo:journey-diagnostic", (event) => {
      const detail = (event as CustomEvent<unknown>).detail;
      if (!detail || typeof detail !== "object" || Array.isArray(detail)) return;
      const diagnostic = detail as { data?: unknown; id?: unknown };
      if (typeof diagnostic.id !== "string") return;
      const data = diagnostic.data && typeof diagnostic.data === "object" && !Array.isArray(diagnostic.data)
        ? diagnostic.data as Record<string, unknown>
        : {};
      const turnId = typeof data.turnId === "string" ? data.turnId : null;
      const refreshSequence = typeof data.refreshSequence === "number"
        ? data.refreshSequence
        : null;
      let diagnosticSampleIndex = turnId === null
        ? sampleIndex
        : turnSamples.get(turnId) ?? sampleIndex;
      if (refreshSequence !== null) {
        if (diagnostic.id === "settle_refresh_scheduled") {
          refreshSamples.set(refreshSequence, sampleIndex);
        }
        diagnosticSampleIndex = refreshSamples.get(refreshSequence) ?? diagnosticSampleIndex;
      }
      record(diagnostic.id, data, diagnosticSampleIndex);
    });

    const originalSend = WebSocket.prototype.send;
    WebSocket.prototype.send = function send(
      this: WebSocket,
      data: string | ArrayBufferLike | Blob | ArrayBufferView
    ): void {
      if (typeof data === "string") {
        const message = parseMessage(data);
        const method = typeof message?.method === "string" ? message.method : null;
        const requestId = message?.id == null ? null : String(message.id);
          if (method && requestId) {
            requestMethods.set(requestId, method);
            requestSamples.set(requestId, sampleIndex);
            pendingRequestIds.add(requestId);
          if ((method === "thread/draft/open" || method === "thread/draft/prepare") && draftResponsesToHold > 0) {
            draftResponsesToHold -= 1;
            draftRequestIds.add(requestId);
            record("draft_response_armed", { requestId });
          }
          if (method === "turn/start" && turnStartsToHold > 0) {
            turnStartsToHold -= 1;
            heldTurnStarts.push({ data, requestId, socket: this });
            record("turn_start_held", { requestId });
            return;
          }
          record("rpc_request_sent", { method, requestId });
        }
      }
      Reflect.apply(originalSend, this, [data]);
    };

    const originalAddEventListener = WebSocket.prototype.addEventListener;
    WebSocket.prototype.addEventListener = function addEventListener(
      this: WebSocket,
      type: string,
      listener: EventListenerOrEventListenerObject | null,
      options?: boolean | AddEventListenerOptions
    ): void {
      if (type !== "message" || listener === null) {
        Reflect.apply(originalAddEventListener, this, [type, listener, options]);
        return;
      }
      const socket = this;
      const wrapped: EventListener = (rawEvent) => {
        const event = rawEvent as MessageEvent;
        const message = parseMessage(event.data);
        const requestId = message?.id == null ? null : String(message.id);
        const params = message?.params && typeof message.params === "object"
          ? message.params as Record<string, unknown>
          : null;
        const eventType = typeof params?.type === "string" ? params.type : null;
        const deliver = () => {
          if (typeof listener === "function") {
            listener.call(socket, event);
          } else {
            listener.handleEvent(event);
          }
        };

        if (message?.method === "gateway/event" && eventType) {
          const eventTurnId = typeof params?.turnId === "string" ? params.turnId : null;
          if (eventTurnId && !turnSamples.has(eventTurnId)) {
            turnSamples.set(eventTurnId, sampleIndex);
          }
          record("gateway_event_received", {
            eventType,
            threadId: typeof params?.threadId === "string" ? params.threadId : null,
            turnId: eventTurnId
          });
          const entry = params?.entry && typeof params.entry === "object"
            ? params.entry as Record<string, unknown>
            : null;
          if (
            entry?.role === "assistant"
            && (eventType === "entryStarted" || eventType === "entryUpdated" || eventType === "entryCompleted")
          ) {
            const hasVisibleAssistantText = entryHasNonEmptyAssistantText(entry);
            record("gateway_assistant_event_received", { eventType, hasVisibleAssistantText });
            if (
              hasVisibleAssistantText
              && eventTurnId
              && !firstNonEmptyAssistantTurns.has(eventTurnId)
            ) {
              firstNonEmptyAssistantTurns.add(eventTurnId);
              record("gateway_first_nonempty_assistant_received", { eventType });
            }
          }
          if (eventType === "turnCompleted") {
            record("turn_completed_received", { transport: "websocket" });
          }
        }
        if (requestId) {
          const result = message?.result && typeof message.result === "object"
            ? message.result as Record<string, unknown>
            : null;
          const originSampleIndex = requestSamples.get(requestId) ?? sampleIndex;
          pendingRequestIds.delete(requestId);
          record("rpc_response_arrived", {
            method: requestMethods.get(requestId) ?? null,
            observedSampleIndex: sampleIndex,
            originSampleIndex,
            requestId,
            threadId: typeof result?.threadId === "string" ? result.threadId : null,
            turnId: typeof result?.turnId === "string" ? result.turnId : null
          }, originSampleIndex);
        }
        if (requestId && draftRequestIds.has(requestId)) {
          heldDeliveries.push({ deliver, requestId });
          record("draft_response_held", { requestId });
          return;
        }
        deliver();
      };
      Reflect.apply(originalAddEventListener, this, [type, wrapped, options]);
    } as typeof WebSocket.prototype.addEventListener;

    window.__psychevoJourneyProbe = {
      beginSample(nextSampleIndex) {
        sampleIndex = nextSampleIndex;
        firstOutputScheduled = false;
        optimisticPromptScheduled = false;
        sendFeedbackScheduled = false;
        turnSettledScheduled = false;
        assistantCountAtSampleStart = assistantTextCount();
        userCountAtSampleStart = nonEmptyMessageCount(".pevo-message.is-user");
        lastComposerState = document.querySelector(".appShell")?.getAttribute("data-composer-state") ?? null;
        record("sample_began");
      },
      holdNextDraftOpen() {
        draftResponsesToHold += 1;
      },
      holdNextTurnStart() {
        turnStartsToHold += 1;
      },
      marks,
      pendingRequestCount(targetSampleIndex) {
        return Array.from(pendingRequestIds).filter((requestId) => (
          requestSamples.get(requestId) === targetSampleIndex
        )).length;
      },
      record,
      releaseDraftOpen() {
        for (const held of heldDeliveries.splice(0)) {
          draftRequestIds.delete(held.requestId);
          record("draft_response_released", { requestId: held.requestId });
          held.deliver();
        }
      },
      releaseTurnStart() {
        for (const held of heldTurnStarts.splice(0)) {
          record("rpc_request_sent", { method: "turn/start", requestId: held.requestId });
          Reflect.apply(originalSend, held.socket, [held.data]);
        }
      },
      resetSample() {
        firstOutputScheduled = false;
        optimisticPromptScheduled = false;
        sendFeedbackScheduled = false;
        turnSettledScheduled = false;
        assistantCountAtSampleStart = assistantTextCount();
        userCountAtSampleStart = nonEmptyMessageCount(".pevo-message.is-user");
        lastComposerState = document.querySelector(".appShell")?.getAttribute("data-composer-state") ?? null;
      }
    };
    window.__psychevoJourneyDiagnosticsEnabled = true;
    record("navigation_started");
  }, options);
}

export async function beginBrowserJourneySample(page: Page, sampleIndex: number): Promise<void> {
  await page.evaluate((nextSampleIndex) => {
    const probe = window.__psychevoJourneyProbe;
    if (!probe) throw new Error("Workbench journey probe is unavailable");
    probe.beginSample(nextSampleIndex);
  }, sampleIndex);
}

export async function resetBrowserJourneySample(page: Page): Promise<void> {
  await page.evaluate(() => window.__psychevoJourneyProbe?.resetSample());
}

export async function holdNextDraftOpen(page: Page): Promise<void> {
  await page.evaluate(() => window.__psychevoJourneyProbe?.holdNextDraftOpen());
}

export async function holdNextTurnStart(page: Page): Promise<void> {
  await page.evaluate(() => window.__psychevoJourneyProbe?.holdNextTurnStart());
}

export async function releaseDraftOpen(page: Page): Promise<void> {
  await page.evaluate(() => window.__psychevoJourneyProbe?.releaseDraftOpen());
}

export async function releaseTurnStart(page: Page): Promise<void> {
  await page.evaluate(() => window.__psychevoJourneyProbe?.releaseTurnStart());
}

export async function recordBrowserJourneyMark(
  page: Page,
  id: string,
  data: Record<string, unknown> = {}
): Promise<BrowserJourneyMark> {
  return page.evaluate(({ data, id }) => {
    const probe = window.__psychevoJourneyProbe;
    if (!probe) throw new Error("Workbench journey probe is unavailable");
    return probe.record(id, data);
  }, { data, id });
}

export async function readBrowserJourneyMarks(page: Page): Promise<BrowserJourneyMark[]> {
  return page.evaluate(() => [...(window.__psychevoJourneyProbe?.marks ?? [])]);
}

export function readBrowserJourneyRunnerMarks(page: Page): BrowserJourneyRunnerMark[] {
  return [...(runnerMarksByPage.get(page) ?? [])];
}

export async function waitForBrowserJourneyMark(
  page: Page,
  id: string,
  timeoutMs = 30_000,
  sampleIndex?: number
): Promise<BrowserJourneyMark> {
  await page.waitForFunction(({ markId, sample }) => (
    window.__psychevoJourneyProbe?.marks.some((mark) => (
      mark.id === markId && (sample === undefined || mark.sampleIndex === sample)
    )) === true
  ), { markId: id, sample: sampleIndex }, { timeout: timeoutMs });
  const marks = await readBrowserJourneyMarks(page);
  const mark = [...marks].reverse().find((candidate) => (
    candidate.id === id && (sampleIndex === undefined || candidate.sampleIndex === sampleIndex)
  ));
  if (!mark) throw new Error(`missing browser journey mark ${id}`);
  return mark;
}

export async function waitForBrowserJourneyRunnerMark(
  page: Page,
  id: string,
  sampleIndex: number,
  timeoutMs = 30_000
): Promise<BrowserJourneyRunnerMark> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const mark = readBrowserJourneyRunnerMarks(page).find((candidate) => (
      candidate.id === id && candidate.sampleIndex === sampleIndex
    ));
    if (mark) return mark;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  throw new Error(`missing runner-observed browser journey mark ${id} for sample ${sampleIndex}`);
}

export async function waitForBrowserJourneyRequestsSettled(
  page: Page,
  sampleIndex: number,
  timeoutMs = 30_000
): Promise<void> {
  await page.waitForFunction((targetSampleIndex) => (
    window.__psychevoJourneyProbe?.pendingRequestCount(targetSampleIndex) === 0
  ), sampleIndex, { timeout: timeoutMs });
}

export async function afterTwoPaints(page: Page): Promise<void> {
  await page.evaluate(() => new Promise<void>((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
  }));
}
