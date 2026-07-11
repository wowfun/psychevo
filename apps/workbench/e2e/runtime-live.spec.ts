import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Page, type TestInfo } from "@playwright/test";
import { startPevoWeb } from "./harness";
import { liveContextFor, screenshotRoot, type XtaskLiveContext } from "./liveContext";
import {
  prepareDeterministicRuntime,
  startDeterministicTelegram,
  type DirectRuntimeKind
} from "./runtime-live.support";

test.describe("direct Runtime Profile live validation", () => {
  test("runs direct Codex through the GUI with a deterministic fake @live", async ({ page }, testInfo) => {
    await runGuiSmoke(page, testInfo, "runtime-codex-gui-smoke", "codex");
  });

  test("runs direct OpenCode through the GUI with a deterministic fake @live", async ({ page }, testInfo) => {
    await runGuiSmoke(page, testInfo, "runtime-opencode-gui-smoke", "opencode");
  });

  test("steers an active direct Codex turn through the public control path @live", async ({ page }, testInfo) => {
    const context = requiredContext("runtime-codex-steer-smoke");
    if (!context) return;
    test.setTimeout(context.timeoutMs);
    const fixture = prepareDeterministicRuntime("codex", context.artifactRoot, "steer");
    const server = await startPevoWeb({
      configAppend: fixture.configAppend,
      cwd: context.cwd,
      dbPath: context.dbPath,
      home: context.home,
      live: false,
      pevoBin: context.pevoBin
    });
    const screenshots = screenshotRoot(context, "runtime-profiles");
    mkdirSync(screenshots, { recursive: true });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      const selector = page.getByRole("button", { name: "Runtime Profile" });
      await selector.click();
      const choice = page
        .getByRole("dialog", { name: "Runtime Profile selection" })
        .getByRole("radio", { name: "Codex", exact: true });
      await choice.click();

      const composer = page.getByPlaceholder("Ask Psychevo...");
      await composer.fill("prime the observed Codex matrix");
      await page.getByRole("button", { name: "Send message" }).click();
      await expect(page.locator(".pevo-message.is-assistant").filter({ hasText: "hello" })).toHaveCount(1, {
        timeout: 60_000
      });
      await expect.poll(() => runtimeTrace(fixture.logPath), { timeout: 10_000 }).toContain('"method":"model/list"');

      await composer.fill("wait for a public steer");
      await page.getByRole("button", { name: "Send message" }).click();
      await expect.poll(
        () => runtimeTrace(fixture.logPath).match(/"method":"turn\/start"/g)?.length ?? 0,
        { timeout: 10_000 }
      ).toBe(2);
      await composer.fill("steer through Gateway now");
      await expect(page.getByRole("button", { name: "Steer", exact: true })).toBeVisible({ timeout: 10_000 });
      await composer.press("Enter");

      await expect(page.locator(".pevo-message.is-assistant").filter({
        hasText: "steered through public control"
      })).toHaveCount(1, { timeout: 60_000 });
      await expect.poll(() => runtimeTrace(fixture.logPath), { timeout: 10_000 }).toContain('"method":"turn/steer"');
      expect(runtimeTrace(fixture.logPath)).toContain("steer through Gateway now");
      await page.screenshot({
        fullPage: true,
        path: path.join(screenshots, `codex-steer-${testInfo.project.name}.png`)
      });
    } finally {
      await server.stop();
    }
  });

  test("proves the dual direct runtime Ready milestone with deterministic fakes @live", async ({ page }, testInfo) => {
    const context = requiredContext("runtime-ready-milestone-smoke");
    if (!context) return;
    test.setTimeout(context.timeoutMs);
    const codex = prepareDeterministicRuntime("codex", context.artifactRoot);
    const opencode = prepareDeterministicRuntime("opencode", context.artifactRoot);
    const server = await startPevoWeb({
      configAppend: `${codex.configAppend}\n${opencode.configAppend}`,
      cwd: context.cwd,
      dbPath: context.dbPath,
      home: context.home,
      live: false,
      pevoBin: context.pevoBin
    });
    const screenshots = screenshotRoot(context, "runtime-profiles");
    mkdirSync(screenshots, { recursive: true });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      await page.getByRole("button", { name: "Runtime Profile" }).click();
      await page
        .getByRole("dialog", { name: "Runtime Profile selection" })
        .getByRole("radio", { name: "Codex", exact: true })
        .click();
      const composer = page.getByPlaceholder("Ask Psychevo...");
      await composer.fill("hydrate the Codex Stable matrix");
      await page.getByRole("button", { name: "Send message" }).click();
      await expect(page.locator(".pevo-message.is-assistant").filter({ hasText: codex.expectedAnswer })).toHaveCount(1, {
        timeout: 60_000
      });

      await page.getByRole("button", { name: /^Bound Runtime Profile / }).click();
      await page
        .getByRole("dialog", { name: "Runtime Profile selection" })
        .getByRole("radio", { name: "Start a new thread with OpenCode", exact: true })
        .click();
      await expect(page.getByRole("button", { name: "Runtime Profile" })).toContainText("OpenCode");
      await composer.fill("hydrate the OpenCode Stable matrix");
      await page.getByRole("button", { name: "Send message" }).click();
      await expect(page.locator(".pevo-message.is-assistant").filter({ hasText: opencode.expectedAnswer })).toHaveCount(1, {
        timeout: 60_000
      });

      const profileList = await gatewayRequest(page, "runtime/profile/list", {
        scope: {
          cwd: server.cwd,
          source: { kind: "web", rawId: null, lifetime: "persistent", rawIdentity: null, visibleName: null }
        }
      }) as { profiles?: Array<{ id?: string; health?: { status?: string } }> };
      for (const runtimeRef of ["codex", "opencode"]) {
        expect(profileList.profiles?.find((profile) => profile.id === runtimeRef)?.health?.status).toBe("ready");
      }

      await page.getByRole("button", { name: "Capabilities" }).click();
      const capabilities = page.getByRole("region", { name: "Capabilities" });
      await capabilities.getByRole("tab", { name: "Agents" }).click();
      await capabilities.getByRole("tab", { name: "Runtime Profiles" }).click();
      await capabilities.getByRole("button", { name: "Runtime Profile codex" }).click();
      const detail = capabilities.getByRole("complementary", { name: "Runtime Profile detail" });
      await expect(detail).toContainText("Ready");
      await expect(detail).toContainText("Last checked");
      await page.screenshot({
        fullPage: true,
        path: path.join(screenshots, `dual-ready-${testInfo.project.name}.png`)
      });
    } finally {
      await server.stop();
    }
  });

  test("routes a Channel-origin turn through direct Codex with a deterministic fake @live", async ({ page }, testInfo) => {
    await runChannelSmoke(page, testInfo, "runtime-codex-channel-smoke", "codex");
  });

  test("routes a Channel-origin turn through direct OpenCode with a deterministic fake @live", async ({ page }, testInfo) => {
    await runChannelSmoke(page, testInfo, "runtime-opencode-channel-smoke", "opencode");
  });
});

