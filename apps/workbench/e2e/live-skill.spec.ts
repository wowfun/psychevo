import { expect, test, type Page, type TestInfo } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import path from "node:path";
import { repoRoot, startPevoWeb } from "./harness";
import { liveContextFor, screenshotRoot } from "./liveContext";

type DomRow = {
  blockId: string | null;
  code: string | null;
  entryId: string | null;
  hasBody: boolean;
  header: string;
  index: number;
  kind: string;
  overflow: boolean;
  running: boolean;
  source: string | null;
  status: string | null;
  text: string;
  turnId: string | null;
};

type DurableRow = {
  entry_id: string;
  seq: number;
  kind: string;
  title: string | null;
  text: string;
};

const defaultSkillCwd = path.resolve(repoRoot, "../feedgarden");

test.describe("pevo Web live skill validation", () => {
  test("runs x-daily with sampled transcript assertions @live", async ({ page, isMobile }, testInfo) => {
    const context = liveContextFor("web-skill-live");
    if (!context) {
      test.skip(true, "run through cargo xtask live");
      return;
    }
    test.skip(isMobile, "live skill validation runs once on the desktop project");
    const timeoutMs = context.timeoutMs;
    const intervalMs = context.intervalMs;
    test.setTimeout(timeoutMs + 120_000);

    const cwd = context.cwd ?? defaultSkillCwd;
    if (!existsSync(cwd)) {
      throw new Error(`live skill cwd not found: ${cwd}`);
    }
    const prompt = context.prompt ?? "$x-daily";
    const screenshotDir = screenshotRoot(context, "live-skill");
    mkdirSync(screenshotDir, { recursive: true });

    const server = await startPevoWeb({
      live: true,
      model: context.model,
      configPath: context.configPath,
      dbPath: context.dbPath,
      home: context.home,
      pevoBin: context.pevoBin,
      cwd
    });
    let sample = 0;
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      const composer = page.getByPlaceholder("Ask Psychevo...");
      await expect(composer).toBeVisible();
      await composer.fill(prompt);
      await page.getByRole("button", { name: "Send" }).click();
      await captureAndAssert(page, testInfo, server.dbPath, screenshotDir, sample++, "submitted");

      const deadline = Date.now() + timeoutMs;
      let completed = false;
      while (Date.now() < deadline) {
        await page.waitForTimeout(intervalMs);
        await captureAndAssert(page, testInfo, server.dbPath, screenshotDir, sample++, "sample");
        if (await liveSkillCompleted(page)) {
          completed = true;
          break;
        }
      }
      await captureAndAssert(page, testInfo, server.dbPath, screenshotDir, 9999, "final");
      expect(completed).toBe(true);
    } finally {
      await server.stop();
    }
  });
});

async function captureAndAssert(
  page: Page,
  testInfo: TestInfo,
  dbPath: string,
  screenshotDir: string,
  sample: number,
  label: string
) {
  const filename = `${String(sample).padStart(4, "0")}-${label}.png`;
  const screenshotPath = path.join(screenshotDir, filename);
  await page.screenshot({ fullPage: true, path: screenshotPath });
  process.stdout.write(
    `[live-skill] screenshot sample=${sample} label=${label} path=${path.relative(repoRoot, screenshotPath)}\n`
  );
  await assertNoWorkbenchRenderError(page, sample);
  const rows = await transcriptRows(page);
  process.stdout.write(
    `[live-skill] rows sample=${sample} ${JSON.stringify(rows.map((row) => ({
      index: row.index,
      entryId: row.entryId,
      blockId: row.blockId,
      kind: row.kind,
      source: row.source,
      turnId: row.turnId,
      header: row.header,
      status: row.status,
      text: row.text.slice(0, 180)
    })))}\n`
  );
  const durableRows = loadDurableRows(dbPath);
  const activeTurnRunning = await page.locator(".pevo-composer.is-running").count() > 0;
  const analysis = analyzeTranscriptRuntimeRows(rows, durableRows, { activeTurnRunning });
  assertNoToolHeaderResultJson(rows, sample);
  assertNoToolRawJson(rows, sample);
  assertNoEvidenceOverflow(rows, sample);
  assertNoInternalReasoningLabels(rows, sample);
  assertNoEmptyReasoningRows(rows, sample);
  assertNoEmptyPreambleAfterTool(rows, sample);
  assertNoStandaloneWriteStdinRows(rows, sample);
  assertPromptBeforeSameTurnLiveRows(rows, sample);
  await assertNoCompletionPopover(page, sample);
  assertNoAssistantTextInsideReasoning(rows, durableRows, sample);
  assertDurableDomOrder(rows, durableRows, sample);
  assertTranscriptRuntimeAnalysis(analysis, rows, sample);
  await testInfo.attach(`${String(sample).padStart(4, "0")}-${label}.json`, {
    body: JSON.stringify({ analysis, durableRows, rows }, null, 2),
    contentType: "application/json"
  });
}

