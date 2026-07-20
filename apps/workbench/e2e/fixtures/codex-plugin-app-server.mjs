#!/usr/bin/env node
import { readFileSync, writeFileSync } from "node:fs";
import readline from "node:readline";

const statePath = process.env.PSYCHEVO_CODEX_PLUGIN_FIXTURE_STATE;
if (!statePath) throw new Error("missing PSYCHEVO_CODEX_PLUGIN_FIXTURE_STATE");
const readState = () => JSON.parse(readFileSync(statePath, "utf8"));
const reply = (message) => process.stdout.write(JSON.stringify(message) + "\n");
const catalog = (state) => ({
  marketplaces: [{
    name: "openai",
    path: null,
    plugins: [{
      id: "review@openai",
      name: "review",
      description: "Review changes with Codex Apps",
      installed: state.installed,
      enabled: state.installed,
      localVersion: state.installed ? "1.0.0" : null
    }]
  }],
  marketplaceLoadErrors: [],
  featuredPluginIds: ["review@openai"]
});
const plugin = (state) => ({
  marketplaceName: "openai",
  summary: {
    id: "review@openai",
    name: "review",
    description: "Review changes with Codex Apps",
    installed: state.installed,
    enabled: state.installed,
    localVersion: state.installed ? "1.0.0" : null
  },
  skills: [{ name: "review", path: "remote://review/SKILL.md", enabled: true }],
  hooks: [{ event: "after_tool", path: "remote://review/hook.json" }],
  mcpServers: [{ name: "review_remote", url: "https://plugins.example.test/mcp", remote: true }],
  apps: [{ id: "review-app", installUrl: "https://apps.example.test/install/review" }],
  appTemplates: [{ id: "review-template", appId: "review-app" }],
  scheduledTasks: [{ id: "weekly-review" }],
  browserExtensions: [{ id: "review-browser" }],
  futureField: { detected: true }
});

const input = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of input) {
  if (!line.trim()) continue;
  const message = JSON.parse(line);
  const method = message.method;
  if (method === "initialized") continue;
  const state = readState();
  if (method === "initialize") {
    reply({ jsonrpc: "2.0", id: message.id, result: {
      codexHome: process.env.CODEX_HOME,
      platformFamily: "unix",
      platformOs: "linux",
      userAgent: "visual-fixture/" + state.version
    } });
    continue;
  }
  if (message.params == null) {
    reply({ jsonrpc: "2.0", id: message.id, error: { code: -32602, message: "invalid params" } });
    continue;
  }
  if (method === "plugin/list" || method === "plugin/installed") {
    reply({ jsonrpc: "2.0", id: message.id, result: catalog(state) });
  } else if (method === "plugin/read") {
    if (state.installed && state.failReadAfterInstall) {
      reply({ jsonrpc: "2.0", id: message.id, error: { code: -32000, message: "deterministic detail reread failure" } });
    } else {
      reply({ jsonrpc: "2.0", id: message.id, result: { plugin: plugin(state) } });
    }
  } else if (method === "plugin/install") {
    writeFileSync(statePath, JSON.stringify({ ...state, installed: true }) + "\n");
    reply({ jsonrpc: "2.0", id: message.id, result: { authPolicy: "ON_USE", appsNeedingAuth: [] } });
  } else if (method === "hooks/list") {
    reply({ jsonrpc: "2.0", id: message.id, result: { data: [] } });
  } else if (method === "app/list") {
    reply({ jsonrpc: "2.0", id: message.id, result: { data: [{ id: "review-app", isAccessible: false, installUrl: "https://apps.example.test/install/review" }] } });
  } else {
    reply({ jsonrpc: "2.0", id: message.id, error: { code: -32601, message: "method not found" } });
  }
}
