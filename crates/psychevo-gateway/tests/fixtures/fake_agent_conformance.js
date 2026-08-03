#!/usr/bin/env node
const fs = require("node:fs");
const path = require("node:path");
const readline = require("node:readline");
const { DatabaseSync } = require("node:sqlite");
const logPath = process.argv[2];
const bindingDb = process.argv[3];
const releaseDir = process.argv[4];
fs.mkdirSync(releaseDir, { recursive: true });
let nextSessionId = 0;
const pendingPrompts = new Map();
const pendingPermissions = new Map();
const fastBySession = new Map();
function send(value) { process.stdout.write(`${JSON.stringify(value)}\n`); }
function record(value) { fs.appendFileSync(logPath, `${JSON.stringify(value)}\n`, "utf8"); }
function update(sessionId, text) { send({ jsonrpc: "2.0", method: "session/update", params: { sessionId, update: { sessionUpdate: "agent_message_chunk", content: { type: "text", text } } } }); }
function completePrompt(sessionId, outcome = "end_turn") {
  const pending = pendingPrompts.get(sessionId); if (!pending) return;
  pendingPrompts.delete(sessionId); update(sessionId, `answer:${pending.prompt}`);
  send({ jsonrpc: "2.0", id: pending.id, result: { stopReason: outcome } });
  record({ event: "prompt_completed", sessionId, prompt: pending.prompt });
}
function bindingExists(sessionId) {
  const db = new DatabaseSync(bindingDb, { readOnly: true });
  try { return db.prepare("SELECT native_session_id FROM gateway_runtime_bindings WHERE native_session_id = ?").get(sessionId)?.native_session_id === sessionId; }
  finally { db.close(); }
}
const watcher = setInterval(() => {
  for (const [sessionId, pending] of pendingPrompts) {
    if (pending.prompt.startsWith("hold:") && fs.existsSync(path.join(releaseDir, `release-${pending.prompt.slice(5)}`))) completePrompt(sessionId);
  }
}, 5);
function handle(message) {
  const method = message.method; const id = message.id; const params = message.params || {};
  if (method === "initialize") {
    record({ event: "initialize" });
    send({ jsonrpc: "2.0", id, result: { protocolVersion: 1, agentCapabilities: { loadSession: true, promptCapabilities: { image: false, embeddedContext: false }, sessionCapabilities: { close: {} } } } });
  } else if (method === "session/new") {
    nextSessionId += 1; const sessionId = `conformance-session-${nextSessionId}`; fastBySession.set(sessionId, false);
    record({ event: "session_new", sessionId });
    send({ jsonrpc: "2.0", id, result: { sessionId, configOptions: [{ id: "fast", name: "Fast mode", type: "boolean", currentValue: false }] } });
  } else if (method === "session/load") {
    record({ event: "session_load", sessionId: params.sessionId }); send({ jsonrpc: "2.0", id, result: {} });
  } else if (method === "session/prompt") {
    const sessionId = params.sessionId;
    const textBlocks = (params.prompt || []).filter((block) => block.type === "text").map((block) => block.text || "");
    const prompt = textBlocks.at(-1) || ""; const bound = bindingExists(sessionId);
    record({ event: "prompt", sessionId, prompt, bindingBeforePrompt: bound });
    if (!bound) throw new Error("Agent session binding was not persisted before prompt");
    pendingPrompts.set(sessionId, { id, prompt });
    if (prompt === "crash-on-prompt") process.exit(0);
    if (prompt === "permission") {
      const permissionId = `permission-request-${sessionId}`; pendingPermissions.set(permissionId, sessionId);
      send({ jsonrpc: "2.0", id: permissionId, method: "session/request_permission", params: { sessionId, toolCall: { toolCallId: "permission-1", title: "Conformance tool", kind: "execute", status: "pending" }, options: [{ optionId: "allow-once", name: "Allow once", kind: "allow_once" }, { optionId: "reject-once", name: "Reject once", kind: "reject_once" }] } });
    } else if (!prompt.startsWith("hold:")) completePrompt(sessionId);
  } else if (method === "session/cancel") {
    record({ event: "cancel", sessionId: params.sessionId }); completePrompt(params.sessionId);
  } else if (method === "session/set_config_option") {
    fastBySession.set(params.sessionId, params.value);
    record({ event: "set_control", sessionId: params.sessionId, controlId: params.configId, value: params.value });
    send({ jsonrpc: "2.0", id, result: { configOptions: [{ id: "fast", name: "Fast mode", type: "boolean", currentValue: params.value }] } });
  } else if (method === "session/close") {
    record({ event: "close", sessionId: params.sessionId }); send({ jsonrpc: "2.0", id, result: {} });
  } else if (method == null && pendingPermissions.has(id)) {
    const sessionId = pendingPermissions.get(id); pendingPermissions.delete(id);
    record({ event: "permission_response", sessionId, result: message.result }); completePrompt(sessionId);
  } else if (id != null) send({ jsonrpc: "2.0", id, error: { code: -32601, message: "method not found" } });
}
const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
rl.on("line", (line) => { if (line.trim()) handle(JSON.parse(line)); });
rl.on("close", () => clearInterval(watcher));
