import { mkdirSync, writeFileSync } from "node:fs";
import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import path from "node:path";
import { expect, type Locator, type Page, type TestInfo } from "@playwright/test";
import { analyzeTranscriptRuntimeRows, type TranscriptRuntimeRowSample } from "../src/transcriptRuntimeAnalyzer";
import { repoRoot } from "./harness";

export { analyzeTranscriptRuntimeRows };
export type { TranscriptRuntimeRowSample };

export const LIVE_TRANSLATE_SUBAGENT_PROMPT = "使用 translate agent 演示简单的中译英和英译中";
export const CHANNELS_VISUAL_CONFIG = `

[[channels.connections]]
id = "wechat"
channel = "wechat"
enabled = true
label = "WeChat"
transport = "polling"
model = "lmstudio/noop"
credential_env = "WECHAT_BOT_TOKEN"
account_env = "WECHAT_ACCOUNT_ID"
allow_users = ["wx-user"]

[[channels.connections]]
id = "ops-lark"
channel = "lark"
enabled = false
label = "Ops Lark"
transport = "long_connection"
credential_env = "LARK_APP_SECRET"
app_id_env = "LARK_APP_ID"
allow_groups = []
`;
export const CHANNELS_VISUAL_ENV = [
  "WECHAT_BOT_TOKEN=test-wechat-token",
  "WECHAT_ACCOUNT_ID=test-wechat-account",
  "LARK_APP_ID=test-lark-app"
].join("\n");
const CHANNELS_SCREENSHOT_DIR = path.join(repoRoot, ".local/playwright/screenshots/channels");

export async function startAutomationToolMockProvider(): Promise<{
  baseUrl: string;
  close(): Promise<void>;
  requests: unknown[];
}> {
  const requests: unknown[] = [];
  const server = createServer(async (request, response) => {
    if (request.method !== "POST" || request.url !== "/v1/chat/completions") {
      response.writeHead(404);
      response.end();
      return;
    }

    const body = await readRequestBody(request);
    const payload = JSON.parse(body) as {
      messages?: Array<{ role?: string }>;
      tools?: Array<{ function?: { name?: string }; name?: string }>;
    };
    requests.push(payload);
    const hasAutomationTool = payload.tools?.some((tool) => tool.name === "automation" || tool.function?.name === "automation") ?? false;
    const hasToolResult = payload.messages?.some((message) => message.role === "tool") ?? false;

    if (hasAutomationTool && !hasToolResult) {
      sendSse(response, [
        {
          id: "mock-tool-call",
          model: "automation",
          choices: [
            {
              delta: {
                tool_calls: [
                  {
                    index: 0,
                    id: "call_automation_tip",
                    function: {
                      name: "automation",
                      arguments: JSON.stringify({
                        action: "create",
                        title: "pevo-deterministic-engineering-tip",
                        prompt: "每次发送一条最有价值的软件工程 tip。",
                        schedule: { kind: "interval", everyMinutes: 1 }
                      })
                    }
                  }
                ]
              },
              finish_reason: "tool_calls"
            }
          ]
        }
      ]);
      return;
    }

    if (hasToolResult) {
      sendSse(response, [
        {
          id: "mock-final",
          model: "automation",
          choices: [
            {
              delta: { content: "已创建 pevo-deterministic-engineering-tip。" },
              finish_reason: "stop"
            }
          ]
        }
      ]);
      return;
    }

    sendSse(response, [
      {
        id: "mock-title",
        model: "automation",
        choices: [
          {
            delta: { content: "Deterministic Automation Tip" },
            finish_reason: "stop"
          }
        ]
      }
    ]);
  });

  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address() as AddressInfo;
  return {
    baseUrl: `http://127.0.0.1:${address.port}/v1`,
    close: () => new Promise<void>((resolve) => server.close(() => resolve())),
    requests
  };
}

function readRequestBody(request: IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    request.on("data", (chunk: Buffer) => chunks.push(chunk));
    request.on("end", () => resolve(Buffer.concat(chunks).toString("utf8")));
    request.on("error", reject);
  });
}

