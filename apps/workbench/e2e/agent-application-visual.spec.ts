import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page, type TestInfo } from "@playwright/test";
import { startPevoWeb } from "./harness";
import {
  prepareDeterministicAcpAgent,
  type DeterministicAcpAgentFixture
} from "./runtime-live.support";
import { visualScreenshotRoot } from "./visualArtifacts";

const screenshotDir = visualScreenshotRoot();

test.describe("Native and ACP Agent application visual contract", () => {
  test("renders Native and arbitrary ACP targets from one catalog without any Direct target", async ({ page }, testInfo) => {
    test.setTimeout(180_000);
    mkdirSync(screenshotDir, { recursive: true });
    const codex = prepareDeterministicAcpAgent("codex", screenshotDir);
    const opencode = prepareDeterministicAcpAgent("opencode", screenshotDir);
    const opaque = prepareDeterministicAcpAgent("opencode", screenshotDir, "stream", {
      runtimeRef: "zz-acp-fixture-4",
      profileLabel: "Boundary ACP",
      agentInfo: { name: "dev.psychevo.fixture.boundary", title: "Boundary" }
    });
    const server = await startPevoWeb({
      configAppend: `${codex.configAppend}\n${opencode.configAppend}\n${opaque.configAppend}`,
      live: false
    });
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      const catalog = await targetCatalog(page, server.cwd);
      const popover = await openRuntimePopover(page);
      const headings = await popover.locator(".agentRuntimeGroupLabel").allTextContents();
      expect(headings).not.toContain("Agent target");
      await expect(popover.getByText("Manage Agent targets", { exact: true })).toHaveCount(0);

      const targets = popover.getByRole("radiogroup", { name: "Agent target" });
      const choices = targets.getByRole("radio");
      await expect(choices).toHaveCount(catalog.compatibleTargets.length);
      expect(await choices.evaluateAll((nodes) => nodes.map((node) => node.getAttribute("aria-label"))))
        .toEqual(catalog.compatibleTargets.map((target) => target.label));
      await expect(targets).not.toContainText(/Direct/i);
      await expect(targets).not.toContainText(/Psychevo \(Native\)/i);

      const nativeTarget = targetByIdentity(catalog, null, "native");
      const namedNativeTarget = targetByIdentity(catalog, "translate", "native");
      const codexTarget = targetByIdentity(catalog, codex.runtimeRef, codex.runtimeRef);
      const opencodeTarget = targetByIdentity(catalog, "opencode", "opencode");
      const opaqueTarget = targetByIdentity(catalog, opaque.runtimeRef, opaque.runtimeRef);
      expect(nativeTarget.profileLabel).toMatch(/Psychevo.*Native/i);
      expect(namedNativeTarget.agentLabel).toBe("translate");
      expect(codexTarget.profileLabel).toMatch(/Codex/i);
      expect(opencodeTarget.profileLabel).toMatch(/OpenCode/i);
      expect(new Set(catalog.compatibleTargets.map((target) => target.targetId)).size)
        .toBe(catalog.compatibleTargets.length);
      expect(opaqueTarget.targetId).toBeTruthy();
      expect(opaqueTarget.agentRef).toBe(opaque.runtimeRef);
      expect(opaqueTarget.targetId).not.toContain(opaqueTarget.runtimeProfileRef);
      expect(opaqueTarget.targetId).not.toContain(opaqueTarget.agentRef as string);
      writeFileSync(
        path.join(screenshotDir, `agent-target-catalog-proof-${testInfo.project.name}.json`),
        JSON.stringify(catalog.compatibleTargets, null, 2)
      );
      await expectInsideViewport(page, popover);
      await capture(page, testInfo, "agent-runtime-selector-native-acp");
    } finally {
      await server.stop();
    }
  });

  test("shows the same model and reasoning controls for Codex ACP and OpenCode ACP", async ({ page }, testInfo) => {
    test.setTimeout(180_000);
    mkdirSync(screenshotDir, { recursive: true });
    const codex = prepareDeterministicAcpAgent("codex", screenshotDir);
    const opencode = prepareDeterministicAcpAgent("opencode", screenshotDir);
    const server = await startPevoWeb({
      configAppend: `${codex.configAppend}\n${opencode.configAppend}`,
      live: false
    });
    const websocketFrames = captureWebSocketFrames(page);
    try {
      await page.goto(server.url);
      const catalog = await targetCatalog(page, server.cwd);
      const codexTarget = targetByIdentity(catalog, codex.runtimeRef, codex.runtimeRef);
      const opencodeTarget = targetByIdentity(catalog, "opencode", "opencode");
      await selectTarget(page, codexTarget);
      await expect(page.getByRole("button", { name: "Agent target" }))
        .toHaveAttribute("title", `${codexTarget.agentLabel} · ${codexTarget.profileLabel}`);
      await runTurn(page, "Initialize the Codex ACP session controls", /Codex ACP response.*model=fixture\/default/i);
      const codexThreadId = await currentThreadId(page, server.cwd);
      await selectControl(page, "Model", "Fixture second");
      await waitForControlValue(page, server.cwd, codexThreadId, "model", "fixture/second");
      await selectControl(page, "Reasoning effort", "High");
      await waitForControlValue(page, server.cwd, codexThreadId, "effort", "high");
      await runTurn(page, "Prove shared Codex ACP controls", /Codex ACP response.*model=fixture\/second.*effort=high/i);

      const codexContext = await threadContext(page, server.cwd, codexThreadId);
      expect(codexContext.targetId).toBe(codexTarget.targetId);
      expect(controlFact(codexContext, "model")).toMatchObject({
        effectiveSource: "threadPreference",
        effectiveValue: "fixture/second",
        surfaceRole: "model"
      });
      expect(controlFact(codexContext, "effort")).toMatchObject({
        effectiveSource: "threadPreference",
        effectiveValue: "high",
        surfaceRole: "reasoning"
      });
      await expect(page.getByRole("button", { name: "Agent target" })).toContainText(/codex/i);
      await capture(page, testInfo, "codex-acp-common-controls");

      await selectTarget(page, opencodeTarget);
      await expect(page.getByRole("button", { name: "Agent target" }))
        .toHaveAttribute("title", `${opencodeTarget.agentLabel} · ${opencodeTarget.profileLabel}`);
      const mode = page.locator('select[aria-label="Mode"]:visible');
      await expect(mode).toHaveCount(1);
      await expect(mode).toHaveValue("0");
      expect(await mode.locator("option").allTextContents()).toEqual(["Build", "Plan"]);
      await mode.hover();
      const modeHover = await mode.evaluate((element) => {
        const wrapper = element.closest<HTMLElement>(".statusSelect");
        return {
          background: wrapper ? getComputedStyle(wrapper).backgroundColor : "",
          cursor: getComputedStyle(element).cursor,
          shadow: wrapper ? getComputedStyle(wrapper).boxShadow : ""
        };
      });
      const agentTrigger = page.getByRole("button", { name: "Agent target" });
      await agentTrigger.hover();
      const agentHover = await agentTrigger.evaluate((element) => ({
        background: getComputedStyle(element).backgroundColor,
        cursor: getComputedStyle(element).cursor,
        shadow: getComputedStyle(element).boxShadow
      }));
      expect(modeHover).toEqual(agentHover);
      expect(modeHover.cursor).toBe("pointer");
      expect(modeHover.shadow).toBe("none");
      await mode.focus();
      expect(await mode.evaluate((element) => getComputedStyle(element).outlineStyle)).toBe("none");
      expect(await mode.locator("..").evaluate((element) => getComputedStyle(element).outlineStyle)).toBe("none");
      await expect(page.locator('select[aria-label="Agent"]:visible')).toHaveCount(0);
      const modelButton = page.getByRole("button", { name: "Model" });
      await expect(modelButton).toBeVisible();
      await selectControl(page, "Model", "Fixture second");
      await expect(page.getByText("Thread Context changed; refresh it before changing this control.")).toHaveCount(0);
      await selectControl(page, "Mode", "Plan");
      await runTurn(
        page,
        "Prove prepared OpenCode ACP controls",
        /OpenCode ACP response.*model=fixture\/second.*mode=plan/i
      );
      const opencodeContext = await threadContext(page, server.cwd, await currentThreadId(page, server.cwd));
      expect(opencodeContext.targetId).toBe(opencodeTarget.targetId);
      await modelButton.click();
      const modelPicker = page.getByRole("dialog", { name: "Model and reasoning" });
      await expect(modelPicker.getByRole("radiogroup", { name: "Model" })).toBeVisible();
      await expect(modelPicker.getByRole("radiogroup", { name: "Reasoning" })).toBeVisible();
      await page.keyboard.press("Escape");
      await expect(page.getByRole("button", { name: "Agent target" })).toContainText(`${opencodeTarget.agentLabel} (ACP)`);
      await capture(page, testInfo, "opencode-acp-common-controls");
    } finally {
      writeFileSync(
        path.join(screenshotDir, `shared-control-rpc-${testInfo.project.name}.json`),
        JSON.stringify(rpcFrameProof(websocketFrames), null, 2)
      );
      await server.stop();
    }
  });

  test("shows an active-turn model change taking effect only on the next ACP turn", async ({ page }, testInfo) => {
    test.setTimeout(240_000);
    mkdirSync(screenshotDir, { recursive: true });
    const fixture = prepareDeterministicAcpAgent("codex", screenshotDir, "active_next_control");
    const server = await startPevoWeb({ configAppend: fixture.configAppend, live: false });
    const websocketFrames = captureWebSocketFrames(page);
    try {
      await page.goto(server.url);
      const target = targetByIdentity(
        await targetCatalog(page, server.cwd),
        fixture.runtimeRef,
        fixture.runtimeRef
      );
      await selectTarget(page, target);
      await runTurn(page, "prime the visual bound session", /Codex ACP response 1.*model=fixture\/default/i);
      const threadId = await currentThreadId(page, server.cwd);
      const before = await threadContext(page, server.cwd, threadId);
      expect(controlFact(before, "model")?.effectiveValue).toBe("fixture/default");

      await page.getByPlaceholder("Ask Psychevo...").fill("hold the visual turn while changing the next model");
      await page.getByRole("button", { name: "Send message" }).click();
      const permission = await waitForPendingInteraction(page, server.cwd, threadId, "permission");
      await expect(page.getByLabel("Pending requests")).toContainText(
        "Approve the once-only interaction",
        { timeout: 60_000 }
      );
      await selectControl(page, "Model", "Fixture second");
      await waitForControlValue(page, server.cwd, threadId, "model", "fixture/second");
      await expect.poll(() => rpcResultsForMethod(websocketFrames, "thread/control/set")
        .some((result) => result.status === "stored"), { timeout: 10_000 }).toBe(true);
      expect(traceEvents(fixture).filter((event) => (
        event.type === "config_set" && event.value === "fixture/second"
      ))).toHaveLength(0);
      await capture(page, testInfo, "active-turn-next-model-queued");
      await respondToInteraction(page, server.cwd, threadId, permission.actionId, {
        kind: "permission",
        decision: "allowOnce"
      });
      const clarify = await waitForPendingInteraction(page, server.cwd, threadId, "clarify");
      await respondToInteraction(page, server.cwd, threadId, clarify.actionId, {
        kind: "clarify",
        answers: [["repo root"]]
      });

      await expect(page.locator(".pevo-message.is-assistant").filter({
        hasText: /Codex ACP response 2.*model=fixture\/default/i
      })).toHaveCount(1, { timeout: 60_000 });
      await expect(page.locator(".pevo-composer").first()).not.toHaveClass(/is-running/, {
        timeout: 30_000
      });
      expect(traceEvents(fixture).filter((event) => (
        event.type === "config_set" && event.value === "fixture/second"
      ))).toHaveLength(0);
      const afterQueuedControl = await threadContext(page, server.cwd, threadId);
      expect(afterQueuedControl.controlRevision).not.toBe(before.controlRevision);
      expect(controlFact(afterQueuedControl, "model")?.effectiveValue).toBe("fixture/second");
      await expect.poll(() => {
        const context = workbenchRpcResultsForMethod(websocketFrames, "thread/context/read").at(-1);
        const sendability = context?.sendability as { allowed?: boolean } | undefined;
        const pending = Array.isArray(context?.pendingInteractions) ? context.pendingInteractions : [];
        return {
          allowed: sendability?.allowed ?? false,
          controlRevision: context?.controlRevision ?? null,
          pending: pending.length
        };
      }, { timeout: 10_000 }).toEqual({
        allowed: true,
        controlRevision: afterQueuedControl.controlRevision,
        pending: 0
      });

      await runTurn(page, "use the queued model on the visual next turn", /Codex ACP response 3.*model=fixture\/second/i);
      const accepted = traceEvents(fixture).filter((event) => event.type === "prompt_accepted");
      expect(accepted.map((event) => (event.config as Record<string, unknown> | undefined)?.model))
        .toEqual(["fixture/default", "fixture/default", "fixture/second"]);
      const ordered = traceEvents(fixture);
      const heldPrompt = ordered.findIndex((event) => event.type === "prompt_accepted" && event.turn === 2);
      const nextControl = ordered.findIndex((event) => (
        event.type === "config_set" && event.value === "fixture/second"
      ));
      const nextPrompt = ordered.findIndex((event) => event.type === "prompt_accepted" && event.turn === 3);
      expect(heldPrompt).toBeGreaterThanOrEqual(0);
      expect(nextControl).toBeGreaterThan(heldPrompt);
      expect(nextPrompt).toBeGreaterThan(nextControl);
      writeFileSync(
        path.join(screenshotDir, `active-turn-next-control-proof-${testInfo.project.name}.json`),
        JSON.stringify({
          accepted,
          afterQueuedControl,
          before,
          controlReceipts: rpcResultsForMethod(websocketFrames, "thread/control/set")
        }, null, 2)
      );
      await capture(page, testInfo, "next-turn-model-observed");
    } finally {
      writeFileSync(
        path.join(screenshotDir, `active-turn-rpc-${testInfo.project.name}.json`),
        JSON.stringify(rpcFrameProof(websocketFrames), null, 2)
      );
      await server.stop();
    }
  });

  test("shows managed Codex ACP as missing and then usable from the pinned offline adapter", async ({ page }, testInfo) => {
    test.setTimeout(240_000);
    mkdirSync(screenshotDir, { recursive: true });
    const home = mkdtempSync(path.join(screenshotDir, "managed-codex-home-"));
    const fixture = prepareDeterministicAcpAgent("codex", screenshotDir, "managed", { home });
    const server = await startPevoWeb({ home, live: false, processEnv: fixture.installEnv ?? undefined });
    try {
      await page.goto(server.url);
      const missingTarget = targetByIdentity(await targetCatalog(page, server.cwd), "codex", "codex");
      const missingTargetId = missingTarget.targetId;
      expect(missingTarget.ready).toBe(false);
      const missingPopover = await openRuntimePopover(page);
      const missingCodex = targetChoice(missingPopover, missingTarget);
      await expect(missingCodex).toBeDisabled();
      await expect(missingCodex).toHaveAttribute("title", /not installed.*backend\/install/i);
      await expect(missingPopover).not.toContainText(/Direct/i);
      await capture(page, testInfo, "managed-codex-acp-missing");

      const installed = await gatewayRequest(page, "backend/install", {
        id: "codex",
        scope: webScope(server.cwd)
      }) as { path: string; status: string };
      expect(installed).toMatchObject({ path: fixture.managedRootPath, status: "ready" });
      expect(fixture.managedSealPath && existsSync(fixture.managedSealPath)).toBe(true);
      expect(JSON.parse(readFileSync(fixture.managedSealPath as string, "utf8"))).toMatchObject({
        schemaVersion: 1,
        treeSha256: expect.stringMatching(/^[0-9a-f]{64}$/)
      });
      expect(JSON.parse(readFileSync(fixture.fakeNpmLogPath as string, "utf8"))).toMatchObject({
        args: ["ci", "--omit=dev", "--ignore-scripts", "--no-audit", "--no-fund"],
        capturedMarker: "captured"
      });
      rmSync(fixture.fakeNpmPath as string);
      expect(existsSync(fixture.fakeNpmPath as string)).toBe(false);

      const listed = await gatewayRequest(page, "backend/list", { scope: webScope(server.cwd) }) as {
        backends: Array<{ id: string; command: string | null; args: string[] }>;
      };
      expect(listed.backends.find((backend) => backend.id === "codex")).toMatchObject({
        command: fixture.managedBinPath,
        args: []
      });
      await page.reload();
      const recoveredTarget = targetByIdentity(await targetCatalog(page, server.cwd), "codex", "codex");
      expect(recoveredTarget.ready).toBe(true);
      expect(recoveredTarget.targetId).toBe(missingTargetId);
      await selectTarget(page, recoveredTarget);
      await runTurn(page, "Run the pinned offline managed adapter", /Codex ACP response/i);
      await expect(page.getByRole("button", { name: "Agent target" })).toContainText(`${recoveredTarget.agentLabel} (ACP)`);
      const recoveredContext = await threadContext(
        page,
        server.cwd,
        await currentThreadId(page, server.cwd)
      );
      expect(recoveredContext.targetId).toBe(missingTargetId);
      expect(fixture.managedBinPath).toContain(path.join("codex-acp", "1.1.2", "node_modules", ".bin"));
      writeFileSync(
        path.join(screenshotDir, `managed-codex-target-proof-${testInfo.project.name}.json`),
        JSON.stringify({
          install: installed,
          installerRemoved: !existsSync(fixture.fakeNpmPath as string),
          managedCommand: fixture.managedBinPath,
          missingTargetId,
          recoveredTarget,
          recoveredBoundTargetId: recoveredContext.targetId,
          seal: JSON.parse(readFileSync(fixture.managedSealPath as string, "utf8"))
        }, null, 2)
      );
      await capture(page, testInfo, "managed-codex-acp-recovered");
    } finally {
      await server.stop();
    }
  });

  test("shows process-ephemeral ACP history as partial and unavailable after restart", async ({ page, isMobile }, testInfo) => {
    test.setTimeout(240_000);
    mkdirSync(screenshotDir, { recursive: true });
    const persistentRoot = mkdtempSync(path.join(screenshotDir, "process-ephemeral-"));
    const cwd = path.join(persistentRoot, "cwd");
    const home = path.join(persistentRoot, "home");
    const dbPath = path.join(persistentRoot, "state.db");
    mkdirSync(cwd, { recursive: true });
    mkdirSync(home, { recursive: true });
    const fixture = prepareDeterministicAcpAgent("opencode", screenshotDir, "process_ephemeral", {
      runtimeRef: "ephemeral-acp",
      profileLabel: "Ephemeral ACP",
      agentInfo: { name: "dev.psychevo.fixture.ephemeral", title: "Ephemeral" }
    });
    let threadId = "";
    let beforeRestart: Record<string, unknown> | null = null;
    const firstServer = await startPevoWeb({ configAppend: fixture.configAppend, cwd, dbPath, home, live: false });
    try {
      await page.goto(firstServer.url);
      const target = targetByIdentity(
        await targetCatalog(page, firstServer.cwd),
        fixture.runtimeRef,
        fixture.runtimeRef
      );
      await selectTarget(page, target);
      await runTurn(page, "process-ephemeral visual first turn", /Ephemeral ACP response 1/i);
      threadId = await currentThreadId(page, firstServer.cwd);
      beforeRestart = await threadContext(page, firstServer.cwd, threadId);
      expect(beforeRestart.history).toMatchObject({ owner: "process" });
      await capture(page, testInfo, "process-ephemeral-before-restart");
    } finally {
      await firstServer.stop();
    }

    const restartedServer = await startPevoWeb({ configAppend: fixture.configAppend, cwd, dbPath, home, live: false });
    try {
      await page.goto(restartedServer.url);
      await reopenOnlyPersistedSession(page, isMobile);
      const afterRestart = await threadContext(page, restartedServer.cwd, threadId);
      expect(afterRestart.history).toMatchObject({ owner: "process", fidelity: "partial" });
      expect(afterRestart.sendability).toMatchObject({ allowed: false, recoveryAction: "thread/start" });
      expect(afterRestart.contextRevision).not.toBe(beforeRestart?.contextRevision);
      expect(String((afterRestart.history as Record<string, unknown>).hint))
        .toMatch(/resident Agent session snapshot|process-ephemeral|cannot be resumed/i);
      await page.getByPlaceholder("Ask Psychevo...").fill("do not fake process-ephemeral recovery");
      const blockedSend = page.getByRole("button", { name: "Send message" });
      await expect(blockedSend).toBeDisabled();
      await expect(blockedSend).toHaveAttribute(
        "title",
        /process-ephemeral.*cannot be resumed after process restart.*new Thread/i
      );
      expect(traceEvents(fixture).filter((event) => event.type === "session_load" || event.type === "session_resume"))
        .toHaveLength(0);
      expect(traceEvents(fixture).filter((event) => event.type === "prompt_accepted")).toHaveLength(1);
      writeFileSync(
        path.join(screenshotDir, `process-ephemeral-history-proof-${testInfo.project.name}.json`),
        JSON.stringify({ afterRestart, beforeRestart, threadId, trace: traceEvents(fixture) }, null, 2)
      );
      await capture(page, testInfo, "process-ephemeral-restart-unavailable");
    } finally {
      await restartedServer.stop();
    }
  });

  test("edits a Channel through the same ACP Runtime Profile catalog", async ({ page, isMobile }, testInfo) => {
    test.setTimeout(180_000);
    mkdirSync(screenshotDir, { recursive: true });
    const codex = prepareDeterministicAcpAgent("codex", screenshotDir, "channel_controls");
    const opencode = prepareDeterministicAcpAgent("opencode", screenshotDir, "channel_controls");
    const opaque = prepareDeterministicAcpAgent("opencode", screenshotDir, "channel_controls", {
      runtimeRef: "zz-acp-fixture-4",
      profileLabel: "Boundary ACP",
      agentInfo: { name: "dev.psychevo.fixture.boundary", title: "Boundary" }
    });
    const channelConfig = [
      "[[channels.connections]]",
      'id = "visual-acp-channel"',
      'channel = "telegram"',
      'label = "ACP Agent Channel"',
      'transport = "polling"',
      "enabled = false",
      `runtime_ref = ${JSON.stringify(codex.runtimeRef)}`,
      'credential_env = "VISUAL_ACP_CHANNEL_TOKEN"',
      'allow_users = ["42"]',
      "require_mention = false",
      ""
    ].join("\n");
    const server = await startPevoWeb({
      configAppend: `${codex.configAppend}\n${opencode.configAppend}\n${opaque.configAppend}\n${channelConfig}`,
      live: false
    });
    try {
      await page.goto(server.url);
      const catalog = await targetCatalog(page, server.cwd);
      const expectedProfiles = [
        targetByIdentity(catalog, null, "native"),
        targetByIdentity(catalog, codex.runtimeRef, codex.runtimeRef),
        targetByIdentity(catalog, "opencode", "opencode"),
        targetByIdentity(catalog, opaque.runtimeRef, opaque.runtimeRef)
      ];
      if (isMobile) {
        await page.getByRole("button", { name: "History", exact: true }).click();
      }
      await page.getByRole("button", { name: "Settings", exact: true }).click();
      const settings = page.getByRole("region", { name: "Settings", exact: true });
      await settings.getByRole("button", { name: "Channels" }).click();
      const channels = settings.getByRole("region", { name: "Channels", exact: true });
      await expect(channels).toContainText("ACP Agent Channel");
      await channels.getByRole("button", { name: "Settings visual-acp-channel" }).click();

      const profile = settings.getByRole("combobox", { name: "Channel Runtime Profile" });
      await expect(profile).toHaveValue(codex.runtimeRef);
      await expect(profile.locator('option[value=""]')).toHaveText("Profile default");
      for (const target of expectedProfiles) {
        await expect(profile.locator(`option[value="${target.runtimeProfileRef}"]`))
          .toHaveText(target.profileLabel);
      }
      const model = settings.getByRole("combobox", { name: "Channel model" });
      await expect(model).toBeEnabled();
      await expect(model.locator('option[value="fixture/default"]')).toHaveText("fixture/default");
      await expect(settings.getByRole("group", { name: "Permission mode" })).toHaveCount(0);
      await profile.selectOption(opencode.runtimeRef);
      await expect(profile).toHaveValue(opencode.runtimeRef);
      await expect(model).toBeEnabled();
      await expect(model.locator('option[value="fixture/default"]')).toHaveText("fixture/default");
      await expect(settings.getByText("Uses runtime default")).toHaveCount(0);
      if (isMobile) {
        await model.scrollIntoViewIfNeeded();
      }
      await expectInsideViewport(page, settings);
      await settings.getByRole("region", { name: "Runtime settings" }).screenshot({
        path: path.join(screenshotDir, `channel-acp-runtime-controls-${testInfo.project.name}.png`)
      });
      await capture(page, testInfo, "channel-acp-runtime-profile");
    } finally {
      await server.stop();
    }
  });

  test("imports Agent-owned sessions and renders negotiated lifecycle actions", async ({ page, isMobile }, testInfo) => {
    test.setTimeout(180_000);
    mkdirSync(screenshotDir, { recursive: true });
    const codex = prepareDeterministicAcpAgent("codex", screenshotDir, "stream");
    const opencode = prepareDeterministicAcpAgent("opencode", screenshotDir, "stream");
    writeFileSync(codex.statePath, JSON.stringify({
      nextSession: 2,
      promptCount: 0,
      sessions: {
        "codex-external": {
          config: { model: "fixture/default", effort: "medium", mode: "build" },
          cwd: null,
          messages: [],
          title: "Codex external session"
        }
      }
    }));
    writeFileSync(opencode.statePath, JSON.stringify({
      nextSession: 2,
      promptCount: 0,
      sessions: {
        "opencode-external": {
          config: { model: "fixture/default", effort: "medium", mode: "build" },
          cwd: null,
          messages: [],
          title: "OpenCode external session"
        }
      }
    }));
    const server = await startPevoWeb({
      configAppend: `${codex.configAppend}\n${opencode.configAppend}`,
      live: false
    });
    try {
      await page.goto(server.url);
      if (isMobile) await page.getByRole("button", { name: "History", exact: true }).click();
      await page.getByRole("button", { name: "Import Agent session" }).click();
      const importDialog = page.getByRole("dialog", { name: "Import Agent session" });
      await expect(importDialog).toContainText("Codex external session");
      await expect(importDialog).toContainText("OpenCode external session");
      expect(JSON.stringify(await gatewayRequest(page, "thread/import/list", {
        scope: { cwd: server.cwd, source: { kind: "web", rawId: "visual-import-proof" } },
        cursors: {}
      }))).not.toContain("opencode-external");
      await capture(page, testInfo, "agent-session-import");

      await importDialog.getByRole("button", { name: /OpenCode external session/ }).click();
      await expect(importDialog).toBeHidden();
      if (isMobile) await page.getByRole("button", { name: "History", exact: true }).click();
      await page.getByRole("button", { name: "Import Agent session" }).click();
      const secondImportDialog = page.getByRole("dialog", { name: "Import Agent session" });
      await secondImportDialog.getByRole("button", { name: /Codex external session/ }).click();
      await expect(secondImportDialog).toBeHidden();
      if (isMobile) await page.getByRole("button", { name: "History", exact: true }).click();

      const openCodeRow = page.locator(".pevo-sessionRow").filter({ hasText: "OpenCode external session" });
      if (!isMobile) await openCodeRow.hover();
      await openCodeRow.locator('summary[aria-label="Session actions"]').click();
      await expect(openCodeRow.getByRole("menuitem", { name: "Fork" })).toBeVisible();
      const openCodeDelete = openCodeRow.getByRole("menuitem", { name: "Delete" });
      await expect(openCodeDelete).toBeDisabled();
      await expect(openCodeDelete).toHaveAttribute("title", /did not advertise persistent session deletion/i);
      await capture(page, testInfo, "agent-session-lifecycle-actions");
      await openCodeRow.locator('summary[aria-label="Session actions"]').click();
      await openCodeRow.getByRole("button", { name: /OpenCode external session/ }).click();
      await expect(openCodeRow).toHaveClass(/is-active/);
      if (isMobile) {
        await page.getByRole("button", { name: "History", exact: true }).click();
      }

      const codexRow = page.locator(".pevo-sessionRow").filter({ hasText: "Codex external session" });
      if (!isMobile) await codexRow.hover();
      await codexRow.locator('summary[aria-label="Session actions"]').click();
      await codexRow.getByRole("menuitem", { name: "Delete" }).click();
      const deleteDialog = page.getByRole("dialog", { name: "Delete session?" });
      await expect(deleteDialog).toContainText(/Codex.*session/i);
      await expect(deleteDialog).toContainText(/Remote deletion must succeed/i);
      await capture(page, testInfo, "agent-session-delete-confirmation");
    } finally {
      await server.stop();
    }
  });
});

