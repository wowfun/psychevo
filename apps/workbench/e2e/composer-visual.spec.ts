import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { repoRoot, startPevoWeb } from "./harness";

const screenshotDir = path.join(repoRoot, ".local/playwright/screenshots");

test.describe("Workbench composer visual contract", () => {
  test("renders composer controls, plan chip, and interrupt state without overlap", async ({ page, isMobile }, testInfo) => {
    const server = await startPevoWeb({ live: false });
    mkdirSync(screenshotDir, { recursive: true });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();

      await openPanel(page, isMobile, "History");
      await page.getByRole("button", { name: "New Session" }).click();
      await openPanel(page, isMobile, "Transcript");

      const composer = page.locator(".pevo-composer");
      await expect(composer).toBeVisible();
      const agentSelect = page.getByRole("combobox", { name: "Agent" });
      await expect(agentSelect).toBeVisible();
      expect(await selectedOptionText(agentSelect)).toBe("Default Agent");
      expect(await optionTexts(agentSelect)).toContain("translate");
      expect(await selectedOptionText(page.getByRole("combobox", { name: "Permission mode" }))).toBe("Default Permission");
      const modelSelect = page.getByRole("combobox", { name: "Model" });
      await expect(modelSelect).toBeVisible();
      expect(await selectedOptionText(modelSelect)).toBe("noop");
      await expectSelectTextFits(modelSelect);
      await expect(page.getByRole("combobox", { name: "Variant" })).toBeVisible();
      await expect(page.getByRole("button", { name: "Context usage" })).toBeVisible();
      await assertComposerGeometry(page, { plan: false });

      await page.getByRole("button", { name: "Context usage" }).click();
      const contextPopover = page.getByRole("dialog", { name: "Context usage" });
      await expect(contextPopover).toBeVisible();
      const contextSummary = page.locator(".composerContextSummary strong");
      await contextSummary.evaluate((element) => {
        element.textContent = "16.7k/1.0M (1.6%)";
      });
      await expectElementInsideViewport(page, contextPopover);
      await expectTextNotClipped(contextSummary);
      await page.keyboard.press("Escape");

      await page.getByRole("button", { name: "Add attachments and options" }).click();
      await expect(page.getByRole("menuitem", { name: "Add images and files" })).toBeVisible();
      await page.screenshot({
        path: path.join(screenshotDir, `composer-menu-${testInfo.project.name}.png`)
      });
      await page.getByRole("switch", { name: "Plan mode" }).click();
      await expect(page.locator(".pevo-planChip")).toContainText("Plan");
      await page.keyboard.press("Escape");
      await assertComposerGeometry(page, { plan: true });
      await page.screenshot({
        path: path.join(screenshotDir, `composer-plan-${testInfo.project.name}.png`)
      });

      const textarea = page.getByPlaceholder("Ask Psychevo...");
      const oneLineBox = await textarea.boundingBox();
      await textarea.fill("Review the composer layout.\nKeep the controls aligned.\nMake the input grow.");
      const multilineBox = await textarea.boundingBox();
      expect(oneLineBox).not.toBeNull();
      expect(multilineBox).not.toBeNull();
      expect(multilineBox!.height).toBeGreaterThan(oneLineBox!.height + 20);
      await assertComposerGeometry(page, { plan: true });
      await page.screenshot({
        path: path.join(screenshotDir, `composer-multiline-${testInfo.project.name}.png`)
      });

      await forceInterruptVisualState(page);
      await expect(page.getByRole("button", { name: "Interrupt active turn" })).toBeVisible({ timeout: 10_000 });
      await assertComposerGeometry(page, { plan: true });
      await page.screenshot({
        path: path.join(screenshotDir, `composer-interrupt-${testInfo.project.name}.png`)
      });
    } finally {
      await server.stop();
    }
  });

  test("fits compact model labels and hides Transcript scrollbars until active", async ({ page, isMobile }) => {
    const server = await startPevoWeb({ live: false, model: "lmstudio/mimo-v2.5-pro" });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();

      await openPanel(page, isMobile, "History");
      await page.getByRole("button", { name: "New Session" }).click();
      await openPanel(page, isMobile, "Transcript");

      const modelSelect = page.getByRole("combobox", { name: "Model" });
      await expect(modelSelect).toBeVisible();
      expect(await selectedOptionText(modelSelect)).toBe("mimo-v2.5-pro");
      await expectSelectTextFits(modelSelect);
      await expect(modelSelect).toHaveAttribute("title", "lmstudio/mimo-v2.5-pro");

      const threadItems = page.locator(".pevo-threadItems");
      await threadItems.evaluate((container) => {
        container.innerHTML = Array.from({ length: 36 }, (_, index) => (
          `<article class="pevo-messageFrame"><div class="pevo-message">Transcript row ${index + 1}</div></article>`
        )).join("") + `<div data-testid="reading-column-probe" style="height: 1px;"></div>`;
      });
      await assertComposerDockTracksTranscriptColumn(page, isMobile);
      if (!isMobile) {
        await page.mouse.move(1, 1);
      }
      const restingColor = await scrollbarColor(threadItems);
      expectTransparentScrollbar(restingColor);
      const restingBox = await threadItems.boundingBox();
      expect(restingBox).not.toBeNull();

      await threadItems.evaluate((element) => {
        element.scrollTop = element.scrollHeight;
        element.dispatchEvent(new Event("scroll", { bubbles: true }));
      });
      await expect.poll(() => threadItems.evaluate((element) => element.classList.contains("is-scrolling"))).toBe(true);
      const scrollingColor = await scrollbarColor(threadItems);
      expect(scrollingColor).not.toBe(restingColor);
      const scrollingBox = await threadItems.boundingBox();
      expect(scrollingBox).not.toBeNull();
      expect(Math.abs(scrollingBox!.width - restingBox!.width)).toBeLessThanOrEqual(1);

      if (!isMobile) {
        await expect.poll(() => threadItems.evaluate((element) => element.classList.contains("is-scrolling"))).toBe(false);
        await threadItems.hover();
        const hoverColor = await scrollbarColor(threadItems);
        expect(hoverColor).not.toBe(restingColor);
      }
    } finally {
      await server.stop();
    }
  });

  test("keeps transcript rows in a readable shared column", async ({ page, isMobile }, testInfo) => {
    const server = await startPevoWeb({ live: false });
    mkdirSync(screenshotDir, { recursive: true });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();

      await openPanel(page, isMobile, "History");
      const newSessionButton = page.getByRole("button", { name: "New Session" });
      const settingsButton = page.getByRole("button", { name: "Settings" });
      await expectCssPropertyMatchesRootVar(newSessionButton, "color", "--pevo-nav-text");
      await expectCssPropertyMatchesRootVar(settingsButton, "color", "--pevo-nav-text");
      await newSessionButton.click();
      await openPanel(page, isMobile, "Transcript");

      const threadItems = page.locator(".pevo-threadItems");
      await threadItems.evaluate((container) => {
        container.innerHTML = `
          <div class="pevo-messageFrame is-user" data-testid="user-frame">
            <article class="pevo-message is-user"><p>Improve the GUI contrast.</p></article>
          </div>
          <div class="pevo-messageFrame is-assistant" data-testid="assistant-frame">
            <article class="pevo-message is-assistant"><p>The sidebar and transcript can be tightened without changing data flow.</p></article>
          </div>
          <article class="pevo-reasoning is-running" data-testid="thinking-row">
            <button class="pevo-reasoningHeader" type="button">
              <svg width="15" height="15" aria-hidden="true"></svg>
              <span>Thinking</span>
              <em>running</em>
            </button>
          </article>
        `;
      });

      const userFrame = page.getByTestId("user-frame");
      const assistantFrame = page.getByTestId("assistant-frame");
      const thinkingRow = page.getByTestId("thinking-row");
      const userBubble = userFrame.locator(".pevo-message.is-user");
      const thinkingHeader = thinkingRow.locator(".pevo-reasoningHeader");
      const [threadBox, userBox, assistantBox, thinkingBox] = await Promise.all([
        threadItems.boundingBox(),
        userFrame.boundingBox(),
        assistantFrame.boundingBox(),
        thinkingRow.boundingBox()
      ]);

      expect(threadBox).not.toBeNull();
      expect(userBox).not.toBeNull();
      expect(assistantBox).not.toBeNull();
      expect(thinkingBox).not.toBeNull();
      expect(assistantBox!.width).toBeLessThanOrEqual(762);
      expect(userBox!.x).toBeGreaterThan(assistantBox!.x);
      expect(userBox!.x + userBox!.width).toBeLessThanOrEqual(assistantBox!.x + 842);
      expect(Math.abs(thinkingBox!.x - assistantBox!.x)).toBeLessThanOrEqual(1);

      if (!isMobile) {
        expect(assistantBox!.x - threadBox!.x).toBeGreaterThan(16);
        expect(threadBox!.x + threadBox!.width - (userBox!.x + userBox!.width)).toBeGreaterThan(16);
      }

      await expectCssPropertyMatchesRootVar(userBubble, "background-color", "--pevo-user-bubble");
      await expectCssPropertyMatchesRootVar(userBubble, "border-top-color", "--pevo-user-bubble-border");
      await expectCssPropertyMatchesRootVar(thinkingHeader, "color", "--pevo-muted-strong");
      await page.screenshot({
        path: path.join(screenshotDir, `transcript-column-${testInfo.project.name}.png`)
      });
    } finally {
      await server.stop();
    }
  });
});

