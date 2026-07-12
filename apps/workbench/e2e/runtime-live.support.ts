import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const STABLE_V1_ACP_AGENT_PATH = fileURLToPath(
  new URL("./fixtures/stable-v1-acp-agent.mjs", import.meta.url)
);

export type DeterministicAcpAgentKind = "codex" | "opencode";

export type DeterministicAcpScenario =
  | "active_next_control"
  | "capability_pack"
  | "channel_controls"
  | "filesystem_permission"
  | "history"
  | "interaction_once"
  | "managed"
  | "process_ephemeral"
  | "stream"
  | "terminal_lifecycle"
  | "unknown_delivery";

export interface DeterministicAcpAgentFixture {
  agent: DeterministicAcpAgentKind;
  agentInfo: { name: string; title: string; version: string };
  args: string[];
  command: string;
  configAppend: string;
  expectedAnswer: string;
  expectedMcpServers: Array<Record<string, unknown>>;
  fakeNpmLogPath: string | null;
  fakeNpmPath: string | null;
  installEnv: NodeJS.ProcessEnv | null;
  logPath: string;
  managedBinPath: string | null;
  managedRootPath: string | null;
  managedSealPath: string | null;
  profileLabel: string;
  root: string;
  runtimeRef: string;
  statePath: string;
  version: string;
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

export interface DeterministicNativeModelFixture {
  baseUrl: string;
  expectedAnswer: string;
  requests(): Array<Record<string, unknown>>;
  stop(): Promise<void>;
}

/**
 * Creates a source-backed ACP wire-v1 Agent fixture. Both named variants use the
 * same protocol implementation; only agentInfo/capabilities differ. This is
 * intentional: the public application path must not depend on a Codex or
 * OpenCode transport branch.
 */
export function prepareDeterministicAcpAgent(
  agent: DeterministicAcpAgentKind,
  artifactRoot: string,
  scenario: DeterministicAcpScenario = "stream",
  options: {
    agentVersion?: string;
    clientCapabilities?: Array<"fs.read" | "fs.write" | "terminal">;
    home?: string;
    mcpServers?: string[];
    profileLabel?: string;
    runtimeRef?: string;
    agentInfo?: { name: string; title: string };
  } = {}
): DeterministicAcpAgentFixture {
  const fakeRoot = path.join(artifactRoot, "acp-agent-fakes");
  mkdirSync(fakeRoot, { recursive: true });
  const root = mkdtempSync(path.join(fakeRoot, `${agent}-`));
  const scriptPath = path.join(root, "stable-v1-agent.mjs");
  const logPath = path.join(root, "agent.ndjson");
  const statePath = path.join(root, "state.json");
  copyFileSync(STABLE_V1_ACP_AGENT_PATH, scriptPath);
  chmodSync(scriptPath, 0o755);

  const defaultTitle = agent === "codex" ? "Codex" : "OpenCode";
  const agentInfo = {
    name: options.agentInfo?.name
      ?? (agent === "codex" ? "@agentclientprotocol/codex-acp" : "OpenCode"),
    title: options.agentInfo?.title ?? defaultTitle,
    version: options.agentVersion ?? (agent === "codex" ? "1.1.2" : "1.17.18")
  };
  const runtimeRef = options.runtimeRef
    ?? (agent === "codex" && scenario !== "managed" ? "codex-fixture" : agent);
  const profileLabel = options.profileLabel ?? defaultTitle;
  const version = agentInfo.version;
  const expectedAnswer = `${agentInfo.title} ACP response`;
  const expectedMcpServers = (options.mcpServers ?? []).map((name) => ({
    type: "http",
    name,
    url: `http://127.0.0.1:9/${encodeURIComponent(name)}`,
    headers: [{ name: "X-Psychevo-Live", value: "deterministic" }]
  }));
  const args = [scriptPath, agent, scenario, logPath, statePath, version, agentInfo.name, agentInfo.title];
  let fakeNpmLogPath: string | null = null;
  let fakeNpmPath: string | null = null;
  let installEnv: NodeJS.ProcessEnv | null = null;
  let managedBinPath: string | null = null;
  let managedRootPath: string | null = null;
  let managedSealPath: string | null = null;
  let command = process.execPath;
  let commandArgs = args;

  if (scenario === "managed") {
    if (!options.home) {
      throw new Error("managed Codex ACP fixture requires an isolated Psychevo home");
    }
    if (agent !== "codex") {
      throw new Error("only the Codex ACP fixture has a managed distribution contract");
    }
    managedRootPath = path.join(
      options.home,
      "runtime-adapters",
      "codex-acp",
      "1.1.2"
    );
    managedBinPath = path.join(
      managedRootPath,
      "node_modules",
      ".bin",
      process.platform === "win32" ? "codex-acp.cmd" : "codex-acp"
    );
    managedSealPath = path.join(managedRootPath, ".psychevo-tree-seal.json");
    const fakeNpm = prepareManagedCodexFakeNpm({ logPath, root, scriptPath, statePath, version });
    fakeNpmLogPath = fakeNpm.logPath;
    fakeNpmPath = fakeNpm.path;
    installEnv = fakeNpm.env;
    command = managedBinPath;
    commandArgs = [];
  }

  return {
    agent,
    agentInfo,
    args: commandArgs,
    command,
    configAppend: scenario === "managed"
      ? ""
      : acpBackendAndProfileConfig(
          runtimeRef,
          profileLabel,
          command,
          commandArgs,
          options.clientCapabilities ?? ["fs.read", "fs.write", "terminal"],
          options.mcpServers ?? []
        ),
    expectedAnswer,
    expectedMcpServers,
    fakeNpmLogPath,
    fakeNpmPath,
    installEnv,
    logPath,
    managedBinPath,
    managedRootPath,
    managedSealPath,
    profileLabel,
    root,
    runtimeRef,
    statePath,
    version
  };
}

function prepareManagedCodexFakeNpm(options: {
  logPath: string;
  root: string;
  scriptPath: string;
  statePath: string;
  version: string;
}): { env: NodeJS.ProcessEnv; logPath: string; path: string } {
  const binDir = path.join(options.root, "fake-npm-bin");
  const installerPath = path.join(options.root, "fake-npm.mjs");
  const npmLogPath = path.join(options.root, "fake-npm.json");
  mkdirSync(binDir, { recursive: true });
  writeFileSync(installerPath, `
import { chmodSync, mkdirSync, symlinkSync, writeFileSync } from "node:fs";
import path from "node:path";

const expectedArgs = ["ci", "--omit=dev", "--ignore-scripts", "--no-audit", "--no-fund"];
const actualArgs = process.argv.slice(2);
if (JSON.stringify(actualArgs) !== JSON.stringify(expectedArgs)) {
  throw new Error("unexpected managed npm args: " + JSON.stringify(actualArgs));
}
if (process.env.PSYCHEVO_MANAGED_FIXTURE_CAPTURED !== "captured") {
  throw new Error("managed npm did not receive the Gateway-captured environment");
}
const packageRoot = path.join(process.cwd(), "node_modules", "@agentclientprotocol", "codex-acp");
const distRoot = path.join(packageRoot, "dist");
const binRoot = path.join(process.cwd(), "node_modules", ".bin");
mkdirSync(distRoot, { recursive: true });
mkdirSync(binRoot, { recursive: true });
writeFileSync(path.join(packageRoot, "package.json"), JSON.stringify({
  name: "@agentclientprotocol/codex-acp",
  version: "1.1.2"
}));
if (process.platform === "win32") {
  writeFileSync(
    path.join(binRoot, "codex-acp.cmd"),
    ${JSON.stringify(`@echo off\r\n"${process.execPath}" "${options.scriptPath}" codex managed "${options.logPath}" "${options.statePath}" "${options.version}"\r\n`)}
  );
} else {
  const launcher = path.join(distRoot, "cli.js");
  writeFileSync(
    launcher,
    ${JSON.stringify(`#!/bin/sh\nexec ${shellQuote(process.execPath)} ${shellQuote(options.scriptPath)} codex managed ${shellQuote(options.logPath)} ${shellQuote(options.statePath)} ${shellQuote(options.version)}\n`)}
  );
  chmodSync(launcher, 0o755);
  symlinkSync("../@agentclientprotocol/codex-acp/dist/cli.js", path.join(binRoot, "codex-acp"));
}
writeFileSync(${JSON.stringify(npmLogPath)}, JSON.stringify({
  args: actualArgs,
  capturedMarker: process.env.PSYCHEVO_MANAGED_FIXTURE_CAPTURED,
  cwd: process.cwd(),
  path: process.env.PATH
}, null, 2));
`);
  const npmPath = path.join(binDir, process.platform === "win32" ? "npm.cmd" : "npm");
  if (process.platform === "win32") {
    writeFileSync(npmPath, `@echo off\r\n"${process.execPath}" "${installerPath}" %*\r\n`);
  } else {
    writeFileSync(
      npmPath,
      `#!/bin/sh\nexec ${shellQuote(process.execPath)} ${shellQuote(installerPath)} "$@"\n`
    );
    chmodSync(npmPath, 0o755);
  }
  return {
    env: {
      PATH: [binDir, process.env.PATH ?? ""].filter(Boolean).join(path.delimiter),
      PSYCHEVO_MANAGED_FIXTURE_CAPTURED: "captured",
      npm_config_offline: "true"
    },
    logPath: npmLogPath,
    path: npmPath
  };
}

