#!/usr/bin/env node
const fs = require("node:fs");
const readline = require("node:readline");
const logPath = process.argv[2];
const statePath = process.argv[3];
function loadState() { try { return JSON.parse(fs.readFileSync(statePath, "utf8")); } catch { return { promptCount: 0, messages: [] }; } }
function saveState(state) { fs.writeFileSync(statePath, JSON.stringify(state), "utf8"); }
function record(method, fields = {}) { fs.appendFileSync(logPath, `${JSON.stringify({ method, ...fields })}\n`, "utf8"); }
function send(value) { process.stdout.write(`${JSON.stringify(value)}\n`); }
function update(sessionId, messageId, text) { send({ jsonrpc: "2.0", method: "session/update", params: { sessionId, update: { sessionUpdate: "agent_message_chunk", messageId, content: { type: "text", text } } } }); }
function handle(message) {
  const { method, id } = message; const params = message.params || {};
  if (method === "initialize") { record(method); send({ jsonrpc: "2.0", id, result: { protocolVersion: 1, agentCapabilities: { loadSession: true } } }); }
  else if (method === "session/new") { record(method); send({ jsonrpc: "2.0", id, result: { sessionId: "native-reconcile" } }); }
  else if (method === "session/load") {
    const state = loadState(); record(method, { sessionId: params.sessionId });
    send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: params.sessionId, update: { sessionUpdate: "tool_call", toolCallId: "replayed-tool-only", title: "Replay tool-only fact", kind: "execute", status: "completed" } } });
    send({ jsonrpc: "2.0", method: "session/update", params: { sessionId: params.sessionId, update: { sessionUpdate: "plan", entries: [{ content: "Replay replacement plan", priority: "high", status: "completed" }] } } });
    for (const replay of state.messages) update(params.sessionId, replay.id, replay.text);
    send({ jsonrpc: "2.0", id, result: {} });
  } else if (method === "session/prompt") {
    const state = loadState(); state.promptCount += 1; const turn = state.promptCount;
    const prompt = (params.prompt || []).filter((block) => block.type === "text").map((block) => block.text || "").join("\n");
    const answer = `reconciled answer ${turn}`; const replay = { id: `assistant-${turn}`, text: answer };
    state.messages.push(replay); saveState(state); record(method, { turn, prompt });
    if (turn === 1) process.exit(17);
    update(params.sessionId, replay.id, answer); send({ jsonrpc: "2.0", id, result: { stopReason: "end_turn" } });
  } else send({ jsonrpc: "2.0", id, error: { code: -32601, message: "method not found" } });
}
readline.createInterface({ input: process.stdin, crlfDelay: Infinity }).on("line", (line) => { if (line.trim()) handle(JSON.parse(line)); });
