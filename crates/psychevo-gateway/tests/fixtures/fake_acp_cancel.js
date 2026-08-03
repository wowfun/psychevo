#!/usr/bin/env node
const fs = require("node:fs");
const readline = require("node:readline");
const logPath = process.argv[2];
let promptId = null;

function send(value) { process.stdout.write(`${JSON.stringify(value)}\n`); }

readline.createInterface({ input: process.stdin, crlfDelay: Infinity }).on("line", (line) => {
  if (!line.trim()) return;
  const message = JSON.parse(line);
  const method = message.method;
  fs.appendFileSync(logPath, `${method || "response"}\n`, "utf8");
  if (method === "initialize") send({ jsonrpc: "2.0", id: message.id, result: { protocolVersion: 1, agentCapabilities: {} } });
  else if (method === "session/new") send({ jsonrpc: "2.0", id: message.id, result: { sessionId: "native-cancel" } });
  else if (method === "session/prompt") promptId = message.id;
  else if (method === "session/cancel" && promptId != null) {
    send({ jsonrpc: "2.0", id: promptId, result: { stopReason: "end_turn" } });
    promptId = null;
  }
});
