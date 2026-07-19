import { mkdirSync } from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { expect, test, type Locator, type Page, type TestInfo } from "@playwright/test";
import { repoRoot, startPevoWeb } from "./harness";
import { liveContextFor, screenshotRoot } from "./liveContext";

let screenshotDir = path.join(repoRoot, ".local/playwright/screenshots/opencode-acp-live");

test.describe("Workbench OpenCode ACP live visual validation", () => {
  test("creates and uses OpenCode ACP from the GUI @live", async ({ page, isMobile }, testInfo) => {
    const context = liveContextFor("opencode-acp-gui-live");
    if (!context) {
      test.skip(true, "run through cargo xtask live");
      return;
    }
    test.skip(isMobile, "OpenCode ACP live validation runs once on the desktop project");
    test.setTimeout(context.timeoutMs);
    screenshotDir = screenshotRoot(context, "opencode-acp-live");
    mkdirSync(screenshotDir, { recursive: true });

    const server = await startPevoWeb({ live: false, pevoBin: context.pevoBin });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      await capture(page, testInfo, "00-transcript");

      const agentsPanel = await openCapabilityBackendPanel(page);
      const existingOpenCode = agentsPanel.locator(".agentBackendRow").filter({ hasText: /opencode \(ACP\)/i });
      if (!(await waitForOpenCodeBackend(existingOpenCode))) {
        await expectElementInsideViewport(page, agentsPanel);
        await capture(page, testInfo, "01-agents-without-opencode");

        await agentsPanel.getByRole("button", { name: "Add ACP backend" }).click();
        const form = agentsPanel.getByRole("form", { name: "Profile ACP backend" });
        await expect(form).toBeVisible();
        await expect(form.getByLabel("ID")).toHaveValue("");
        await expect(form.getByLabel("Target")).toHaveCount(0);
        await form.getByLabel("ID").fill("opencode");
        await form.getByLabel("Command JSON").fill(JSON.stringify({
          command: "opencode",
          args: ["acp"],
          env: {}
        }, null, 2));
        await expectElementInsideViewport(page, form);
        await expectDialogControlsFit(form);
        await capture(page, testInfo, "02-opencode-dialog");

        await form.getByRole("button", { name: "Save" }).click();
        await expect(form).toBeHidden();
        await expect(existingOpenCode.first()).toBeVisible();
      } else {
        await expect(existingOpenCode.first()).toBeVisible();
        await expectElementInsideViewport(page, agentsPanel);
        await capture(page, testInfo, "01-agents-existing");
      }
      await ensureOpenCodeBackend(agentsPanel);
      const backendRow = agentsPanel.locator(".agentBackendRow").filter({ hasText: /opencode \(ACP\)/i });
      await expect(backendRow.getByText(/^opencode \(ACP\)$/i)).toBeVisible();
      await expect(agentsPanel.getByRole("switch", { name: "Disable opencode" })).toBeVisible();
      await expect(agentsPanel.getByLabel("opencode peer entrypoint")).toBeChecked();
      await expect(agentsPanel.getByLabel("opencode subagent entrypoint")).toBeChecked();
      await assertBackendRowsFit(agentsPanel);
      await capture(page, testInfo, "03-opencode-backend");

      await agentsPanel.getByRole("button", { name: "Doctor opencode" }).click();
      await expect(agentsPanel.getByText(/command: ok/)).toBeVisible();
      await capture(page, testInfo, "04-opencode-doctor");

      await page.getByRole("button", { name: "New Session", exact: true }).click();
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      const selector = page.getByRole("button", { name: "Agent target", exact: true });
      await selector.click();
      const targetPopover = page.getByRole("dialog", { name: "Agent target" });
      const targetGroup = targetPopover.getByRole("radiogroup", { name: "Agent target" });
      const opencodeTarget = targetGroup.getByRole("radio", { name: "opencode · OpenCode (ACP)" });
      await expect(opencodeTarget).toBeVisible();
      await opencodeTarget.click();
      await expect(selector).toContainText(/opencode \(ACP\)/i);
      await expect(page.getByLabel("Runtime control state")).toHaveCount(0);
      const mode = page.getByRole("combobox", { name: "Session Mode" });
      await expect(mode).toBeVisible({ timeout: 30_000 });
      await expect(mode).toHaveText("build");
      await mode.click();
      await expect(page.getByRole("listbox", { name: "Session Mode" }).getByRole("option"))
        .toHaveText(["build", "plan"]);
      await page.keyboard.press("Escape");
      await expect(page.getByRole("button", { name: "Model" })).toBeVisible();
      await capture(page, testInfo, "05-opencode-selected");

      await page.getByPlaceholder("Ask Psychevo...").fill(
        "请用两到三句中文说明 ACP streaming 是什么，最后单独输出 OPENCODE_ACP_GUI_LIVE_OK。不要修改文件。"
      );
      await page.getByRole("button", { name: "Send message" }).click();

      const assistantMessage = page.locator(".pevo-message.is-assistant").last();
      await expect(assistantMessage).toBeVisible({ timeout: 240_000 });
      const semanticResponse = /ACP[\s\S]*(?:streaming|流式)|(?:streaming|流式)[\s\S]*ACP/i;
      await expectTextGrowthOrCompletion(
        assistantMessage,
        semanticResponse,
        20_000
      );
      await expect(assistantMessage).toContainText(semanticResponse, { timeout: 240_000 });

      await openPanel(page, isMobile, "Status");
      const statusRegion = page.getByRole("region", { name: "Workspace status" });
      await expect(statusRegion.getByRole("region", { name: "Session observability" })).toContainText("exact", {
        timeout: 30_000
      });
      await expect(statusRegion).not.toContainText("reported by ACP peer");
      await expect(statusRegion).toContainText("Session tokens");
      const listedThreads = await gatewayRequest(page, "thread/list", {
        cwd: server.cwd,
        archived: false,
        limit: 20
      }) as { sessions?: Array<{ id?: string; lifecycle?: { actions?: Array<{ id?: string; enabled?: boolean }> } }> };
      const thread = listedThreads.sessions?.find((session) => session.id);
      expect(thread?.id).toBeTruthy();
      const contextResult = await gatewayRequest(page, "thread/context/read", {
        scope: { cwd: server.cwd, source: { kind: "web", rawId: "opencode-lifecycle-live" } },
        threadId: thread!.id
      }) as { actions?: Array<{ id?: string; enabled?: boolean }> };
      expect(contextResult.actions).toContainEqual(expect.objectContaining({ id: "fork", enabled: true }));
      const forked = await gatewayRequest(page, "thread/action/run", {
        scope: { cwd: server.cwd, source: { kind: "web", rawId: "opencode-lifecycle-live" } },
        threadId: thread!.id,
        action: { kind: "fork" }
      }) as { kind?: string; snapshot?: { thread?: { id?: string } } };
      const forkedThreadId = forked.snapshot?.thread?.id;
      expect(forked.kind).toBe("fork");
      expect(forkedThreadId).toBeTruthy();
      await gatewayRequest(page, "thread/archive", { threadId: forkedThreadId });
      await gatewayRequest(page, "thread/restore", { threadId: forkedThreadId });
      const importable = await gatewayRequest(page, "thread/import/list", {
        scope: { cwd: server.cwd, source: { kind: "web", rawId: "opencode-lifecycle-live" } },
        cursors: {}
      }) as {
        profiles?: Array<{
          alreadyImportedCount?: number;
          runtimeProfileRef?: string;
          sessions?: unknown[];
          status?: string;
        }>;
      };
      const openCodeImportProfile = importable.profiles
        ?.find((profile) => profile.runtimeProfileRef === "opencode");
      expect(openCodeImportProfile).toEqual(expect.objectContaining({
        alreadyImportedCount: expect.any(Number),
        sessions: expect.any(Array),
        status: "ready"
      }));
      const refreshedThreads = await gatewayRequest(page, "thread/list", {
        cwd: server.cwd,
        archived: false,
        limit: 20
      }) as { sessions?: Array<{ id?: string; lifecycle?: { actions?: Array<{ id?: string; enabled?: boolean }> } }> };
      const opencodeLifecycle = refreshedThreads.sessions
        ?.find((session) => session.id === thread!.id)
        ?.lifecycle;
      expect(opencodeLifecycle?.actions).toContainEqual(expect.objectContaining({ id: "delete", enabled: false }));
      await assertNoWorkbenchRenderError(page);
      await assertTranscriptRowsFit(page);
      await capture(page, testInfo, "06-live-response");
      await capture(page, testInfo, "07-status-usage");
    } finally {
      await server.stop();
    }
  });

  test("delegates @opencode through the native runtime @live", async ({ page, isMobile }, testInfo) => {
    const context = liveContextFor("opencode-acp-delegate-live");
    if (!context) {
      test.skip(true, "run through cargo xtask live");
      return;
    }
    test.skip(isMobile, "OpenCode ACP delegate live validation runs once on the desktop project");
    test.setTimeout(context.timeoutMs);
    screenshotDir = screenshotRoot(context, "opencode-acp-live");
    mkdirSync(screenshotDir, { recursive: true });

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
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      const agentsPanel = await openCapabilityBackendPanel(page);
      await ensureOpenCodeBackend(agentsPanel);
      await page.getByRole("button", { name: "New Session", exact: true }).click();
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();

      await page.getByRole("button", { name: "Agent target", exact: true }).click();
      const targetPopover = page.getByRole("dialog", { name: "Agent target" });
      await expect(
        targetPopover
          .getByRole("radiogroup", { name: "Agent target" })
          .getByRole("radio", { name: "Psychevo · Psychevo (Native)" })
      ).toHaveAttribute("aria-checked", "true");
      await page.keyboard.press("Escape");

      const textarea = page.getByPlaceholder("Ask Psychevo...");
      await textarea.fill("@op");
      const opencodeMention = page.getByRole("option", { name: /@opencode/ });
      await expect(opencodeMention).toBeVisible({ timeout: 30_000 });
      await opencodeMention.click();
      await expect(textarea).toHaveValue("@opencode ");
      await textarea.fill(
        "@opencode 请完成一个只读说明任务：先单独输出 ACP_STREAM_START，再用中文写 12 个编号段落说明你有哪些工具以及 ACP streaming 如何工作，每段至少两句完整句子。最后单独输出 OPENCODE_ACP_DELEGATE_LIVE_OK，不要提前输出该标记。不要修改文件。"
      );
      await page.getByRole("button", { name: "Send message" }).click();

      const mainTranscript = page.getByRole("region", { name: "Transcript" }).first();
      const parentComposer = page.locator(".pevo-composer").first();
      const parentCompletion = mainTranscript.locator(".pevo-message.is-assistant");
      const openAgentSession = mainTranscript.getByRole("button", { name: /Open .*agent session/i }).first();
      await expect(parentComposer).toHaveClass(/is-running/, { timeout: 30_000 });
      await expect(openAgentSession).toBeVisible({ timeout: 240_000 });
      await expect(parentCompletion).toHaveCount(0);
      await openAgentSession.click();

      const childPanel = page.locator(".threadPanel");
      await expect(childPanel).toBeVisible({ timeout: 30_000 });
      await expect(childPanel).toContainText(/Parent/);
      await expect(parentCompletion).toHaveCount(0);
      const childAssistant = childPanel.locator(".pevo-message.is-assistant").last();
      await expectChildTextGrowthBeforeParentCompletion(childAssistant, parentCompletion, 300_000);
      await expect(parentCompletion).toHaveCount(0);
      await capture(page, testInfo, "08-delegate-streaming");

      await expect(childAssistant).toContainText(/OPENCODE_ACP_DELEGATE_LIVE_OK/, { timeout: 420_000 });
      await expect(parentComposer).not.toHaveClass(/is-running/, { timeout: 420_000 });
      await expect(parentCompletion.last()).toHaveText(/\S/, { timeout: 420_000 });
      await expectDelegatePersistence(server.dbPath);
      await expectProviderSession(server.dbPath, "acp:opencode");
      await assertNoWorkbenchRenderError(page);
      await assertTranscriptRowsFit(page);
      await capture(page, testInfo, "09-delegate-response");
    } finally {
      await server.stop();
    }
  });
});