async function selectedOptionText(select: Locator): Promise<string> {
  return select.evaluate((element) => {
    const control = element as HTMLSelectElement;
    return control.selectedOptions[0]?.textContent?.trim() ?? "";
  });
}

async function optionTexts(select: Locator): Promise<string[]> {
  return select.evaluate((element) => {
    const control = element as HTMLSelectElement;
    return Array.from(control.options).map((option) => option.textContent?.trim() ?? "");
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
      paddingRight,
      selectedText,
      textWidth
    };
  });
  expect(result.paddingRight).toBeGreaterThanOrEqual(22);
  expect(result.textWidth).toBeLessThanOrEqual(result.contentWidth + 1);
}

async function expectTextNotClipped(locator: Locator) {
  const result = await locator.evaluate((element) => ({
    clippedX: element.scrollWidth > element.clientWidth + 1,
    clippedY: element.scrollHeight > element.clientHeight + 1,
    overflow: getComputedStyle(element).overflow
  }));
  expect(result.overflow).not.toBe("hidden");
  expect(result.clippedX).toBe(false);
  expect(result.clippedY).toBe(false);
}

async function expectElementInsideViewport(page: Page, locator: Locator) {
  const [box, viewport] = await Promise.all([locator.boundingBox(), page.viewportSize()]);
  expect(box).not.toBeNull();
  expect(viewport).not.toBeNull();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(viewport!.width);
}

