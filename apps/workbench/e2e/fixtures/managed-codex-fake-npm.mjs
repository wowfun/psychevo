#!/usr/bin/env node
import {
  chmodSync,
  mkdirSync,
  readFileSync,
  symlinkSync,
  writeFileSync
} from "node:fs";
import path from "node:path";

const configPath = process.argv[2];
if (!configPath) throw new Error("missing managed Codex fixture config path");
const config = JSON.parse(readFileSync(configPath, "utf8"));
const expectedArgs = ["ci", "--omit=dev", "--ignore-scripts", "--no-audit", "--no-fund"];
const actualArgs = process.argv.slice(3);
if (JSON.stringify(actualArgs) !== JSON.stringify(expectedArgs)) {
  throw new Error("unexpected managed npm args: " + JSON.stringify(actualArgs));
}
if (process.env.PSYCHEVO_MANAGED_FIXTURE_CAPTURED !== "captured") {
  throw new Error("managed npm did not receive the Gateway-captured environment");
}

const packageRoot = path.join(process.cwd(), "node_modules", "@agentclientprotocol", "codex-acp");
const distRoot = path.join(packageRoot, "dist");
const binRoot = path.join(process.cwd(), "node_modules", ".bin");
mkdirSync(distRoot, { recursive: true });
mkdirSync(binRoot, { recursive: true });
writeFileSync(path.join(packageRoot, "package.json"), JSON.stringify({
  name: "@agentclientprotocol/codex-acp",
  version: "1.1.2"
}));

if (process.platform === "win32") {
  writeFileSync(
    path.join(binRoot, "codex-acp.cmd"),
    `@echo off\r\n"${process.execPath}" "${config.scriptPath}" codex managed "${config.logPath}" "${config.statePath}" "${config.version}"\r\n`
  );
} else {
  const launcher = path.join(distRoot, "cli.js");
  writeFileSync(
    launcher,
    `#!/bin/sh\nexec ${shellQuote(process.execPath)} ${shellQuote(config.scriptPath)} codex managed ${shellQuote(config.logPath)} ${shellQuote(config.statePath)} ${shellQuote(config.version)}\n`
  );
  chmodSync(launcher, 0o755);
  symlinkSync("../@agentclientprotocol/codex-acp/dist/cli.js", path.join(binRoot, "codex-acp"));
}

writeFileSync(config.npmLogPath, JSON.stringify({
  args: actualArgs,
  capturedMarker: process.env.PSYCHEVO_MANAGED_FIXTURE_CAPTURED,
  cwd: process.cwd(),
  path: process.env.PATH
}, null, 2));

function shellQuote(value) {
  return `'${value.replaceAll("'", `'"'"'`)}'`;
}
