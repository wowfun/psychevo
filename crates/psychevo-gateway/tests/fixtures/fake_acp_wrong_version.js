#!/usr/bin/env node
const fs = require("node:fs");
const readline = require("node:readline");
const logPath = process.argv[2];

readline.createInterface({ input: process.stdin, crlfDelay: Infinity }).on("line", (line) => {
  if (!line.trim()) return;
  const message = JSON.parse(line);
  fs.appendFileSync(logPath, `${JSON.stringify({ method: message.method })}\n`, "utf8");
  if (message.method === "initialize") {
    process.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", id: message.id, result: { protocolVersion: 2, agentCapabilities: {} } })}\n`);
  }
});
