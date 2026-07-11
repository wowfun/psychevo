import { spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "../../..");

export type DirectRuntimeKind = "codex" | "opencode";

export interface DeterministicRuntimeFixture {
  configAppend: string;
  expectedAnswer: string;
  logPath: string;
  root: string;
  runtimeRef: DirectRuntimeKind;
}

export interface DeterministicTelegramFixture {
  baseUrl: string;
  baseUrlEnv: string;
  credentialEnv: string;
  envFile: string;
  push(text: string): void;
  sent(): Array<{ chatId: string; text: string }>;
  stop(): Promise<void>;
  waitForText(text: string, timeoutMs?: number): Promise<Array<{ chatId: string; text: string }>>;
}

export function prepareDeterministicRuntime(
  runtime: DirectRuntimeKind,
  artifactRoot: string,
  scenario = "ordering"
): DeterministicRuntimeFixture {
  const fakeRoot = path.join(artifactRoot, "runtime-fakes");
  mkdirSync(fakeRoot, { recursive: true });
  const root = mkdtempSync(path.join(fakeRoot, `${runtime}-`));
  return runtime === "codex" ? prepareCodex(root, scenario) : prepareOpenCode(root);
}

export async function startDeterministicTelegram(): Promise<DeterministicTelegramFixture> {
  const token = "runtime-live-token";
  const credentialEnv = "RUNTIME_LIVE_TELEGRAM_TOKEN";
  const baseUrlEnv = "RUNTIME_LIVE_TELEGRAM_BASE_URL";
  const updates: Array<Record<string, unknown>> = [];
  const outbound: Array<{ chatId: string; text: string }> = [];
  let nextUpdateId = 1;
  const server = createServer(async (request, response) => {
    const url = new URL(request.url ?? "/", "http://127.0.0.1");
    const requestBody = await readJsonBody(request);
    if (request.method === "POST" && url.pathname === `/bot${token}/getUpdates`) {
      const offset = typeof requestBody.offset === "number" ? requestBody.offset : 0;
      const result = updates.filter((update) => Number(update.update_id) >= offset);
      if (result.length === 0) await new Promise((resolve) => setTimeout(resolve, 25));
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({ ok: true, result }));
      return;
    }
    if (request.method === "POST" && url.pathname === `/bot${token}/sendMessage`) {
      outbound.push({
        chatId: String(requestBody.chat_id ?? ""),
        text: String(requestBody.text ?? "")
      });
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({ ok: true, result: { message_id: outbound.length } }));
      return;
    }
    response.writeHead(404, { "content-type": "application/json" });
    response.end(JSON.stringify({ ok: false, description: "not found" }));
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => resolve());
  });
  const address = server.address() as AddressInfo;
  const baseUrl = `http://127.0.0.1:${address.port}`;
  return {
    baseUrl,
    baseUrlEnv,
    credentialEnv,
    envFile: `${credentialEnv}=${token}\n${baseUrlEnv}=${baseUrl}\n`,
    push(text) {
      const updateId = nextUpdateId++;
      updates.push({
        update_id: updateId,
        message: {
          message_id: 1000 + updateId,
          text,
          chat: { id: 42, type: "private" },
          from: { id: 42, username: "runtime-live" }
        }
      });
    },
    sent: () => [...outbound],
    async waitForText(text, timeoutMs = 60_000) {
      const deadline = Date.now() + timeoutMs;
      while (Date.now() < deadline) {
        if (outbound.some((message) => message.text.includes(text))) return [...outbound];
        await new Promise((resolve) => setTimeout(resolve, 25));
      }
      throw new Error(`timed out waiting for Telegram outbound text: ${text}\n${JSON.stringify(outbound)}`);
    },
    async stop() {
      server.closeAllConnections?.();
      await new Promise<void>((resolve) => server.close(() => resolve()));
    }
  };
}

async function readJsonBody(request: import("node:http").IncomingMessage): Promise<Record<string, unknown>> {
  let body = "";
  for await (const chunk of request) body += chunk.toString("utf8");
  if (!body) return {};
  const parsed = JSON.parse(body) as unknown;
  return parsed !== null && typeof parsed === "object" && !Array.isArray(parsed)
    ? parsed as Record<string, unknown>
    : {};
}

