#!/usr/bin/env node
const readline = require("readline");

readline.createInterface({ input: process.stdin, crlfDelay: Infinity }).on("line", (line) => {
  const request = JSON.parse(line);
  if (request.method === "tools/call") {
    setTimeout(() => {}, 30_000);
    return;
  }
  const result = request.method === "initialize" ? { ok: true } : {};
  process.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", id: request.id, result })}\n`);
});