async function openRuntimePopover(page: Page): Promise<Locator> {
  const entry = page.getByRole("button", { name: "Agent target", exact: true });
  await expect(entry).toBeVisible({ timeout: 30_000 });
  await entry.click();
  const popover = page.getByRole("dialog", { name: "Agent target" });
  await expect(popover).toBeVisible();
  return popover;
}

async function selectTarget(page: Page, target: TargetCatalog["compatibleTargets"][number]) {
  const popover = await openRuntimePopover(page);
  const choice = targetChoice(popover, target);
  await expect(choice).toBeEnabled();
  await choice.click();
}

function targetChoice(
  popover: Locator,
  target: TargetCatalog["compatibleTargets"][number]
): Locator {
  return popover.getByRole("radiogroup", { name: "Agent target" })
    .getByRole("radio", {
      name: new RegExp(`^(?:Start a new thread with )?${escapeRegExp(target.label)}$`)
    });
}

async function selectControl(page: Page, label: string, optionLabel: string) {
  if (label === "Model" || /Reasoning/i.test(label)) {
    const button = page.getByRole("button", { name: "Model" }).first();
    await expect(button).toBeVisible({ timeout: 30_000 });
    const existing = page.getByRole("dialog", { name: "Model and reasoning" });
    if (!await existing.isVisible().catch(() => false)) await button.click();
    const picker = page.getByRole("dialog", { name: "Model and reasoning" });
    const group = picker.getByRole("radiogroup", { name: label === "Model" ? "Model" : "Reasoning" });
    await group.getByRole("radio", { name: optionLabel }).click();
    return;
  }
  const control = page.getByRole("combobox", { name: label }).first();
  await expect(control).toBeVisible({ timeout: 30_000 });
  await control.selectOption({ label: optionLabel });
}

