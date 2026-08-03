#!/usr/bin/env node
const fs = require("node:fs");
const readline = require("node:readline");
const { DatabaseSync } = require("node:sqlite");
let loadedSession = null;
const counterPath = `${__filename}.counter`;
const processCounterPath = `${__filename}.processes`;
let processCounter = 0;
try { processCounter = Number.parseInt(fs.readFileSync(processCounterPath, "utf8"), 10) || 0; } catch {}
fs.writeFileSync(processCounterPath, String(processCounter + 1), "utf8");
function send(value) { process.stdout.write(`${JSON.stringify(value)}\n`); }
function update(sessionId, value) { send({ jsonrpc: "2.0", method: "session/update", params: { sessionId, update: value } }); }
function bindingExists(sessionId) {
  const db = new DatabaseSync(process.env.PSYCHEVO_BINDING_DB, { readOnly: true });
  try {
    return db.prepare("SELECT native_session_id FROM gateway_runtime_bindings WHERE native_session_id = ?").get(sessionId)?.native_session_id === sessionId;
  } finally { db.close(); }
}
function handle(message) {
  const { method, id } = message; const params = message.params || {};
  if (method === "initialize") send({ jsonrpc: "2.0", id, result: { protocolVersion: 1, agentCapabilities: {} } });
  else if (method === "session/new") {
    let counter = 0; try { counter = Number.parseInt(fs.readFileSync(counterPath, "utf8"), 10) || 0; } catch {}
    counter += 1; fs.writeFileSync(counterPath, String(counter), "utf8");
    send({ jsonrpc: "2.0", id, result: { sessionId: `native-${counter}` } });
  } else if (method === "session/load") {
    loadedSession = params.sessionId;
    update(loadedSession, { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "old answer from loaded history" } });
    send({ jsonrpc: "2.0", id, result: {} });
  } else if (method === "session/prompt") {
    const sessionId = params.sessionId || "native-1";
    if (!bindingExists(sessionId)) throw new Error("native session binding was not persisted before prompt");
    const chunks = (params.prompt || []).filter((block) => block.type === "text").map((block) => block.text || "");
    const prefix = loadedSession ? `loaded:${loadedSession}` : `new:${sessionId}`;
    update(sessionId, { sessionUpdate: "agent_message_chunk", content: { type: "text", text: `${prefix}:${chunks.join("\n")}` } });
    send({ jsonrpc: "2.0", id, result: { stopReason: "end_turn" } });
  } else send({ jsonrpc: "2.0", id, error: { code: -32601, message: "method not found" } });
}
readline.createInterface({ input: process.stdin, crlfDelay: Infinity }).on("line", (line) => { if (line.trim()) handle(JSON.parse(line)); });