async function assertNoWorkbenchRenderError(page: Page, sample: number) {
  const alert = page.getByRole("alert");
  const alertText = await alert.textContent().catch(() => null);
  if (alertText?.includes("Workbench render failed")) {
    const stack = await alert.getAttribute("data-error-stack").catch(() => null);
    throw new Error(`sample ${sample}: ${normalize(alertText)}${stack ? `\n${stack}` : ""}`);
  }
}

async function transcriptRows(page: Page): Promise<DomRow[]> {
  return page.locator(".pevo-threadItems > article").evaluateAll((elements) => {
    return elements.map((element, index) => {
      const className = element.getAttribute("class") ?? "";
      const line = element.querySelector(".pevo-evidenceLine");
      const header = (line?.textContent ?? element.querySelector("button")?.textContent ?? "").replace(/\s+/g, " ").trim();
      const text = (element.textContent ?? "").replace(/\s+/g, " ").trim();
      const code = element.querySelector("code")?.textContent?.trim() ?? null;
      const hasBody = element.getAttribute("data-has-body") === "true" ||
        Boolean(element.querySelector(".pevo-markdown, pre")?.textContent?.trim());
      const status = element.querySelector("em")?.textContent?.trim() ?? null;
      const measured = line ?? element;
      const overflow = measured.scrollWidth > measured.clientWidth + 2;
      const blockKind = element.getAttribute("data-block-kind");
      const running = className.includes("is-running") ||
        className.includes("is-streaming") ||
        className.includes("is-runningTool");
      const kind = (() => {
        if (blockKind === "reasoning") return "reasoning";
        if (blockKind && blockKind !== "text") return "tool";
        if (className.includes("pevo-message")) {
          return className.includes("is-assistant") ? "assistant" : "prompt";
        }
        if (className.includes("pevo-reasoning")) return "reasoning";
        return "tool";
      })();
      return {
        blockId: element.getAttribute("data-block-id"),
        code,
        entryId: element.getAttribute("data-entry-id"),
        hasBody,
        header,
        index,
        kind,
        overflow,
        running,
        source: element.getAttribute("data-source"),
        status,
        text,
        turnId: element.getAttribute("data-turn-id")
      };
    });
  });
}

function loadDurableRows(dbPath: string): DurableRow[] {
  if (!existsSync(dbPath)) {
    return [];
  }
  const query = `
    select
      session_seq,
      role,
      coalesce(content_text, '') as content_text,
      message_json
    from messages
    where session_id = (select id from sessions order by updated_at_ms desc limit 1)
    order by session_seq
  `;
  try {
    const stdout = execFileSync("sqlite3", ["-json", dbPath, query], { encoding: "utf8" });
    return flattenMessageRows(JSON.parse(stdout || "[]") as Array<Record<string, unknown>>);
  } catch {
    return [];
  }
}