async function runTurn(page: Page, prompt: string, answer: RegExp) {
  await page.getByPlaceholder("Ask Psychevo...").fill(prompt);
  const send = page.getByRole("button", { name: "Send message" });
  await expect(send).toBeEnabled();
  await send.click();
  await expect(page.locator(".pevo-message.is-assistant").filter({ hasText: answer })).toHaveCount(1, {
    timeout: 60_000
  });
  await expect(page.locator(".pevo-composer").first()).not.toHaveClass(/is-running/, { timeout: 30_000 });
}

async function threadContext(page: Page, cwd: string, threadId: string): Promise<Record<string, unknown>> {
  return gatewayRequest(page, "thread/context/read", {
    threadId,
    target: null,
    scope: webScope(cwd)
  }) as Promise<Record<string, unknown>>;
}

type TargetCatalog = {
  compatibleTargets: Array<{
    targetId: string;
    agentRef: string | null;
    runtimeProfileRef: string;
    agentLabel: string;
    profileLabel: string;
    label: string;
    ready: boolean;
    unavailableReason: string | null;
  }>;
};

async function targetCatalog(page: Page, cwd: string): Promise<TargetCatalog> {
  // Workbench loads backend administration before refreshing Thread Context.
  // Wait on the same explicit materialization boundary so catalog assertions
  // cannot race the managed Codex product shortcut into existence.
  await gatewayRequest(page, "backend/list", { scope: webScope(cwd) });
  return gatewayRequest(page, "thread/context/read", {
    threadId: null,
    target: null,
    scope: webScope(cwd)
  }) as Promise<TargetCatalog>;
}

