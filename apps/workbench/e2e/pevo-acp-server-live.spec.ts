import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createServer, type Server } from "node:http";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { createInterface } from "node:readline";
import { expect, test, type Page, type TestInfo } from "@playwright/test";
import { repoRoot } from "./harness";
import { liveContextFor, screenshotRoot } from "./liveContext";

type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

interface JsonRpcMessage {
  jsonrpc?: string;
  id?: number | string;
  method?: string;
  params?: JsonValue;
  result?: JsonValue;
  error?: JsonValue;
}

test.describe("Psychevo ACP server live validation", () => {
  test("streams standard ACP updates, accepts model config, and reports usage @live", async ({ page }, testInfo) => {
    const context = liveContextFor("pevo-acp-server-live");
    if (!context) {
      test.skip(true, "run through cargo xtask live");
      return;
    }
    test.setTimeout(context.timeoutMs);
    const screenshotDir = screenshotRoot(context, "pevo-acp-server-live");
    const testRoot = path.join(context.artifactRoot, "work");
    mkdirSync(screenshotDir, { recursive: true });
    mkdirSync(testRoot, { recursive: true });
    const root = mkdtempSync(path.join(testRoot, "pevo-acp-server-"));
    const mockServer = await startMockOpenAiServer();
    const process = spawnPevoAcp(root, mockServer.baseUrl, context.pevoBin);
    const rpc = new JsonRpcLineClient(process);
    const cwd = path.join(root, "cwd");
    mkdirSync(cwd, { recursive: true });

    try {
      const initialize = await rpc.request("initialize", {
        protocolVersion: 2,
        clientCapabilities: {},
        clientInfo: {
          name: "psychevo-playwright-acp-client",
          title: "Psychevo Playwright ACP Client",
          version: "0.0.0"
        }
      });
      expect(initialize.protocolVersion).toBe(2);

      const session = await rpc.request("session/new", {
        cwd: cwd,
        mcpServers: []
      });
      const sessionId = readString(session, "sessionId");
      const initialOptions = readArray(session, "configOptions");
      expect(selectCurrentValue(initialOptions, "model")).toBe("mock/default");
      expect(selectCurrentValue(initialOptions, "effort")).toBe("none");

      const modelUpdate = await rpc.request("session/set_config_option", {
        sessionId,
        configId: "model",
        value: "mock/other"
      });
      expect(selectCurrentValue(readArray(modelUpdate, "configOptions"), "model")).toBe("mock/other");
      const effortUpdate = await rpc.request("session/set_config_option", {
        sessionId,
        configId: "effort",
        value: "high"
      });
      expect(selectCurrentValue(readArray(effortUpdate, "configOptions"), "effort")).toBe("high");

      const prompt = rpc.request("session/prompt", {
        sessionId,
        prompt: [
          {
            type: "text",
            text: "Say hello from the Psychevo ACP server live verification."
          }
        ]
      }, 120_000);
      const firstChunk = await rpc.waitForNotification((message) =>
        isSessionUpdate(message, "agent_message_chunk", "pevo server streaming")
      );
      const usage = await rpc.waitForNotification((message) => isSessionUpdate(message, "usage_update"));
      const promptResult = await prompt;
      expect(readString(promptResult, "stopReason")).toBe("end_turn");

      expect(mockServer.requests.length).toBeGreaterThanOrEqual(1);
      const requestBody = mockServer.requests[0];
      expect(requestBody.model).toBe("other");
      expect(requestBody.reasoning_effort).toBe("high");
      const usageUpdate = readUpdate(usage);
      expect(readNumber(usageUpdate, "used")).toBeGreaterThan(0);
      expect(readNumber(usageUpdate, "size")).toBe(4096);

      await renderProtocolSummary(page, {
        initialize,
        firstChunk,
        promptResult,
        requestBody,
        usageUpdate
      });
      await capture(page, testInfo, screenshotDir, "01-server-protocol-desktop");
    } finally {
      await rpc.close();
      await mockServer.stop();
      rmSync(root, { force: true, recursive: true });
    }
  });
});

