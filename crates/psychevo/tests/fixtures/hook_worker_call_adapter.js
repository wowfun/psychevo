#!/usr/bin/env node
const readline = require("readline");

readline.createInterface({ input: process.stdin, crlfDelay: Infinity }).on("line", (line) => {
  const request = JSON.parse(line);
  let result = {};
  if (request.method === "initialize" || request.method === "shutdown") {
    result = { ok: true };
  } else if (request.method === "hooks/call") {
    result = { feedback: "worker saw hook" };
  }
  process.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", id: request.id, result })}\n`);
});