function sendSse(response: ServerResponse, chunks: unknown[]) {
  response.writeHead(200, {
    "content-type": "text/event-stream",
    connection: "close"
  });
  for (const chunk of chunks) {
    response.write(`data: ${JSON.stringify(chunk)}\n\n`);
  }
  response.end("data: [DONE]\n\n");
}

export function ensureLiveSubagentCwd(contextCwd: string | undefined): string {
  const cwd = path.resolve(contextCwd ?? path.join(repoRoot, ".local/.psychevo-dev/live-validation/gui-cwd"));
  const agentDir = path.join(cwd, ".psychevo", "agents");
  mkdirSync(agentDir, { recursive: true });
  writeFileSync(
    path.join(agentDir, "translate.md"),
    `---
description: Translate between Chinese and English.
---
Translate the assigned text between Chinese and English. Return only the translation and direction.
`
  );
  return cwd;
}

export function ensureLiveAutomationCwd(contextCwd: string | undefined): string {
  const cwd = path.resolve(contextCwd ?? path.join(repoRoot, ".local/.psychevo-dev/live-validation/gui-automation-cwd"));
  mkdirSync(cwd, { recursive: true });
  writeFileSync(path.join(cwd, "README.md"), "Live GUI automation validation workspace.\n");
  return cwd;
}

export async function assertLeftNavigationSectionAlignment(page: Page) {
  const actionIcon = page.locator(".leftActions button").first().locator("svg");
  const actionLabel = page.locator(".leftActions button").first().locator(".pevo-actionButtonLabel");
  const pinnedIcon = page.locator(".leftPinnedPanel header svg");
  const pinnedLabel = page.locator(".leftPinnedPanel header span");
  const sessionsIcon = page.locator(".pevo-sessionsHeader .pevo-titleLine svg");
  const sessionsLabel = page.locator(".pevo-sessionsHeader h2");
  const [actionIconBox, actionLabelBox, pinnedIconBox, pinnedLabelBox, sessionsIconBox, sessionsLabelBox] =
    await Promise.all([
      actionIcon.boundingBox(),
      actionLabel.boundingBox(),
      pinnedIcon.boundingBox(),
      pinnedLabel.boundingBox(),
      sessionsIcon.boundingBox(),
      sessionsLabel.boundingBox()
    ]);

  expect(actionIconBox).not.toBeNull();
  expect(actionLabelBox).not.toBeNull();
  expect(pinnedIconBox).not.toBeNull();
  expect(pinnedLabelBox).not.toBeNull();
  expect(sessionsIconBox).not.toBeNull();
  expect(sessionsLabelBox).not.toBeNull();
  expect(Math.abs(pinnedIconBox!.x - actionIconBox!.x)).toBeLessThanOrEqual(1);
  expect(Math.abs(sessionsIconBox!.x - actionIconBox!.x)).toBeLessThanOrEqual(1);
  expect(Math.abs(pinnedLabelBox!.x - actionLabelBox!.x)).toBeLessThanOrEqual(1);
  expect(Math.abs(sessionsLabelBox!.x - actionLabelBox!.x)).toBeLessThanOrEqual(1);

  const [actionFont, pinnedFont, sessionsFont] = await Promise.all([
    actionLabel.evaluate(fontSignature),
    pinnedLabel.evaluate(fontSignature),
    sessionsLabel.evaluate(fontSignature)
  ]);
  expect(pinnedFont).toEqual(actionFont);
  expect(sessionsFont).toEqual(actionFont);
}

function fontSignature(element: Element) {
  const style = getComputedStyle(element);
  return {
    fontSize: style.fontSize,
    fontWeight: style.fontWeight
  };
}

export async function captureWorkbench(page: Page, testInfo: TestInfo, label: string) {
  await page.screenshot({
    fullPage: true,
    path: testInfo.outputPath(`${label}-${testInfo.project.name}.png`)
  });
}