function prepareCodex(root: string, scenario: string): DeterministicRuntimeFixture {
  const executable = path.join(root, process.platform === "win32" ? "fake-codex.exe" : "fake-codex");
  const source = path.join(
    repoRoot,
    "crates/psychevo-runtime-host/tests/fixtures/fake_codex_app_server.rs"
  );
  const compile = spawnSync("rustc", ["--edition=2024", source, "-o", executable], {
    cwd: repoRoot,
    encoding: "utf8"
  });
  if (compile.status !== 0) {
    throw new Error(`failed to compile deterministic Codex fixture\n${compile.stdout}\n${compile.stderr}`);
  }
  const logPath = path.join(root, "requests.jsonl");
  return {
    configAppend: [
      "[runtime_profiles.codex]",
      'runtime = "codex"',
      'label = "Codex"',
      `command = ${tomlString(executable)}`,
      'args = ["app-server", "--stdio"]',
      'default_model = "gpt-fixture"',
      'approval_mode = "on-request"',
      'sandbox = "workspace-write"',
      "[runtime_profiles.codex.env]",
      `CODEX_FAKE_SCENARIO = ${tomlString(scenario)}`,
      `CODEX_FAKE_LOG = ${tomlString(logPath)}`,
      ""
    ].join("\n"),
    expectedAnswer: "hello",
    logPath,
    root,
    runtimeRef: "codex"
  };
}

function prepareOpenCode(root: string): DeterministicRuntimeFixture {
  const executable = path.join(root, "fake-opencode.mjs");
  const logPath = path.join(root, "requests.log");
  writeFileSync(executable, FAKE_OPENCODE_SERVER);
  chmodSync(executable, 0o700);
  return {
    configAppend: [
      "[runtime_profiles.opencode]",
      'runtime = "opencode"',
      'label = "OpenCode"',
      `command = ${tomlString(executable)}`,
      'args = ["serve"]',
      'default_model = "fake/model"',
      'default_mode = "build"',
      'default_agent = "build"',
      "[runtime_profiles.opencode.env]",
      `RUNTIME_FAKE_LOG = ${tomlString(logPath)}`,
      ""
    ].join("\n"),
    expectedAnswer: "hello from deterministic OpenCode",
    logPath,
    root,
    runtimeRef: "opencode"
  };
}

function tomlString(value: string): string {
  return JSON.stringify(value);
}