function targetByIdentity(
  catalog: TargetCatalog,
  agentRef: string | null,
  runtimeProfileRef: string
): TargetCatalog["compatibleTargets"][number] {
  const target = catalog.compatibleTargets.find((candidate) => (
    candidate.agentRef === agentRef && candidate.runtimeProfileRef === runtimeProfileRef
  ));
  if (!target) {
    throw new Error(`missing Agent target for ${agentRef ?? "<default>"} · ${runtimeProfileRef}`);
  }
  return target;
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function webScope(cwd: string) {
  return {
    cwd,
    source: { kind: "web", rawId: null, lifetime: "persistent", rawIdentity: null, visibleName: null }
  };
}

async function waitForPendingInteraction(
  page: Page,
  _cwd: string,
  threadId: string,
  kind: "permission" | "clarify"
): Promise<{ actionId: string; kind: string }> {
  let interaction: { actionId: string; kind: string } | undefined;
  await expect.poll(async () => {
    const snapshot = await gatewayRequest(page, "thread/read", { threadId }) as {
      pendingActions?: unknown[];
    };
    const pending = Array.isArray(snapshot.pendingActions) ? snapshot.pendingActions : [];
    interaction = pending.find((candidate): candidate is { actionId: string; kind: string } => (
      typeof candidate === "object"
      && candidate !== null
      && (candidate as Record<string, unknown>).kind === kind
      && typeof (candidate as Record<string, unknown>).actionId === "string"
    ));
    return interaction?.actionId ?? null;
  }, { timeout: 30_000 }).not.toBeNull();
  return interaction as { actionId: string; kind: string };
}

async function respondToInteraction(
  page: Page,
  cwd: string,
  threadId: string,
  interactionId: string,
  response: Record<string, unknown>
) {
  const result = await gatewayRequest(page, "thread/interaction/respond", {
    interactionId,
    response,
    scope: webScope(cwd),
    threadId
  }) as { accepted: boolean };
  expect(result.accepted).toBe(true);
}

async function reopenOnlyPersistedSession(page: Page, isMobile: boolean) {
  if (isMobile) {
    await page.getByRole("button", { name: "History", exact: true }).click();
  }
  const session = page.getByRole("region", { name: "Sessions" })
    .locator(".pevo-sessionRow:not(.is-draft) .pevo-sessionMain");
  await expect(session).toHaveCount(1, { timeout: 30_000 });
  await session.click();
  if (isMobile) {
    await page.getByRole("button", { name: "Transcript", exact: true }).click();
  }
}

type FixtureTraceEvent = {
  type?: string;
  value?: unknown;
  [key: string]: unknown;
};

type WebSocketFrameCapture = { received: string[]; sent: string[] };

function captureWebSocketFrames(page: Page): WebSocketFrameCapture {
  const capture: WebSocketFrameCapture = { received: [], sent: [] };
  page.on("websocket", (socket) => {
    socket.on("framesent", (event) => capture.sent.push(String(event.payload)));
    socket.on("framereceived", (event) => capture.received.push(String(event.payload)));
  });
  return capture;
}

function rpcFrameProof(capture: WebSocketFrameCapture) {
  const relevantMethods = new Set(["thread/context/read", "thread/control/set", "turn/start"]);
  const sent = capture.sent.flatMap((payload) => {
    try {
      const message = JSON.parse(payload) as { id?: unknown; method?: string; params?: unknown };
      return message.method && relevantMethods.has(message.method) ? [message] : [];
    } catch {
      return [];
    }
  });
  const ids = new Set(sent.map((message) => JSON.stringify(message.id)));
  const received = capture.received.flatMap((payload) => {
    try {
      const message = JSON.parse(payload) as { id?: unknown; result?: unknown; error?: unknown };
      return ids.has(JSON.stringify(message.id)) ? [message] : [];
    } catch {
      return [];
    }
  });
  return { received, sent };
}

function rpcResultsForMethod(capture: WebSocketFrameCapture, method: string): Array<Record<string, unknown>> {
  const requestIds = new Set(capture.sent.flatMap((payload) => {
    try {
      const message = JSON.parse(payload) as { id?: unknown; method?: string };
      return message.method === method && message.id != null ? [String(message.id)] : [];
    } catch {
      return [];
    }
  }));
  return capture.received.flatMap((payload) => {
    try {
      const message = JSON.parse(payload) as { id?: unknown; result?: unknown };
      return message.id != null
        && requestIds.has(String(message.id))
        && typeof message.result === "object"
        && message.result !== null
        ? [message.result as Record<string, unknown>]
        : [];
    } catch {
      return [];
    }
  });
}

function workbenchRpcResultsForMethod(
  capture: WebSocketFrameCapture,
  method: string
): Array<Record<string, unknown>> {
  const requestIds = new Set(capture.sent.flatMap((payload) => {
    try {
      const message = JSON.parse(payload) as { id?: unknown; method?: string };
      const id = message.id == null ? "" : String(message.id);
      return message.method === method && /^\d+$/.test(id) ? [id] : [];
    } catch {
      return [];
    }
  }));
  return capture.received.flatMap((payload) => {
    try {
      const message = JSON.parse(payload) as { id?: unknown; result?: unknown };
      return message.id != null
        && requestIds.has(String(message.id))
        && typeof message.result === "object"
        && message.result !== null
        ? [message.result as Record<string, unknown>]
        : [];
    } catch {
      return [];
    }
  });
}

function traceEvents(fixture: DeterministicAcpAgentFixture): FixtureTraceEvent[] {
  try {
    return readFileSync(fixture.logPath, "utf8").split("\n").flatMap((line) => {
      if (!line.trim()) return [];
      try {
        return [JSON.parse(line) as FixtureTraceEvent];
      } catch {
        return [];
      }
    });
  } catch {
    return [];
  }
}

async function currentThreadId(page: Page, cwd: string): Promise<string> {
  const listed = await gatewayRequest(page, "thread/list", { cwd, archived: false, limit: 20 }) as {
    sessions?: Array<{ id?: string; cwd?: string; updatedAtMs?: number }>;
  };
  const current = (listed.sessions ?? [])
    .filter((session) => session.cwd === cwd && session.id)
    .sort((left, right) => Number(right.updatedAtMs ?? 0) - Number(left.updatedAtMs ?? 0))[0];
  if (!current?.id) throw new Error(`no current thread found for ${cwd}`);
  return current.id;
}

function controlFact(context: Record<string, unknown>, id: string): Record<string, unknown> | undefined {
  const controls = Array.isArray(context.controls) ? context.controls : [];
  return controls.find((value): value is Record<string, unknown> => (
    typeof value === "object" && value !== null && (value as Record<string, unknown>).id === id
  ));
}

async function waitForControlValue(
  page: Page,
  cwd: string,
  threadId: string,
  id: string,
  value: string
) {
  await expect.poll(async () => (
    controlFact(await threadContext(page, cwd, threadId), id)?.effectiveValue
  ), { timeout: 30_000 }).toBe(value);
}

async function expectInsideViewport(page: Page, locator: Locator) {
  const box = await locator.boundingBox();
  const viewport = page.viewportSize();
  expect(box).not.toBeNull();
  expect(viewport).not.toBeNull();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.y).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(viewport!.width + 1);
  expect(box!.y + box!.height).toBeLessThanOrEqual(viewport!.height + 1);
}

async function capture(page: Page, testInfo: TestInfo, name: string) {
  await page.screenshot({
    fullPage: true,
    path: path.join(screenshotDir, `${name}-${testInfo.project.name}.png`)
  });
}

async function gatewayRequest(page: Page, method: string, params: unknown): Promise<unknown> {
  return page.evaluate(async ({ method, params }) => await new Promise((resolve, reject) => {
    const url = new URL("/ws", window.location.origin);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    const socket = new WebSocket(url);
    const id = `agent-application-visual-${method}`;
    const timeout = window.setTimeout(() => {
      socket.close();
      reject(new Error(`${method} timed out`));
    }, 30_000);
    socket.addEventListener("open", () => {
      socket.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
    });
    socket.addEventListener("message", (event) => {
      const message = JSON.parse(String(event.data)) as { id?: string; result?: unknown; error?: unknown };
      if (message.id !== id) return;
      window.clearTimeout(timeout);
      socket.close();
      if (message.error) reject(new Error(JSON.stringify(message.error)));
      else resolve(message.result);
    });
    socket.addEventListener("error", () => {
      window.clearTimeout(timeout);
      reject(new Error(`${method} WebSocket failed`));
    });
  }), { method, params });
}
