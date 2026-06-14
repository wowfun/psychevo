import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page, type TestInfo } from "@playwright/test";
import { repoRoot, startPevoWeb } from "./harness";

const screenshotDir = path.join(repoRoot, ".local/playwright/screenshots/opencode-acp-live");

test.describe("Workbench OpenCode ACP live visual validation", () => {
  test("creates and uses OpenCode ACP from the GUI @live", async ({ page, isMobile }, testInfo) => {
    test.skip(
      process.env.PSYCHEVO_PLAYWRIGHT_OPENCODE_ACP_LIVE !== "1",
      "OpenCode ACP live GUI validation is opt-in"
    );
    test.skip(isMobile, "OpenCode ACP live validation runs once on the desktop project");
    test.setTimeout(numericEnv("PSYCHEVO_PLAYWRIGHT_OPENCODE_ACP_TIMEOUT_MS", 360_000));
    mkdirSync(screenshotDir, { recursive: true });

    const server = await startPevoWeb({ live: false });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      await capture(page, testInfo, "00-transcript");

      await page.getByRole("button", { name: "Settings" }).click();
      const settings = page.getByRole("region", { name: "Settings" });
      await expect(settings).toBeVisible();
      await settings.getByRole("button", { name: "Agents" }).click();
      const agentsPanel = settings.getByRole("region", { name: "Agents" });
      await expect(agentsPanel).toBeVisible();
      await expect(agentsPanel.getByText("No Profile ACP backends configured.")).toBeVisible();
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
      const backendRow = agentsPanel.locator(".agentBackendRow").filter({ hasText: "opencode acp" });
      await expect(backendRow.getByText("opencode acp", { exact: true })).toBeVisible();
      await expect(agentsPanel.getByRole("switch", { name: "Disable opencode" })).toBeVisible();
      await expect(agentsPanel.getByLabel("opencode peer entrypoint")).toBeChecked();
      await expect(agentsPanel.getByLabel("opencode subagent entrypoint")).toBeChecked();
      await assertBackendRowsFit(agentsPanel);
      await capture(page, testInfo, "03-opencode-backend");

      await agentsPanel.getByRole("button", { name: "Doctor opencode" }).click();
      await expect(agentsPanel.getByText(/command: ok/)).toBeVisible();
      await capture(page, testInfo, "04-opencode-doctor");

      await settings.getByRole("button", { name: "Back to app" }).click();
      const agentSelect = page.getByRole("combobox", { name: "Agent" });
      await expect(agentSelect).toContainText("opencode (ACP)");
      await agentSelect.selectOption({ label: "opencode (ACP)" });
      expect(await selectedOptionText(agentSelect)).toBe("opencode (ACP)");
      await expectSelectTextFits(agentSelect);
      await page.getByRole("button", { name: "Add attachments and options" }).click();
      await page.getByRole("switch", { name: "Plan mode" }).click();
      await page.keyboard.press("Escape");
      await capture(page, testInfo, "05-opencode-selected");

      await page.getByPlaceholder("Ask Psychevo...").fill(
        "请用两到三句中文说明 ACP streaming 是什么，最后单独输出 OPENCODE_ACP_GUI_LIVE_OK。不要修改文件。"
      );
      await page.getByRole("button", { name: "Send message" }).click();

      const assistantMessage = page.locator(".pevo-message.is-assistant").last();
      await expect(assistantMessage).toBeVisible({ timeout: 240_000 });
      await expectVisibleTextGrowth(assistantMessage, 20_000);
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

async function expectVisibleTextGrowth(locator: Locator, timeout: number) {
  const initial = (await locator.textContent())?.length ?? 0;
  await expect.poll(async () => (await locator.textContent())?.length ?? 0, {
    intervals: [150, 250, 500, 750, 1000],
    timeout
  }).toBeGreaterThan(initial);
}

async function selectedOptionText(select: Locator): Promise<string> {
  return select.evaluate((element) => {
    const control = element as HTMLSelectElement;
    return control.selectedOptions[0]?.textContent?.trim() ?? "";
  });
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

function numericEnv(name: string, fallback: number): number {
  const value = process.env[name];
  if (!value) {
    return fallback;
  }
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}