async function scrollbarColor(locator: Locator): Promise<string> {
  return locator.evaluate((element) => getComputedStyle(element).scrollbarColor);
}

function expectTransparentScrollbar(value: string) {
  expect(value.toLowerCase()).toMatch(/transparent|rgba\(0, 0, 0, 0\)|color\(srgb 0 0 0 \/ 0\)/);
}

async function expectCssPropertyMatchesRootVar(locator: Locator, property: string, variableName: string) {
  const values = await locator.evaluate((element, [cssProperty, cssVariable]) => {
    const style = getComputedStyle(element);
    const probe = document.createElement("span");
    probe.style.setProperty(cssProperty, `var(${cssVariable})`);
    document.documentElement.appendChild(probe);
    const expected = getComputedStyle(probe).getPropertyValue(cssProperty);
    probe.remove();
    return {
      actual: style.getPropertyValue(cssProperty).trim().replace(/\s+/g, " "),
      expected: expected.trim().replace(/\s+/g, " ")
    };
  }, [property, variableName]);
  expect(values.actual).toBe(values.expected);
}

async function assertComposerDockTracksTranscriptColumn(page: Page, isMobile: boolean) {
  if (isMobile) {
    return;
  }
  const composerDock = page.locator(".composerDock");
  const readingProbe = page.getByTestId("reading-column-probe");
  const [dockBox, probeBox] = await Promise.all([
    composerDock.boundingBox(),
    readingProbe.boundingBox()
  ]);
  expect(dockBox).not.toBeNull();
  expect(probeBox).not.toBeNull();
  expect(Math.abs(dockBox!.x - probeBox!.x)).toBeLessThanOrEqual(12);
  expect(Math.abs(dockBox!.width - probeBox!.width)).toBeLessThanOrEqual(16);
}

