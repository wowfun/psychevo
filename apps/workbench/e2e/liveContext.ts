import { existsSync, readFileSync } from "node:fs";
import path from "node:path";

export type XtaskLiveContext = {
  checkId: string;
  provider: string;
  model: string;
  envMode: "shared" | "isolated";
  configPath: string;
  home: string;
  dbPath: string;
  pevoBin: string;
  cwd?: string;
  artifactRoot: string;
  timeoutMs: number;
  intervalMs: number;
  prompt?: string;
};

export function liveContextFor(checkId: string): XtaskLiveContext | null {
  const context = readXtaskLiveContext();
  return context?.checkId === checkId ? context : null;
}

export function screenshotRoot(context: XtaskLiveContext, name: string): string {
  return path.join(context.artifactRoot, "screenshots", name);
}

function readXtaskLiveContext(): XtaskLiveContext | null {
  const contextPath = process.env.PSYCHEVO_XTASK_LIVE_CONTEXT;
  if (!contextPath) {
    return null;
  }
  if (!existsSync(contextPath)) {
    throw new Error(`xtask live context not found: ${contextPath}`);
  }
  return JSON.parse(readFileSync(contextPath, "utf8")) as XtaskLiveContext;
}
