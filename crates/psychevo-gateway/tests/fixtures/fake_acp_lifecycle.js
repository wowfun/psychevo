#!/usr/bin/env node
const fs = require("node:fs");
const readline = require("node:readline");

const LOG_PATH = process.env.ACP_LIFECYCLE_LOG;
const MODE = process.env.ACP_LIFECYCLE_MODE || "all";
let nextCallbackId = 9000;
let cleanupProbes = [];
const sessionConfig = { model: "test/default", mode: "build" };
const sleepCell = new Int32Array(new SharedArrayBuffer(4));

function configOptionId(category) {
  return MODE === "custom-control-ids" ? `preferred-${category}` : category;
}

function configOptions() {
  return [
    {
      id: configOptionId("model"), name: "Model", category: "model", type: "select",
      currentValue: sessionConfig.model,
      options: [
        { value: "test/default", name: "Default model" },
        { value: "test/second", name: "Second model" },
      ],
    },
    {
      id: configOptionId("mode"), name: "Session Mode", category: "mode", type: "select",
      currentValue: sessionConfig.mode,
      options: [
        { value: "build", name: "build" },
        { value: "plan", name: "plan" },
      ],
    },
  ];
}

function legacyModels() {
  return {
    currentModelId: sessionConfig.model,
    availableModels: [
      { modelId: "test/default", name: "Default model", description: "Legacy default" },
      { modelId: "test/second", name: "Second model", description: "Legacy second" },
    ],
  };
}

function usesLegacyModels() {
  return ["legacy-models", "legacy-models-error", "legacy-models-and-config"].includes(MODE);
}

function emit(value) {
  process.stdout.write(`${JSON.stringify(value)}\n`);
}

function record(value) {
  fs.appendFileSync(LOG_PATH, `${JSON.stringify(value)}\n`, "utf8");
}

function respond(id, result) {
  emit({ jsonrpc: "2.0", id, result });
}

function fail(id, code, message, data) {
  const error = { code, message };
  if (data !== undefined) error.data = data;
  emit({ jsonrpc: "2.0", id, error });
}

function sessionUpdate(sessionId, update) {
  emit({ jsonrpc: "2.0", method: "session/update", params: { sessionId, update } });
}

function probeCleanedContext(sessionId) {
  const callbackId = nextCallbackId++;
  emit({
    jsonrpc: "2.0", id: callbackId, method: "fs/read_text_file",
    params: { sessionId, path: "/definitely-not-read-after-cleanup" },
  });
  record({ event: "cleanup_probe_sent", callbackId, sessionId });
}

