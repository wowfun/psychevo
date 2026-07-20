import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  writeFileSync
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const STABLE_V1_ACP_AGENT_PATH = fileURLToPath(
  new URL("./fixtures/stable-v1-acp-agent.mjs", import.meta.url)
);
const MANAGED_CODEX_FAKE_NPM_PATH = fileURLToPath(
  new URL("./fixtures/managed-codex-fake-npm.mjs", import.meta.url)
);

export type DeterministicAcpAgentKind = "codex" | "opencode";

export type DeterministicAcpScenario =
  | "active_next_control"
  | "capability_pack"
  | "channel_controls"
  | "critical_journey"
  | "filesystem_permission"
  | "history"
  | "interaction_once"
  | "managed"
  | "process_ephemeral"
  | "stream"
  | "terminal_lifecycle"
  | "unknown_delivery";

export type DeterministicJourneyMode = "profile" | "visual";

export type DeterministicJourneyEventName =
  | "request_received"
  | "first_output_emitted"
  | "completion_emitted";

export type DeterministicJourneyRequestPurpose = "async_title" | "main_turn";

export type DeterministicJourneyRequestSelector = {
  purpose: DeterministicJourneyRequestPurpose;
  sequence: number;
};

/**
 * A deliberately content-free observation emitted by a deterministic runtime
 * fixture. Epoch and monotonic values describe the fixture's Node clock only;
 * callers must not subtract them from browser or Rust monotonic clocks.
 */
export interface DeterministicJourneyEvent {
  adapter: "acp" | "native";
  clock: "node-fixture";
  epochMs: number;
  event: DeterministicJourneyEventName;
  monotonicNs: string;
  plannedDelayMs: number;
  purpose: DeterministicJourneyRequestPurpose;
  purposeSequence: number;
  requestIndex: number;
  schemaVersion: 1;
  sequence: number;
  sessionId?: string;
}

