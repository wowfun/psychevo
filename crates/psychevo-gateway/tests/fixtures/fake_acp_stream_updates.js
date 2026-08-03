#!/usr/bin/env node
const readline = require("node:readline");
let promptCount = 0;
function send(value) { process.stdout.write(`${JSON.stringify(value)}\n`); }
function update(sessionId, value) { send({ jsonrpc: "2.0", method: "session/update", params: { sessionId, update: value } }); }

function handle(message) {
  const { method, id } = message;
  const params = message.params || {};
  if (method === "initialize") send({ jsonrpc: "2.0", id, result: { protocolVersion: 1, agentCapabilities: {} } });
  else if (method === "session/new") send({ jsonrpc: "2.0", id, result: { sessionId: "native-stream" } });
  else if (method === "session/prompt") {
    promptCount += 1;
    const sessionId = params.sessionId || "native-stream";
    update(sessionId, { sessionUpdate: "session_info_update", title: "ACP streamed title" });
    update(sessionId, { sessionUpdate: "available_commands_update", availableCommands: [{ name: "research", description: "Run peer research" }] });
    update(sessionId, { sessionUpdate: "agent_thought_chunk", content: { type: "text", text: "think " } });
    update(sessionId, { sessionUpdate: "agent_thought_chunk", content: { type: "text", text: "first" } });
    update(sessionId, { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "hello " } });
    update(sessionId, { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "world" } });
    update(sessionId, { sessionUpdate: "tool_call", toolCallId: "call-echo", title: "Run echo", kind: "execute", status: "pending", rawInput: { cmd: "echo done" } });
    update(sessionId, { sessionUpdate: "tool_call_update", toolCallId: "call-echo", status: "in_progress", content: [{ type: "content", content: { type: "text", text: "running\n" } }] });
    update(sessionId, { sessionUpdate: "plan", entries: [{ content: "Inspect repo", priority: "high", status: "completed" }, { content: "Patch bridge", priority: "high", status: "in_progress" }] });
    update(sessionId, { sessionUpdate: "plan", entries: [{ content: "Persist replacement plan", priority: "high", status: "completed" }, { content: "Verify terminal history", priority: "high", status: "in_progress" }] });
    update(sessionId, { sessionUpdate: "tool_call_update", toolCallId: "call-echo", status: "completed", content: [{ type: "content", content: { type: "text", text: "done\n" } }], rawOutput: { output: "done\n" } });
    send({ jsonrpc: "2.0", id, result: { stopReason: "end_turn", usage: {
      totalTokens: promptCount === 1 ? 144 : 200,
      inputTokens: promptCount === 1 ? 100 : 140,
      outputTokens: promptCount === 1 ? 44 : 60,
      cachedReadTokens: promptCount === 1 ? 30 : 50,
      thoughtTokens: promptCount === 1 ? 4 : 8,
    } } });
    update(sessionId, { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "must remain after the response fence" } });
  } else send({ jsonrpc: "2.0", id, error: { code: -32601, message: "method not found" } });
}
readline.createInterface({ input: process.stdin, crlfDelay: Infinity }).on("line", (line) => { if (line.trim()) handle(JSON.parse(line)); });