export async function startDeterministicNativeModel(): Promise<DeterministicNativeModelFixture> {
  const expectedAnswer = "Native deterministic response";
  const requests: Array<Record<string, unknown>> = [];
  const server = createServer(async (request, response) => {
    if (request.method === "GET" && request.url === "/v1/models") {
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({ data: [{ id: "default" }] }));
      return;
    }
    if (request.method !== "POST" || request.url !== "/v1/chat/completions") {
      response.writeHead(404);
      response.end();
      return;
    }
    requests.push(await readJsonBody(request));
    response.writeHead(200, {
      "cache-control": "no-cache",
      "content-type": "text/event-stream",
      connection: "close"
    });
    response.write(`data: ${JSON.stringify({
      id: `native-live-${requests.length}`,
      model: "default",
      choices: [{ index: 0, delta: { content: expectedAnswer }, finish_reason: "stop" }]
    })}\n\n`);
    response.end("data: [DONE]\n\n");
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => resolve());
  });
  const address = server.address() as AddressInfo;
  return {
    baseUrl: `http://127.0.0.1:${address.port}/v1`,
    expectedAnswer,
    requests: () => [...requests],
    async stop() {
      server.closeAllConnections?.();
      await new Promise<void>((resolve) => server.close(() => resolve()));
    }
  };
}

