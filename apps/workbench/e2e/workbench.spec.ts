import { expect, test, type Page } from "@playwright/test";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "../../..");
const staticDir = path.join(repoRoot, "apps/workbench/dist");
const testRoot = path.join(repoRoot, ".local/playwright");

interface PevoWebServer {
  env: NodeJS.ProcessEnv;
  root: string;
  url: string;
  stop(): Promise<void>;
}

test.describe("pevo Web Workbench", () => {
  test("connects to Gateway and manages a source thread", async ({ page, isMobile }) => {
    const server = await startPevoWeb({ live: false });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("heading", { name: "pevo" })).toBeVisible();
      await expect(page.locator(".statePill")).toHaveText("connected");

      await openPanel(page, isMobile, "History");
      await page.getByRole("button", { name: "New thread" }).click();
      await expect(page.locator(".pevo-sessionRow")).toHaveCount(1);

      await openPanel(page, isMobile, "Timeline");
      await expect(page.getByText("No messages yet")).toBeVisible();

      await openPanel(page, isMobile, "Status");
      await expect(page.getByText("idle")).toBeVisible();
      await expect(page.getByText("status_only")).toBeVisible();
    } finally {
      await server.stop();
    }
  });

  test("submits a real provider turn through the composer @live", async ({ page, isMobile }) => {
    test.skip(process.env.PSYCHEVO_PLAYWRIGHT_LIVE !== "1", "live provider validation is opt-in");
    test.skip(isMobile, "live provider validation runs once on the desktop project");
    const server = await startPevoWeb({ live: true });
    try {
      await page.goto(server.url);
      await expect(page.locator(".statePill")).toHaveText("connected");

      await page.getByPlaceholder("Ask pevo...").fill(
        "Reply with exactly this text and nothing else: psychevo web live ok"
      );
      await page.getByRole("button", { name: "Send" }).click();

      await expect(
        page.locator(".pevo-message.is-assistant").getByText(/psychevo web live ok/i)
      ).toBeVisible({ timeout: 240_000 });
    } finally {
      await server.stop();
    }
  });
});

async function openPanel(page: Page, isMobile: boolean, name: "History" | "Status" | "Timeline") {
  if (isMobile) {
    await page.getByRole("button", { name }).click();
  }
}

async function startPevoWeb({ live }: { live: boolean }): Promise<PevoWebServer> {
  if (!existsSync(staticDir)) {
    throw new Error(`Workbench dist is missing: ${staticDir}`);
  }
  mkdirSync(testRoot, { recursive: true });
  const root = mkdtempSync(path.join(testRoot, live ? "live-" : "deterministic-"));
  const workdir = path.join(root, "workdir");
  mkdirSync(workdir, { recursive: true });

  const configPath = live
    ? process.env.PSYCHEVO_CONFIG ?? path.join(homedir(), ".psychevo/config.toml")
    : path.join(root, "config.toml");
  if (!live) {
    writeFileSync(configPath, "[model]\nid = \"test/noop\"\n");
  }
  if (live && !existsSync(configPath)) {
    throw new Error(`live config not found: ${configPath}`);
  }

  const child = spawnPevoWeb({
    configPath,
    dbPath: path.join(root, "state.db"),
    staticDir,
    workdir,
    home: path.join(root, "home")
  });
  const env = child.spawnargs ? gatewayEnv(configPath, path.join(root, "state.db"), path.join(root, "home")) : process.env;
  const url = modelUrl(await waitForServerUrl(child), live ? process.env.PSYCHEVO_PLAYWRIGHT_MODEL : undefined);

  return {
    env,
    root,
    url,
    async stop() {
      await stopManagedGateway(env);
      rmSync(root, { force: true, recursive: true });
    }
  };
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