export async function expectNoTransientAssistantDuplicateDuring(
  page: Page,
  testInfo: TestInfo,
  rows: Locator,
  label: string,
  timeoutMs: number,
  stableAfterVisibleMs: number
) {
  const deadline = Date.now() + timeoutMs;
  let stableDeadline: number | null = null;
  const samples: Array<{
    allRows: TranscriptRowSample[];
    elapsedMs: number;
    matchingRows: TranscriptRowSample[];
  }> = [];
  while (Date.now() < deadline) {
    const currentRows = await transcriptRowSamples(rows);
    const allRows = await allTranscriptRowSamples(page);
    const elapsedMs = timeoutMs - Math.max(0, deadline - Date.now());
    samples.push({ allRows, elapsedMs, matchingRows: currentRows });
    if (allRows.some((row) => row.text.includes("current thread is not available"))) {
      await captureWorkbench(page, testInfo, `${label}-current-thread-unavailable`);
      await testInfo.attach(`${label}-current-thread-unavailable-rows.json`, {
        body: JSON.stringify(samples, null, 2),
        contentType: "application/json"
      });
      throw new Error(`${label}: automation tool reported missing current thread: ${JSON.stringify(allRows, null, 2)}`);
    }
    if (currentRows.length > 1) {
      await captureWorkbench(page, testInfo, `${label}-duplicate`);
      await testInfo.attach(`${label}-duplicate-rows.json`, {
        body: JSON.stringify(samples, null, 2),
        contentType: "application/json"
      });
      throw new Error(`${label}: duplicate assistant rows appeared transiently: ${JSON.stringify(currentRows, null, 2)}`);
    }
    if (currentRows.length === 1) {
      stableDeadline ??= Date.now() + stableAfterVisibleMs;
      if (Date.now() >= stableDeadline) {
        await expect(rows).toHaveCount(1);
        return;
      }
    } else {
      stableDeadline = null;
    }
    await page.waitForTimeout(250);
  }
  await testInfo.attach(`${label}-rows-timeout.json`, {
    body: JSON.stringify(samples, null, 2),
    contentType: "application/json"
  });
  throw new Error(`${label}: assistant row did not become visible within ${timeoutMs}ms`);
}

type TranscriptRowSample = TranscriptRuntimeRowSample;

async function transcriptRowSamples(rows: Locator): Promise<TranscriptRowSample[]> {
  return rows.evaluateAll((elements) => {
    const rowHeader = (element: Element): string => {
      const line = element.querySelector(".pevo-evidenceLine");
      return (line?.textContent ?? element.querySelector("button")?.textContent ?? "").replace(/\s+/g, " ").trim();
    };
    const rowKind = (element: Element): string => {
      const className = element.getAttribute("class") ?? "";
      const blockKind = element.getAttribute("data-block-kind");
      if (blockKind === "reasoning") return "reasoning";
      if (blockKind && blockKind !== "text") return "tool";
      if (className.includes("pevo-message")) {
        return className.includes("is-assistant") ? "assistant" : "prompt";
      }
      if (className.includes("pevo-reasoning")) return "reasoning";
      return "tool";
    };
    const rowRunning = (element: Element): boolean => {
      const className = element.getAttribute("class") ?? "";
      return className.includes("is-running") ||
        className.includes("is-streaming") ||
        className.includes("is-runningTool");
    };
    return elements.map((element) => ({
      blockId: element.getAttribute("data-block-id"),
      entryId: element.getAttribute("data-entry-id"),
      header: rowHeader(element),
      kind: rowKind(element),
      running: rowRunning(element),
      source: element.getAttribute("data-source"),
      status: element.querySelector("em")?.textContent?.trim() ?? null,
      text: (element.textContent ?? "").replace(/\s+/g, " ").trim(),
      turnId: element.getAttribute("data-turn-id")
    }));
  });
}

async function allTranscriptRowSamples(page: Page): Promise<TranscriptRowSample[]> {
  return transcriptRowSamples(
    page.locator(".pevo-threadItems .pevo-message, .pevo-threadItems .pevo-evidence")
  );
}

export async function sampleTranscriptRuntimeRows(page: Page): Promise<TranscriptRuntimeRowSample[]> {
  return allTranscriptRowSamples(page);
}

