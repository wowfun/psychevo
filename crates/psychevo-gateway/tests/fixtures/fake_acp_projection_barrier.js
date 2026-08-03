#!/usr/bin/env node
const readline = require("node:readline");

function send(value) {
  process.stdout.write(`${JSON.stringify(value)}\n`);
}

function update(sessionId, value) {
  send({ jsonrpc: "2.0", method: "session/update", params: { sessionId, update: value } });
}

function option(currentValue) {
  return {
    id: "model", name: "Model", category: "model", type: "select", currentValue,
    options: [
      { value: "from-response", name: "Response" },
      { value: "from-update", name: "Update" },
    ],
  };
}

function handle(message) {
  const method = message.method;
  const id = message.id;
  const params = message.params || {};
  if (method === "initialize") {
    send({ jsonrpc: "2.0", id, result: {
      protocolVersion: 1,
      agentInfo: { name: "fixture-acp", title: "Fixture ACP", version: "1.2.3" },
      agentCapabilities: {
        loadSession: true,
        promptCapabilities: { image: true, audio: true, embeddedContext: true },
        sessionCapabilities: {
          list: {}, delete: {}, fork: {}, resume: {}, close: {}, additionalDirectories: {},
        },
        auth: { logout: {} }, providers: {},
        mcpCapabilities: { http: true, sse: true, acp: true },
      },
    } });
  } else if (method === "session/load") {
    const sessionId = params.sessionId;
    update(sessionId, { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "loaded history" } });
    update(sessionId, { sessionUpdate: "available_commands_update", availableCommands: [{ name: "review", description: "Review this workspace", input: { hint: "workspace path", _meta: { secret: "drop" } } }] });
    update(sessionId, { sessionUpdate: "current_mode_update", currentModeId: "plan" });
    update(sessionId, { sessionUpdate: "config_option_update", configOptions: [option("from-update")] });
    update(sessionId, { sessionUpdate: "session_info_update", title: "Loaded fixture" });
    update(sessionId, { sessionUpdate: "usage_update", used: 42, size: 100, cost: { amount: 0.25, currency: "USD", _meta: { secret: "drop" } }, _meta: { secret: "drop" } });
    send({ jsonrpc: "2.0", id, result: {
      modes: { currentModeId: "ask", availableModes: [
        { id: "ask", name: "Ask", description: "Answer questions" },
        { id: "plan", name: "Plan", description: "Plan changes" },
      ] },
      configOptions: [option("from-response")],
    } });
    update(sessionId, { sessionUpdate: "current_mode_update", currentModeId: "ask" });
  } else if (method === "session/set_mode") {
    update(params.sessionId, { sessionUpdate: "current_mode_update", currentModeId: params.modeId });
    send({ jsonrpc: "2.0", id, result: {} });
  } else if (method === "session/close") {
    send({ jsonrpc: "2.0", id, result: {} });
  } else {
    send({ jsonrpc: "2.0", id, error: { code: -32601, message: "method not found" } });
  }
}

readline.createInterface({ input: process.stdin, crlfDelay: Infinity }).on("line", (line) => {
  if (line.trim()) handle(JSON.parse(line));
});