function spawnPevoAcp(root: string, baseUrl: string, pevoBin?: string): ChildProcessWithoutNullStreams {
  const home = path.join(root, "home");
  mkdirSync(home, { recursive: true });
  const configPath = path.join(root, "config.toml");
  const config = `model = "mock/default"

[provider.mock]
api = "${baseUrl}/v1"
no_auth = true

[provider.mock.models.default]
reasoning_effort = "low"

[provider.mock.models.default.limit]
context = 4096

[provider.mock.models.other]
reasoning_effort = "high"

[provider.mock.models.other.limit]
context = 4096
`;
  writeFileSync(configPath, config);
  writeFileSync(path.join(home, "config.toml"), config);

  const command = pevoBin ?? process.env.PEVO_BIN ?? "cargo";
  const args = (pevoBin ?? process.env.PEVO_BIN)
    ? ["acp"]
    : ["run", "-p", "psychevo-cli", "--", "acp"];
  return spawn(command, args, {
    cwd: repoRoot,
    env: {
      ...process.env,
      PSYCHEVO_CONFIG: configPath,
      PSYCHEVO_DB: path.join(root, "state.db"),
      PSYCHEVO_HOME: home
    },
    stdio: ["pipe", "pipe", "pipe"]
  });
}

async function startMockOpenAiServer(): Promise<{
  baseUrl: string;
  requests: Array<Record<string, unknown>>;
  stop(): Promise<void>;
}> {
  const requests: Array<Record<string, unknown>> = [];
  const server = createServer((request, response) => {
    if (request.method === "GET" && request.url?.endsWith("/models")) {
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({ data: [{ id: "default" }, { id: "other" }] }));
      return;
    }
    const chunks: Buffer[] = [];
    request.on("data", (chunk: Buffer) => chunks.push(chunk));
    request.on("end", () => {
      const body = Buffer.concat(chunks).toString("utf8");
      if (body.trim()) {
        requests.push(JSON.parse(body));
      }
      response.writeHead(200, {
        "cache-control": "no-cache",
        "content-type": "text/event-stream"
      });
      response.write(sse({ choices: [{ delta: { reasoning_content: "server thinking " }, finish_reason: null }] }));
      setTimeout(() => {
        response.write(sse({ choices: [{ delta: { content: "pevo server streaming " }, finish_reason: null }] }));
      }, 100);
      setTimeout(() => {
        response.write(sse({
          choices: [{ delta: { content: "live ok" }, finish_reason: "stop" }],
          usage: {
            prompt_tokens: 5,
            completion_tokens: 7,
            total_tokens: 12
          }
        }));
        response.end("data: [DONE]\n\n");
      }, 250);
    });
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
  const address = server.address();
  if (!address || typeof address === "string") {
    throw new Error("mock OpenAI server did not bind a TCP address");
  }
  return {
    baseUrl: `http://127.0.0.1:${address.port}`,
    requests,
    stop: () => closeServer(server)
  };
}

function closeServer(server: Server): Promise<void> {
  return new Promise((resolve) => server.close(() => resolve()));
}

function sse(value: unknown): string {
  return `data: ${JSON.stringify(value)}\n\n`;
}

class JsonRpcLineClient {
  private nextId = 1;
  private readonly pending = new Map<number | string, {
    reject(error: Error): void;
    resolve(value: Record<string, unknown>): void;
    timer: NodeJS.Timeout;
  }>();
  private readonly notifications: JsonRpcMessage[] = [];
  private readonly notificationWaiters: Array<{
    predicate(message: JsonRpcMessage): boolean;
    reject(error: Error): void;
    resolve(message: JsonRpcMessage): void;
    timer: NodeJS.Timeout;
  }> = [];
  private readonly logs: string[] = [];
  private closed = false;

  constructor(private readonly child: ChildProcessWithoutNullStreams) {
    createInterface({ input: child.stdout }).on("line", (line) => {
      if (!line.trim()) {
        return;
      }
      let message: JsonRpcMessage;
      try {
        message = JSON.parse(line);
      } catch {
        this.logs.push(line);
        return;
      }
      if (message.id !== undefined) {
        const waiter = this.pending.get(message.id);
        if (waiter) {
          this.pending.delete(message.id);
          clearTimeout(waiter.timer);
          if (message.error !== undefined) {
            waiter.reject(new Error(`ACP error response: ${JSON.stringify(message.error)}`));
          } else {
            waiter.resolve(asObject(message.result));
          }
          return;
        }
      }
      this.notifications.push(message);
      this.flushNotificationWaiters(message);
    });
    child.stderr.on("data", (chunk: Buffer) => this.logs.push(chunk.toString("utf8")));
    child.once("exit", (code, signal) => {
      this.closed = true;
      const error = new Error(`pevo acp exited code=${code} signal=${signal}\n${this.logs.join("")}`);
      for (const waiter of this.pending.values()) {
        clearTimeout(waiter.timer);
        waiter.reject(error);
      }
      this.pending.clear();
      for (const waiter of this.notificationWaiters.splice(0)) {
        clearTimeout(waiter.timer);
        waiter.reject(error);
      }
    });
  }

  request(method: string, params: JsonValue, timeout = 60_000): Promise<Record<string, unknown>> {
    if (this.closed) {
      throw new Error(`pevo acp is already closed\n${this.logs.join("")}`);
    }
    const id = this.nextId;
    this.nextId += 1;
    const message = { jsonrpc: "2.0", id, method, params };
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`timed out waiting for ${method}\n${this.logs.join("")}`));
      }, timeout);
      this.pending.set(id, { reject, resolve, timer });
      this.child.stdin.write(`${JSON.stringify(message)}\n`);
    });
  }

  waitForNotification(predicate: (message: JsonRpcMessage) => boolean, timeout = 60_000): Promise<JsonRpcMessage> {
    const existing = this.notifications.find(predicate);
    if (existing) {
      return Promise.resolve(existing);
    }
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        const seen = this.notifications.map((message) => JSON.stringify(message)).join("\n");
        reject(new Error(`timed out waiting for ACP notification\nseen:\n${seen}\nlogs:\n${this.logs.join("")}`));
      }, timeout);
      this.notificationWaiters.push({ predicate, reject, resolve, timer });
    });
  }

  async close(): Promise<void> {
    if (this.closed) {
      return;
    }
    await new Promise<void>((resolve) => {
      const timer = setTimeout(() => {
        this.child.kill("SIGKILL");
        resolve();
      }, 2_000);
      this.child.once("exit", () => {
        clearTimeout(timer);
        resolve();
      });
      this.child.stdin.end();
      this.child.kill("SIGTERM");
    });
  }

  private flushNotificationWaiters(message: JsonRpcMessage) {
    for (const waiter of [...this.notificationWaiters]) {
      if (!waiter.predicate(message)) {
        continue;
      }
      const index = this.notificationWaiters.indexOf(waiter);
      if (index >= 0) {
        this.notificationWaiters.splice(index, 1);
      }
      clearTimeout(waiter.timer);
      waiter.resolve(message);
    }
  }
}

