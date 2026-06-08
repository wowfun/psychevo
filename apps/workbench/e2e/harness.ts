import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";

export const repoRoot = path.resolve(import.meta.dirname, "../../..");
export const staticDir = path.join(repoRoot, "apps/workbench/dist");
const testRoot = path.join(repoRoot, ".local/playwright");

export interface PevoWebServer {
  dbPath: string;
  env: NodeJS.ProcessEnv;
  root: string;
  url: string;
  workdir: string;
  stop(): Promise<void>;
}

export async function startPevoWeb({
  live,
  workdir
}: {
  live: boolean;
  workdir?: string;
}): Promise<PevoWebServer> {
  if (!existsSync(staticDir)) {
    throw new Error(`Workbench dist is missing: ${staticDir}`);
  }
  mkdirSync(testRoot, { recursive: true });
  const root = mkdtempSync(path.join(testRoot, live ? "live-" : "deterministic-"));
  const resolvedWorkdir = workdir ? path.resolve(workdir) : path.join(root, "workdir");
  if (!workdir) {
    mkdirSync(resolvedWorkdir, { recursive: true });
    writeWorkbenchFixtures(resolvedWorkdir);
  }

  const configPath = live
    ? process.env.PSYCHEVO_CONFIG ?? path.join(homedir(), ".psychevo/config.toml")
    : path.join(root, "config.toml");
  if (!live) {
    writeFileSync(configPath, "model = \"lmstudio/noop\"\n");
  }
  if (live && !existsSync(configPath)) {
    throw new Error(`live config not found: ${configPath}`);
  }

  const dbPath = path.join(root, "state.db");
  const home = path.join(root, "home");
  const child = spawnPevoWeb({
    configPath,
    dbPath,
    staticDir,
    workdir: resolvedWorkdir,
    home
  });
  const env = gatewayEnv(configPath, dbPath, home);
  const url = modelUrl(
    await waitForServerUrl(child),
    live ? process.env.PSYCHEVO_PLAYWRIGHT_MODEL : undefined
  );

  return {
    dbPath,
    env,
    root,
    url,
    workdir: resolvedWorkdir,
    async stop() {
      await stopManagedGateway(env);
      rmSync(root, { force: true, recursive: true });
    }
  };
}

function writeWorkbenchFixtures(workdir: string) {
  const srcDir = path.join(workdir, "src");
  mkdirSync(srcDir, { recursive: true });
  writeFileSync(path.join(srcDir, "main.rs"), "fn main() {}\n");

  const skillDir = path.join(workdir, ".psychevo", "skills", "reviewer");
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

  const agentDir = path.join(workdir, ".psychevo", "agents");
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
  staticDir: string;
  workdir: string;
}): ChildProcessWithoutNullStreams {
  const pevoBin = process.env.PEVO_BIN;
  const command = pevoBin ?? "cargo";
  const args = pevoBin
    ? [
        "gateway",
        "open",
        "--no-browser",
        "--print-url",
        "--dir",
        options.workdir
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
        options.workdir
      ];

  return spawn(command, args, {
    cwd: repoRoot,
    env: gatewayEnv(options.configPath, options.dbPath, options.home),
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

function gatewayEnv(configPath: string, dbPath: string, home: string): NodeJS.ProcessEnv {
  return {
    ...process.env,
    PSYCHEVO_CONFIG: configPath,
    PSYCHEVO_DB: dbPath,
    PSYCHEVO_HOME: home
  };
}

function stopManagedGateway(env: NodeJS.ProcessEnv): Promise<void> {
  return new Promise((resolve) => {
    const pevoBin = process.env.PEVO_BIN;
    const command = pevoBin ?? "cargo";
    const args = pevoBin
      ? ["gateway", "stop"]
      : ["run", "-p", "psychevo-cli", "--", "gateway", "stop"];
    const child = spawn(command, args, { cwd: repoRoot, env, stdio: "ignore" });
    child.once("exit", () => resolve());
    child.once("error", () => resolve());
  });
}