async function assertComposerGeometry(page: Page, options: { plan: boolean }) {
  const add = page.getByRole("button", { name: "Add attachments and options" });
  const input = page.locator(".pevo-composerInput");
  const footer = page.locator(".pevo-composerFooter");
  const action = page.locator(".pevo-sendButton");
  const agent = page.getByRole("combobox", { name: "Agent" });
  const model = page.getByRole("combobox", { name: "Model" });
  const variant = page.getByRole("combobox", { name: "Variant" });
  const context = page.getByRole("button", { name: "Context usage" });
  const chip = page.locator(".pevo-planChip");
  const [addBox, inputBox, footerBox, actionBox, agentBox, modelBox, variantBox, contextBox, chipBox] = await Promise.all([
    add.boundingBox(),
    input.boundingBox(),
    footer.boundingBox(),
    action.boundingBox(),
    agent.boundingBox(),
    model.boundingBox(),
    variant.boundingBox(),
    context.boundingBox(),
    options.plan ? chip.boundingBox() : Promise.resolve(null)
  ]);

  expect(addBox).not.toBeNull();
  expect(inputBox).not.toBeNull();
  expect(footerBox).not.toBeNull();
  expect(actionBox).not.toBeNull();
  expect(agentBox).not.toBeNull();
  expect(modelBox).not.toBeNull();
  expect(variantBox).not.toBeNull();
  expect(contextBox).not.toBeNull();
  expect(agentBox!.width).toBeLessThanOrEqual(150);

  const actionCenterY = actionBox!.y + actionBox!.height / 2;
  const agentCenterY = agentBox!.y + agentBox!.height / 2;
  const modelCenterY = modelBox!.y + modelBox!.height / 2;
  const variantCenterY = variantBox!.y + variantBox!.height / 2;
  const contextCenterY = contextBox!.y + contextBox!.height / 2;
  expect(Math.abs(actionCenterY - agentCenterY)).toBeLessThanOrEqual(4);
  expect(Math.abs(actionCenterY - modelCenterY)).toBeLessThanOrEqual(4);
  expect(Math.abs(actionCenterY - variantCenterY)).toBeLessThanOrEqual(4);
  expect(Math.abs(actionCenterY - contextCenterY)).toBeLessThanOrEqual(4);

  if (options.plan) {
    expect(chipBox).not.toBeNull();
    expect(chipBox!.x).toBeGreaterThan(agentBox!.x);
    expect(modelBox!.x).toBeGreaterThan(chipBox!.x);
    expect(Math.abs(actionCenterY - (chipBox!.y + chipBox!.height / 2))).toBeLessThanOrEqual(5);
  }

  expect(inputBox!.y + inputBox!.height).toBeLessThanOrEqual(footerBox!.y + 2);
  expect(addBox!.x).toBeLessThan(agentBox!.x);
  expect(modelBox!.x).toBeGreaterThan(agentBox!.x);
  expect(modelBox!.x).toBeLessThan(actionBox!.x);
  expect(variantBox!.x).toBeGreaterThan(modelBox!.x);
  expect(contextBox!.x).toBeGreaterThan(variantBox!.x);
  expect(contextBox!.x).toBeLessThan(actionBox!.x);
}

async function forceInterruptVisualState(page: Page) {
  await page.locator(".pevo-composer").evaluate((composer) => {
    composer.classList.add("is-running");
    const button = composer.querySelector<HTMLButtonElement>(".pevo-sendButton");
    if (!button) {
      throw new Error("send button not found");
    }
    button.disabled = false;
    button.type = "button";
    button.classList.add("is-interrupt");
    button.setAttribute("aria-label", "Interrupt active turn");
    button.innerHTML = `<span class="pevo-stopGlyph" aria-hidden="true"></span>`;
  });
}

async function openPanel(page: Page, isMobile: boolean, name: "History" | "Status" | "Transcript") {
  if (isMobile) {
    await page.getByRole("button", { name }).click();
  }
}
