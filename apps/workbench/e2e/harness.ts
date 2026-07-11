import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";

export const repoRoot = path.resolve(import.meta.dirname, "../../..");
export const staticDir = path.join(repoRoot, "apps/workbench/dist");
const testRoot = path.join(repoRoot, ".local/playwright");

export interface PevoWebServer {
  dbPath: string;
  env: NodeJS.ProcessEnv;
  root: string;
  url: string;
  cwd: string;
  stop(): Promise<void>;
}

export async function startPevoWeb({
  configAppend,
  channelRuntime,
  configPath: explicitConfigPath,
  dbPath: explicitDbPath,
  envFile,
  home: explicitHome,
  live,
  model,
  pevoBin,
  cwd
}: {
  configAppend?: string;
  channelRuntime?: boolean;
  configPath?: string;
  dbPath?: string;
  envFile?: string;
  home?: string;
  live: boolean;
  model?: string;
  pevoBin?: string;
  cwd?: string;
}): Promise<PevoWebServer> {
  if (!existsSync(staticDir)) {
    throw new Error(`Workbench dist is missing: ${staticDir}`);
  }
  mkdirSync(testRoot, { recursive: true });
  const root = mkdtempSync(path.join(testRoot, live ? "live-" : "deterministic-"));
  const resolvedCwd = cwd ? path.resolve(cwd) : path.join(root, "cwd");
  const home = explicitHome ? path.resolve(explicitHome) : path.join(root, "home");
  if (!cwd) {
    mkdirSync(resolvedCwd, { recursive: true });
    writeWorkbenchFixtures(resolvedCwd);
  }

  const configPath = live ? explicitConfigPath : path.join(root, "config.toml");
  if (!configPath) {
    throw new Error("live Workbench validation requires an xtask live context configPath");
  }
  const resolvedConfigPath = configPath;
  if (!live) {
    const configText = `model = "${model ?? "lmstudio/noop"}"\n${configAppend ?? ""}`;
    mkdirSync(home, { recursive: true });
    writeFileSync(resolvedConfigPath, configText);
    writeFileSync(path.join(home, "config.toml"), configText);
    if (envFile) {
      writeFileSync(path.join(root, ".env"), envFile);
    }
  }
  if (live && !existsSync(resolvedConfigPath)) {
    throw new Error(`live config not found: ${resolvedConfigPath}`);
  }

  const dbPath = explicitDbPath ? path.resolve(explicitDbPath) : path.join(root, "state.db");
  const child = spawnPevoWeb({
    configPath: resolvedConfigPath,
    dbPath,
    live,
    pevoBin,
    staticDir,
    cwd: resolvedCwd,
    home,
    channelRuntime
  });
  const env = gatewayEnv(resolvedConfigPath, dbPath, home, live, channelRuntime);
  const url = modelUrl(
    await waitForServerUrl(child),
    live ? model : undefined
  );

  return {
    dbPath,
    env,
    root,
    url,
    cwd: resolvedCwd,
    async stop() {
      await stopManagedGateway(env, pevoBin);
      rmSync(root, { force: true, recursive: true });
    }
  };
}

function writeWorkbenchFixtures(cwd: string) {
  const srcDir = path.join(cwd, "src");
  mkdirSync(srcDir, { recursive: true });
  writeFileSync(path.join(srcDir, "main.rs"), "fn main() {}\n");

  const skillDir = path.join(cwd, ".psychevo", "skills", "reviewer");
  mkdirSync(skillDir, { recursive: true });
  writeFileSync(
    path.join(skillDir, "SKILL.md"),
    `---
name: reviewer
description: Review a change for correctness.
---
Review the current change and call out concrete risks.
`
  );

  const agentDir = path.join(cwd, ".psychevo", "agents");
  mkdirSync(agentDir, { recursive: true });
  writeFileSync(
    path.join(agentDir, "translate.md"),
    `---
description: Translate user messages.
---
Translate the user's message.
`
  );
}

function modelUrl(url: string, model: string | undefined): string {
  if (!model?.trim()) {
    return url;
  }
  const parsed = new URL(url);
  parsed.searchParams.set("model", model.trim());
  return parsed.toString();
}

function spawnPevoWeb(options: {
  configPath: string;
  dbPath: string;
  home: string;
  channelRuntime?: boolean;
  live: boolean;
  pevoBin?: string;
  staticDir: string;
  cwd: string;
}): ChildProcessWithoutNullStreams {
  const command = options.pevoBin ?? "cargo";
  const args = options.pevoBin
    ? [
        "gateway",
        "open",
        "--no-browser",
        "--print-url",
        "--dir",
        options.cwd
      ]
    : [
        "run",
        "-p",
        "psychevo-cli",
        "--",
        "gateway",
        "open",
        "--no-browser",
        "--print-url",
        "--dir",
        options.cwd
      ];

  return spawn(command, args, {
    cwd: repoRoot,
    env: gatewayEnv(options.configPath, options.dbPath, options.home, options.live, options.channelRuntime),
    stdio: ["ignore", "pipe", "pipe"]
  });
}

function waitForServerUrl(child: ChildProcessWithoutNullStreams): Promise<string> {
  return new Promise((resolve, reject) => {
    const logs: string[] = [];
    const timer = setTimeout(() => {
      reject(new Error(`timed out waiting for pevo gateway URL\n${logs.join("")}`));
    }, 90_000);

    child.stdout.on("data", (chunk: Buffer) => {
      const text = chunk.toString("utf8");
      logs.push(text);
      const line = text.split(/\r?\n/).find((item) => item.trim().startsWith("{"));
      if (line) {
        const parsed = JSON.parse(line) as { openUrl?: string };
        if (!parsed.openUrl) {
          reject(new Error(`pevo gateway did not print openUrl\n${logs.join("")}`));
          return;
        }
        clearTimeout(timer);
        resolve(parsed.openUrl);
      }
    });
    child.stderr.on("data", (chunk: Buffer) => logs.push(chunk.toString("utf8")));
    child.once("exit", (code, signal) => {
      clearTimeout(timer);
      reject(new Error(`pevo gateway exited before URL code=${code} signal=${signal}\n${logs.join("")}`));
    });
    child.once("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
  });
}

function gatewayEnv(
  configPath: string,
  dbPath: string,
  home: string,
  live: boolean,
  channelRuntime?: boolean
): NodeJS.ProcessEnv {
  return {
    ...process.env,
    PSYCHEVO_CONFIG: configPath,
    PSYCHEVO_DB: dbPath,
    PSYCHEVO_HOME: home,
    PSYCHEVO_CHANNEL_RUNTIME: channelRuntime == null
      ? process.env.PSYCHEVO_CHANNEL_RUNTIME ?? (live ? "on" : "off")
      : channelRuntime ? "on" : "off"
  };
}

function stopManagedGateway(env: NodeJS.ProcessEnv, pevoBin?: string): Promise<void> {
  return new Promise((resolve) => {
    const command = pevoBin ?? "cargo";
    const args = pevoBin
      ? ["gateway", "stop"]
      : ["run", "-p", "psychevo-cli", "--", "gateway", "stop"];
    const child = spawn(command, args, { cwd: repoRoot, env, stdio: "ignore" });
    child.once("exit", () => resolve());
    child.once("error", () => resolve());
  });
}