export async function startDeterministicTelegram(): Promise<DeterministicTelegramFixture> {
  const token = "agent-acp-live-token";
  const credentialEnv = "AGENT_ACP_LIVE_TELEGRAM_TOKEN";
  const baseUrlEnv = "AGENT_ACP_LIVE_TELEGRAM_BASE_URL";
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
          from: { id: 42, username: "agent-acp-live" }
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

function acpBackendAndProfileConfig(
  id: string,
  label: string,
  command: string,
  args: string[],
  clientCapabilities: Array<"fs.read" | "fs.write" | "terminal">,
  mcpServers: string[]
): string {
  return [
    `[agents.backends.${id}]`,
    'kind = "acp"',
    "enabled = true",
    `label = ${tomlString(label)}`,
    `description = ${tomlString(`${label} stable ACP v1 deterministic Agent.`)}`,
    `command = ${tomlString(command)}`,
    `args = ${JSON.stringify(args)}`,
    'entrypoints = ["peer", "subagent"]',
    `client_capabilities = ${JSON.stringify(clientCapabilities)}`,
    `mcp_servers = ${JSON.stringify(mcpServers)}`,
    "",
    `[runtime_profiles.${id}]`,
    'runtime = "acp"',
    "enabled = true",
    `label = ${tomlString(label)}`,
    `backend_ref = ${tomlString(id)}`,
    'default_model = "fixture/default"',
    'default_mode = "build"',
    "",
    ...mcpServers.flatMap((name) => [
      `[mcp_servers.${tomlString(name)}]`,
      'transport = "streamable_http"',
      `url = ${tomlString(`http://127.0.0.1:9/${encodeURIComponent(name)}`)}`,
      'headers = { "X-Psychevo-Live" = "deterministic" }',
      ""
    ])
  ].join("\n");
}

function tomlString(value: string): string {
  return JSON.stringify(value);
}

function shellQuote(value: string): string {
  return `'${value.replaceAll("'", `'"'"'`)}'`;
}

async function readJsonBody(request: import("node:http").IncomingMessage): Promise<Record<string, unknown>> {
  let body = "";
  for await (const chunk of request) body += chunk.toString("utf8");
  if (!body) return {};
  const parsed = JSON.parse(body) as unknown;
  return parsed && typeof parsed === "object" && !Array.isArray(parsed)
    ? parsed as Record<string, unknown>
    : {};
}
