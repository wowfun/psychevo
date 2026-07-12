import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { repoRoot, startPevoWeb } from "./harness";
import { liveContextFor, screenshotRoot } from "./liveContext";

test.describe("Codex ACP session lifecycle live validation", () => {
  test("creates and deletes only its test-owned Codex ACP session @live", async ({ page, isMobile }, testInfo) => {
    const context = liveContextFor("codex-acp-session-lifecycle-live");
    if (!context) {
      test.skip(true, "run through cargo xtask live");
      return;
    }
    test.skip(isMobile, "Codex ACP lifecycle live validation runs once on desktop");
    test.setTimeout(context.timeoutMs);
    const screenshots = screenshotRoot(context, "codex-acp-session-lifecycle-live");
    mkdirSync(screenshots, { recursive: true });
    const server = await startPevoWeb({
      live: true,
      model: context.model,
      configPath: context.configPath,
      dbPath: context.dbPath,
      home: context.home,
      pevoBin: context.pevoBin
    });
    try {
      await page.goto(server.url);
      const catalog = await gatewayRequest(page, "thread/context/read", {
        scope: { cwd: server.cwd, source: { kind: "web", rawId: "codex-lifecycle-live" } }
      }) as { compatibleTargets?: Array<{ runtimeProfileRef?: string; ready?: boolean; label?: string }> };
      const codexTarget = catalog.compatibleTargets
        ?.find((target) => target.runtimeProfileRef === "codex" && target.ready);
      if (!codexTarget) {
        test.skip(true, "Codex ACP target is not installed and ready in the live environment");
        return;
      }

      await page.getByRole("button", { name: "New Session", exact: true }).click();
      await page.getByRole("button", { name: "Agent target", exact: true }).click();
      const target = page.getByRole("dialog", { name: "Agent target" })
        .getByRole("radio", { name: codexTarget.label ?? /Codex/i });
      await target.click();
      await page.getByPlaceholder("Ask Psychevo...").fill(
        "Reply exactly CODEX_ACP_SESSION_LIFECYCLE_LIVE_OK. Do not call tools or modify files."
      );
      await page.getByRole("button", { name: "Send message" }).click();
      await expect(page.locator(".pevo-message.is-assistant").last())
        .toContainText("CODEX_ACP_SESSION_LIFECYCLE_LIVE_OK", { timeout: 240_000 });

      const listed = await gatewayRequest(page, "thread/list", {
        cwd: server.cwd,
        archived: false,
        limit: 20
      }) as { sessions?: Array<{ id?: string; lifecycle?: { actions?: Array<{ id?: string; enabled?: boolean }> } }> };
      const owned = listed.sessions?.find((session) => session.id);
      expect(owned?.id).toBeTruthy();
      expect(owned?.lifecycle?.actions).toContainEqual(expect.objectContaining({ id: "delete", enabled: true }));

      const importable = await gatewayRequest(page, "thread/import/list", {
        scope: { cwd: server.cwd, source: { kind: "web", rawId: "codex-lifecycle-live" } },
        cursors: {}
      }) as { profiles?: Array<{ runtimeProfileRef?: string; alreadyImportedCount?: number }> };
      expect(importable.profiles?.find((profile) => profile.runtimeProfileRef === "codex")?.alreadyImportedCount)
        .toBeGreaterThan(0);
      await gatewayRequest(page, "thread/archive", { threadId: owned!.id });
      await gatewayRequest(page, "thread/restore", { threadId: owned!.id });
      await page.getByRole("button", { name: "New Session", exact: true }).click();
      await gatewayRequest(page, "thread/delete", { threadId: owned!.id });
      const after = await gatewayRequest(page, "thread/list", {
        cwd: server.cwd,
        archived: false,
        limit: 20
      }) as { sessions?: Array<{ id?: string }> };
      expect(after.sessions?.some((session) => session.id === owned!.id)).toBe(false);
      await page.screenshot({
        fullPage: true,
        path: path.join(screenshots, `codex-session-deleted-${testInfo.project.name}.png`)
      });
    } finally {
      await server.stop();
    }
  });
});

async function gatewayRequest(page: Page, method: string, params: unknown): Promise<unknown> {
  return page.evaluate(async ({ method, params }) => await new Promise((resolve, reject) => {
    const url = new URL("/ws", window.location.origin);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    const socket = new WebSocket(url);
    const id = `codex-session-lifecycle-${method}`;
    const timeout = window.setTimeout(() => {
      socket.close();
      reject(new Error(`${method} timed out`));
    }, 30_000);
    socket.addEventListener("open", () => {
      socket.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
    });
    socket.addEventListener("message", (event) => {
      const message = JSON.parse(String(event.data));
      if (message.id !== id) return;
      window.clearTimeout(timeout);
      socket.close();
      if (message.error) reject(new Error(message.error.message));
      else resolve(message.result);
    });
  }), { method, params });
}