export interface DeterministicJourneyControl {
  mode: DeterministicJourneyMode;
  events(): DeterministicJourneyEvent[];
  waitFor(
    event: DeterministicJourneyEventName,
    request?: number | DeterministicJourneyRequestSelector,
    timeoutMs?: number
  ): Promise<DeterministicJourneyEvent>;
  releaseFirstOutput(request?: number | DeterministicJourneyRequestSelector): void;
  releaseCompletion(request?: number | DeterministicJourneyRequestSelector): void;
}

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
  journey: DeterministicJourneyControl | null;
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
  journey: DeterministicJourneyControl | null;
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
    journeyMode?: DeterministicJourneyMode;
  } = {}
): DeterministicAcpAgentFixture {
  const fakeRoot = path.join(artifactRoot, "acp-agent-fakes");
  mkdirSync(fakeRoot, { recursive: true });
  const root = mkdtempSync(path.join(fakeRoot, `${agent}-`));
  const scriptPath = path.join(root, "stable-v1-agent.mjs");
  const logPath = path.join(root, "agent.ndjson");
  const statePath = path.join(root, "state.json");
  const journeyMode = options.journeyMode ?? "profile";
  const journeyControlRoot = path.join(root, "journey-control");
  const journeyEventPath = path.join(root, "journey.ndjson");
  const journey = scenario === "critical_journey"
    ? createFileJourneyControl(journeyMode, journeyControlRoot, journeyEventPath)
    : null;
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
  const args = [
    scriptPath,
    agent,
    scenario,
    logPath,
    statePath,
    version,
    agentInfo.name,
    agentInfo.title,
    ...(scenario === "critical_journey"
      ? [journeyMode, journeyControlRoot, journeyEventPath]
      : [])
  ];
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
    journey,
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
  const configPath = path.join(options.root, "fake-npm-config.json");
  const npmLogPath = path.join(options.root, "fake-npm.json");
  mkdirSync(binDir, { recursive: true });
  copyFileSync(MANAGED_CODEX_FAKE_NPM_PATH, installerPath);
  chmodSync(installerPath, 0o755);
  writeFileSync(configPath, `${JSON.stringify({
    logPath: options.logPath,
    npmLogPath,
    scriptPath: options.scriptPath,
    statePath: options.statePath,
    version: options.version
  })}\n`);
  const npmPath = path.join(binDir, process.platform === "win32" ? "npm.cmd" : "npm");
  if (process.platform === "win32") {
    writeFileSync(
      npmPath,
      `@echo off\r\n"${process.execPath}" "${installerPath}" "${configPath}" %*\r\n`
    );
  } else {
    writeFileSync(
      npmPath,
      `#!/bin/sh\nexec ${shellQuote(process.execPath)} ${shellQuote(installerPath)} ${shellQuote(configPath)} "$@"\n`
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

export async function startDeterministicNativeModel(
  options: { journeyMode?: DeterministicJourneyMode } = {}
): Promise<DeterministicNativeModelFixture> {
  const expectedAnswer = "Native deterministic response";
  const requests: Array<Record<string, unknown>> = [];
  const purposeCounts: Record<DeterministicJourneyRequestPurpose, number> = {
    async_title: 0,
    main_turn: 0
  };
  const journeyRuntime = options.journeyMode
    ? createMemoryJourneyRuntime(options.journeyMode)
    : null;
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
    const requestBody = await readJsonBody(request);
    requests.push(requestBody);
    const requestIndex = requests.length;
    const purpose = classifyDeterministicNativeRequestPurpose(requestBody);
    const purposeSequence = ++purposeCounts[purpose];
    const requestIdentity = { purpose, purposeSequence, requestIndex };
    journeyRuntime?.record("request_received", requestIdentity, 0);
    response.writeHead(200, {
      "cache-control": "no-cache",
      "content-type": "text/event-stream",
      connection: "close"
    });
    if (!journeyRuntime) {
      response.write(`data: ${JSON.stringify({
        id: `native-live-${requestIndex}`,
        model: "default",
        choices: [{ index: 0, delta: { content: expectedAnswer }, finish_reason: "stop" }]
      })}\n\n`);
      response.end("data: [DONE]\n\n");
      return;
    }

    response.flushHeaders();
    const responseText = purpose === "async_title"
      ? "Deterministic session"
      : expectedAnswer;
    const firstOutputDelayMs = await journeyRuntime.waitForRelease("first-output", requestIndex);
    const splitIndex = Math.ceil(responseText.length / 2);
    response.write(`data: ${JSON.stringify({
      id: `native-live-${requestIndex}`,
      model: "default",
      choices: [{
        index: 0,
        delta: { content: responseText.slice(0, splitIndex) },
        finish_reason: null
      }]
    })}\n\n`);
    journeyRuntime.record("first_output_emitted", requestIdentity, firstOutputDelayMs);

    const completionDelayMs = await journeyRuntime.waitForRelease("completion", requestIndex);
    response.write(`data: ${JSON.stringify({
      id: `native-live-${requestIndex}`,
      model: "default",
      choices: [{
        index: 0,
        delta: { content: responseText.slice(splitIndex) },
        finish_reason: "stop"
      }]
    })}\n\n`);
    response.end("data: [DONE]\n\n");
    journeyRuntime.record("completion_emitted", requestIdentity, completionDelayMs);
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => resolve());
  });
  const address = server.address() as AddressInfo;
  return {
    baseUrl: `http://127.0.0.1:${address.port}/v1`,
    expectedAnswer,
    journey: journeyRuntime?.control ?? null,
    requests: () => [...requests],
    async stop() {
      journeyRuntime?.releaseAll();
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

type JourneyReleaseStage = "completion" | "first-output";

const DETERMINISTIC_PROFILE_STAGE_DELAY_MS = 32;

type JourneyRequestIdentity = {
  purpose: DeterministicJourneyRequestPurpose;
  purposeSequence: number;
  requestIndex: number;
};

function createFileJourneyControl(
  mode: DeterministicJourneyMode,
  controlRoot: string,
  eventPath: string
): DeterministicJourneyControl {
  mkdirSync(controlRoot, { recursive: true });
  const events = () => readJourneyEvents(eventPath);
  return {
    mode,
    events,
    waitFor: (event, request = 1, timeoutMs = 30_000) => (
      waitForJourneyEvent(events, event, request, timeoutMs)
    ),
    releaseFirstOutput(request = 1) {
      releaseFileJourneyGate(controlRoot, "first-output", resolveRequestIndex(events(), request));
    },
    releaseCompletion(request = 1) {
      releaseFileJourneyGate(controlRoot, "completion", resolveRequestIndex(events(), request));
    }
  };
}

function createMemoryJourneyRuntime(mode: DeterministicJourneyMode): {
  control: DeterministicJourneyControl;
  record(
    event: DeterministicJourneyEventName,
    request: JourneyRequestIdentity,
    plannedDelayMs: number
  ): void;
  releaseAll(): void;
  waitForRelease(stage: JourneyReleaseStage, requestIndex: number): Promise<number>;
} {
  const recorded: DeterministicJourneyEvent[] = [];
  const released = new Set<string>();
  const waiters = new Map<string, Set<() => void>>();
  let sequence = 0;
  let allReleased = false;
  const release = (stage: JourneyReleaseStage, requestIndex: number) => {
    const key = journeyGateKey(stage, requestIndex);
    released.add(key);
    const pending = waiters.get(key);
    waiters.delete(key);
    pending?.forEach((resolve) => resolve());
  };
  const events = () => recorded.map((event) => ({ ...event }));
  const control: DeterministicJourneyControl = {
    mode,
    events,
    waitFor: (event, request = 1, timeoutMs = 30_000) => (
      waitForJourneyEvent(events, event, request, timeoutMs)
    ),
    releaseFirstOutput: (request = 1) => release(
      "first-output",
      resolveRequestIndex(events(), request)
    ),
    releaseCompletion: (request = 1) => release(
      "completion",
      resolveRequestIndex(events(), request)
    )
  };
  return {
    control,
    record(event, request, plannedDelayMs) {
      recorded.push({
        adapter: "native",
        clock: "node-fixture",
        epochMs: Date.now(),
        event,
        monotonicNs: process.hrtime.bigint().toString(),
        plannedDelayMs,
        purpose: request.purpose,
        purposeSequence: request.purposeSequence,
        requestIndex: request.requestIndex,
        schemaVersion: 1,
        sequence: ++sequence
      });
    },
    releaseAll() {
      allReleased = true;
      for (const pending of waiters.values()) pending.forEach((resolve) => resolve());
      waiters.clear();
    },
    waitForRelease(stage, requestIndex) {
      if (allReleased) return Promise.resolve(0);
      if (mode === "profile") {
        return new Promise((resolve) => {
          setTimeout(() => resolve(DETERMINISTIC_PROFILE_STAGE_DELAY_MS), DETERMINISTIC_PROFILE_STAGE_DELAY_MS);
        });
      }
      const key = journeyGateKey(stage, requestIndex);
      if (released.has(key)) return Promise.resolve(0);
      return new Promise<number>((resolve) => {
        const pending = waiters.get(key) ?? new Set<() => void>();
        pending.add(() => resolve(0));
        waiters.set(key, pending);
      });
    }
  };
}

function releaseFileJourneyGate(
  controlRoot: string,
  stage: JourneyReleaseStage,
  requestIndex: number
): void {
  writeFileSync(path.join(controlRoot, journeyGateKey(stage, requestIndex)), "released\n");
}

function journeyGateKey(stage: JourneyReleaseStage, requestIndex: number): string {
  return `${requestIndex}.${stage}.release`;
}

function readJourneyEvents(eventPath: string): DeterministicJourneyEvent[] {
  try {
    return readFileSync(eventPath, "utf8").split("\n").flatMap((line) => {
      if (!line.trim()) return [];
      try {
        const event = JSON.parse(line) as Partial<DeterministicJourneyEvent>;
        return isDeterministicJourneyEvent(event) ? [event] : [];
      } catch {
        return [];
      }
    });
  } catch {
    return [];
  }
}

function isDeterministicJourneyEvent(
  value: Partial<DeterministicJourneyEvent>
): value is DeterministicJourneyEvent {
  return value.schemaVersion === 1
    && (value.adapter === "acp" || value.adapter === "native")
    && value.clock === "node-fixture"
    && typeof value.epochMs === "number"
    && ["request_received", "first_output_emitted", "completion_emitted"].includes(
      String(value.event)
    )
    && typeof value.monotonicNs === "string"
    && typeof value.plannedDelayMs === "number"
    && (value.purpose === "async_title" || value.purpose === "main_turn")
    && typeof value.purposeSequence === "number"
    && typeof value.requestIndex === "number"
    && typeof value.sequence === "number"
    && (value.sessionId === undefined || typeof value.sessionId === "string");
}

async function waitForJourneyEvent(
  readEvents: () => DeterministicJourneyEvent[],
  eventName: DeterministicJourneyEventName,
  request: number | DeterministicJourneyRequestSelector,
  timeoutMs: number
): Promise<DeterministicJourneyEvent> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const event = readEvents().find((candidate) => (
      candidate.event === eventName && matchesJourneyRequest(candidate, request)
    ));
    if (event) return event;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error(
    `timed out waiting for deterministic journey ${eventName} event for request ${JSON.stringify(request)}`
  );
}

function matchesJourneyRequest(
  event: DeterministicJourneyEvent,
  request: number | DeterministicJourneyRequestSelector
): boolean {
  return typeof request === "number"
    ? event.requestIndex === request
    : event.purpose === request.purpose && event.purposeSequence === request.sequence;
}

function resolveRequestIndex(
  events: DeterministicJourneyEvent[],
  request: number | DeterministicJourneyRequestSelector
): number {
  if (typeof request === "number") return request;
  const matched = events.find((event) => (
    event.event === "request_received" && matchesJourneyRequest(event, request)
  ));
  if (!matched) {
    throw new Error(`deterministic journey request is not yet observable: ${JSON.stringify(request)}`);
  }
  return matched.requestIndex;
}

export function classifyDeterministicNativeRequestPurpose(
  request: Record<string, unknown>
): DeterministicJourneyRequestPurpose {
  const messages = Array.isArray(request.messages) ? request.messages : [];
  const isTitle = messages.some((candidate) => {
    if (!candidate || typeof candidate !== "object" || Array.isArray(candidate)) return false;
    const message = candidate as Record<string, unknown>;
    return message.role === "system"
      && messageText(message.content).includes("Generate a concise title for this coding-agent session.");
  });
  return isTitle ? "async_title" : "main_turn";
}

function messageText(content: unknown): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content.flatMap((part) => {
    if (!part || typeof part !== "object" || Array.isArray(part)) return [];
    const text = (part as Record<string, unknown>).text;
    return typeof text === "string" ? [text] : [];
  }).join("\n");
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
