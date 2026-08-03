#!/usr/bin/env node
const fs = require("fs");
const path = require("path");
const readline = require("readline");

readline.createInterface({ input: process.stdin, crlfDelay: Infinity }).on("line", (line) => {
  const request = JSON.parse(line);
  let result = {};
  if (request.method === "initialize" || request.method === "shutdown") {
    result = { ok: true };
  } else if (request.method === "contributions/list") {
    result = { tools: [] };
  } else if (request.method === "hooks/call") {
    const data = process.env.PSYCHEVO_PLUGIN_DATA;
    fs.mkdirSync(data, { recursive: true });
    fs.appendFileSync(
      path.join(data, "child-hook.jsonl"),
      `${JSON.stringify({ event: request.params?.hook?.event })}\n`,
      "utf8",
    );
    result = { feedback: "plugin child hook ran" };
  }
  process.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", id: request.id, result })}\n`);
});