async function runGuiSmoke(
  page: Page,
  testInfo: TestInfo,
  checkId: string,
  runtime: DirectRuntimeKind
) {
  const context = requiredContext(checkId);
  if (!context) return;
  test.setTimeout(context.timeoutMs);
  const fixture = prepareDeterministicRuntime(runtime, context.artifactRoot);
  const server = await startPevoWeb({
    configAppend: fixture.configAppend,
    cwd: context.cwd,
    dbPath: context.dbPath,
    home: context.home,
    live: false,
    pevoBin: context.pevoBin
  });
  const screenshots = screenshotRoot(context, "runtime-profiles");
  mkdirSync(screenshots, { recursive: true });
  try {
    await page.goto(server.url);
    await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
    const selector = page.getByRole("button", { name: "Runtime Profile" });
    await expect(selector).toBeVisible({ timeout: 30_000 });
    await selector.click();
    const popover = page.getByRole("dialog", { name: "Runtime Profile selection" });
    const label = runtimeLabel(runtime);
    const choice = popover.getByRole("radio", { name: label, exact: true });
    await expect(choice).toBeVisible();
    await choice.click();
    await expect(page.getByLabel("Runtime control state")).toHaveText("Runtime default");

    const prompt = `deterministic ${runtime} GUI smoke`;
    await page.getByPlaceholder("Ask Psychevo...").fill(prompt);
    await page.getByRole("button", { name: "Send message" }).click();
    const user = page.locator(".pevo-message.is-user").filter({ hasText: prompt });
    await expect(user).toHaveCount(1, { timeout: 60_000 });
    const assistant = page.locator(".pevo-message.is-assistant").filter({
      hasText: fixture.expectedAnswer
    });
    await expect(assistant).toHaveCount(1, { timeout: 60_000 });
    const capsule = page.getByRole("button", { name: /^Bound Runtime Profile / });
    await expect(capsule).toContainText(`${label} · Direct`);
    await expect(capsule).toHaveAttribute("title", /Runtime bindings are immutable/);
    await page.screenshot({
      fullPage: true,
      path: path.join(screenshots, `${runtime}-gui-${testInfo.project.name}.png`)
    });

    await expect.poll(() => runtimeTrace(fixture.logPath), { timeout: 10_000 }).toContain(
      runtime === "codex" ? '"method":"turn/start"' : "/prompt_async"
    );
    const trace = runtimeTrace(fixture.logPath);
    if (runtime === "codex") {
      expect(trace).toContain('"method":"initialize"');
      expect(trace).toContain('"method":"thread/start"');
    } else {
      expect(trace).toContain("ARGS serve --hostname 127.0.0.1 --port 0 --no-mdns");
      expect(trace).toContain("GET /global/event");
    }
  } finally {
    await server.stop();
  }
}