export function assertTranscriptRuntimeRowsHealthy(rows: TranscriptRuntimeRowSample[], label: string) {
  const analysis = analyzeTranscriptRuntimeRows(rows);
  if (analysis.errors.length > 0) {
    throw new Error(`${label}: transcript runtime analyzer failed: ${JSON.stringify({ analysis, rows }, null, 2)}`);
  }
}

export async function captureChannelsWorkbench(page: Page, testInfo: TestInfo, label: string) {
  await captureWorkbench(page, testInfo, label);
  mkdirSync(CHANNELS_SCREENSHOT_DIR, { recursive: true });
  await page.screenshot({
    fullPage: true,
    scale: "css",
    path: path.join(CHANNELS_SCREENSHOT_DIR, `${label}.png`)
  });
}

export function sideConversationPanel(page: Page): Locator {
  return page.getByRole("region", { name: /^Side chat$/i });
}

export async function composerBoxMetrics(composer: Locator) {
  return composer.evaluate((element) => {
    const input = element.querySelector(".pevo-composerInput");
    const textarea = element.querySelector("textarea");
    return {
      composer: element.getBoundingClientRect().height,
      composerTop: element.getBoundingClientRect().top,
      input: input?.getBoundingClientRect().height ?? 0,
      inputTop: input?.getBoundingClientRect().top ?? 0,
      textarea: textarea?.getBoundingClientRect().height ?? 0
    };
  });
}

export async function assertNoHorizontalOverflow(page: Page, locator: Locator) {
  const [viewport, result] = await Promise.all([
    page.viewportSize(),
    locator.evaluate((element) => {
      const box = element.getBoundingClientRect();
      return {
        clientWidth: element.clientWidth,
        left: box.left,
        right: box.right,
        scrollWidth: element.scrollWidth
      };
    })
  ]);
  expect(viewport).not.toBeNull();
  expect(result.left).toBeGreaterThanOrEqual(-1);
  expect(result.right).toBeLessThanOrEqual(viewport!.width + 1);
  expect(result.scrollWidth).toBeLessThanOrEqual(result.clientWidth + 1);
}

export async function assertNoPageVerticalOverflow(page: Page) {
  const metrics = await page.evaluate(() => {
    const scrollingElement = document.scrollingElement ?? document.documentElement;
    window.scrollTo(0, scrollingElement.scrollHeight);
    const afterScroll = {
      bodyClientHeight: document.body.clientHeight,
      bodyScrollHeight: document.body.scrollHeight,
      clientHeight: scrollingElement.clientHeight,
      scrollHeight: scrollingElement.scrollHeight,
      scrollTop: scrollingElement.scrollTop,
      scrollY: window.scrollY
    };
    window.scrollTo(0, 0);
    return afterScroll;
  });
  expect(metrics.scrollHeight).toBeLessThanOrEqual(metrics.clientHeight + 1);
  expect(metrics.bodyScrollHeight).toBeLessThanOrEqual(metrics.bodyClientHeight + 1);
  expect(metrics.scrollTop).toBeLessThanOrEqual(1);
  expect(metrics.scrollY).toBeLessThanOrEqual(1);
}

