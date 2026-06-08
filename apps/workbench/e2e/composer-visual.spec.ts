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
      await expect(page.getByRole("combobox", { name: "Variant" })).toBeVisible();
      await expect(page.getByRole("button", { name: "Context usage" })).toBeVisible();
      await assertComposerGeometry(page, { plan: false });

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

async function assertComposerGeometry(page: Page, options: { plan: boolean }) {
  const input = page.locator(".pevo-composerInput");
  const footer = page.locator(".pevo-composerFooter");
  const action = page.locator(".pevo-sendButton");
  const agent = page.getByRole("combobox", { name: "Agent" });
  const model = page.getByRole("combobox", { name: "Model" });
  const variant = page.getByRole("combobox", { name: "Variant" });
  const context = page.getByRole("button", { name: "Context usage" });
  const chip = page.locator(".pevo-planChip");
  const [inputBox, footerBox, actionBox, agentBox, modelBox, variantBox, contextBox, chipBox] = await Promise.all([
    input.boundingBox(),
    footer.boundingBox(),
    action.boundingBox(),
    agent.boundingBox(),
    model.boundingBox(),
    variant.boundingBox(),
    context.boundingBox(),
    options.plan ? chip.boundingBox() : Promise.resolve(null)
  ]);

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
    expect(Math.abs(actionCenterY - (chipBox!.y + chipBox!.height / 2))).toBeLessThanOrEqual(5);
  }

  expect(inputBox!.y + inputBox!.height).toBeLessThanOrEqual(footerBox!.y + 2);
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