async function runChannelSmoke(
  page: Page,
  testInfo: TestInfo,
  checkId: string,
  runtime: DirectRuntimeKind
) {
  const context = requiredContext(checkId);
  if (!context) return;
  test.setTimeout(context.timeoutMs);
  const fixture = prepareDeterministicRuntime(runtime, context.artifactRoot);
  const telegram = await startDeterministicTelegram();
  const channelConfig = [
    "[[channels.connections]]",
    `id = ${JSON.stringify(`runtime-live-${runtime}`)}`,
    'channel = "telegram"',
    `label = ${JSON.stringify(`Runtime live Telegram ${runtime}`)}`,
    'transport = "polling"',
    "enabled = true",
    `cwd = ${JSON.stringify(context.cwd)}`,
    `runtime_ref = ${JSON.stringify(runtime)}`,
    `credential_env = ${JSON.stringify(telegram.credentialEnv)}`,
    `base_url_env = ${JSON.stringify(telegram.baseUrlEnv)}`,
    'allow_users = ["42"]',
    "require_mention = false",
    ""
  ].join("\n");
  let server: Awaited<ReturnType<typeof startPevoWeb>> | null = null;
  const screenshots = screenshotRoot(context, "runtime-profiles");
  mkdirSync(screenshots, { recursive: true });
  try {
    server = await startPevoWeb({
      channelRuntime: true,
      configAppend: `${fixture.configAppend}\n${channelConfig}`,
      cwd: context.cwd,
      dbPath: context.dbPath,
      envFile: telegram.envFile,
      home: context.home,
      live: false,
      pevoBin: context.pevoBin
    });
    await page.goto(server.url);
    await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
    const inbound = `deterministic ${runtime} Channel smoke`;
    telegram.push(inbound);
    const outbound = await telegram.waitForText(fixture.expectedAnswer);
    expect(outbound.filter((message) => message.text.includes(fixture.expectedAnswer))).toHaveLength(1);
    expect(outbound.every((message) => message.chatId === "42")).toBe(true);

    const proofPath = path.join(context.artifactRoot, `${runtime}-channel-proof.json`);
    writeFileSync(proofPath, JSON.stringify({
      adapter: "telegram-polling",
      inbound,
      outbound,
      runtimeRef: runtime,
      terminalFinalCount: outbound.filter((message) => message.text.includes(fixture.expectedAnswer)).length
    }, null, 2));
    await page.getByRole("button", { name: "Settings", exact: true }).click();
    const settings = page.getByRole("region", { name: "Settings", exact: true });
    await settings.getByRole("button", { name: "Channels" }).click();
    const channels = settings.getByRole("region", { name: "Channels", exact: true });
    await expect(channels).toContainText(`Runtime live Telegram ${runtime}`);
    await expect(channels).toContainText(runtime);
    await page.screenshot({
      fullPage: true,
      path: path.join(screenshots, `${runtime}-channel-${testInfo.project.name}.png`)
    });
    await expect.poll(() => runtimeTrace(fixture.logPath), { timeout: 10_000 }).toContain(
      runtime === "codex" ? '"method":"turn/start"' : "/prompt_async"
    );
  } finally {
    await server?.stop();
    await telegram.stop();
  }
}

function requiredContext(checkId: string): XtaskLiveContext | null {
  const context = liveContextFor(checkId);
  if (!context) {
    test.skip(true, `run ${checkId} through cargo xtask live`);
    return null;
  }
  if (context.provider !== "deterministic-fake") {
    throw new Error(`${checkId} requires the deterministic runtime context`);
  }
  return context;
}

function runtimeLabel(runtime: DirectRuntimeKind): string {
  return runtime === "codex" ? "Codex" : "OpenCode";
}

function runtimeTrace(logPath: string): string {
  try {
    return readFileSync(logPath, "utf8");
  } catch {
    return "";
  }
}

async function gatewayRequest(page: Page, method: string, params: unknown): Promise<unknown> {
  return page.evaluate(async ({ method, params }) => await new Promise((resolve, reject) => {
    const url = new URL("/ws", window.location.origin);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    const socket = new WebSocket(url);
    const id = `runtime-live-${method}`;
    const timeout = window.setTimeout(() => {
      socket.close();
      reject(new Error(`${method} timed out`));
    }, 30_000);
    socket.addEventListener("open", () => {
      socket.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
    });
    socket.addEventListener("message", (event) => {
      const message = JSON.parse(String(event.data)) as { id?: string; result?: unknown; error?: unknown };
      if (message.id !== id) return;
      window.clearTimeout(timeout);
      socket.close();
      if (message.error) reject(new Error(JSON.stringify(message.error)));
      else resolve(message.result);
    });
    socket.addEventListener("error", () => {
      window.clearTimeout(timeout);
      reject(new Error(`${method} WebSocket failed`));
    });
  }), { method, params });
}