function flattenMessageRows(rows: Array<Record<string, unknown>>): DurableRow[] {
  const durableRows: DurableRow[] = [];
  for (const row of rows) {
    const seq = Number(row.session_seq);
    const role = typeof row.role === "string" ? row.role : "";
    const contentText = typeof row.content_text === "string" ? row.content_text : "";
    const message = parseJsonObject(row.message_json);
    if (role === "user") {
      durableRows.push({
        entry_id: `message:${seq}:user`,
        seq: seq * 100,
        kind: "prompt",
        title: null,
        text: contentText || messageText(message)
      });
      continue;
    }
    if (role !== "assistant") {
      continue;
    }
    const content = Array.isArray(message.content) ? message.content : [];
    content.forEach((part, index) => {
      const block = parseJsonObject(part);
      const type = typeof block.type === "string" ? block.type : "";
      if (type === "reasoning") {
        durableRows.push({
          entry_id: `message:${seq}:reasoning:${index}`,
          seq: seq * 100 + index,
          kind: "reasoning",
          title: "Reasoning",
          text: typeof block.text === "string" ? block.text : ""
        });
      } else if (type === "text") {
        durableRows.push({
          entry_id: `message:${seq}:text:${index}`,
          seq: seq * 100 + index,
          kind: "assistant",
          title: null,
          text: typeof block.text === "string" ? block.text : ""
        });
      } else if (type === "tool_call") {
        durableRows.push({
          entry_id: `message:${seq}:tool:${index}`,
          seq: seq * 100 + index,
          kind: "tool",
          title: typeof block.name === "string" ? block.name : null,
          text: JSON.stringify(block.arguments ?? {})
        });
      }
    });
  }
  return durableRows;
}

