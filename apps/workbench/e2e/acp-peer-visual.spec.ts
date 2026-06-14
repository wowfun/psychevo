import { chmodSync, mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page, type TestInfo } from "@playwright/test";
import { repoRoot, startPevoWeb } from "./harness";

const screenshotDir = path.join(repoRoot, ".local/playwright/screenshots/acp-peer-visual");

test.describe("Workbench ACP peer client visual streaming", () => {
  test("renders standard ACP message, thought, tool, and plan updates", async ({ page, isMobile }, testInfo) => {
    test.setTimeout(180_000);
    mkdirSync(screenshotDir, { recursive: true });
    const server = await startPevoWeb({ live: false });
    try {
      const script = writeFakeAcpServer(server.root);
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();

      if (isMobile) {
        await openPanel(page, isMobile, "History");
      }
      let settings = await openSettingsAgents(page);
      await settings.getByRole("button", { name: "Add ACP backend" }).click();

      const form = settings.getByRole("form", { name: "Profile ACP backend" });
      await expect(form).toBeVisible();
      await form.getByLabel("ID").fill("visual-acp");
      await form.getByLabel("Command JSON").fill(JSON.stringify({
        command: "python3",
        args: [script],
        env: {}
      }, null, 2));
      await expect(form.getByRole("button", { name: "Save" })).toBeEnabled();
      await capture(page, testInfo, `01-backend-form-${projectSuffix(isMobile)}`);
      await form.getByRole("button", { name: "Save" }).click();
      await expect(form).toBeHidden({ timeout: 30_000 });

      settings = await openSettingsAgents(page);
      await expect(settings.getByRole("switch", { name: "Disable visual-acp" })).toBeVisible();
      await expect(settings.getByLabel("visual-acp peer entrypoint")).toBeChecked();
      await expect(settings.getByLabel("visual-acp subagent entrypoint")).toBeChecked();
      await capture(page, testInfo, `02-backend-configured-${projectSuffix(isMobile)}`);

      await settings.getByRole("button", { name: "Back to app" }).click();
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      await openPanel(page, isMobile, "Transcript");
      const agentSelect = page.getByRole("combobox", { name: "Agent" });
      await expect(agentSelect).toContainText("visual-acp (ACP)");
      await agentSelect.selectOption({ label: "visual-acp (ACP)" });

      await page.getByPlaceholder("Ask Psychevo...").fill("Exercise the ACP standard event stream.");
      await page.getByRole("button", { name: "Send message" }).click();

      const assistantMessage = page.locator(".pevo-message.is-assistant").last();
      await expect(page.locator(".pevo-reasoning").getByText("Thinking", { exact: true })).toBeVisible();
      await expect(page.locator(".pevo-reasoning")).toContainText("visual thinking");
      await expect(assistantMessage).toContainText("streaming hello");
      await expectVisibleTextGrowth(assistantMessage);
      await expect(assistantMessage).toContainText("model=lmstudio/noop");
      await expect(page.locator(".pevo-evidence").filter({ hasText: "Run visual tool" })).toBeVisible();
      await expect(page.locator(".pevo-evidence").filter({ hasText: "Plan" })).toContainText("Patch ACP bridge");
      await assertNoHorizontalOverflow(page, page.getByRole("region", { name: "Transcript" }));
      await assertTranscriptRowsFit(page);
      await capture(page, testInfo, `03-live-stream-${projectSuffix(isMobile)}`);

      await expect(page.locator(".pevo-message.is-assistant")).toContainText("done", {
        timeout: 30_000
      });
      const completedTool = page.locator(".pevo-evidence").filter({ hasText: "Run visual tool" });
      await expect(completedTool).toBeVisible();
      await completedTool.getByRole("button", { name: /Run visual tool/ }).click();
      await expect(completedTool).toContainText("done");
      await openPanel(page, isMobile, "Status");
      const statusRegion = page.getByRole("region", { name: "Workspace status" });
      await expect(statusRegion).toContainText("reported by ACP peer");
      await expect(statusRegion).toContainText("Session tokens");
      await expect(statusRegion).toContainText("128");
      await assertNoWorkbenchRenderError(page);
      await assertNoHorizontalOverflow(page, page.getByRole("region", { name: "Workspace status" }));
      await capture(page, testInfo, `04-final-${projectSuffix(isMobile)}`);

      if (isMobile) {
        await openPanel(page, isMobile, "History");
      }
      await page.getByRole("button", { name: "New Session", exact: true }).click();
      await openPanel(page, isMobile, "Status");
      const draftStatusRegion = page.getByRole("region", { name: "Workspace status" });
      await expect(draftStatusRegion.getByText("draft")).toBeVisible();
      await expect(draftStatusRegion).toContainText(/No active (session|context)/);
      await expect(draftStatusRegion).toContainText("No session usage yet.");
      await expect(draftStatusRegion).not.toContainText("reported by ACP peer");
      await assertNoHorizontalOverflow(page, draftStatusRegion);
      await capture(page, testInfo, `05-new-draft-status-${projectSuffix(isMobile)}`);
    } finally {
      await server.stop();
    }
  });
});

