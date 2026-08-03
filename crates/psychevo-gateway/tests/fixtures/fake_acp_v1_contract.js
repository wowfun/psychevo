#!/usr/bin/env node
const fs = require("node:fs");
const readline = require("node:readline");
const logPath = process.argv[2];
const values = { model: "test/default-model", effort: "low", mode: "ask", fast: false };
let nextSessionId = 0;
function send(value) { process.stdout.write(`${JSON.stringify(value)}\n`); }
function record(value) { fs.appendFileSync(logPath, `${JSON.stringify(value)}\n`, "utf8"); }
function update(sessionId, value) { send({ jsonrpc: "2.0", method: "session/update", params: { sessionId, update: value } }); }
function configOptions() { return [
  { id: "model", name: "Model", category: "model", type: "select", currentValue: values.model, options: [{ value: "test/default-model", name: "Default" }, { value: "test/second-model", name: "Second" }] },
  { id: "effort", name: "Effort", category: "thought_level", type: "select", currentValue: values.effort, options: [{ value: "low", name: "Low" }, { value: "high", name: "High" }] },
  { id: "mode", name: "Mode", category: "mode", type: "select", currentValue: values.mode, options: [{ value: "ask", name: "Ask" }, { value: "code", name: "Code" }] },
  { id: "fast", name: "Fast", type: "boolean", currentValue: values.fast },
]; }
function handle(message) {
  const { method, id } = message; const params = message.params || {};
  if (method === "initialize") {
    record({ event: "initialize", version: params.protocolVersion });
    send({ jsonrpc: "2.0", id, result: { protocolVersion: 1, agentCapabilities: { loadSession: true, promptCapabilities: { image: true, embeddedContext: true }, sessionCapabilities: { close: {} } } } });
  } else if (method === "session/new") {
    nextSessionId += 1; record({ event: "new" });
    send({ jsonrpc: "2.0", id, result: { sessionId: `native-v1-contract-${nextSessionId}`, configOptions: configOptions() } });
  } else if (method === "session/set_config_option") {
    let value = params.value;
    if (value && typeof value === "object") value = value.value ?? value.boolean;
    values[params.configId] = value; record({ event: "set", id: params.configId, value });
    send({ jsonrpc: "2.0", id, result: { configOptions: configOptions() } });
  } else if (method === "session/prompt") {
    const blocks = params.prompt || []; const types = blocks.map((block) => block.type);
    const resource = (blocks.find((block) => block.type === "resource") || {}).resource || {};
    const image = blocks.find((block) => block.type === "image") || {};
    record({ event: "prompt", types, resourceText: resource.text, resourceMime: resource.mimeType, imageMime: image.mimeType, imageDataLength: (image.data || "").length, values });
    update(params.sessionId, { sessionUpdate: "_future_status", label: "forward compatible" });
    update(params.sessionId, { sessionUpdate: "agent_message_chunk", content: { type: "text", text: `structured:${types.join(",")}:${values.model}:${values.effort}:${values.mode}:${String(values.fast)}` } });
    send({ jsonrpc: "2.0", id, result: { stopReason: "end_turn" } });
  } else if (method === "session/close") {
    record({ event: "close", sessionId: params.sessionId }); send({ jsonrpc: "2.0", id, result: {} });
  }
}
readline.createInterface({ input: process.stdin, crlfDelay: Infinity }).on("line", (line) => { if (line.trim()) handle(JSON.parse(line)); });
