#!/usr/bin/env node
const readline = require("readline");

readline.createInterface({ input: process.stdin, crlfDelay: Infinity }).on("line", (line) => {
  const request = JSON.parse(line);
  let result = {};
  if (request.method === "initialize") {
    result = { ok: true };
  } else if (request.method === "contributions/list") {
    result = {
      tools: [{ name: "cleanup_status", description: "status", parameters: { type: "object", properties: {} } }],
    };
  } else if (request.method === "tools/call") {
    result = { json: { status: "ok", plugin: request.params.name }, content: "ok" };
  }
  process.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", id: request.id, result })}\n`);
});