function writeFakeAcpServer(root: string): string {
  const script = path.join(root, "fake-acp-stream.py");
  writeFileSync(script, `#!/usr/bin/env python3
import json
import sys
import time

model_value = "unset"

def send(value):
    print(json.dumps(value), flush=True)

def update(session_id, payload):
    send({"jsonrpc": "2.0", "method": "session/update", "params": {
        "sessionId": session_id,
        "update": payload
    }})

def config_options():
    return [{
        "id": "model",
        "name": "Model",
        "category": "model",
        "type": "select",
        "currentValue": model_value if model_value != "unset" else "lmstudio/noop",
        "options": [
            {"value": "lmstudio/noop", "name": "Noop"}
        ]
    }]

for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    method = message.get("method")
    mid = message.get("id")
    params = message.get("params") or {}
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": mid, "result": {"protocolVersion": 2, "capabilities": {}}})
    elif method == "session/new":
        send({"jsonrpc": "2.0", "id": mid, "result": {"sessionId": "native-visual", "configOptions": config_options()}})
    elif method == "session/set_config_option":
        config_id = params.get("configId") or params.get("config_id")
        value = params.get("value")
        if isinstance(value, dict):
            value = value.get("value")
        if config_id == "model":
            model_value = value
        send({"jsonrpc": "2.0", "id": mid, "result": {"configOptions": config_options()}})
    elif method == "session/prompt":
        session_id = params.get("sessionId") or "native-visual"
        update(session_id, {"sessionUpdate": "session_info_update", "title": "Visual ACP session"})
        update(session_id, {"sessionUpdate": "available_commands_update", "availableCommands": [
            {"name": "peer_research", "description": "Peer research command"}
        ]})
        update(session_id, {"sessionUpdate": "agent_thought_chunk", "messageId": "thought-visual", "content": {"type": "text", "text": "visual thinking"}})
        update(session_id, {"sessionUpdate": "agent_message_chunk", "messageId": "message-visual", "content": {"type": "text", "text": "streaming hello "}})
        time.sleep(0.4)
        update(session_id, {"sessionUpdate": "agent_message_chunk", "messageId": "message-visual", "content": {"type": "text", "text": "model=" + model_value + " "}})
        time.sleep(0.4)
        update(session_id, {"sessionUpdate": "tool_call", "toolCallId": "call-visual", "title": "Run visual tool", "kind": "execute", "status": "pending", "rawInput": {"cmd": "echo done"}})
        update(session_id, {"sessionUpdate": "tool_call_update", "toolCallId": "call-visual", "status": "in_progress", "content": [
            {"type": "content", "content": {"type": "text", "text": "running\\n"}}
        ]})
        update(session_id, {"sessionUpdate": "plan_update", "plan": {"type": "items", "id": "plan-visual", "entries": [
            {"content": "Inspect ACP stream", "priority": "high", "status": "completed"},
            {"content": "Patch ACP bridge", "priority": "high", "status": "in_progress"}
        ]}})
        update(session_id, {"sessionUpdate": "usage_update", "used": 128, "size": 2048})
        update(session_id, {"sessionUpdate": "_visual_status", "label": "custom"})
        time.sleep(2)
        update(session_id, {"sessionUpdate": "agent_message_chunk", "messageId": "message-visual", "content": {"type": "text", "text": "done"}})
        update(session_id, {"sessionUpdate": "tool_call_update", "toolCallId": "call-visual", "status": "completed", "content": [
            {"type": "content", "content": {"type": "text", "text": "done\\n"}}
        ], "rawOutput": {"output": "done\\n"}})
        send({"jsonrpc": "2.0", "id": mid, "result": {"stopReason": "end_turn"}})
    else:
        send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "method not found"}})
`);
  chmodSync(script, 0o755);
  return script;
}