function handle(message) {
  const method = message.method;
  const id = message.id;
  const params = message.params || {};
  if (method == null) {
    record({ event: "callback_response", callbackId: id, response: message });
    return;
  }
  record({ event: "request", method, params, hasId: id != null });

  if (method === "initialize") {
    const isCodex = MODE.startsWith("codex-auth-");
    const sessionCapabilities = ["none", "process-ephemeral"].includes(MODE)
      ? {}
      : { list: {}, delete: {}, fork: {}, resume: {}, close: {} };
    if (MODE === "no-delete") delete sessionCapabilities.delete;
    respond(id, {
      protocolVersion: MODE === "protocol-v2" ? 2 : 1,
      agentInfo: {
        name: isCodex ? "@agentclientprotocol/codex-acp" : "fixture-lifecycle-acp",
        title: isCodex ? "Codex ACP" : "Lifecycle fixture",
        version: MODE === "codex-auth-future" ? "1.1.3" : (isCodex ? "1.1.2" : "1.0.0"),
      },
      agentCapabilities: {
        loadSession: !["resume-only", "process-ephemeral"].includes(MODE),
        promptCapabilities: { image: isCodex, embeddedContext: isCodex },
        sessionCapabilities,
        mcpCapabilities: {},
      },
    });
  } else if (method === "authentication/status") {
    if (MODE === "codex-auth-unauthenticated") respond(id, { type: "unauthenticated" });
    else if (MODE === "codex-auth-api-key") respond(id, { type: "api-key" });
    else if (MODE === "codex-auth-chat-gpt") respond(id, { type: "chat-gpt", email: "fixture@example.test" });
    else if (MODE === "codex-auth-gateway") respond(id, { type: "gateway", name: "fixture-gateway" });
    else fail(id, -32601, "authentication/status is unavailable");
  } else if (method === "session/new") {
    if (MODE === "session-new-error") {
      fail(id, -32001, "fixture session preparation failed");
      return;
    }
    const result = { sessionId: "draft-native" };
    if (!["legacy-models", "legacy-models-error"].includes(MODE)) result.configOptions = configOptions();
    if (usesLegacyModels()) result.models = legacyModels();
    respond(id, result);
    sessionUpdate("draft-native", {
      sessionUpdate: "available_commands_update",
      availableCommands: [{ name: "fixture_status", description: "Show deterministic ACP fixture status" }],
    });
  } else if (method === "session/set_config_option") {
    for (const category of Object.keys(sessionConfig)) {
      if (params.configId === configOptionId(category)) sessionConfig[category] = params.value;
    }
    respond(id, { configOptions: configOptions() });
    sessionUpdate(params.sessionId, { sessionUpdate: "session_info_update", title: "Prepared fixture after control readback" });
    sessionUpdate(params.sessionId, { sessionUpdate: "usage_update", used: 12, size: 100 });
  } else if (method === "session/set_model") {
    if (MODE === "legacy-models-error") fail(id, -32001, "legacy model switch rejected");
    else {
      sessionConfig.model = params.modelId;
      respond(id, {});
    }
  } else if (method === "session/prompt") {
    const sessionId = params.sessionId;
    if (MODE === "blocking-prompt") {
      const releasePath = process.env.ACP_LIFECYCLE_RELEASE;
      record({ event: "prompt_blocked", sessionId });
      while (!fs.existsSync(releasePath)) Atomics.wait(sleepCell, 0, 0, 10);
    }
    sessionUpdate(sessionId, {
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text: "draft session response" },
    });
    respond(id, { stopReason: "end_turn" });
  } else if (method === "session/load") {
    const sessionId = params.sessionId;
    if (sessionId === "listed-native" && MODE === "history-replay-review") {
      sessionUpdate(sessionId, { sessionUpdate: "user_message_chunk", messageId: "history-user-reliable", content: { type: "text", text: "Reliable imported question" } });
      sessionUpdate(sessionId, { sessionUpdate: "user_message_chunk", content: { type: "text", text: "Unidentified imported question" } });
      sessionUpdate(sessionId, { sessionUpdate: "agent_message_chunk", messageId: "history-assistant-ordered", content: { type: "text", text: "Before tool" } });
      sessionUpdate(sessionId, { sessionUpdate: "tool_call", toolCallId: "history-tool-ordered", title: "Inspect ordered history", kind: "execute", status: "pending", rawInput: { cmd: "printf ordered" } });
      sessionUpdate(sessionId, { sessionUpdate: "tool_call_update", toolCallId: "history-tool-ordered", status: "completed", rawOutput: { output: "ordered tool output\n" } });
      sessionUpdate(sessionId, { sessionUpdate: "agent_message_chunk", messageId: "history-assistant-ordered", content: { type: "text", text: "After tool" } });
      sessionUpdate(sessionId, { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "Unidentified imported answer" } });
      for (const [content, status] of [["Inspect replay", "pending"], ["Implement replay", "in_progress"], ["Verify replay", "completed"]]) {
        sessionUpdate(sessionId, { sessionUpdate: "plan", entries: [{ content, priority: "high", status }] });
      }
    } else if (sessionId === "listed-native") {
      sessionUpdate(sessionId, { sessionUpdate: "user_message_chunk", messageId: "history-user-1", content: { type: "text", text: "Imported user question" } });
      sessionUpdate(sessionId, { sessionUpdate: "agent_thought_chunk", messageId: "history-assistant-1", content: { type: "text", text: "Imported reasoning" } });
      sessionUpdate(sessionId, { sessionUpdate: "agent_message_chunk", messageId: "history-assistant-1", content: { type: "text", text: "Imported assistant answer" } });
      sessionUpdate(sessionId, { sessionUpdate: "tool_call", toolCallId: "history-tool-1", title: "Inspect imported history", kind: "execute", status: "pending", rawInput: { cmd: "printf imported" } });
      sessionUpdate(sessionId, { sessionUpdate: "tool_call_update", toolCallId: "history-tool-1", status: "completed", content: [{ type: "content", content: { type: "text", text: "imported tool output\n" } }], rawOutput: { output: "imported tool output\n" } });
      sessionUpdate(sessionId, { sessionUpdate: "plan", entries: [{ content: "Verify imported replay", priority: "high", status: "completed" }] });
    }
    const result = {};
    if (!["legacy-models", "legacy-models-error"].includes(MODE)) result.configOptions = configOptions();
    if (usesLegacyModels()) result.models = legacyModels();
    respond(id, result);
  } else if (method === "session/resume") {
    const sessionId = params.sessionId;
    sessionUpdate(sessionId, { sessionUpdate: "current_mode_update", currentModeId: "resume-mode" });
    const result = {
      modes: { currentModeId: "resume-mode", availableModes: [{ id: "resume-mode", name: "Resume mode" }] },
      configOptions: [],
    };
    if (usesLegacyModels()) result.models = legacyModels();
    respond(id, result);
  } else if (method === "session/fork") {
    sessionUpdate("fork-native", { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "forked history" } });
    const result = { sessionId: "fork-native", configOptions: [] };
    if (usesLegacyModels()) result.models = legacyModels();
    respond(id, result);
  } else if (method === "session/list") {
    if (MODE === "auth-list") {
      fail(id, -32000, "Authentication\nrequired", { secret: "must-not-leak-from-agent-data" });
      return;
    }
    const pending = cleanupProbes;
    cleanupProbes = [];
    for (const sessionId of pending) probeCleanedContext(sessionId);
    const cwd = params.cwd || (process.platform === "win32" ? "C:\\fixture\\workspace" : "/fixture/workspace");
    respond(id, { sessions: [{ sessionId: "listed-native", cwd, title: "Listed fixture" }], nextCursor: "next-cursor" });
  } else if (method === "session/cancel") {
    // Notifications have no response.
  } else if (method === "session/close") {
    cleanupProbes.push(params.sessionId);
    respond(id, {});
  } else if (method === "session/delete") {
    if (MODE === "delete-fails") {
      fail(id, -32000, "fixture remote delete failed");
      return;
    }
    cleanupProbes.push(params.sessionId);
    respond(id, {});
  } else {
    fail(id, -32601, `unsupported fixture method: ${method}`);
  }
}

readline.createInterface({ input: process.stdin, crlfDelay: Infinity }).on("line", (line) => {
  if (line.trim()) handle(JSON.parse(line));
});