export async function expectSettingsGutterScrollsContent(page: Page, settings: Locator) {
  const content = settings.locator(".settingsContent");
  const inner = settings.locator(".settingsContentInner");
  const metrics = await content.evaluate((element) => ({
    clientHeight: element.clientHeight,
    scrollHeight: element.scrollHeight
  }));
  if (metrics.scrollHeight <= metrics.clientHeight + 1) {
    return;
  }
  await content.evaluate((element) => { element.scrollTop = 0; });
  const [contentBox, innerBox] = await Promise.all([
    content.boundingBox(),
    inner.boundingBox()
  ]);
  expect(contentBox).not.toBeNull();
  expect(innerBox).not.toBeNull();
  const contentRight = contentBox!.x + contentBox!.width;
  const contentBottom = contentBox!.y + contentBox!.height;
  const innerRight = innerBox!.x + innerBox!.width;
  const gutter = Math.max(8, contentRight - innerRight);
  const x = Math.min(contentRight - 12, innerRight + gutter / 2);
  const y = Math.min(contentBottom - 80, contentBox!.y + 220);
  await page.mouse.move(x, y);
  await page.mouse.wheel(0, 520);
  await expect.poll(() => content.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
  await content.evaluate((element) => { element.scrollTop = 0; });
}

export async function expectControlsFitHorizontally(locator: Locator) {
  const clipped = await locator.locator("input, textarea, select, button").evaluateAll((controls) =>
    controls
      .map((control) => {
        const element = control as HTMLElement;
        return {
          label: element.getAttribute("aria-label") ?? element.textContent?.trim() ?? element.tagName,
          clippedX: element.scrollWidth > element.clientWidth + 2
        };
      })
      .filter((item) => item.clippedX)
  );
  expect(clipped).toEqual([]);
}

export async function expectModelRowsFillPopover(popover: Locator) {
  const gaps = await popover.locator(".modelReasoningModelRows .modelReasoningRow").evaluateAll((rows) => (
    rows.map((row) => {
      const element = row as HTMLElement;
      const parent = element.parentElement as HTMLElement | null;
      return parent ? parent.clientWidth - element.clientWidth : 0;
    })
  ));
  expect(Math.max(...gaps)).toBeLessThanOrEqual(8);
}

export async function openPanel(page: Page, isMobile: boolean, name: "History" | "Status" | "Transcript") {
  if (name === "Status") {
    if (isMobile) {
      await page.getByRole("button", { name: "Transcript" }).click();
    }
    await ensureRightInspectorOpen(page);
  }
  if (isMobile) {
    await page.getByRole("button", { name, exact: true }).click();
  }
  if (name === "Status") {
    await expect(page.getByRole("region", { name: "Workspace status" })).toBeVisible();
  }
}

export async function ensureRightInspectorOpen(page: Page) {
  const inspector = page.getByRole("button", { name: "Right inspector" });
  await expect(inspector).toBeVisible();
  if ((await inspector.getAttribute("aria-expanded")) !== "true") {
    await inspector.click();
  }
  await expect(inspector).toHaveAttribute("aria-expanded", "true");
}

export async function injectStructuredToolRows(page: Page) {
  await page.locator(".pevo-threadItems").evaluate((container) => {
    container.innerHTML = `
      <article class="pevo-evidence is-completed is-tool-run" data-block-kind="shell" data-testid="structured-exec-row">
        <button class="pevo-evidenceLine is-singleTitle" type="button">
          <svg width="15" height="15" aria-hidden="true"></svg>
          <code>exec_command python fetch.py</code>
        </button>
        <div class="pevo-toolDetail">
          <section class="pevo-toolSection is-text is-code">
            <h4>Command</h4>
            <pre>python fetch.py</pre>
          </section>
          <section class="pevo-toolSection is-kv">
            <h4>Input</h4>
            <dl><div><dt>cwd</dt><dd>/tmp/project</dd></div></dl>
          </section>
          <section class="pevo-toolSection is-text is-code">
            <h4>Output</h4>
            <pre>first
second</pre>
          </section>
          <section class="pevo-toolSection is-kv">
            <h4>Status</h4>
            <dl><div><dt>exit</dt><dd>0</dd></div></dl>
          </section>
        </div>
      </article>
      <article class="pevo-evidence is-completed is-tool-update" data-block-kind="file" data-testid="structured-write-row">
        <button class="pevo-evidenceLine" type="button">
          <svg width="15" height="15" aria-hidden="true"></svg>
          <code>write feeds/report.md</code>
          <span>34,093 bytes / ok</span>
        </button>
        <div class="pevo-toolDetail">
          <section class="pevo-toolSection is-kv">
            <h4>Input</h4>
            <dl><div><dt>path</dt><dd>feeds/report.md</dd></div></dl>
          </section>
          <section class="pevo-toolSection is-kv">
            <h4>Change</h4>
            <dl><div><dt>bytes</dt><dd>34093</dd></div><div><dt>status</dt><dd>ok</dd></div></dl>
          </section>
        </div>
      </article>
    `;
  });
}