async function capture(page: Page, testInfo: TestInfo, label: string) {
  const fileName = `${label}-${testInfo.project.name}.png`;
  const stablePath = path.join(screenshotDir, fileName);
  await page.screenshot({ fullPage: true, path: stablePath });
  await testInfo.attach(fileName, { path: stablePath, contentType: "image/png" });
  process.stdout.write(`[opencode-acp-live] screenshot ${path.relative(repoRoot, stablePath)}\n`);
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

async function openCapabilityBackendPanel(page: Page): Promise<Locator> {
  await page.getByRole("button", { name: "Capabilities" }).click();
  const capabilities = page.getByRole("region", { name: "Capabilities" });
  await expect(capabilities).toBeVisible();
  await capabilities.getByRole("tab", { name: "Agents" }).click();
  await capabilities.getByRole("tab", { name: "ACP Backends" }).click();
  const agentsPanel = capabilities.getByRole("region", { name: "Agents" });
  await expect(agentsPanel).toBeVisible();
  return agentsPanel;
}

async function ensureOpenCodeBackend(agentsPanel: Locator) {
  const existing = agentsPanel.locator(".agentBackendRow").filter({ hasText: /opencode \(ACP\)/i });
  if (!(await waitForOpenCodeBackend(existing))) {
    await agentsPanel.getByRole("button", { name: "Add ACP backend" }).click();
    const form = agentsPanel.getByRole("form", { name: "Profile ACP backend" });
    await expect(form).toBeVisible();
    await form.getByLabel("ID").fill("opencode");
    await form.getByLabel("Command JSON").fill(JSON.stringify({
      command: "opencode",
      args: ["acp"],
      env: {}
    }, null, 2));
    await form.getByRole("button", { name: "Save" }).click();
    await expect(form).toBeHidden();
  }
  const backendRow = agentsPanel.locator(".agentBackendRow").filter({ hasText: /opencode \(ACP\)/i });
  await expect(backendRow).toBeVisible();
  const enabled = agentsPanel.getByRole("switch", { name: /opencode/ });
  if (!(await enabled.isChecked())) {
    await enabled.click();
  }
  const subagent = agentsPanel.getByLabel("opencode subagent entrypoint");
  if (!(await subagent.isChecked())) {
    await subagent.click();
  }
  const peer = agentsPanel.getByLabel("opencode peer entrypoint");
  if (!(await peer.isChecked())) {
    await peer.click();
  }
}

async function waitForOpenCodeBackend(existing: Locator) {
  return existing.first().waitFor({ state: "visible", timeout: 10_000 })
    .then(() => true)
    .catch(() => false);
}

async function expectProviderSession(dbPath: string, provider: string) {
  const output = execFileSync("sqlite3", [
    dbPath,
    "select provider from sessions order by started_at_ms;"
  ], { encoding: "utf8" });
  expect(output.split(/\r?\n/).filter(Boolean)).toContain(provider);
}

async function expectDelegatePersistence(dbPath: string) {
  await expect.poll(() => {
    const output = execFileSync("sqlite3", [
      "-json",
      dbPath,
      `SELECT
      e.status AS edge_status,
      EXISTS(
        SELECT 1 FROM messages child_message
        WHERE child_message.session_id = child.id
          AND child_message.role = 'assistant'
          AND instr(child_message.content_text, 'OPENCODE_ACP_DELEGATE_LIVE_OK') > 0
      ) AS child_marker,
      EXISTS(
        SELECT 1 FROM messages parent_result
        WHERE parent_result.session_id = parent.id
          AND parent_result.role = 'tool_result'
          AND instr(parent_result.content_text, 'OPENCODE_ACP_DELEGATE_LIVE_OK') > 0
      ) AS tool_result_marker,
      EXISTS(
        SELECT 1 FROM messages parent_message
        WHERE parent_message.session_id = parent.id
          AND parent_message.role = 'assistant'
          AND length(trim(coalesce(parent_message.content_text, ''))) > 0
      ) AS parent_final,
      (
        SELECT terminal.status FROM gateway_turn_terminals terminal
        WHERE terminal.thread_id = parent.id
        ORDER BY terminal.completed_at_ms DESC LIMIT 1
      ) AS parent_terminal_status,
      (
        SELECT terminal.outcome FROM gateway_turn_terminals terminal
        WHERE terminal.thread_id = parent.id
        ORDER BY terminal.completed_at_ms DESC LIMIT 1
      ) AS parent_terminal_outcome
    FROM sessions parent
    JOIN agent_edges e ON e.parent_session_id = parent.id
    JOIN sessions child ON child.id = e.child_session_id
    WHERE EXISTS(
      SELECT 1 FROM messages request
      WHERE request.session_id = parent.id
        AND request.role = 'user'
        AND instr(request.content_text, 'OPENCODE_ACP_DELEGATE_LIVE_OK') > 0
    )
    ORDER BY parent.started_at_ms DESC
    LIMIT 1;`
    ], { encoding: "utf8" });
    return JSON.parse(output) as Array<Record<string, string | number | null>>;
  }, { timeout: 30_000 }).toEqual([{
    edge_status: "closed",
    child_marker: 1,
    tool_result_marker: 1,
    parent_final: 1,
    parent_terminal_status: "completed",
    parent_terminal_outcome: "normal"
  }]);
}

async function expectTextGrowthOrCompletion(
  locator: Locator,
  completion: RegExp,
  timeout: number
) {
  const initialText = (await locator.textContent()) ?? "";
  if (completion.test(initialText)) {
    return;
  }
  const initial = initialText.length;
  await expect.poll(async () => {
    const text = (await locator.textContent()) ?? "";
    return completion.test(text) || text.length > initial;
  }, {
    intervals: [150, 250, 500, 750, 1000],
    timeout
  }).toBe(true);
}

async function expectChildTextGrowthBeforeParentCompletion(
  childAssistant: Locator,
  parentCompletion: Locator,
  timeout: number
) {
  let firstText: string | null = null;
  await expect.poll(async () => {
    if (await parentCompletion.count() > 0) {
      throw new Error("parent completed before child streaming text growth was observed");
    }
    if (await childAssistant.count() === 0) {
      return false;
    }
    const text = ((await childAssistant.textContent().catch(() => null)) ?? "").trim();
    if (!text) {
      return false;
    }
    if (firstText === null) {
      firstText = text;
      return false;
    }
    return text.length > firstText.length && await parentCompletion.count() === 0;
  }, {
    intervals: [100, 150, 250, 500, 750, 1000],
    timeout
  }).toBe(true);
}

async function expectElementInsideViewport(page: Page, locator: Locator) {
  const [box, viewport] = await Promise.all([locator.boundingBox(), page.viewportSize()]);
  expect(box).not.toBeNull();
  expect(viewport).not.toBeNull();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.y).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(viewport!.width);
  expect(box!.y + Math.min(box!.height, viewport!.height)).toBeLessThanOrEqual(viewport!.height);
}