const FAKE_OPENCODE_SERVER = String.raw`#!/usr/bin/env node
import { appendFileSync } from "node:fs";
import http from "node:http";

const username = process.env.OPENCODE_SERVER_USERNAME || "opencode";
const password = process.env.OPENCODE_SERVER_PASSWORD || "";
const expectedAuth = "Basic " + Buffer.from(username + ":" + password).toString("base64");
const requestLog = process.env.RUNTIME_FAKE_LOG;
const clients = new Set();
const messages = [];
let promptCount = 0;

function record(line) {
  if (requestLog) appendFileSync(requestLog, line + "\n");
}

function json(response, status, value) {
  response.writeHead(status, { "content-type": "application/json" });
  response.end(JSON.stringify(value));
}

function session(id, directory) {
  return {
    id,
    title: id,
    directory,
    time: { created: 1, updated: 2 },
    agent: "build",
    model: { id: "model", providerID: "fake" }
  };
}

function broadcast(value) {
  const line = "data: " + JSON.stringify(value) + "\n\n";
  for (const client of clients) client.write(line);
}

async function body(request) {
  let text = "";
  for await (const chunk of request) text += chunk.toString("utf8");
  return text ? JSON.parse(text) : {};
}

const server = http.createServer(async (request, response) => {
  const url = new URL(request.url || "/", "http://127.0.0.1");
  record((request.method || "GET") + " " + url.pathname + url.search);
  if (request.headers.authorization !== expectedAuth) {
    response.writeHead(401);
    response.end();
    return;
  }

  if (request.method === "GET" && url.pathname === "/global/health") {
    json(response, 200, { healthy: true, version: "1.17.17-fixture" });
    return;
  }
  if (request.method === "GET" && url.pathname === "/global/event") {
    response.writeHead(200, {
      "content-type": "text/event-stream",
      "cache-control": "no-cache",
      connection: "keep-alive"
    });
    clients.add(response);
    response.write("data: " + JSON.stringify({
      payload: { id: "evt_connected", type: "server.connected", properties: {} }
    }) + "\n\n");
    request.on("close", () => clients.delete(response));
    return;
  }

  const directory = url.searchParams.get("directory") || process.cwd();
  if (request.method === "POST" && url.pathname === "/session") {
    json(response, 200, session("ses_runtime_live", directory));
    return;
  }
  if (request.method === "GET" && url.pathname === "/session") {
    json(response, 200, [session("ses_runtime_live", directory)]);
    return;
  }
  if (request.method === "GET" && url.pathname === "/session/status") {
    json(response, 200, {});
    return;
  }
  if (request.method === "GET" && url.pathname === "/permission") {
    json(response, 200, []);
    return;
  }
  if (request.method === "GET" && url.pathname === "/question") {
    json(response, 200, []);
    return;
  }
  if (request.method === "GET" && url.pathname === "/agent") {
    json(response, 200, [{ name: "build", mode: "primary", hidden: false }]);
    return;
  }
  if (request.method === "GET" && url.pathname === "/mcp") {
    json(response, 200, {});
    return;
  }

  let match = url.pathname.match(/^\/session\/([^/]+)\/message$/);
  if (request.method === "GET" && match) {
    json(response, 200, messages);
    return;
  }
  match = url.pathname.match(/^\/session\/([^/]+)\/children$/);
  if (request.method === "GET" && match) {
    json(response, 200, []);
    return;
  }
  match = url.pathname.match(/^\/session\/([^/]+)\/todo$/);
  if (request.method === "GET" && match) {
    json(response, 200, []);
    return;
  }
  match = url.pathname.match(/^\/session\/([^/]+)\/diff$/);
  if (request.method === "GET" && match) {
    json(response, 200, []);
    return;
  }
  match = url.pathname.match(/^\/session\/([^/]+)\/prompt_async$/);
  if (request.method === "POST" && match) {
    const sessionId = decodeURIComponent(match[1]);
    const prompt = await body(request);
    const messageId = String(prompt.messageID || "msg_user");
    promptCount += 1;
    const assistantId = "msg_assistant_" + promptCount;
    const answer = "hello from deterministic OpenCode";
    const userInfo = {
      id: messageId,
      sessionID: sessionId,
      role: "user",
      agent: String(prompt.agent || "build"),
      model: prompt.model || { providerID: "fake", modelID: "model" },
      time: { created: 1 }
    };
    const info = {
      id: assistantId,
      sessionID: sessionId,
      role: "assistant",
      parentID: messageId,
      providerID: "fake",
      modelID: "model",
      time: { created: 2, completed: 3 }
    };
    messages.push({ info: userInfo, parts: Array.isArray(prompt.parts) ? prompt.parts : [] });
    messages.push({ info, parts: [{ id: "part_" + promptCount, type: "text", text: answer }] });
    response.writeHead(204);
    response.end();
    setTimeout(() => {
      broadcast({
        directory,
        payload: {
          id: "evt_todo_" + promptCount,
          type: "todo.updated",
          properties: {
            sessionID: sessionId,
            todos: [{ content: "Validate direct runtime", status: "in_progress", priority: "high" }]
          }
        }
      });
      broadcast({
        directory,
        payload: {
          id: "evt_diff_" + promptCount,
          type: "session.diff",
          properties: {
            sessionID: sessionId,
            diff: [{
              file: "src/runtime-live.ts",
              patch: "--- a/src/runtime-live.ts\n+++ b/src/runtime-live.ts\n@@ -0,0 +1 @@\n+verified",
              additions: 1,
              deletions: 0,
              status: "added"
            }]
          }
        }
      });
      broadcast({
        directory,
        payload: {
          id: "evt_assistant_" + promptCount,
          type: "message.updated",
          properties: { sessionID: sessionId, info }
        }
      });
      broadcast({
        directory,
        payload: {
          id: "evt_delta_" + promptCount,
          type: "message.part.delta",
          properties: {
            sessionID: sessionId,
            messageID: assistantId,
            partID: "part_" + promptCount,
            field: "text",
            delta: answer
          }
        }
      });
      broadcast({
        directory,
        payload: {
          id: "evt_idle_" + promptCount,
          type: "session.status",
          properties: { sessionID: sessionId, status: { type: "idle" } }
        }
      });
    }, 25);
    return;
  }
  match = url.pathname.match(/^\/session\/([^/]+)$/);
  if (request.method === "GET" && match) {
    json(response, 200, session(decodeURIComponent(match[1]), directory));
    return;
  }

  json(response, 404, { error: "not found" });
});

server.listen(0, "127.0.0.1", () => {
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : 0;
  record("ARGS " + process.argv.slice(2).join(" "));
  console.log("opencode server listening on http://127.0.0.1:" + port);
});

function shutdown() {
  for (const client of clients) client.end();
  server.close(() => process.exit(0));
  setTimeout(() => process.exit(0), 500).unref();
}
process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);
`;
