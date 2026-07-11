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
      if ((await existingOpenCode.count()) === 0) {
        await expect(agentsPanel.getByText("No ACP backends configured.")).toBeVisible();
        await expectElementInsideViewport(page, agentsPanel);
        await capture(page, testInfo, "01-agents-empty");

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
      await page.getByRole("button", { name: "Agent", exact: true }).click();
      const agentPopover = page.getByRole("dialog", { name: "Agent Definition" });
      const agentGroup = agentPopover.getByRole("radiogroup", { name: "Main agent" });
      await expect(agentGroup.getByRole("radio", { name: /^opencode$/i })).toBeHidden();
      await page.getByRole("button", { name: "Agent", exact: true }).click();
      await page.getByRole("button", { name: "Runtime Profile", exact: true }).click();
      const runtimePopover = page.getByRole("dialog", { name: "Runtime Profile selection" });
      const runtimeGroup = runtimePopover.getByRole("radiogroup", { name: "Runtime" });
      const opencodeRuntime = runtimeGroup.getByRole("radio", { name: /^opencode \(ACP\)$/i });
      await expect(opencodeRuntime).toBeVisible();
      await opencodeRuntime.click();
      await expect(page.getByRole("button", { name: "Runtime Profile" })).toContainText("opencode (ACP)");
      await expect(page.getByLabel("Runtime control state")).toHaveText("Runtime default");
      await capture(page, testInfo, "05-opencode-selected");

      await page.getByPlaceholder("Ask Psychevo...").fill(
        "请用两到三句中文说明 ACP streaming 是什么，最后单独输出 OPENCODE_ACP_GUI_LIVE_OK。不要修改文件。"
      );
      await page.getByRole("button", { name: "Send message" }).click();

      const assistantMessage = page.locator(".pevo-message.is-assistant").last();
      await expect(assistantMessage).toBeVisible({ timeout: 240_000 });
      await expectTextGrowthOrCompletion(
        assistantMessage,
        /OPENCODE_ACP_GUI_LIVE_OK/,
        20_000
      );
      await expect(assistantMessage).toContainText(/OPENCODE_ACP_GUI_LIVE_OK/, { timeout: 240_000 });

      await openPanel(page, isMobile, "Status");
      const statusRegion = page.getByRole("region", { name: "Workspace status" });
      await expect(statusRegion).toContainText("reported by ACP peer", { timeout: 30_000 });
      await expect(statusRegion).toContainText("Session tokens");
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

      await page.getByRole("button", { name: "Agent", exact: true }).click();
      const agentPopover = page.getByRole("dialog", { name: "Agent Definition" });
      await expect(agentPopover.getByRole("radio", { name: "Default Agent" })).toHaveAttribute("aria-checked", "true");
      await page.keyboard.press("Escape");
      await page.getByRole("button", { name: "Runtime Profile", exact: true }).click();
      const runtimePopover = page.getByRole("dialog", { name: "Runtime Profile selection" });
      await expect(runtimePopover.getByRole("radio", { name: "Native" })).toHaveAttribute("aria-checked", "true");
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
      const parentCompletion = mainTranscript
        .locator(".pevo-message.is-assistant")
        .filter({ hasText: /OPENCODE_ACP_DELEGATE_LIVE_OK/ });
      const openAgentSession = mainTranscript.getByRole("button", { name: /Open .*agent session/i }).first();
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
      await expect(parentCompletion).toContainText(/OPENCODE_ACP_DELEGATE_LIVE_OK/, { timeout: 420_000 });
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
  if (await existing.count() === 0) {
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

async function expectProviderSession(dbPath: string, provider: string) {
  const output = execFileSync("sqlite3", [
    dbPath,
    "select provider from sessions order by started_at_ms;"
  ], { encoding: "utf8" });
  expect(output.split(/\r?\n/).filter(Boolean)).toContain(provider);
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

async function expectSelectTextFits(select: Locator) {
  const result = await select.evaluate((element) => {
    const control = element as HTMLSelectElement;
    const style = getComputedStyle(control);
    const selectedText = control.selectedOptions[0]?.textContent?.trim() ?? "";
    const probe = document.createElement("span");
    probe.style.font = style.font;
    probe.style.letterSpacing = style.letterSpacing;
    probe.style.position = "absolute";
    probe.style.visibility = "hidden";
    probe.style.whiteSpace = "nowrap";
    probe.textContent = selectedText;
    document.body.appendChild(probe);
    const textWidth = probe.getBoundingClientRect().width;
    probe.remove();
    const paddingLeft = Number.parseFloat(style.paddingLeft) || 0;
    const paddingRight = Number.parseFloat(style.paddingRight) || 0;
    return {
      contentWidth: control.clientWidth - paddingLeft - paddingRight,
      selectedText,
      textWidth
    };
  });
  expect(result.textWidth).toBeLessThanOrEqual(result.contentWidth + 1);
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