async function expectDialogControlsFit(dialog: Locator) {
  const clipped = await dialog.locator("input, textarea, select, button").evaluateAll((controls) =>
    controls
      .map((control) => {
        const element = control as HTMLElement;
        return {
          label: element.getAttribute("aria-label") ?? element.textContent?.trim() ?? element.tagName,
          clippedX: element.scrollWidth > element.clientWidth + 2,
          clippedY: element.scrollHeight > element.clientHeight + 2
        };
      })
      .filter((item) => item.clippedX || item.clippedY)
  );
  expect(clipped).toEqual([]);
}

async function assertBackendRowsFit(overlay: Locator) {
  const violations = await overlay.locator(".agentBackendRow").evaluateAll((rows) =>
    rows.flatMap((row, index) => {
      const rowBox = row.getBoundingClientRect();
      return Array.from(row.querySelectorAll<HTMLElement>("strong, span, small, button")).flatMap((child) => {
        const childBox = child.getBoundingClientRect();
        const inside = childBox.left >= rowBox.left - 1 &&
          childBox.right <= rowBox.right + 1 &&
          childBox.top >= rowBox.top - 1 &&
          childBox.bottom <= rowBox.bottom + 1;
        return inside ? [] : [{
          index,
          text: child.textContent?.trim() ?? child.getAttribute("aria-label") ?? child.tagName
        }];
      });
    })
  );
  expect(violations).toEqual([]);
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

async function gatewayRequest(page: Page, method: string, params: unknown): Promise<unknown> {
  return page.evaluate(async ({ method, params }) => await new Promise((resolve, reject) => {
    const url = new URL("/ws", window.location.origin);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    const socket = new WebSocket(url);
    const id = `opencode-session-lifecycle-${method}`;
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