function assertNoToolHeaderResultJson(rows: DomRow[], sample: number) {
  const offenders = rows.filter((row) =>
    row.kind === "tool" && /\{.*"(chunk_id|bytes_written|exit_code|output|error|wall_time_seconds)"/.test(row.header)
  );
  if (offenders.length > 0) {
    throw new Error(`sample ${sample}: tool header contains result JSON: ${JSON.stringify(offenders, null, 2)}`);
  }
}

function assertNoToolRawJson(rows: DomRow[], sample: number) {
  const offenders = rows.filter((row) =>
    row.kind === "tool" && /\{.*"(args|result|chunk_id|bytes_written|exit_code|output|error|wall_time_seconds)"/.test(row.text)
  );
  if (offenders.length > 0) {
    throw new Error(`sample ${sample}: tool row contains raw JSON: ${JSON.stringify(offenders, null, 2)}`);
  }
}

function assertNoEvidenceOverflow(rows: DomRow[], sample: number) {
  const offenders = rows.filter((row) => row.kind === "tool" && row.overflow);
  if (offenders.length > 0) {
    throw new Error(`sample ${sample}: evidence row header overflow: ${JSON.stringify(offenders, null, 2)}`);
  }
}

function assertNoInternalReasoningLabels(rows: DomRow[], sample: number) {
  const offenders = rows.filter((row) =>
    row.kind === "reasoning" && /\b(?:Reasoning|Preamble)\b/.test(row.header)
  );
  if (offenders.length > 0) {
    throw new Error(`sample ${sample}: reasoning header exposed internal label: ${JSON.stringify(offenders, null, 2)}`);
  }
}

function assertNoEmptyReasoningRows(rows: DomRow[], sample: number) {
  const offenders = rows.filter((row) =>
    row.kind === "reasoning" && !row.hasBody
  );
  if (offenders.length > 0) {
    throw new Error(`sample ${sample}: empty reasoning row rendered: ${JSON.stringify(offenders, null, 2)}`);
  }
}

function assertNoEmptyPreambleAfterTool(rows: DomRow[], sample: number) {
  const offenders = rows.filter((row, index) => {
    const previous = index > 0 ? rows[index - 1] : null;
    return row.kind === "reasoning" &&
      row.status === "running" &&
      /\bThinking\b/.test(row.header) &&
      normalize(row.text) === normalize(row.header) &&
      previous?.kind === "tool";
  });
  if (offenders.length > 0) {
    throw new Error(`sample ${sample}: empty running preamble appeared after a tool row: ${JSON.stringify(offenders, null, 2)}`);
  }
}

function assertNoStandaloneWriteStdinRows(rows: DomRow[], sample: number) {
  const offenders = rows.filter((row) => row.kind === "tool" && /\bwrite_stdin\b/.test(row.header));
  if (offenders.length > 0) {
    throw new Error(`sample ${sample}: write_stdin rendered as a standalone transcript row: ${JSON.stringify(offenders, null, 2)}`);
  }
}

function assertPromptBeforeSameTurnLiveRows(rows: DomRow[], sample: number) {
  const promptIndex = rows.findIndex((row) => row.kind === "prompt");
  if (promptIndex < 0) {
    return;
  }
  const promptTurnId = rows[promptIndex]?.turnId;
  const liveIndex = rows.findIndex((row) =>
    row.source === "runtime.stream" &&
    row.kind !== "prompt" &&
    (!promptTurnId || !row.turnId || row.turnId === promptTurnId)
  );
  if (liveIndex >= 0 && promptIndex > liveIndex) {
    throw new Error(`sample ${sample}: prompt rendered after same-turn live rows: ${JSON.stringify(rows, null, 2)}`);
  }
}

function assertNoAssistantTextInsideReasoning(rows: DomRow[], durableRows: DurableRow[], sample: number) {
  const assistantRows = durableRows.filter((row) => row.kind === "assistant" && normalize(row.text).length >= 16);
  const offenders = rows.filter((row) => {
    if (row.kind !== "reasoning") {
      return false;
    }
    const text = normalize(row.text);
    return assistantRows.some((assistant) => textOverlaps(text, normalize(assistant.text)));
  });
  if (offenders.length > 0) {
    throw new Error(`sample ${sample}: assistant text rendered inside Thinking: ${JSON.stringify(offenders, null, 2)}`);
  }
}

function assertDurableDomOrder(rows: DomRow[], durableRows: DurableRow[], sample: number) {
  const matched: Array<{ dom: number; seq: number; entry: string }> = [];
  const used = new Set<string>();
  for (const row of rows) {
    const text = normalize(row.text);
    if (text.length < 16) {
      continue;
    }
    const durable = durableRows.find((candidate) => {
      if (used.has(candidate.entry_id) || candidate.kind !== row.kind) {
        return false;
      }
      return textOverlaps(text, normalize(candidate.text));
    });
    if (durable) {
      used.add(durable.entry_id);
      matched.push({ dom: row.index, seq: durable.seq, entry: durable.entry_id });
    }
  }
  for (let index = 1; index < matched.length; index += 1) {
    if (matched[index]!.seq < matched[index - 1]!.seq) {
      throw new Error(`sample ${sample}: DOM durable transcript order regressed: ${JSON.stringify(matched, null, 2)}`);
    }
  }
}

function analyzeTranscriptRuntimeRows(
  rows: DomRow[],
  durableRows: DurableRow[],
  options: { activeTurnRunning?: boolean } = {}
) {
  const errors: string[] = [];
  const warnings: string[] = [];
  const blockIds = new Set<string>();
  const runningToolIds = new Set<string>();
  const durableToolTitles = durableRows
    .filter((row) => row.kind === "tool" && row.title)
    .map((row) => normalize(row.title ?? ""));

  for (const row of rows) {
    if (row.blockId) {
      const key = `${row.turnId ?? ""}:${row.blockId}`;
      if (blockIds.has(key)) {
        errors.push(`duplicateLiveToolIdentity: duplicate block identity ${key}`);
      }
      blockIds.add(key);
    }
    if (row.kind === "tool" && row.running) {
      const identity = `${row.turnId ?? ""}:${row.blockId ?? row.header}`;
      if (runningToolIds.has(identity)) {
        errors.push(`duplicateLiveToolIdentity: duplicate running tool ${identity}`);
      }
      runningToolIds.add(identity);
      if (row.source === "runtime.message" && !options.activeTurnRunning) {
        errors.push(`activeRowAfterTerminal: committed tool row is still active ${row.blockId ?? row.header}`);
      }
      if (barePendingExecCommandRow(row)) {
        errors.push(`barePendingToolInvocation: pending exec_command row lacks invocation identity ${row.blockId ?? row.header}`);
      }
      const header = normalize(row.header);
      if (durableToolTitles.some((title) => title && header.includes(title))) {
        warnings.push(`liveAfterTerminalIgnored: running live row overlaps durable tool title ${row.blockId ?? row.header}`);
      }
    }
    if (!row.running && barePendingExecCommandRow(row)) {
      errors.push(`barePendingToolInvocation: pending exec_command row lacks invocation identity ${row.blockId ?? row.header}`);
    }
  }

  return { errors, warnings };
}

function barePendingExecCommandRow(row: DomRow): boolean {
  if (row.kind !== "tool" || normalizeNullable(row.status) !== "pending") {
    return false;
  }
  const header = normalize(row.header);
  const text = normalize(row.text);
  return header === "exec_command" ||
    header === "exec_command pending" ||
    text === "exec_command" ||
    text === "exec_command pending";
}

function assertTranscriptRuntimeAnalysis(
  analysis: { errors: string[]; warnings: string[] },
  rows: DomRow[],
  sample: number
) {
  if (analysis.errors.length > 0) {
    throw new Error(`sample ${sample}: transcript runtime analyzer failed: ${JSON.stringify({ analysis, rows }, null, 2)}`);
  }
}

async function liveSkillCompleted(page: Page): Promise<boolean> {
  const transcript = page.getByRole("region", { name: "Transcript" });
  const runningRows = await transcript.locator(
    ".pevo-message.is-streaming, .pevo-reasoning.is-streaming, .pevo-evidence.is-running"
  ).count();
  const assistantText = normalize(await page.locator(".pevo-message.is-assistant").last().textContent().catch(() => "") ?? "");
  return runningRows === 0 &&
    /(x-daily|日报|daily)/.test(assistantText) &&
    /(执行完成|已生成|生成|完成|完成总结|all done|complete)/.test(assistantText);
}

async function assertNoCompletionPopover(page: Page, sample: number) {
  const popovers = await page.locator(".pevo-completionPopover").count();
  if (popovers > 0) {
    throw new Error(`sample ${sample}: completion popover remained visible after submission`);
  }
}

function textOverlaps(left: string, right: string): boolean {
  if (!left || !right) {
    return false;
  }
  const shorter = left.length < right.length ? left : right;
  const longer = left.length < right.length ? right : left;
  if (shorter.length < 16) {
    return false;
  }
  return longer.includes(shorter) || shorter.includes(longer.slice(0, Math.min(longer.length, 160)));
}

function normalize(value: string): string {
  return value.replace(/\s+/g, " ").trim().toLowerCase();
}

function normalizeNullable(value: string | null): string {
  return normalize(value ?? "");
}

function parseJsonObject(value: unknown): Record<string, unknown> {
  if (typeof value === "string") {
    try {
      const parsed = JSON.parse(value) as unknown;
      return parseJsonObject(parsed);
    } catch {
      return {};
    }
  }
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function messageText(message: Record<string, unknown>): string {
  const content = message.content;
  if (typeof content === "string") {
    return content;
  }
  if (Array.isArray(content)) {
    return content
      .map((part) => {
        const block = parseJsonObject(part);
        return typeof block.text === "string" ? block.text : "";
      })
      .filter(Boolean)
      .join("\n");
  }
  return "";
}
