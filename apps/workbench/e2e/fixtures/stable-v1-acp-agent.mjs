#!/usr/bin/env node
import { appendFileSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import readline from "node:readline";

const agent = process.argv[2] || "codex";
const scenario = process.argv[3] || "stream";
const logPath = process.argv[4];
const statePath = process.argv[5];
const agentVersion = process.argv[6] || (agent === "codex" ? "1.1.2" : "1.17.18");
const agentInfoName = process.argv[7] || (agent === "codex" ? "@agentclientprotocol/codex-acp" : "OpenCode");
const title = process.argv[8] || (agent === "codex" ? "Codex" : "OpenCode");
const journeyMode = process.argv[9] || "profile";
const journeyControlRoot = process.argv[10];
const journeyEventPath = process.argv[11];
let journeySequence = 0;

function defaultState() {
  return { nextSession: 1, promptCount: 0, sessions: {} };
}

function loadState() {
  if (!statePath || !existsSync(statePath)) return defaultState();
  try {
    return { ...defaultState(), ...JSON.parse(readFileSync(statePath, "utf8")) };
  } catch {
    return defaultState();
  }
}

let state = loadState();

function saveState() {
  if (statePath) writeFileSync(statePath, JSON.stringify(state, null, 2));
}

function record(type, fields = {}) {
  if (!logPath) return;
  appendFileSync(logPath, JSON.stringify({ type, agent, scenario, pid: process.pid, ...fields }) + "\n");
}

function recordJourney(event, requestIndex, sessionId) {
  if (scenario !== "critical_journey" || !journeyEventPath) return;
  appendFileSync(journeyEventPath, JSON.stringify({
    adapter: "acp",
    clock: "node-fixture",
    epochMs: Date.now(),
    event,
    monotonicNs: process.hrtime.bigint().toString(),
    plannedDelayMs: 0,
    purpose: "main_turn",
    purposeSequence: requestIndex,
    requestIndex,
    schemaVersion: 1,
    sequence: ++journeySequence,
    sessionId
  }) + "\n");
}

async function waitForJourneyRelease(stage, requestIndex) {
  if (scenario !== "critical_journey" || journeyMode !== "visual") return;
  if (!journeyControlRoot) throw new Error("critical journey visual mode requires a control root");
  const releasePath = path.join(journeyControlRoot, requestIndex + "." + stage + ".release");
  while (!existsSync(releasePath)) {
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
}

function send(value) {
  process.stdout.write(JSON.stringify(value) + "\n");
}

let nextClientRequestId = 100000;
const pendingClientRequests = new Map();

function clientRequest(method, params) {
  const id = nextClientRequestId++;
  record("client_request", { id, method, params });
  send({ jsonrpc: "2.0", id, method, params });
  return new Promise((resolve, reject) => {
    pendingClientRequests.set(id, { method, resolve, reject });
  });
}

function result(id, value) {
  if (id !== undefined && id !== null) send({ jsonrpc: "2.0", id, result: value });
}

function update(sessionId, value) {
  send({ jsonrpc: "2.0", method: "session/update", params: { sessionId, update: value } });
}

function sessionState(sessionId) {
  if (!state.sessions[sessionId]) {
    state.sessions[sessionId] = {
      config: { model: "fixture/default", effort: "medium", mode: "build" },
      cwd: null,
      messages: [],
      title: title + " ACP fixture"
    };
  }
  return state.sessions[sessionId];
}

function configOptions(sessionId) {
  const config = sessionState(sessionId).config;
  return [
    {
      id: "model",
      name: "Model",
      category: "model",
      type: "select",
      currentValue: config.model,
      options: [
        { value: "fixture/default", name: "Fixture default" },
        { value: "fixture/second", name: "Fixture second" }
      ]
    },
    {
      id: "effort",
      name: "Reasoning effort",
      category: "thought_level",
      type: "select",
      currentValue: config.effort,
      options: [
        { value: "low", name: "Low" },
        { value: "medium", name: "Medium" },
        { value: "high", name: "High" }
      ]
    },
    {
      id: "mode",
      name: "Mode",
      category: "mode",
      type: "select",
      currentValue: config.mode,
      options: [
        { value: "build", name: "Build" },
        { value: "plan", name: "Plan" }
      ]
    }
  ];
}

function initializeResponse() {
  // Keep this inventory aligned with the reviewed adapters, not a synthetic
  // ACP superset: .references/codex-acp/src/CodexAcpServer.ts initialize()
  // and .references/opencode/packages/opencode/src/acp/service.ts initialize().
  const processEphemeral = scenario === "process_ephemeral";
  const sessionCapabilities = processEphemeral
    ? { close: {} }
    : agent === "codex"
    ? {
        close: {},
        delete: {},
        list: {},
        resume: {},
        additionalDirectories: {}
      }
    : {
        close: {},
        fork: {},
        list: {},
        resume: {}
      };
  return {
    protocolVersion: 1,
    agentInfo: {
      name: agentInfoName,
      title,
      version: agentVersion
    },
    agentCapabilities: {
      loadSession: !processEphemeral,
      promptCapabilities: { embeddedContext: true, image: true },
      sessionCapabilities,
      mcpCapabilities: { acp: false, http: true, sse: agent === "opencode" }
    },
    authMethods: []
  };
}

function textParts(prompt) {
  return (Array.isArray(prompt) ? prompt : [])
    .filter((part) => part && part.type === "text")
    .map((part) => String(part.text || ""));
}

function partKinds(prompt) {
  return (Array.isArray(prompt) ? prompt : [])
    .map((part) => part && typeof part.type === "string" ? part.type : "unknown");
}

function replayHistory(sessionId) {
  const session = sessionState(sessionId);
  for (const message of session.messages) {
    if (message.update) {
      update(sessionId, message.update);
      continue;
    }
    if (message.role !== "assistant" && message.role !== "user") continue;
    update(sessionId, {
      sessionUpdate: message.role === "user" ? "user_message_chunk" : "agent_message_chunk",
      content: { type: "text", text: message.text },
      messageId: message.id
    });
  }
}

async function exerciseFilesystemAndPermissionCallbacks(sessionId, session) {
  if (!session.cwd) throw new Error("filesystem fixture session is missing cwd");
  const seedPath = session.cwd + "/acp-live-seed.txt";
  const writePath = session.cwd + "/acp-live-written.txt";
  const read = await clientRequest("fs/read_text_file", {
    sessionId,
    path: seedPath,
    line: 2,
    limit: 1
  });
  record("fs_read_result", { sessionId, path: seedPath, result: read });
  const writtenContent = "written through ACP fs/write_text_file";
  const write = await clientRequest("fs/write_text_file", {
    sessionId,
    path: writePath,
    content: writtenContent
  });
  record("fs_write_result", { sessionId, path: writePath, content: writtenContent, result: write });
  const permission = await clientRequest("session/request_permission", {
    sessionId,
    toolCall: {
      toolCallId: "fixture-explicit-permission",
      title: "Run deterministic ACP callback",
      kind: "execute",
      status: "pending",
      rawInput: { command: "fixture callback" }
    },
    options: [
      { optionId: "allow-once", name: "Allow once", kind: "allow_once" },
      { optionId: "allow-always", name: "Always allow", kind: "allow_always" },
      { optionId: "reject-once", name: "Reject", kind: "reject_once" }
    ]
  });
  record("permission_result", { sessionId, result: permission });
  return { read, write, permission, writePath, writtenContent };
}

async function exerciseOnceOnlyInteractions(sessionId) {
  const permission = await clientRequest("session/request_permission", {
    sessionId,
    toolCall: {
      toolCallId: "fixture-once-permission",
      title: "Approve the once-only interaction",
      kind: "execute",
      status: "pending",
      rawInput: { command: "fixture once" }
    },
    options: [
      { optionId: "allow-once", name: "Allow once", kind: "allow_once" },
      { optionId: "reject-once", name: "Reject", kind: "reject_once" }
    ]
  });
  record("once_permission_result", { sessionId, result: permission });
  const elicitation = await clientRequest("elicitation/create", {
    sessionId,
    mode: "form",
    message: "Which workspace should the once-only interaction use?",
    requestedSchema: {
      type: "object",
      properties: {
        workspace: {
          type: "string",
          title: "Workspace",
          description: "Enter the workspace name."
        }
      },
      required: ["workspace"]
    }
  });
  record("once_elicitation_result", { sessionId, result: elicitation });
  return { permission, elicitation };
}

async function exerciseTerminalLifecycle(sessionId, session) {
  if (!session.cwd) throw new Error("terminal fixture session is missing cwd");
  const completed = await clientRequest("terminal/create", {
    sessionId,
    command: process.execPath,
    args: [
      "-e",
      "process.stdout.write('terminal-live-ok:' + process.env.PSYCHEVO_ACP_TERMINAL_LIVE + '\\n')"
    ],
    cwd: session.cwd,
    env: [{ name: "PSYCHEVO_ACP_TERMINAL_LIVE", value: "yes" }],
    outputByteLimit: 4096
  });
  record("terminal_create_completed", { sessionId, result: completed });
  const completedId = completed.terminalId;
  const earlyOutput = await clientRequest("terminal/output", { sessionId, terminalId: completedId });
  record("terminal_output_early", { sessionId, result: earlyOutput });
  const completedWait = await clientRequest("terminal/wait_for_exit", {
    sessionId,
    terminalId: completedId
  });
  record("terminal_wait_completed", { sessionId, result: completedWait });
  const completedOutput = await clientRequest("terminal/output", { sessionId, terminalId: completedId });
  record("terminal_output_completed", { sessionId, result: completedOutput });
  const completedRelease = await clientRequest("terminal/release", {
    sessionId,
    terminalId: completedId
  });
  record("terminal_release_completed", { sessionId, result: completedRelease });

  const killed = await clientRequest("terminal/create", {
    sessionId,
    command: process.execPath,
    args: [
      "-e",
      "process.stdout.write('terminal-kill-ready\\n'); setInterval(() => {}, 1000)"
    ],
    cwd: session.cwd,
    outputByteLimit: 4096
  });
  record("terminal_create_killed", { sessionId, result: killed });
  const killedId = killed.terminalId;
  await new Promise((resolve) => setTimeout(resolve, 50));
  const killedOutput = await clientRequest("terminal/output", { sessionId, terminalId: killedId });
  record("terminal_output_before_kill", { sessionId, result: killedOutput });
  const kill = await clientRequest("terminal/kill", { sessionId, terminalId: killedId });
  record("terminal_kill_result", { sessionId, result: kill });
  const killedWait = await clientRequest("terminal/wait_for_exit", { sessionId, terminalId: killedId });
  record("terminal_wait_killed", { sessionId, result: killedWait });
  const killedRelease = await clientRequest("terminal/release", { sessionId, terminalId: killedId });
  record("terminal_release_killed", { sessionId, result: killedRelease });
  return { completed, completedWait, completedOutput, killed, killedOutput, kill, killedWait };
}

async function prompt(message, params) {
  const sessionId = params.sessionId;
  const session = sessionState(sessionId);
  const promptValue = params.prompt || [];
  const promptText = textParts(promptValue).join("\n");
  const kinds = partKinds(promptValue);
  state.promptCount += 1;
  const turn = state.promptCount;
  if (scenario === "critical_journey") {
    recordJourney("request_received", turn, sessionId);
  }
  const answer = title + " ACP response " + turn
    + "; model=" + session.config.model
    + "; effort=" + session.config.effort
    + "; mode=" + session.config.mode
    + "; parts=" + kinds.join(",");
  if (scenario !== "critical_journey") {
    session.messages.push({ id: "user-" + turn, role: "user", text: promptText });
    session.messages.push({ id: "assistant-" + turn, role: "assistant", text: answer });
  }
  saveState();
  record(
    "prompt_accepted",
    scenario === "critical_journey"
      ? { id: message.id, sessionId, turn, config: session.config }
      : { id: message.id, sessionId, turn, prompt: promptValue, config: session.config }
  );

  if (scenario === "critical_journey") {
    await waitForJourneyRelease("first-output", turn);
    const splitIndex = Math.ceil(answer.length / 2);
    update(sessionId, {
      sessionUpdate: "agent_message_chunk",
      messageId: "assistant-" + turn,
      content: { type: "text", text: answer.slice(0, splitIndex) }
    });
    recordJourney("first_output_emitted", turn, sessionId);
    await waitForJourneyRelease("completion", turn);
    update(sessionId, {
      sessionUpdate: "agent_message_chunk",
      messageId: "assistant-" + turn,
      content: { type: "text", text: answer.slice(splitIndex) }
    });
    result(message.id, { stopReason: "end_turn" });
    recordJourney("completion_emitted", turn, sessionId);
    return;
  }

  if (scenario === "unknown_delivery" && turn === 1) {
    record("connection_lost_after_acceptance", { sessionId, turn });
    setTimeout(() => process.exit(17), 10);
    return;
  }

  let callbackProof = null;
  if (scenario === "filesystem_permission") {
    callbackProof = await exerciseFilesystemAndPermissionCallbacks(sessionId, session);
  } else if (scenario === "interaction_once" || (scenario === "active_next_control" && turn === 2)) {
    callbackProof = await exerciseOnceOnlyInteractions(sessionId);
  } else if (scenario === "terminal_lifecycle") {
    callbackProof = await exerciseTerminalLifecycle(sessionId, session);
  }

  update(sessionId, { sessionUpdate: "session_info_update", title: session.title });
  update(sessionId, { sessionUpdate: "available_commands_update", availableCommands: [
    { name: "fixture_status", description: "Show deterministic ACP fixture status" }
  ] });
  update(sessionId, {
    sessionUpdate: "agent_thought_chunk",
    messageId: "thought-" + turn,
    content: { type: "text", text: "stable v1 reasoning" }
  });
  update(sessionId, {
    sessionUpdate: "agent_message_chunk",
    messageId: "assistant-" + turn,
    content: { type: "text", text: answer.slice(0, Math.ceil(answer.length / 2)) }
  });
  await new Promise((resolve) => setTimeout(resolve, 120));
  update(sessionId, {
    sessionUpdate: "agent_message_chunk",
    messageId: "assistant-" + turn,
    content: { type: "text", text: answer.slice(Math.ceil(answer.length / 2)) }
  });
  update(sessionId, {
    sessionUpdate: "tool_call",
    toolCallId: "fixture-tool-" + turn,
    title: "Inspect ACP fixture",
    kind: "execute",
    status: "pending",
    rawInput: { command: "fixture inspect" }
  });
  update(sessionId, {
    sessionUpdate: "tool_call_update",
    toolCallId: "fixture-tool-" + turn,
    status: "completed",
    content: [{ type: "content", content: { type: "text", text: "fixture complete\n" } }],
    rawOutput: { output: "fixture complete\n" }
  });
  update(sessionId, { sessionUpdate: "plan", entries: [
    { content: "Negotiate stable ACP v1", priority: "high", status: "completed" },
    { content: "Project through the common application path", priority: "high", status: "in_progress" }
  ] });
  update(sessionId, { sessionUpdate: "usage_update", used: 128 + turn, size: 4096 });
  if (callbackProof) {
    const callbackSummary = scenario === "interaction_once" || scenario === "active_next_control"
      ? "; interactions=permission,elicitation"
      : scenario === "terminal_lifecycle"
        ? "; callbacks=terminal.create,output,wait,kill,release"
        : "; callbacks=fs.read,fs.write,permission; terminal=unsupported";
    update(sessionId, {
      sessionUpdate: "agent_message_chunk",
      messageId: "assistant-" + turn,
      content: {
        type: "text",
        text: callbackSummary
      }
    });
  }
  result(message.id, { stopReason: "end_turn" });
  if (scenario === "history" && turn === 2) {
    record("connection_closed_after_completed_turn", { sessionId, turn });
    setTimeout(() => process.exit(0), 20);
  }
}

async function handle(message) {
  if (message && message.method === undefined && message.id !== undefined && message.id !== null) {
    const pending = pendingClientRequests.get(message.id);
    if (!pending) {
      record("unmatched_client_response", { id: message.id });
      return;
    }
    pendingClientRequests.delete(message.id);
    record("client_response", {
      id: message.id,
      method: pending.method,
      result: message.result ?? null,
      error: message.error ?? null
    });
    if (message.error) pending.reject(new Error(JSON.stringify(message.error)));
    else pending.resolve(message.result);
    return;
  }
  const method = message.method;
  const params = message.params || {};
  record(
    "request",
    scenario === "critical_journey" && method === "session/prompt"
      ? { id: message.id ?? null, method, sessionId: params.sessionId ?? null }
      : { id: message.id ?? null, method, params }
  );
  if (method === "initialize") {
    record("initialize", { requestedProtocolVersion: params.protocolVersion });
    const response = initializeResponse();
    record("initialize_result", {
      agentInfo: response.agentInfo,
      agentCapabilities: response.agentCapabilities,
      clientCapabilities: params.clientCapabilities ?? null
    });
    result(message.id, response);
    return;
  }
  if (method === "session/new") {
    const sessionId = agent + "-fixture-" + state.nextSession++;
    const session = sessionState(sessionId);
    session.cwd = params.cwd || null;
    saveState();
    record("session_new", { sessionId, mcpServers: params.mcpServers ?? [] });
    result(message.id, { sessionId, configOptions: configOptions(sessionId) });
    setTimeout(() => update(sessionId, {
      sessionUpdate: "available_commands_update",
      availableCommands: [
        { name: "fixture_status", description: "Show deterministic ACP fixture status" }
      ]
    }), 0);
    return;
  }
  if (method === "session/load" || method === "session/resume") {
    const sessionId = params.sessionId;
    const session = sessionState(sessionId);
    if (params.cwd) session.cwd = params.cwd;
    record(method === "session/load" ? "session_load" : "session_resume", {
      sessionId,
      mcpServers: params.mcpServers ?? []
    });
    if (method === "session/load") replayHistory(sessionId);
    result(message.id, { configOptions: configOptions(sessionId) });
    return;
  }
  if (method === "session/set_config_option") {
    const sessionId = params.sessionId;
    const session = sessionState(sessionId);
    if (["model", "effort", "mode"].includes(params.configId)) {
      session.config[params.configId] = params.value;
    }
    saveState();
    record("config_set", { sessionId, configId: params.configId, value: params.value });
    result(message.id, { configOptions: configOptions(sessionId) });
    return;
  }
  if (method === "session/prompt") {
    await prompt(message, params);
    return;
  }
  if (method === "session/cancel") {
    record("cancel", { sessionId: params.sessionId });
    result(message.id, {});
    return;
  }
  if (method === "session/list") {
    const sessions = Object.entries(state.sessions).map(([sessionId, session]) => ({
      sessionId,
      cwd: params.cwd,
      title: session.title
    }));
    result(message.id, { sessions, nextCursor: null });
    return;
  }
  if (method === "session/fork") {
    const source = sessionState(params.sessionId);
    const sessionId = agent + "-fixture-" + state.nextSession++;
    state.sessions[sessionId] = JSON.parse(JSON.stringify(source));
    saveState();
    result(message.id, { sessionId, configOptions: configOptions(sessionId) });
    return;
  }
  if (method === "session/close" || method === "session/delete") {
    record(method === "session/close" ? "session_close" : "session_delete", { sessionId: params.sessionId });
    if (method === "session/delete") delete state.sessions[params.sessionId];
    saveState();
    result(message.id, {});
    return;
  }
  send({ jsonrpc: "2.0", id: message.id, error: { code: -32601, message: "method not found: " + method } });
}

record("boot");
const input = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
input.on("line", (line) => {
  if (!line.trim()) return;
  try {
    void handle(JSON.parse(line)).catch((error) => {
      record("handler_error", { message: String(error?.stack || error) });
      send({ jsonrpc: "2.0", id: null, error: { code: -32603, message: String(error) } });
    });
  } catch (error) {
    record("parse_error", { message: String(error) });
  }
});