function isSessionUpdate(message: JsonRpcMessage, kind: string, text?: string): boolean {
  if (message.method !== "session/update") {
    return false;
  }
  const update = readUpdate(message);
  if (readString(update, "sessionUpdate") !== kind) {
    return false;
  }
  return text === undefined || JSON.stringify(update).includes(text);
}

function readUpdate(message: JsonRpcMessage): Record<string, unknown> {
  const params = asObject(message.params);
  return asObject(params.update);
}

function readArray(object: Record<string, unknown>, key: string): Array<Record<string, unknown>> {
  const value = object[key];
  if (!Array.isArray(value)) {
    throw new Error(`expected ${key} array, got ${JSON.stringify(value)}`);
  }
  return value.map(asObject);
}

function readString(object: Record<string, unknown>, key: string): string {
  const value = object[key];
  if (typeof value !== "string") {
    throw new Error(`expected ${key} string, got ${JSON.stringify(value)}`);
  }
  return value;
}

function readNumber(object: Record<string, unknown>, key: string): number {
  const value = object[key];
  if (typeof value !== "number") {
    throw new Error(`expected ${key} number, got ${JSON.stringify(value)}`);
  }
  return value;
}

function asObject(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`expected object, got ${JSON.stringify(value)}`);
  }
  return value as Record<string, unknown>;
}