async function openPanel(page: Page, isMobile: boolean, name: "History" | "Status" | "Transcript") {
  if (name === "Status") {
    if (isMobile) {
      await page.getByRole("button", { name: "Transcript" }).click();
    }
    const expandInspector = page.getByRole("button", { name: "Show right inspector" });
    const collapseInspector = page.getByRole("button", { name: "Collapse right inspector" });
    if (await collapseInspector.count() === 0) {
      await expect(expandInspector).toBeVisible();
      await expandInspector.click();
      await expect(collapseInspector).toBeVisible();
    }
  }
  if (isMobile) {
    await page.getByRole("button", { name, exact: true }).click();
  }
  if (name === "Status") {
    await expect(page.getByRole("region", { name: "Workspace status" })).toBeVisible();
  }
}

async function expectVisibleTextGrowth(locator: Locator) {
  const initial = (await locator.textContent())?.length ?? 0;
  await expect.poll(async () => (await locator.textContent())?.length ?? 0, {
    intervals: [100, 150, 250, 500],
    timeout: 2_000
  }).toBeGreaterThan(initial);
}

async function openSettingsAgents(page: Page): Promise<Locator> {
  for (let attempt = 0; attempt < 3; attempt += 1) {
    try {
      let settings = page.getByRole("region", { name: "Settings" });
      if (!(await settings.count()) || !(await settings.isVisible().catch(() => false))) {
        await page.getByRole("button", { name: "Settings" }).click();
        settings = page.getByRole("region", { name: "Settings" });
      }
      await expect(settings).toBeVisible();
      const agentsPanel = settings.getByRole("region", { name: "Agents" });
      if (!(await agentsPanel.isVisible().catch(() => false))) {
        await settings.getByRole("button", { name: "Agents" }).click();
      }
      await expect(settings.getByRole("region", { name: "Agents" })).toBeVisible();
      return settings;
    } catch (error) {
      if (attempt === 2) {
        throw error;
      }
      await page.waitForTimeout(100);
    }
  }
  throw new Error("unreachable");
}

async function capture(page: Page, testInfo: TestInfo, label: string) {
  const fileName = `${label}-${testInfo.project.name}.png`;
  const stablePath = path.join(screenshotDir, fileName);
  await page.screenshot({ fullPage: true, path: stablePath });
  await testInfo.attach(fileName, { path: stablePath, contentType: "image/png" });
  process.stdout.write(`[acp-peer-visual] screenshot ${path.relative(repoRoot, stablePath)}\\n`);
}

function projectSuffix(isMobile: boolean) {
  return isMobile ? "mobile" : "desktop";
}

async function assertNoHorizontalOverflow(page: Page, locator: Locator) {
  const [viewport, result] = await Promise.all([
    page.viewportSize(),
    locator.evaluate((element) => {
      const box = element.getBoundingClientRect();
      return {
        clientWidth: element.clientWidth,
        left: box.left,
        right: box.right,
        scrollWidth: element.scrollWidth
      };
    })
  ]);
  expect(viewport).not.toBeNull();
  expect(result.left).toBeGreaterThanOrEqual(-1);
  expect(result.right).toBeLessThanOrEqual(viewport!.width + 1);
  expect(result.scrollWidth).toBeLessThanOrEqual(result.clientWidth + 1);
}

async function assertTranscriptRowsFit(page: Page) {
  const violations = await page.locator(".pevo-threadItems > article, .pevo-messageFrame").evaluateAll((rows) =>
    rows.flatMap((row, index) => {
      const element = row as HTMLElement;
      const rowBox = element.getBoundingClientRect();
      const measured = Array.from(element.querySelectorAll<HTMLElement>(".pevo-message, .pevo-evidenceLine, .pevo-reasoningHeader"));
      return measured.flatMap((child) => {
        const childBox = child.getBoundingClientRect();
        const inside = childBox.left >= rowBox.left - 1 && childBox.right <= rowBox.right + 1;
        return inside ? [] : [{ index, text: child.textContent?.trim() ?? child.className }];
      });
    })
  );
  expect(violations).toEqual([]);
}

async function assertNoWorkbenchRenderError(page: Page) {
  const alert = page.getByRole("alert");
  const alertText = await alert.textContent().catch(() => null);
  if (alertText?.includes("Workbench render failed")) {
    throw new Error(alertText);
  }
}