function selectCurrentValue(options: Array<Record<string, unknown>>, id: string): string | null {
  const option = options.find((item) => item.id === id);
  if (!option) {
    return null;
  }
  const current = option.currentValue;
  if (typeof current === "string") {
    return current;
  }
  return null;
}

async function renderProtocolSummary(page: Page, data: {
  firstChunk: JsonRpcMessage;
  initialize: Record<string, unknown>;
  promptResult: Record<string, unknown>;
  requestBody: Record<string, unknown>;
  usageUpdate: Record<string, unknown>;
}) {
  await page.setContent(`<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <title>Psychevo ACP Server Live</title>
    <style>
      :root {
        color-scheme: dark;
        background: #050505;
        color: #f3efe5;
        font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      }
      body {
        margin: 0;
        min-height: 100vh;
        background: #050505;
      }
      main {
        align-content: start;
        box-sizing: border-box;
        display: grid;
        gap: 18px;
        min-height: 100vh;
        padding: 42px;
      }
      header {
        align-items: baseline;
        border-bottom: 1px solid #2d2a24;
        display: flex;
        gap: 18px;
        padding-bottom: 18px;
      }
      h1 {
        font-size: 22px;
        letter-spacing: 0;
        line-height: 1.2;
        margin: 0;
      }
      .status {
        color: #9fd08a;
        font-size: 13px;
        font-weight: 700;
      }
      .grid {
        display: grid;
        gap: 12px;
        grid-template-columns: repeat(2, minmax(0, 1fr));
      }
      section {
        border: 1px solid #2d2a24;
        border-radius: 8px;
        background: #11100d;
        min-width: 0;
        padding: 16px;
      }
      h2 {
        font-size: 13px;
        margin: 0 0 12px;
        text-transform: uppercase;
      }
      dl {
        display: grid;
        gap: 8px 14px;
        grid-template-columns: 160px minmax(0, 1fr);
        margin: 0;
      }
      dt {
        color: #a8a096;
      }
      dd {
        margin: 0;
        min-width: 0;
        overflow-wrap: anywhere;
      }
      pre {
        background: #080806;
        border: 1px solid #29251f;
        border-radius: 6px;
        color: #ded8cc;
        font-size: 12px;
        line-height: 1.45;
        margin: 0;
        max-height: 360px;
        overflow: auto;
        padding: 12px;
        white-space: pre-wrap;
      }
      @media (max-width: 760px) {
        main {
          padding: 20px;
        }
        header,
        .grid,
        dl {
          display: block;
        }
        dt {
          margin-top: 10px;
        }
      }
    </style>
  </head>
  <body>
    <main>
      <header>
        <h1>Psychevo ACP Server Live</h1>
        <span class="status">protocol verified</span>
      </header>
      <div class="grid">
        <section aria-label="ACP summary">
          <h2>Summary</h2>
          <dl>
            <dt>Protocol</dt>
            <dd>${escapeHtml(String(data.initialize.protocolVersion))}</dd>
            <dt>Provider model</dt>
            <dd>${escapeHtml(String(data.requestBody.model))}</dd>
            <dt>Reasoning effort</dt>
            <dd>${escapeHtml(String(data.requestBody.reasoning_effort))}</dd>
            <dt>Stop reason</dt>
            <dd>${escapeHtml(String(data.promptResult.stopReason))}</dd>
            <dt>Usage</dt>
            <dd>${escapeHtml(`${data.usageUpdate.used} / ${data.usageUpdate.size} tokens`)}</dd>
          </dl>
        </section>
        <section aria-label="First stream chunk">
          <h2>First stream chunk</h2>
          <pre>${escapeHtml(JSON.stringify(data.firstChunk, null, 2))}</pre>
        </section>
      </div>
    </main>
  </body>
</html>`);
}

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

async function capture(page: Page, testInfo: TestInfo, screenshotDir: string, label: string) {
  const fileName = `${label}-${testInfo.project.name}.png`;
  const stablePath = path.join(screenshotDir, fileName);
  await page.screenshot({ fullPage: true, path: stablePath });
  await testInfo.attach(fileName, { path: stablePath, contentType: "image/png" });
  process.stdout.write(`[pevo-acp-server-live] screenshot ${path.relative(repoRoot, stablePath)}\n`);
}
