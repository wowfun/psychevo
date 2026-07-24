import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page, type TestInfo } from "@playwright/test";
import { startPevoWeb } from "./harness";
import { liveContextFor, screenshotRoot, type XtaskLiveContext } from "./liveContext";
import {
  prepareDeterministicAcpAgent,
  startDeterministicNativeModel,
  startDeterministicTelegram,
  type DeterministicAcpAgentFixture
} from "./runtime-live.support";

test.describe("Native and ACP Agent application-path validation", () => {
  test("keeps first send live across a pending atomic draft open on the real Gateway", async ({ page }) => {
    const context = requiredContext("web-composer-draft-open-first-send");
    if (!context) return;
    test.setTimeout(context.timeoutMs);
    const nativeModel = await startDeterministicNativeModel();
    const cwd = context.cwd ?? path.join(context.artifactRoot, "cwd");
    mkdirSync(cwd, { recursive: true });
    const providerConfig = [
      "[provider.native-live]",
      `api = ${JSON.stringify(nativeModel.baseUrl)}`,
      "no_auth = true",
      "",
      "[provider.native-live.models.default]",
      ""
    ].join("\n");
    const server = await startPevoWeb({
      configAppend: providerConfig,
      cwd,
      dbPath: context.dbPath,
      home: context.home,
      live: false,
      model: "native-live/default",
      pevoBin: context.pevoBin
    });
    const websocketFrames = await captureWebSocketFramesWithDelayedRpcResult(
      page,
      "thread/draft/open",
      2
    );
    const prompt = "submit exactly once while the draft is opening";
    try {
      await page.goto(server.url, { waitUntil: "domcontentloaded" });
      const input = page.getByPlaceholder("Ask Psychevo...");
      await expect(input).toBeVisible();
      await expect.poll(() => rpcRequestsForMethod(websocketFrames, "thread/draft/open").length)
        .toBe(1);
      await expect.poll(() => rpcResultsForMethod(websocketFrames, "thread/draft/open").length)
        .toBe(1);

      await page.getByRole("button", { name: "New Session", exact: true }).click();
      await expect.poll(() => rpcRequestsForMethod(websocketFrames, "thread/draft/open").length)
        .toBe(2);
      expect(rpcResultsForMethod(websocketFrames, "thread/draft/open")).toHaveLength(1);

      await input.fill(prompt);
      const send = page.getByRole("button", { name: "Send message" });
      await expect(send).toBeEnabled();
      await send.click();
      await expect(input).toHaveValue(prompt);
      expect(rpcRequestsForMethod(websocketFrames, "turn/start")).toHaveLength(0);
      expect(rpcRequestsForMethod(websocketFrames, "thread/context/read")).toHaveLength(0);
      expect(rpcRequestsForMethod(websocketFrames, "settings/read")).toHaveLength(0);
      expect(rpcRequestsForMethod(websocketFrames, "completion/list")).toHaveLength(0);
      websocketFrames.releaseDelayedResponses();

      await expect.poll(() => rpcResultsForMethod(websocketFrames, "thread/draft/open").length)
        .toBe(2);
      await expect.poll(() => rpcRequestsForMethod(websocketFrames, "turn/start").length)
        .toBe(1);
      await expect(input).toHaveValue("");
      await expect(page.locator(".pevo-message.is-assistant").filter({
        hasText: nativeModel.expectedAnswer
      })).toHaveCount(1, { timeout: 60_000 });
      expect(rpcRequestsForMethod(websocketFrames, "turn/start")).toHaveLength(1);
      const boundContextReads = rpcRequestsForMethod(websocketFrames, "thread/context/read");
      expect(boundContextReads.length).toBeGreaterThan(0);
      expect(boundContextReads.every((request) => (
          typeof request.params === "object"
          && request.params !== null
          && typeof (request.params as { threadId?: unknown }).threadId === "string"
      ))).toBe(true);
    } finally {
      writeFileSync(
        path.join(context.artifactRoot, "draft-open-first-send-proof.json"),
        `${JSON.stringify({
          dbPath: server.dbPath,
          providerRequests: nativeModel.requests(),
          rpc: {
            draftOpenRequests: rpcRequestsForMethod(websocketFrames, "thread/draft/open"),
            draftOpenResults: rpcResultsForMethod(websocketFrames, "thread/draft/open"),
            turnStartRequests: rpcRequestsForMethod(websocketFrames, "turn/start"),
            turnStartResults: rpcResultsForMethod(websocketFrames, "turn/start"),
            contextReadRequests: rpcRequestsForMethod(websocketFrames, "thread/context/read")
          }
        }, null, 2)}\n`,
        "utf8"
      );
      await server.stop();
      await nativeModel.stop();
    }
  });

  test("runs Codex ACP and OpenCode ACP through one GUI control path @live", async ({ page }, testInfo) => {
    const context = requiredContext("agent-acp-gui-parity");
    if (!context) return;
    test.setTimeout(context.timeoutMs);
    const codex = prepareDeterministicAcpAgent("codex", context.artifactRoot);
    const opencode = prepareDeterministicAcpAgent("opencode", context.artifactRoot);
    const opaque = prepareDeterministicAcpAgent("opencode", context.artifactRoot, "stream", {
      runtimeRef: "zz-acp-fixture-4",
      profileLabel: "Boundary ACP",
      agentInfo: { name: "dev.psychevo.fixture.boundary", title: "Boundary" }
    });
    writeAgentDefinition(
      context.cwd,
      "reviewer",
      "Review the request and cite concrete evidence.",
      codex.runtimeRef
    );
    const server = await startPevoWeb({
      configAppend: `${codex.configAppend}\n${opencode.configAppend}\n${opaque.configAppend}`,
      cwd: context.cwd,
      dbPath: context.dbPath,
      home: context.home,
      live: false,
      pevoBin: context.pevoBin
    });
    const screenshots = screenshotRoot(context, "agent-acp");
    mkdirSync(screenshots, { recursive: true });
    const websocketFrames = captureWebSocketFrames(page);
    try {
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();

      const catalog = await readTargetCatalog(page, server.cwd);
      const nativeTarget = targetByIdentity(catalog, null, "native");
      const codexTarget = targetByIdentity(catalog, codex.runtimeRef, codex.runtimeRef);
      const reviewerTarget = targetByIdentity(catalog, "reviewer", codex.runtimeRef);
      const opencodeTarget = targetByIdentity(catalog, "opencode", "opencode");
      const opaqueTarget = targetByIdentity(catalog, opaque.runtimeRef, opaque.runtimeRef);
      const choices = await openTargetChoices(page);
      await expect(targetChoice(choices, nativeTarget)).toBeVisible();
      await expect(targetChoice(choices, codexTarget)).toContainText(/Codex/i);
      await expect(targetChoice(choices, opencodeTarget)).toContainText(/OpenCode/i);
      await expect(targetChoice(choices, opaqueTarget))
        .toHaveAttribute("aria-label", opaqueTarget.label);
      await expect(choices).not.toContainText(/Direct/i);
      await targetChoice(choices, reviewerTarget).click();
      await runPrompt(page, "codex stable ACP v1 GUI baseline", /Codex ACP response/i);
      const firstThreadId = await currentThreadId(page, server.cwd);
      const codexBefore = await readThreadContext(page, server.cwd, firstThreadId);
      expect(codexBefore.selectedTargetId).toBe(reviewerTarget.targetId);
      const codexAfterModel = await setThreadControl(
        page,
        server.cwd,
        firstThreadId,
        codexBefore,
        "model",
        "fixture/second"
      );
      await setThreadControl(
        page,
        server.cwd,
        firstThreadId,
        codexAfterModel,
        "reasoning",
        "high"
      );
      await page.reload();
      await reopenOnlyPersistedSession(page);
      await runPrompt(page, "codex stable ACP v1 GUI turn", /Codex ACP response.*model=fixture\/second.*effort=high/i);
      const reasoning = page.locator(".pevo-reasoning").last();
      await expect(reasoning).toBeVisible();
      await reasoning.getByRole("button").click();
      await expect(reasoning).toContainText(/stable v1 reasoning/i);
      const toolEvidence = page.locator(".pevo-evidence").filter({ hasText: "Inspect ACP fixture" });
      await expect(toolEvidence).toHaveCount(2);
      await expect(toolEvidence.last()).toBeVisible();
      await expect(page.getByRole("button", { name: "Agent target" })).toContainText("reviewer");
      await capture(page, testInfo, screenshots, "codex-acp-common-controls");

      const codexContext = await readThreadContext(page, server.cwd, firstThreadId);
      expectCapability(codexContext, "pack.codex", true);
      expectCapability(codexContext, "direct.steer", false);
      const switchedChoices = await openTargetChoices(page);
      await targetChoice(switchedChoices, opencodeTarget).click();
      await expect(page.getByRole("button", { name: "Agent target" }))
        .toHaveAttribute("title", `${opencodeTarget.agentLabel} · ${opencodeTarget.profileLabel}`);
      await runPrompt(page, "opencode stable ACP v1 GUI turn", /OpenCode ACP response/i);
      const secondThreadId = await currentThreadId(page, server.cwd);
      expect(secondThreadId).not.toBe(firstThreadId);
      const opencodeContext = await readThreadContext(page, server.cwd, secondThreadId);
      expectCapability(opencodeContext, "pack.opencode", true);
      expectCapability(opencodeContext, "opencode.sessionFork", false);
      expect(
        opencodeContext.capabilities.find((capability) => capability.id === "opencode.sessionFork")
          ?.unavailableReason
      ).toMatch(/ThreadApplication does not expose/i);
      expectCapability(opencodeContext, "direct.steer", false);
      expect(opencodeContext.selectedTargetId).toBe(opencodeTarget.targetId);
      await capture(page, testInfo, screenshots, "opencode-acp-common-controls");

      const opaqueChoices = await openTargetChoices(page);
      await targetChoice(opaqueChoices, opaqueTarget).click();
      await expect(page.getByRole("button", { name: "Agent target" }))
        .toHaveAttribute("title", `${opaqueTarget.agentLabel} · ${opaqueTarget.profileLabel}`);
      await runPrompt(page, "opaque fourth ACP GUI turn", /Boundary ACP response/i);
      const opaqueThreadId = await currentThreadId(page, server.cwd);
      expect(opaqueThreadId).not.toBe(firstThreadId);
      expect(opaqueThreadId).not.toBe(secondThreadId);
      const opaqueContext = await readThreadContext(page, server.cwd, opaqueThreadId);
      expect(opaqueContext.binding?.runtimeRef).toBe(opaque.runtimeRef);
      expect(opaqueContext.selectedTargetId).toBe(opaqueTarget.targetId);
      expect(opaqueContext.selectedTargetId).not.toContain(opaque.runtimeRef);
      expect(new Set([codexContext.selectedTargetId, opencodeContext.selectedTargetId, opaqueContext.selectedTargetId]).size).toBe(3);
      expectCapability(opaqueContext, "turn.start", true);
      expectCapability(opaqueContext, "history.read", true);
      expect(opaqueContext.capabilities.some((capability) => (
        capability.enabled && ["pack.codex", "pack.opencode"].includes(capability.id)
      ))).toBe(false);
      writeFileSync(
        path.join(context.artifactRoot, "agent-acp-opaque-target-proof.json"),
        JSON.stringify({
          runtimeRef: opaque.runtimeRef,
          agentInfo: opaque.agentInfo,
          targetId: opaqueContext.selectedTargetId,
          binding: opaqueContext.binding,
          enabledCapabilities: opaqueContext.capabilities
            .filter((capability) => capability.enabled)
            .map((capability) => capability.id)
            .sort()
        }, null, 2)
      );
      await capture(page, testInfo, screenshots, "opaque-fourth-acp-target");

      for (const fixture of [codex, opencode, opaque]) {
        await expect.poll(() => traceEvents(fixture).filter((event) => event.type === "initialize").length, {
          timeout: 10_000
        }).toBe(1);
        const initialize = traceEvents(fixture).find((event) => event.type === "initialize");
        expect(initialize?.requestedProtocolVersion).toBe(1);
        const accepted = traceEvents(fixture).filter((event) => event.type === "prompt_accepted");
        expect(accepted).toHaveLength(fixture.agent === "codex" ? 2 : 1);
      }
      expect(traceEvents(codex).find((event) => event.type === "initialize_result")?.agentInfo).toEqual({
        name: "@agentclientprotocol/codex-acp",
        title: "Codex",
        version: "1.1.2"
      });
      expect(traceEvents(opencode).find((event) => event.type === "initialize_result")?.agentInfo).toEqual({
        name: "OpenCode",
        title: "OpenCode",
        version: "1.17.18"
      });
      expect(traceEvents(opaque).find((event) => event.type === "initialize_result")?.agentInfo)
        .toEqual(opaque.agentInfo);
      expect(JSON.stringify(traceEvents(codex).find((event) => event.type === "prompt_accepted")?.prompt))
        .toContain("Review the request and cite concrete evidence.");
      expect(JSON.stringify(traceEvents(opencode).find((event) => event.type === "prompt_accepted")?.prompt))
        .not.toContain("Review the request and cite concrete evidence.");
      expect(JSON.stringify(traceEvents(opaque).find((event) => event.type === "prompt_accepted")?.prompt))
        .not.toContain("Review the request and cite concrete evidence.");
    } finally {
      writeFileSync(
        path.join(context.artifactRoot, "agent-acp-gui-parity-rpc.json"),
        JSON.stringify(rpcFrameProof(websocketFrames), null, 2)
      );
      await server.stop();
    }
  });

  test("proves Native GUI and Channel equivalent binding intent and history semantics @live", async ({ page }, testInfo) => {
    const context = requiredContext("agent-native-application-surface-parity");
    if (!context) return;
    test.setTimeout(context.timeoutMs);
    const nativeModel = await startDeterministicNativeModel();
    const telegram = await startDeterministicTelegram();
    const connectionId = "agent-native-application-parity";
    const intent = "prove the same deterministic Native intent";
    const providerConfig = [
      "[provider.native-live]",
      `api = ${JSON.stringify(nativeModel.baseUrl)}`,
      "no_auth = true",
      "",
      "[provider.native-live.models.default]",
      ""
    ].join("\n");
    const channelConfig = [
      "[[channels.connections]]",
      `id = ${JSON.stringify(connectionId)}`,
      'channel = "telegram"',
      'label = "Native application parity"',
      'transport = "polling"',
      "enabled = true",
      `cwd = ${JSON.stringify(context.cwd)}`,
      'runtime_ref = "native"',
      `credential_env = ${JSON.stringify(telegram.credentialEnv)}`,
      `base_url_env = ${JSON.stringify(telegram.baseUrlEnv)}`,
      'allow_users = ["42"]',
      "require_mention = false",
      ""
    ].join("\n");
    let server: Awaited<ReturnType<typeof startPevoWeb>> | null = null;
    const screenshots = screenshotRoot(context, "agent-native");
    mkdirSync(screenshots, { recursive: true });
    try {
      server = await startPevoWeb({
        channelRuntime: true,
        configAppend: `${providerConfig}\n${channelConfig}`,
        cwd: context.cwd,
        dbPath: context.dbPath,
        envFile: telegram.envFile,
        home: context.home,
        live: false,
        model: "native-live/default",
        pevoBin: context.pevoBin
      });
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
      const nativeTarget = await selectTarget(page, server.cwd, null, "native");
      await runPrompt(page, intent, new RegExp(escapeRegExp(nativeModel.expectedAnswer)));
      const guiThreadId = await currentThreadId(page, server.cwd);
      const guiContext = await readThreadContext(page, server.cwd, guiThreadId);
      const guiHistory = await readThreadHistory(page, server.cwd, guiThreadId);
      expect(guiContext.selectedTargetId).toBe(nativeTarget.targetId);
      expectCapability(guiContext, "turn.start", true);
      expectCapability(guiContext, "history.read", true);

      const beforeChannel = telegram.sent().length;
      telegram.push(intent);
      await waitForTelegramMessage(
        telegram,
        beforeChannel,
        (message) => message.text.includes(nativeModel.expectedAnswer)
      );
      const channelThreadId = latestChannelLane(context.dbPath)?.threadId;
      expect(channelThreadId).toBeTruthy();
      expect(channelThreadId).not.toBe(guiThreadId);
      const channelContext = await readThreadContext(page, server.cwd, channelThreadId as string);
      const channelHistory = await readThreadHistory(page, server.cwd, channelThreadId as string);

      expect(channelContext.selectedTargetId).toBe(nativeTarget.targetId);
      expect(bindingTargetProof(channelContext)).toEqual(bindingTargetProof(guiContext));
      expect(channelHistory.history).toEqual(guiHistory.history);
      expect(historyEntrySemantics(channelHistory)).toEqual(historyEntrySemantics(guiHistory));
      for (const history of [guiHistory, channelHistory]) {
        expect(JSON.stringify(history.entries)).toContain(intent);
        expect(JSON.stringify(history.entries)).toContain(nativeModel.expectedAnswer);
      }
      const intentRequests = nativeModel.requests().filter((request) => (
        JSON.stringify(request).includes(intent)
      ));
      expect(intentRequests.length).toBeGreaterThanOrEqual(2);
      writeFileSync(
        path.join(context.artifactRoot, "agent-native-application-surface-proof.json"),
        JSON.stringify({
          connectionId,
          intent,
          target: nativeTarget,
          gui: {
            threadId: guiThreadId,
            binding: bindingTargetProof(guiContext),
            history: guiHistory.history,
            entries: historyEntrySemantics(guiHistory)
          },
          channel: {
            threadId: channelThreadId,
            binding: bindingTargetProof(channelContext),
            history: channelHistory.history,
            entries: historyEntrySemantics(channelHistory)
          },
          matchingProviderRequests: intentRequests
        }, null, 2)
      );
      await capture(page, testInfo, screenshots, "native-gui-channel-application-parity");
    } finally {
      await server?.stop();
      await telegram.stop();
      await nativeModel.stop();
    }
  });

  test("disables an incompatible reviewed ACP capability pack with an explicit diagnostic @live", async ({ page }) => {
    const context = requiredContext("agent-acp-capability-pack-version");
    if (!context) return;
    test.setTimeout(context.timeoutMs);
    const fixture = prepareDeterministicAcpAgent(
      "codex",
      context.artifactRoot,
      "capability_pack",
      { agentVersion: "1.2.0" }
    );
    const server = await startPevoWeb({
      configAppend: fixture.configAppend,
      cwd: context.cwd,
      dbPath: context.dbPath,
      home: context.home,
      live: false,
      pevoBin: context.pevoBin
    });
    try {
      await page.goto(server.url);
      await selectTarget(page, server.cwd, fixture.runtimeRef, fixture.runtimeRef);
      await runPrompt(page, "probe incompatible Codex ACP capability pack", /Codex ACP response/i);
      const threadId = await currentThreadId(page, server.cwd);
      const threadContext = await readThreadContext(page, server.cwd, threadId);
      const pack = expectCapability(threadContext, "pack.codex", false);
      expect(pack.unavailableReason).toMatch(/codex.*does not support.*1\.2\.0/i);
      expect(traceEvents(fixture).find((event) => event.type === "initialize_result")?.agentInfo).toEqual({
        name: "@agentclientprotocol/codex-acp",
        title: "Codex",
        version: "1.2.0"
      });
    } finally {
      await server.stop();
    }
  });

  test("reuses one ACP process and restores agent-owned history without duplicate turns @live", async ({ page }, testInfo) => {
    const context = requiredContext("agent-acp-history-reconnect");
    if (!context) return;
    test.setTimeout(context.timeoutMs);
    const fixture = prepareDeterministicAcpAgent(
      "codex",
      context.artifactRoot,
      "history",
      { mcpServers: ["repo"] }
    );
    const server = await startPevoWeb({
      configAppend: fixture.configAppend,
      cwd: context.cwd,
      dbPath: context.dbPath,
      home: context.home,
      live: false,
      pevoBin: context.pevoBin
    });
    const screenshots = screenshotRoot(context, "agent-acp");
    mkdirSync(screenshots, { recursive: true });
    try {
      await page.goto(server.url);
      await selectTarget(page, server.cwd, fixture.runtimeRef, fixture.runtimeRef);
      await runPrompt(page, "first resident ACP turn", /Codex ACP response 1/i);
      await runPrompt(page, "second resident ACP turn", /Codex ACP response 2/i);

      const beforeReload = traceEvents(fixture);
      const newEvents = beforeReload.filter((event) => event.type === "session_new");
      expect(newEvents).toHaveLength(1);
      const newEvent = newEvents[0];
      expect(newEvent?.sessionId).toBeTruthy();
      expect(newEvent?.mcpServers).toEqual(fixture.expectedMcpServers);
      const accepted = beforeReload.filter((event) => event.type === "prompt_accepted");
      expect(new Set(accepted.map((event) => event.sessionId)).size).toBe(1);
      expect(new Set(accepted.map((event) => event.pid)).size).toBe(1);
      await expect.poll(
        () => traceEvents(fixture).filter((event) => event.type === "connection_closed_after_completed_turn").length,
        { timeout: 10_000 }
      ).toBe(1);

      await page.reload();
      await reopenOnlyPersistedSession(page);
      await expect(page.locator(".pevo-message.is-user").filter({ hasText: "first resident ACP turn" })).toHaveCount(1);
      await expect(page.locator(".pevo-message.is-user").filter({ hasText: "second resident ACP turn" })).toHaveCount(1);
      await expect(page.locator(".pevo-message.is-assistant").filter({ hasText: "Codex ACP response 1" })).toHaveCount(1);
      await expect(page.locator(".pevo-message.is-assistant").filter({ hasText: "Codex ACP response 2" })).toHaveCount(1);
      expect(traceEvents(fixture).filter((event) => event.type === "prompt_accepted")).toHaveLength(2);
      expect(traceEvents(fixture).filter((event) => event.type === "boot")).toHaveLength(1);
      await runPrompt(page, "third turn reconnects the resident ACP session", /Codex ACP response 3/i);
      await expect.poll(() => traceEvents(fixture).filter((event) => event.type === "boot").length, {
        timeout: 10_000
      }).toBeGreaterThanOrEqual(2);
      expect(traceEvents(fixture).filter((event) => event.type === "initialize").length).toBeGreaterThanOrEqual(2);
      const loadEvents = traceEvents(fixture).filter((event) => (
        event.type === "session_load" && event.sessionId === newEvent?.sessionId
      ));
      expect(loadEvents).toHaveLength(1);
      const loadEvent = loadEvents[0];
      expect(loadEvent?.mcpServers).toEqual(fixture.expectedMcpServers);
      writeFileSync(
        path.join(context.artifactRoot, "agent-acp-mcp-new-load-proof.json"),
        JSON.stringify({
          expected: fixture.expectedMcpServers,
          sessionId: newEvent?.sessionId,
          sessionNew: newEvent?.mcpServers,
          sessionLoad: loadEvent?.mcpServers
        }, null, 2)
      );
      await capture(page, testInfo, screenshots, "acp-history-reconnect");
    } finally {
      await server.stop();
    }
  });

  test("keeps process-ephemeral ACP history partial after restart and refuses fake recovery @live", async ({ page }, testInfo) => {
    const context = requiredContext("agent-acp-process-ephemeral-history");
    if (!context) return;
    test.setTimeout(context.timeoutMs);
    const fixture = prepareDeterministicAcpAgent("opencode", context.artifactRoot, "process_ephemeral", {
      runtimeRef: "ephemeral-acp",
      profileLabel: "Ephemeral ACP",
      agentInfo: { name: "dev.psychevo.fixture.ephemeral", title: "Ephemeral" }
    });
    const screenshots = screenshotRoot(context, "agent-acp");
    mkdirSync(screenshots, { recursive: true });
    let threadId = "";
    let beforeRestart: ThreadContextProof | null = null;
    let firstServer: Awaited<ReturnType<typeof startPevoWeb>> | null = null;
    try {
      firstServer = await startPevoWeb({
        configAppend: fixture.configAppend,
        cwd: context.cwd,
        dbPath: context.dbPath,
        home: context.home,
        live: false,
        pevoBin: context.pevoBin
      });
      await page.goto(firstServer.url);
      await selectTarget(page, firstServer.cwd, fixture.runtimeRef, fixture.runtimeRef);
      await runPrompt(page, "process-ephemeral first turn", /Ephemeral ACP response 1/i);
      threadId = await currentThreadId(page, firstServer.cwd);
      beforeRestart = await readThreadContext(page, firstServer.cwd, threadId);
      expect(beforeRestart.history).toMatchObject({ owner: "process" });
      expect(beforeRestart.history.hint).toMatch(/process-ephemeral|cannot be resumed/i);
      await capture(page, testInfo, screenshots, "process-ephemeral-before-restart");
    } finally {
      await firstServer?.stop();
    }

    let restartedServer: Awaited<ReturnType<typeof startPevoWeb>> | null = null;
    try {
      restartedServer = await startPevoWeb({
        configAppend: fixture.configAppend,
        cwd: context.cwd,
        dbPath: context.dbPath,
        home: context.home,
        live: false,
        pevoBin: context.pevoBin
      });
      await page.goto(restartedServer.url);
      await reopenOnlyPersistedSession(page);
      const afterRestart = await readThreadContext(page, restartedServer.cwd, threadId);
      const historyAfterRestart = await readThreadHistory(page, restartedServer.cwd, threadId);
      expect(afterRestart.history).toMatchObject({ owner: "process", fidelity: "partial" });
      expect(afterRestart.history.hint).toMatch(/resident Agent session snapshot|process-ephemeral|cannot be resumed/i);
      expect(historyAfterRestart.history).toEqual(afterRestart.history);

      const composer = page.getByPlaceholder("Ask Psychevo...");
      await composer.fill("must not pretend the ephemeral Agent session recovered");
      const blockedSend = page.getByRole("button", { name: "Send message" });
      await expect(blockedSend).toBeDisabled();
      await expect(blockedSend).toHaveAttribute(
        "title",
        /process-ephemeral.*cannot be resumed after process restart.*new Thread/i
      );
      await expect.poll(() => traceEvents(fixture).filter((event) => event.type === "boot").length, {
        timeout: 10_000
      }).toBe(1);
      expect(traceEvents(fixture).filter((event) => event.type === "session_load" || event.type === "session_resume"))
        .toHaveLength(0);
      expect(traceEvents(fixture).filter((event) => event.type === "prompt_accepted")).toHaveLength(1);
      const finalContext = await readThreadContext(page, restartedServer.cwd, threadId);
      expect(finalContext.history).toMatchObject({ owner: "process", fidelity: "partial" });
      writeFileSync(
        path.join(context.artifactRoot, "agent-acp-process-ephemeral-proof.json"),
        JSON.stringify({
          afterRestart: finalContext.history,
          beforeRestart: beforeRestart?.history,
          threadId,
          trace: traceEvents(fixture)
        }, null, 2)
      );
      await capture(page, testInfo, screenshots, "process-ephemeral-restart-unavailable");
    } finally {
      await restartedServer?.stop();
    }
  });

  test("applies Channel controls through the same ACP preference and delivery path @live", async ({ page }, testInfo) => {
    const context = requiredContext("agent-acp-channel-parity");
    if (!context) return;
    test.setTimeout(context.timeoutMs);
    const fixture = prepareDeterministicAcpAgent("codex", context.artifactRoot, "channel_controls");
    const telegram = await startDeterministicTelegram();
    writeAgentDefinition(
      context.cwd,
      "reviewer",
      "Review the requested change with concrete evidence.",
      fixture.runtimeRef
    );
    const connectionId = "agent-acp-codex-channel";
    const channelConfig = [
      "[[channels.connections]]",
      `id = ${JSON.stringify(connectionId)}`,
      'channel = "telegram"',
      'label = "Codex ACP Channel parity"',
      'transport = "polling"',
      "enabled = true",
      `cwd = ${JSON.stringify(context.cwd)}`,
      `runtime_ref = ${JSON.stringify(fixture.runtimeRef)}`,
      `credential_env = ${JSON.stringify(telegram.credentialEnv)}`,
      `base_url_env = ${JSON.stringify(telegram.baseUrlEnv)}`,
      'allow_users = ["42"]',
      "require_mention = false",
      ""
    ].join("\n");
    let server: Awaited<ReturnType<typeof startPevoWeb>> | null = null;
    const screenshots = screenshotRoot(context, "agent-acp");
    mkdirSync(screenshots, { recursive: true });
    try {
      server = await startPevoWeb({
        channelRuntime: true,
        configAppend: `${fixture.configAppend}\n${channelConfig}`,
        cwd: context.cwd,
        dbPath: context.dbPath,
        envFile: telegram.envFile,
        home: context.home,
        live: false,
        pevoBin: context.pevoBin
      });
      await page.goto(server.url);
      await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();

      telegram.push("/agent reviewer");
      await waitForTelegramMessage(
        telegram,
        0,
        (message) => message.text.toLowerCase().includes("top-level agent `reviewer`")
          && message.text.toLowerCase().includes(`runtime profile \`${fixture.runtimeRef.toLowerCase()}\``)
      );
      telegram.push("first bound ACP Channel turn");
      const firstOutbound = await telegram.waitForText("Codex ACP response 1");
      const firstFinal = firstOutbound.find((message) => message.text.includes("Codex ACP response 1"));
      expect(firstFinal).toBeTruthy();
      const firstLane = latestChannelLane(context.dbPath);
      expect(firstLane?.threadId).toBeTruthy();

      telegram.push("/model fixture/second");
      const controlOutbound = await telegram.waitForText("fixture/second");
      expect(controlOutbound.some((message) => /next turn|next message|saved|stored/i.test(message.text))).toBe(true);

      telegram.push("second bound ACP Channel turn");
      const secondOutbound = await telegram.waitForText("model=fixture/second");
      expect(secondOutbound.filter((message) => message.text.includes("Codex ACP response 2"))).toHaveLength(1);
      const secondLane = latestChannelLane(context.dbPath);
      expect(secondLane?.threadId).toBe(firstLane?.threadId);
      await expect.poll(
        () => traceEvents(fixture).filter((event) => event.type === "config_set" && event.value === "fixture/second").length,
        { timeout: 10_000 }
      ).toBeGreaterThan(0);

      await expect.poll(
        () => latestChannelOutbox(context.dbPath)?.status ?? null,
        { timeout: 10_000 }
      ).toBe("acknowledged");
      const outbox = latestChannelOutbox(context.dbPath);
      expect(outbox?.status).toBe("acknowledged");
      expect(outbox?.payloadText).toBeNull();
      expect(outbox?.payloadHash).toMatch(/^[0-9a-f]{64}$/i);

      const beforeAgentList = telegram.sent().length;
      telegram.push("/agent");
      await expect.poll(
        () => telegram.sent().slice(beforeAgentList).some((message) => /reviewer/i.test(message.text)),
        { timeout: 10_000 }
      ).toBe(true);

      const beforeAgentSelect = telegram.sent().length;
      telegram.push("/agent reviewer");
      await expect.poll(
        () => telegram.sent().slice(beforeAgentSelect).some((message) => (
          message.text.includes("Started a new channel thread ")
            && message.text.includes("top-level Agent `reviewer`")
            && message.text.includes(`Runtime Profile \`${fixture.runtimeRef}\``)
        )),
        { timeout: 10_000 }
      ).toBe(true);
      telegram.push("third turn after top-level Agent selection");
      const thirdOutbound = await telegram.waitForText("Codex ACP response 3");
      expect(thirdOutbound.filter((message) => message.text.includes("Codex ACP response 3"))).toHaveLength(1);
      const thirdLane = latestChannelLane(context.dbPath);
      expect(thirdLane?.threadId).not.toBe(firstLane?.threadId);
      await expect.poll(
        () => new Set(
          traceEvents(fixture)
            .filter((event) => event.type === "prompt_accepted")
            .map((event) => event.sessionId)
        ).size,
        { timeout: 10_000 }
      ).toBe(2);

      await page.getByRole("button", { name: "Settings", exact: true }).click();
      const settings = page.getByRole("region", { name: "Settings", exact: true });
      await settings.getByRole("button", { name: "Channels" }).click();
      await expect(settings.getByRole("region", { name: "Channels", exact: true })).toContainText("Codex ACP Channel parity");
      await capture(page, testInfo, screenshots, "acp-channel-parity");

      writeFileSync(path.join(context.artifactRoot, "agent-acp-channel-proof.json"), JSON.stringify({
        connectionId,
        firstThreadId: firstLane?.threadId,
        secondThreadId: secondLane?.threadId,
        thirdThreadId: thirdLane?.threadId,
        outbox,
        trace: traceEvents(fixture)
      }, null, 2));
    } finally {
      await server?.stop();
      await telegram.stop();
    }
  });

  test("routes ACP filesystem permissions once through Channel and keeps terminal explicitly unsupported @live", async () => {
    const context = requiredContext("agent-acp-client-callback-fidelity");
    if (!context) return;
    test.setTimeout(context.timeoutMs);
    const fixture = prepareDeterministicAcpAgent(
      "opencode",
      context.artifactRoot,
      "filesystem_permission",
      { clientCapabilities: ["fs.read", "fs.write"] }
    );
    const telegram = await startDeterministicTelegram();
    const connectionId = "agent-acp-client-callbacks";
    const channelConfig = [
      "[[channels.connections]]",
      `id = ${JSON.stringify(connectionId)}`,
      'channel = "telegram"',
      'label = "ACP client callbacks"',
      'transport = "polling"',
      "enabled = true",
      `cwd = ${JSON.stringify(context.cwd)}`,
      'runtime_ref = "opencode"',
      `credential_env = ${JSON.stringify(telegram.credentialEnv)}`,
      `base_url_env = ${JSON.stringify(telegram.baseUrlEnv)}`,
      'allow_users = ["42"]',
      "require_mention = false",
      ""
    ].join("\n");
    writeFileSync(path.join(context.cwd, "acp-live-seed.txt"), "first line\nsecond line\nthird line\n");
    let server: Awaited<ReturnType<typeof startPevoWeb>> | null = null;
    try {
      server = await startPevoWeb({
        channelRuntime: true,
        configAppend: `${fixture.configAppend}\n${channelConfig}`,
        cwd: context.cwd,
        dbPath: context.dbPath,
        envFile: telegram.envFile,
        home: context.home,
        live: false,
        pevoBin: context.pevoBin
      });

      telegram.push("/agent opencode");
      await waitForTelegramMessage(
        telegram,
        0,
        (message) => /top-level Agent `opencode`.*Runtime Profile `opencode`/i.test(message.text)
      );
      const beforeCallbackTurn = telegram.sent().length;
      telegram.push("exercise bounded ACP client callbacks");
      const writePrompt = await waitForTelegramMessage(
        telegram,
        beforeCallbackTurn,
        (message) => /Permission required for fs\/write_text_file/i.test(message.text)
      );
      const writeToken = interactionToken(writePrompt.text, "approve");
      const beforeWriteApproval = telegram.sent().length;
      telegram.push(`/approve ${writeToken}`);
      await waitForTelegramMessage(
        telegram,
        beforeWriteApproval,
        (message) => message.text === `Approved request ${writeToken}.`
      );

      const beforeWriteReplay = telegram.sent().length;
      telegram.push(`/approve ${writeToken}`);
      await waitForTelegramMessage(
        telegram,
        beforeWriteReplay,
        (message) => message.text === "No matching permission request token."
      );

      const explicitPrompt = await waitForTelegramMessage(
        telegram,
        beforeWriteApproval,
        (message) => /Permission required for Run deterministic ACP callback/i.test(message.text)
      );
      const explicitToken = interactionToken(explicitPrompt.text, "approve");
      expect(explicitToken).not.toBe(writeToken);
      const beforeExplicitApproval = telegram.sent().length;
      telegram.push(`/approve ${explicitToken}`);
      await waitForTelegramMessage(
        telegram,
        beforeExplicitApproval,
        (message) => message.text === `Approved request ${explicitToken}.`
      );
      await telegram.waitForText("callbacks=fs.read,fs.write,permission; terminal=unsupported");

      expect(readFileSync(path.join(context.cwd, "acp-live-written.txt"), "utf8"))
        .toBe("written through ACP fs/write_text_file");
      const initialize = traceEvents(fixture).find((event) => event.type === "initialize_result");
      expect(initialize?.clientCapabilities).toMatchObject({
        fs: { readTextFile: true, writeTextFile: true },
        terminal: false
      });
      expect(traceEvents(fixture).find((event) => event.type === "fs_read_result")?.result)
        .toEqual({ content: "second line" });
      expect(traceEvents(fixture).find((event) => event.type === "permission_result")?.result)
        .toEqual({ outcome: { outcome: "selected", optionId: "allow-once" } });
      expect(traceEvents(fixture).filter((event) => event.method?.startsWith("terminal/"))).toHaveLength(0);
      expect(traceEvents(fixture).filter((event) => event.type === "client_request").map((event) => event.method))
        .toEqual(["fs/read_text_file", "fs/write_text_file", "session/request_permission"]);
    } finally {
      await server?.stop();
      await telegram.stop();
    }
  });

  test("consumes Channel approve and answer tokens exactly once for ACP interactions @live", async () => {
    const context = requiredContext("agent-channel-interaction-once");
    if (!context) return;
    test.setTimeout(context.timeoutMs);
    const fixture = prepareDeterministicAcpAgent("codex", context.artifactRoot, "interaction_once");
    const telegram = await startDeterministicTelegram();
    const connectionId = "agent-channel-interaction-once";
    const channelConfig = [
      "[[channels.connections]]",
      `id = ${JSON.stringify(connectionId)}`,
      'channel = "telegram"',
      'label = "ACP interaction once-only"',
      'transport = "polling"',
      "enabled = true",
      `cwd = ${JSON.stringify(context.cwd)}`,
      `runtime_ref = ${JSON.stringify(fixture.runtimeRef)}`,
      `credential_env = ${JSON.stringify(telegram.credentialEnv)}`,
      `base_url_env = ${JSON.stringify(telegram.baseUrlEnv)}`,
      'allow_users = ["42"]',
      "require_mention = false",
      ""
    ].join("\n");
    let server: Awaited<ReturnType<typeof startPevoWeb>> | null = null;
    try {
      server = await startPevoWeb({
        channelRuntime: true,
        configAppend: `${fixture.configAppend}\n${channelConfig}`,
        cwd: context.cwd,
        dbPath: context.dbPath,
        envFile: telegram.envFile,
        home: context.home,
        live: false,
        pevoBin: context.pevoBin
      });
      telegram.push(`/agent ${fixture.runtimeRef}`);
      await waitForTelegramMessage(
        telegram,
        0,
        (message) => message.text.toLowerCase().includes(
          `top-level agent \`${fixture.runtimeRef.toLowerCase()}\``
        ) && message.text.toLowerCase().includes(
          `runtime profile \`${fixture.runtimeRef.toLowerCase()}\``
        )
      );
      const beforeTurn = telegram.sent().length;
      telegram.push("exercise once-only ACP interactions");

      const permissionPrompt = await waitForTelegramMessage(
        telegram,
        beforeTurn,
        (message) => /Permission required for Approve the once-only interaction/i.test(message.text)
      );
      const permissionToken = interactionToken(permissionPrompt.text, "approve");
      const beforeApprove = telegram.sent().length;
      telegram.push(`/approve ${permissionToken}`);
      await waitForTelegramMessage(
        telegram,
        beforeApprove,
        (message) => message.text === `Approved request ${permissionToken}.`
      );
      const beforeApproveReplay = telegram.sent().length;
      telegram.push(`/approve ${permissionToken}`);
      await waitForTelegramMessage(
        telegram,
        beforeApproveReplay,
        (message) => message.text === "No matching permission request token."
      );

      const askPrompt = await waitForTelegramMessage(
        telegram,
        beforeApprove,
        (message) => /Which workspace should the once-only interaction use/i.test(message.text)
      );
      const answerToken = interactionToken(askPrompt.text, "answer");
      const beforeAnswer = telegram.sent().length;
      telegram.push(`/answer ${answerToken} repo root`);
      await waitForTelegramMessage(
        telegram,
        beforeAnswer,
        (message) => message.text === `Answered request ${answerToken}.`
      );
      const beforeAnswerReplay = telegram.sent().length;
      telegram.push(`/answer ${answerToken} replayed answer`);
      await waitForTelegramMessage(
        telegram,
        beforeAnswerReplay,
        (message) => message.text === "No matching Ask request token."
      );
      await telegram.waitForText("interactions=permission,elicitation");

      expect(traceEvents(fixture).find((event) => event.type === "once_permission_result")?.result)
        .toEqual({ outcome: { outcome: "selected", optionId: "allow-once" } });
      expect(traceEvents(fixture).find((event) => event.type === "once_elicitation_result")?.result)
        .toEqual({ action: "accept", content: { workspace: "repo root" } });
      const initialize = traceEvents(fixture).find((event) => event.type === "initialize_result");
      expect(initialize?.clientCapabilities).toMatchObject({ elicitation: { form: {} } });
      expect(traceEvents(fixture).filter((event) => event.type === "client_request").map((event) => event.method))
        .toEqual(["session/request_permission", "elicitation/create"]);
    } finally {
      await server?.stop();
      await telegram.stop();
    }
  });

  test("queues an active-turn ACP model change for the next turn without mutating the current turn @live", async ({ page }, testInfo) => {
    const context = requiredContext("agent-acp-active-turn-next-control");
    if (!context) return;
    test.setTimeout(context.timeoutMs);
    const fixture = prepareDeterministicAcpAgent("codex", context.artifactRoot, "active_next_control");
    const server = await startPevoWeb({
      configAppend: fixture.configAppend,
      cwd: context.cwd,
      dbPath: context.dbPath,
      home: context.home,
      live: false,
      pevoBin: context.pevoBin
    });
    const screenshots = screenshotRoot(context, "agent-acp");
    mkdirSync(screenshots, { recursive: true });
    const websocketFrames = captureWebSocketFrames(page);
    try {
      await page.goto(server.url);
      await selectTarget(page, server.cwd, fixture.runtimeRef, fixture.runtimeRef);
      await runPrompt(page, "prime the bound session before the held turn", /Codex ACP response 1.*model=fixture\/default/i);
      const threadId = await currentThreadId(page, server.cwd);
      const before = await readThreadContext(page, server.cwd, threadId);
      expect(controlBySurfaceRole(before, "model")?.effectiveValue).toBe("fixture/default");

      await page.getByPlaceholder("Ask Psychevo...").fill("hold the active turn while changing the next model");
      await page.getByRole("button", { name: "Send message" }).click();
      await expect(page.getByLabel("Pending requests")).toContainText(
        "Approve the once-only interaction",
        { timeout: 60_000 }
      );
      await selectControlWhenAvailable(page, /Model/i, "fixture/second");
      await expect.poll(() => rpcResultsForMethod(websocketFrames, "thread/control/set")
        .some((result) => result.status === "stored"), { timeout: 10_000 }).toBe(true);
      const queuedContext = await readThreadContext(page, server.cwd, threadId);
      expect(controlBySurfaceRole(queuedContext, "model")?.effectiveValue).toBe("fixture/second");
      expect(traceEvents(fixture).filter((event) => (
        event.type === "config_set" && event.value === "fixture/second"
      ))).toHaveLength(0);
      await capture(page, testInfo, screenshots, "active-turn-next-model-queued");
      const permission = await waitForPendingInteraction(page, server.cwd, threadId, "permission");
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
      const afterQueuedControl = await readThreadContext(page, server.cwd, threadId);
      expect(afterQueuedControl.controlRevision).not.toBe(before.controlRevision);
      expect(controlBySurfaceRole(afterQueuedControl, "model")?.effectiveValue).toBe("fixture/second");

      await runPrompt(
        page,
        "use the queued model on the next turn",
        /Codex ACP response 3.*model=fixture\/second/i
      );
      const finalContext = await readThreadContext(page, server.cwd, threadId);
      expect(finalContext.controlRevision).not.toBe(afterQueuedControl.controlRevision);
      expect(controlBySurfaceRole(finalContext, "model")?.effectiveValue).toBe("fixture/second");
      const accepted = traceEvents(fixture).filter((event) => event.type === "prompt_accepted");
      expect(accepted.map((event) => (event.config as Record<string, unknown> | undefined)?.model))
        .toEqual(["fixture/default", "fixture/default", "fixture/second"]);
      const trace = traceEvents(fixture);
      const heldPromptIndex = trace.findIndex((event) => event.type === "prompt_accepted" && event.turn === 2);
      const configSetIndex = trace.findIndex((event) => event.type === "config_set" && event.value === "fixture/second");
      const nextPromptIndex = trace.findIndex((event) => event.type === "prompt_accepted" && event.turn === 3);
      expect(heldPromptIndex).toBeGreaterThanOrEqual(0);
      expect(configSetIndex).toBeGreaterThan(heldPromptIndex);
      expect(nextPromptIndex).toBeGreaterThan(configSetIndex);
      writeFileSync(path.join(context.artifactRoot, "agent-acp-active-turn-control-proof.json"), JSON.stringify({
        accepted: accepted.map((event) => ({ config: event.config, turn: event.turn })),
        afterQueuedControl,
        before,
        controlReceipts: rpcResultsForMethod(websocketFrames, "thread/control/set"),
        traceOrder: { configSetIndex, heldPromptIndex, nextPromptIndex }
      }, null, 2));
      await capture(page, testInfo, screenshots, "next-turn-model-observed");
    } finally {
      await server.stop();
    }
  });

  test("runs the granted ACP terminal lifecycle through Channel approval and typed callbacks @live", async () => {
    const context = requiredContext("agent-acp-terminal-callback-fidelity");
    if (!context) return;
    test.setTimeout(context.timeoutMs);
    const fixture = prepareDeterministicAcpAgent(
      "codex",
      context.artifactRoot,
      "terminal_lifecycle",
      { clientCapabilities: ["terminal"] }
    );
    const telegram = await startDeterministicTelegram();
    const connectionId = "agent-acp-terminal-callbacks";
    const channelConfig = [
      "[[channels.connections]]",
      `id = ${JSON.stringify(connectionId)}`,
      'channel = "telegram"',
      'label = "ACP terminal callbacks"',
      'transport = "polling"',
      "enabled = true",
      `cwd = ${JSON.stringify(context.cwd)}`,
      `runtime_ref = ${JSON.stringify(fixture.runtimeRef)}`,
      `credential_env = ${JSON.stringify(telegram.credentialEnv)}`,
      `base_url_env = ${JSON.stringify(telegram.baseUrlEnv)}`,
      'allow_users = ["42"]',
      "require_mention = false",
      ""
    ].join("\n");
    let server: Awaited<ReturnType<typeof startPevoWeb>> | null = null;
    try {
      server = await startPevoWeb({
        channelRuntime: true,
        configAppend: `${fixture.configAppend}\n${channelConfig}`,
        cwd: context.cwd,
        dbPath: context.dbPath,
        envFile: telegram.envFile,
        home: context.home,
        live: false,
        pevoBin: context.pevoBin
      });
      telegram.push(`/agent ${fixture.runtimeRef}`);
      await waitForTelegramMessage(
        telegram,
        0,
        (message) => message.text.toLowerCase().includes(
          `top-level agent \`${fixture.runtimeRef.toLowerCase()}\``
        ) && message.text.toLowerCase().includes(
          `runtime profile \`${fixture.runtimeRef.toLowerCase()}\``
        )
      );
      const beforeTurn = telegram.sent().length;
      telegram.push("exercise the granted ACP terminal lifecycle");

      const firstPrompt = await waitForTelegramMessage(
        telegram,
        beforeTurn,
        (message) => /Permission required.*terminal|Permission required.*node/i.test(message.text)
      );
      const firstToken = interactionToken(firstPrompt.text, "approve");
      const beforeFirstApproval = telegram.sent().length;
      telegram.push(`/approve ${firstToken}`);
      await waitForTelegramMessage(
        telegram,
        beforeFirstApproval,
        (message) => message.text === `Approved request ${firstToken}.`
      );

      const secondPrompt = await waitForTelegramMessage(
        telegram,
        beforeFirstApproval,
        (message) => /Permission required.*terminal|Permission required.*node/i.test(message.text)
          && !message.text.includes(firstToken)
      );
      const secondToken = interactionToken(secondPrompt.text, "approve");
      expect(secondToken).not.toBe(firstToken);
      const beforeSecondApproval = telegram.sent().length;
      telegram.push(`/approve ${secondToken}`);
      await waitForTelegramMessage(
        telegram,
        beforeSecondApproval,
        (message) => message.text === `Approved request ${secondToken}.`
      );
      await telegram.waitForText("callbacks=terminal.create,output,wait,kill,release");

      const initialize = traceEvents(fixture).find((event) => event.type === "initialize_result");
      expect(initialize?.clientCapabilities).toMatchObject({ terminal: true });
      expect(traceEvents(fixture).find((event) => event.type === "terminal_output_completed")?.result)
        .toMatchObject({ output: expect.stringContaining("terminal-live-ok:yes"), truncated: false });
      expect(traceEvents(fixture).find((event) => event.type === "terminal_wait_completed")?.result)
        .toMatchObject({ exitCode: 0 });
      expect(traceEvents(fixture).find((event) => event.type === "terminal_output_before_kill")?.result)
        .toMatchObject({ output: expect.stringContaining("terminal-kill-ready"), truncated: false });
      const killedWait = traceEvents(fixture).find((event) => event.type === "terminal_wait_killed")?.result as {
        exitCode?: number | null;
        signal?: string | null;
      } | undefined;
      expect(killedWait).toBeTruthy();
      expect(killedWait?.exitCode).not.toBe(0);
      expect(traceEvents(fixture).filter((event) => event.type === "client_request").map((event) => event.method))
        .toEqual([
          "terminal/create",
          "terminal/output",
          "terminal/wait_for_exit",
          "terminal/output",
          "terminal/release",
          "terminal/create",
          "terminal/output",
          "terminal/kill",
          "terminal/wait_for_exit",
          "terminal/release"
        ]);
    } finally {
      await server?.stop();
      await telegram.stop();
    }
  });

  test("proves GUI and Channel equivalent binding control delivery and history semantics @live", async ({ page }, testInfo) => {
    const context = requiredContext("agent-application-surface-parity");
    if (!context) return;
    test.setTimeout(context.timeoutMs);
    const fixture = prepareDeterministicAcpAgent("codex", context.artifactRoot, "channel_controls");
    const telegram = await startDeterministicTelegram();
    writeAgentDefinition(
      context.cwd,
      "reviewer",
      "Review through the common application contract.",
      fixture.runtimeRef
    );
    const connectionId = "agent-surface-parity";
    const channelConfig = [
      "[[channels.connections]]",
      `id = ${JSON.stringify(connectionId)}`,
      'channel = "telegram"',
      'label = "Agent surface parity"',
      'transport = "polling"',
      "enabled = true",
      `cwd = ${JSON.stringify(context.cwd)}`,
      `runtime_ref = ${JSON.stringify(fixture.runtimeRef)}`,
      `credential_env = ${JSON.stringify(telegram.credentialEnv)}`,
      `base_url_env = ${JSON.stringify(telegram.baseUrlEnv)}`,
      'allow_users = ["42"]',
      "require_mention = false",
      ""
    ].join("\n");
    const server = await startPevoWeb({
      channelRuntime: true,
      configAppend: `${fixture.configAppend}\n${channelConfig}`,
      cwd: context.cwd,
      dbPath: context.dbPath,
      envFile: telegram.envFile,
      home: context.home,
      live: false,
      pevoBin: context.pevoBin
    });
    const screenshots = screenshotRoot(context, "agent-surface-parity");
    mkdirSync(screenshots, { recursive: true });
    const websocketFrames = captureWebSocketFrames(page);
    try {
      await page.goto(server.url);
      await selectTarget(page, server.cwd, "reviewer", fixture.runtimeRef);
      await runPrompt(page, "GUI parity baseline", /Codex ACP response/i);
      const guiThreadId = await currentThreadId(page, server.cwd);
      const guiBefore = await readThreadContext(page, server.cwd, guiThreadId);
      const guiAfterModel = await setThreadControl(
        page,
        server.cwd,
        guiThreadId,
        guiBefore,
        "model",
        "fixture/second"
      );
      const guiAfter = await setThreadControl(
        page,
        server.cwd,
        guiThreadId,
        guiAfterModel,
        "reasoning",
        "high"
      );
      await page.reload();
      await reopenOnlyPersistedSession(page);
      const modelControl = page.getByRole("button", { name: "Model", exact: true }).first();
      await expect(modelControl).toHaveAttribute("title", "fixture/second / High");
      await expect(modelControl).toContainText(/second High/i);
      const guiReloadContext = await readThreadContext(page, server.cwd, guiThreadId);
      writeFileSync(
        path.join(context.artifactRoot, "agent-application-surface-parity-preflight.json"),
        JSON.stringify({
          guiBefore,
          guiAfter,
          guiReloadContext,
          rpc: rpcFrameProof(websocketFrames)
        }, null, 2)
      );
      await runPrompt(
        page,
        "GUI parity controlled turn",
        /Codex ACP response.*model=fixture\/second.*effort=high/i
      );
      const guiStable = await readThreadContext(page, server.cwd, guiThreadId);
      const guiHistory = await readThreadHistory(page, server.cwd, guiThreadId);
      const guiDelivery = latestTurnDelivery(context.dbPath, guiThreadId);

      const beforeAgent = telegram.sent().length;
      telegram.push("/agent reviewer");
      await waitForTelegramMessage(
        telegram,
        beforeAgent,
        (message) => message.text.toLowerCase().includes("top-level agent `reviewer`")
          && message.text.toLowerCase().includes(`runtime profile \`${fixture.runtimeRef.toLowerCase()}\``)
      );
      telegram.push("Channel parity baseline");
      await waitForTelegramMessage(
        telegram,
        beforeAgent,
        (message) => /Codex ACP response/i.test(message.text)
      );
      const channelThreadId = latestChannelLane(context.dbPath)?.threadId;
      expect(channelThreadId).toBeTruthy();
      expect(channelThreadId).not.toBe(guiThreadId);
      const channelBefore = await readThreadContext(page, server.cwd, channelThreadId as string);

      const beforeModel = telegram.sent().length;
      telegram.push("/model fixture/second");
      await waitForTelegramMessage(
        telegram,
        beforeModel,
        (message) => /Model.*fixture\/second/i.test(message.text)
      );
      const beforeVariant = telegram.sent().length;
      telegram.push("/variant high");
      await waitForTelegramMessage(
        telegram,
        beforeVariant,
        (message) => /Reasoning effort.*high/i.test(message.text)
      );
      const channelAfter = await readThreadContext(page, server.cwd, channelThreadId as string);
      const beforeControlledTurn = telegram.sent().length;
      telegram.push("Channel parity controlled turn");
      await waitForTelegramMessage(
        telegram,
        beforeControlledTurn,
        (message) => /Codex ACP response.*model=fixture\/second.*effort=high/i.test(message.text)
      );
      const channelStable = await readThreadContext(page, server.cwd, channelThreadId as string);
      const channelHistory = await readThreadHistory(page, server.cwd, channelThreadId as string);
      const channelDelivery = latestTurnDelivery(context.dbPath, channelThreadId as string);

      expect(bindingTargetProof(channelAfter)).toEqual(bindingTargetProof(guiAfter));
      expect(effectiveControls(channelAfter)).toEqual(effectiveControls(guiAfter));
      expect(revisionSemantics(channelBefore, channelAfter, channelStable))
        .toEqual(revisionSemantics(guiBefore, guiAfter, guiStable));
      expect(channelDelivery).toEqual(guiDelivery);
      expect(channelDelivery).toMatchObject({ status: "terminal", inputJson: null });
      expect(channelHistory.history).toEqual(guiHistory.history);
      expect(historyEntrySemantics(channelHistory)).toEqual(historyEntrySemantics(guiHistory));
      expect(channelAfter.history).toEqual(guiAfter.history);

      const proof = {
        gui: {
          threadId: guiThreadId,
          binding: bindingTargetProof(guiAfter),
          controlRevision: guiAfter.controlRevision,
          revisionSemantics: revisionSemantics(guiBefore, guiAfter, guiStable),
          delivery: guiDelivery,
          history: guiHistory.history,
          entrySemantics: historyEntrySemantics(guiHistory)
        },
        channel: {
          threadId: channelThreadId,
          binding: bindingTargetProof(channelAfter),
          controlRevision: channelAfter.controlRevision,
          revisionSemantics: revisionSemantics(channelBefore, channelAfter, channelStable),
          delivery: channelDelivery,
          history: channelHistory.history,
          entrySemantics: historyEntrySemantics(channelHistory)
        },
        rpc: rpcFrameProof(websocketFrames)
      };
      writeFileSync(
        path.join(context.artifactRoot, "agent-application-surface-parity-proof.json"),
        JSON.stringify(proof, null, 2)
      );
      await capture(page, testInfo, screenshots, "gui-channel-application-parity");
    } finally {
      await server.stop();
      await telegram.stop();
    }
  });

  test("does not retry an ACP prompt after unknown delivery and reconciles from load @live", async ({ page }, testInfo) => {
    const context = requiredContext("agent-acp-unknown-delivery");
    if (!context) return;
    test.setTimeout(context.timeoutMs);
    const fixture = prepareDeterministicAcpAgent("opencode", context.artifactRoot, "unknown_delivery");
    const server = await startPevoWeb({
      configAppend: fixture.configAppend,
      cwd: context.cwd,
      dbPath: context.dbPath,
      home: context.home,
      live: false,
      pevoBin: context.pevoBin
    });
    const screenshots = screenshotRoot(context, "agent-acp");
    mkdirSync(screenshots, { recursive: true });
    try {
      await page.goto(server.url);
      await selectTarget(page, server.cwd, "opencode", "opencode");
      const prompt = "accept once then lose the ACP response";
      await page.getByPlaceholder("Ask Psychevo...").fill(prompt);
      await page.getByRole("button", { name: "Send message" }).click();
      await expect.poll(() => traceEvents(fixture).filter((event) => event.type === "prompt_accepted").length, {
        timeout: 30_000
      }).toBe(1);
      await expect.poll(() => traceEvents(fixture).filter((event) => event.type === "connection_lost_after_acceptance").length, {
        timeout: 10_000
      }).toBe(1);
      await expect.poll(() => latestTurnDelivery(context.dbPath)?.status, { timeout: 10_000 }).toBe("unknown");
      expect(latestTurnDelivery(context.dbPath)?.inputJson).toContain(prompt);
      await page.waitForTimeout(500);
      expect(traceEvents(fixture).filter((event) => event.type === "prompt_accepted")).toHaveLength(1);
      await expect(page.locator(".pevo-message.is-user").filter({ hasText: prompt })).toHaveCount(1);

      await page.reload();
      await reopenOnlyPersistedSession(page);
      await runPrompt(page, "continue after reconciling unknown delivery", /OpenCode ACP response 2/i);
      await expect(page.locator(".pevo-message.is-assistant").filter({ hasText: "OpenCode ACP response 1" })).toHaveCount(1, {
        timeout: 30_000
      });
      expect(traceEvents(fixture).filter((event) => event.type === "prompt_accepted")).toHaveLength(2);
      expect(traceEvents(fixture).filter((event) => (
        event.type === "prompt_accepted"
        && (event.prompt as Array<{ text?: string }> | undefined)?.some((part) => part.text === prompt)
      ))).toHaveLength(1);
      expect(traceEvents(fixture).some((event) => event.type === "session_load")).toBe(true);
      await expect.poll(() => latestTurnDelivery(context.dbPath)?.status, { timeout: 10_000 }).toBe("terminal");
      expect(latestTurnDelivery(context.dbPath)?.inputJson).toBeNull();
      await capture(page, testInfo, screenshots, "acp-unknown-delivery-reconciled");
    } finally {
      await server.stop();
    }
  });

  test("launches the pinned managed Codex ACP adapter from an offline absolute path @live", async ({ page }, testInfo) => {
    const context = requiredContext("agent-managed-codex-offline");
    if (!context) return;
    test.setTimeout(context.timeoutMs);
    const fixture = prepareDeterministicAcpAgent("codex", context.artifactRoot, "managed", { home: context.home });
    let server: Awaited<ReturnType<typeof startPevoWeb>> | null = null;
    const screenshots = screenshotRoot(context, "agent-acp");
    mkdirSync(screenshots, { recursive: true });
    try {
      server = await startPevoWeb({
        cwd: context.cwd,
        dbPath: context.dbPath,
        home: context.home,
        live: false,
        pevoBin: context.pevoBin,
        processEnv: fixture.installEnv ?? undefined
      });
      await page.goto(server.url);
      await gatewayRequest(page, "backend/list", { scope: webScope(server.cwd) });
      const missingTarget = targetByIdentity(await readTargetCatalog(page, server.cwd), "codex", "codex");
      expect(missingTarget.ready).toBe(false);
      const installed = await gatewayRequest(page, "backend/install", {
        id: "codex",
        scope: webScope(server.cwd)
      }) as { path: string; status: string };
      expect(installed).toMatchObject({ path: fixture.managedRootPath, status: "ready" });
      expect(fixture.managedSealPath && existsSync(fixture.managedSealPath)).toBe(true);
      const seal = JSON.parse(readFileSync(fixture.managedSealPath as string, "utf8")) as Record<string, unknown>;
      expect(seal).toMatchObject({
        schemaVersion: 1,
        treeSha256: expect.stringMatching(/^[0-9a-f]{64}$/)
      });
      const npmProof = JSON.parse(readFileSync(fixture.fakeNpmLogPath as string, "utf8")) as Record<string, unknown>;
      expect(npmProof).toMatchObject({
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
      const readyTarget = targetByIdentity(await readTargetCatalog(page, server.cwd), "codex", "codex");
      expect(readyTarget).toMatchObject({ ready: true, targetId: missingTarget.targetId });
      await selectTarget(page, server.cwd, "codex", "codex");
      await runPrompt(page, "managed Codex ACP must stay offline", /Codex ACP response/i);
      const threadContext = await readThreadContext(page, server.cwd, await currentThreadId(page, server.cwd));
      expect(threadContext.selectedTargetId).toBe(missingTarget.targetId);
      const boot = traceEvents(fixture).find((event) => event.type === "boot");
      expect(boot).toBeTruthy();
      expect(fixture.managedBinPath).toContain("runtime-adapters/codex-acp/1.1.2/node_modules/.bin");
      writeFileSync(path.join(context.artifactRoot, "agent-managed-codex-install-proof.json"), JSON.stringify({
        backend: listed.backends.find((backend) => backend.id === "codex"),
        boundTargetId: threadContext.selectedTargetId,
        install: installed,
        installerRemoved: !existsSync(fixture.fakeNpmPath as string),
        missingTarget,
        npm: npmProof,
        readyTarget,
        seal
      }, null, 2));
      await capture(page, testInfo, screenshots, "managed-codex-acp-offline");
    } finally {
      await server?.stop();
    }
  });
});

async function openTargetChoices(page: Page): Promise<Locator> {
  const selector = page.getByRole("button", { name: "Agent target", exact: true });
  await expect(selector).toBeVisible({ timeout: 30_000 });
  await selector.click();
  const dialog = page.getByRole("dialog", { name: "Agent target" });
  await expect(dialog).toBeVisible();
  return dialog.getByRole("radiogroup", { name: "Agent target" });
}

function targetChoice(choices: Locator, target: RunnableTargetProof): Locator {
  return choices.getByRole("radio", {
    name: new RegExp(`^(?:Start a new thread with )?${escapeRegExp(target.label)}$`)
  });
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

async function selectTarget(
  page: Page,
  cwd: string,
  agentRef: string | null,
  runtimeProfileRef: string
) {
  const target = targetByIdentity(await readTargetCatalog(page, cwd), agentRef, runtimeProfileRef);
  const choices = await openTargetChoices(page);
  const choice = targetChoice(choices, target);
  await expect(choice).toBeEnabled();
  await choice.click();
  return target;
}

async function selectControlWhenAvailable(page: Page, label: RegExp, value: string) {
  const control = page.getByRole("button", { name: label }).first();
  await expect(control).toBeVisible({ timeout: 30_000 });
  await control.click();
  const popover = page.getByRole("dialog", { name: /Model and reasoning/i });
  await expect(popover).toBeVisible();
  await popover.locator(`[data-model-value=${JSON.stringify(value)}]`).click();
  await page.keyboard.press("Escape");
}

async function runPrompt(page: Page, prompt: string, expected: RegExp) {
  const composer = page.getByPlaceholder("Ask Psychevo...");
  await composer.fill(prompt);
  const send = page.getByRole("button", { name: "Send message" });
  await expect(send).toBeEnabled({ timeout: 30_000 });
  await send.click();
  await expect(page.locator(".pevo-message.is-user").filter({ hasText: prompt })).toHaveCount(1, {
    timeout: 60_000
  });
  await expect(page.locator(".pevo-message.is-assistant").filter({ hasText: expected })).toHaveCount(1, {
    timeout: 60_000
  });
  await expect(page.locator(".pevo-composer").first()).not.toHaveClass(/is-running/, { timeout: 30_000 });
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

type ThreadCapabilityProof = {
  id: string;
  enabled: boolean;
  stability?: string;
  unavailableReason?: string | null;
};

type RunnableTargetProof = {
  targetId: string;
  agentRef: string | null;
  runtimeProfileRef: string;
  agentLabel: string;
  profileLabel: string;
  label: string;
  ready: boolean;
  unavailableReason: string | null;
};

type TargetCatalogProof = {
  compatibleTargets: RunnableTargetProof[];
};

type ThreadContextProof = {
  targetId: string;
  runtimeProfileRef: string;
  binding?: {
    threadId: string;
    agentRef?: string | null;
    agentFingerprint: string;
    runtimeRef: string;
    backendKind: string;
    nativeKind?: string | null;
    profileFingerprint: string;
    bindingRevision: number;
  } | null;
  controls: Array<{
    id: string;
    surfaceRole: string;
    effectiveValue?: unknown;
    effectiveSource: string;
    capabilityRevision: string;
  }>;
  capabilities: ThreadCapabilityProof[];
  history: { owner: string; fidelity: string; hint?: string | null };
  pendingInteractions: Array<{
    actionId: string;
    kind: string;
  }>;
  contextRevision: string;
  controlRevision: string;
};

type ThreadHistoryProof = {
  threadId: string;
  history: { owner: string; fidelity: string; cursor?: string | null; hint?: string | null };
  entries: Array<{
    role?: string;
    kind?: string;
    blocks?: Array<{ kind?: string }>;
  }>;
  nextCursor?: string | null;
};

async function readThreadContext(page: Page, cwd: string, threadId: string): Promise<ThreadContextProof> {
  return await gatewayRequest(page, "thread/context/read", {
    scope: webScope(cwd),
    threadId,
    target: null
  }) as ThreadContextProof;
}

async function readTargetCatalog(page: Page, cwd: string): Promise<TargetCatalogProof> {
  return await gatewayRequest(page, "thread/context/read", {
    scope: webScope(cwd),
    threadId: null,
    target: null
  }) as TargetCatalogProof;
}

function targetByIdentity(
  catalog: TargetCatalogProof,
  agentRef: string | null,
  runtimeProfileRef: string
): RunnableTargetProof {
  const target = catalog.compatibleTargets.find((candidate) => (
    candidate.agentRef === agentRef && candidate.runtimeProfileRef === runtimeProfileRef
  ));
  if (!target) {
    throw new Error(`missing Agent target for ${agentRef ?? "<default>"} · ${runtimeProfileRef}`);
  }
  return target;
}

function controlBySurfaceRole(context: ThreadContextProof, surfaceRole: string) {
  return context.controls.find((control) => control.surfaceRole === surfaceRole);
}

async function waitForPendingInteraction(
  page: Page,
  _cwd: string,
  threadId: string,
  kind: "permission" | "clarify"
): Promise<ThreadContextProof["pendingInteractions"][number]> {
  let interaction: ThreadContextProof["pendingInteractions"][number] | undefined;
  await expect.poll(async () => {
    const snapshot = await gatewayRequest(page, "thread/read", { threadId }) as {
      pendingActions: ThreadContextProof["pendingInteractions"];
    };
    interaction = snapshot.pendingActions
      .find((candidate) => candidate.kind === kind);
    return interaction?.actionId ?? null;
  }, { timeout: 30_000 }).not.toBeNull();
  return interaction as ThreadContextProof["pendingInteractions"][number];
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

async function readThreadHistory(page: Page, cwd: string, threadId: string): Promise<ThreadHistoryProof> {
  return await gatewayRequest(page, "thread/history/read", {
    scope: webScope(cwd),
    threadId,
    limit: 100
  }) as ThreadHistoryProof;
}

async function setThreadControl(
  page: Page,
  cwd: string,
  threadId: string,
  context: ThreadContextProof,
  surfaceRole: "model" | "reasoning",
  value: string
): Promise<ThreadContextProof> {
  const control = context.controls.find((candidate) => candidate.surfaceRole === surfaceRole);
  if (!control || !context.binding) {
    throw new Error(`bound Thread control is missing for ${surfaceRole}`);
  }
  const receipt = await gatewayRequest(page, "thread/control/set", {
    threadId,
    targetId: context.selectedTargetId,
    controlId: control.id,
    value,
    expectedCapabilityRevision: control.capabilityRevision,
    expectedBindingRevision: context.binding.bindingRevision,
    expectedContextRevision: context.contextRevision,
    expectedControlRevision: context.controlRevision,
    scope: webScope(cwd)
  }) as {
    status: string;
    controlRevision: string;
  };
  expect(receipt.status).toMatch(/applied|observed/i);
  expect(receipt.controlRevision).not.toBe(context.controlRevision);
  return await readThreadContext(page, cwd, threadId);
}

function bindingTargetProof(context: ThreadContextProof) {
  const binding = context.binding;
  expect(binding, "bound ThreadContext").toBeTruthy();
  expect(context.selectedTargetId, "bound selectedTargetId").toBeTruthy();
  return {
    targetId: context.selectedTargetId,
    agentRef: binding?.agentRef ?? null,
    agentFingerprint: binding?.agentFingerprint,
    runtimeRef: binding?.runtimeRef,
    backendKind: binding?.backendKind,
    nativeKind: binding?.nativeKind ?? null,
    profileFingerprint: binding?.profileFingerprint
  };
}

function effectiveControls(context: ThreadContextProof): Record<string, unknown> {
  return Object.fromEntries(
    context.controls
      .map((control) => [control.id, control.effectiveValue] as const)
      .sort(([left], [right]) => left.localeCompare(right))
  );
}

function revisionSemantics(
  before: ThreadContextProof,
  after: ThreadContextProof,
  stable: ThreadContextProof
) {
  expect(before.controlRevision).toMatch(/^[0-9a-f]{16}$/i);
  expect(after.controlRevision).toMatch(/^[0-9a-f]{16}$/i);
  return {
    changedAfterControls: before.controlRevision !== after.controlRevision,
    stableAfterObservation: after.controlRevision === stable.controlRevision,
    revisionLength: after.controlRevision.length
  };
}

function historyEntrySemantics(history: ThreadHistoryProof) {
  return history.entries.map((entry) => ({
    role: entry.role ?? null,
    kind: entry.kind ?? null,
    blockKinds: (entry.blocks ?? []).map((block) => block.kind ?? null)
  }));
}

function expectCapability(
  context: ThreadContextProof,
  id: string,
  enabled: boolean
): ThreadCapabilityProof {
  const capability = context.capabilities.find((candidate) => candidate.id === id);
  expect(capability, `missing Thread capability ${id}`).toBeTruthy();
  expect(capability?.enabled, `Thread capability ${id}`).toBe(enabled);
  return capability as ThreadCapabilityProof;
}

function webScope(cwd: string) {
  return {
    cwd,
    source: {
      kind: "web",
      rawId: null,
      lifetime: "persistent",
      rawIdentity: null,
      visibleName: null
    }
  };
}

async function reopenOnlyPersistedSession(page: Page) {
  const session = page.getByRole("region", { name: "Sessions" })
    .locator(".pevo-sessionRow:not(.is-draft) .pevo-sessionMain");
  await expect(session).toHaveCount(1, { timeout: 30_000 });
  await session.click();
}

function requiredContext(checkId: string): XtaskLiveContext | null {
  const context = liveContextFor(checkId);
  if (!context) {
    test.skip(true, `run ${checkId} through cargo xtask live`);
    return null;
  }
  if (context.provider !== "deterministic-fake") {
    throw new Error(`${checkId} requires the deterministic ACP Agent context`);
  }
  return context;
}

type FixtureTraceEvent = {
  type?: string;
  pid?: number;
  method?: string;
  requestedProtocolVersion?: number;
  sessionId?: string;
  value?: unknown;
  [key: string]: unknown;
};

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

async function waitForTelegramMessage(
  telegram: Awaited<ReturnType<typeof startDeterministicTelegram>>,
  fromIndex: number,
  predicate: (message: { chatId: string; text: string }) => boolean,
  timeoutMs = 60_000
): Promise<{ chatId: string; text: string }> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const match = telegram.sent().slice(fromIndex).find(predicate);
    if (match) return match;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error(`timed out waiting for Telegram message\n${JSON.stringify(telegram.sent(), null, 2)}`);
}

function interactionToken(text: string, command: "approve" | "answer"): string {
  const token = text
    .split(`/${command} `)
    .at(1)
    ?.split(/\s+/)
    .at(0)
    ?.replace(/[.,;:!?]+$/, "");
  if (!token?.startsWith("ia_")) {
    throw new Error(`missing opaque /${command} interaction token in: ${text}`);
  }
  return token;
}

type ChannelLaneProof = { threadId: string | null };
type ChannelOutboxProof = { status: string; payloadText: string | null; payloadHash: string };
type TurnDeliveryProof = { status: string; inputJson: string | null };

function latestChannelLane(dbPath: string): ChannelLaneProof | null {
  return sqliteRows<ChannelLaneProof>(dbPath, `
    SELECT thread_id AS threadId
    FROM gateway_source_bindings
    ORDER BY updated_at_ms DESC
    LIMIT 1
  `).at(0) ?? null;
}

function latestChannelOutbox(dbPath: string): ChannelOutboxProof | null {
  return sqliteRows<ChannelOutboxProof>(dbPath, `
    SELECT status, payload_text AS payloadText, payload_hash AS payloadHash
    FROM gateway_channel_outbox
    ORDER BY created_at_ms DESC
    LIMIT 1
  `).at(0) ?? null;
}

function latestTurnDelivery(dbPath: string, threadId?: string): TurnDeliveryProof | null {
  const where = threadId ? `WHERE thread_id = ${sqliteString(threadId)}` : "";
  return sqliteRows<TurnDeliveryProof>(dbPath, `
    SELECT status, input_json AS inputJson
    FROM gateway_turn_deliveries
    ${where}
    ORDER BY created_at_ms DESC
    LIMIT 1
  `).at(0) ?? null;
}

function sqliteString(value: string): string {
  return `'${value.replaceAll("'", "''")}'`;
}

function sqliteRows<T>(dbPath: string, query: string): T[] {
  try {
    const stdout = execFileSync("sqlite3", ["-json", dbPath, query], { encoding: "utf8" });
    return JSON.parse(stdout || "[]") as T[];
  } catch {
    return [];
  }
}

function writeAgentDefinition(cwd: string, name: string, instructions: string, backendRef?: string) {
  const agentDir = path.join(cwd, ".psychevo", "agents");
  mkdirSync(agentDir, { recursive: true });
  writeFileSync(path.join(agentDir, `${name}.md`), [
    "---",
    `description: ${instructions}`,
    ...(backendRef ? [
      "backend:",
      `  ref: ${JSON.stringify(backendRef)}`,
      "entrypoints: [peer]"
    ] : []),
    "---",
    instructions,
    ""
  ].join("\n"));
}

async function capture(page: Page, testInfo: TestInfo, root: string, name: string) {
  const file = path.join(root, `${name}-${testInfo.project.name}.png`);
  await page.screenshot({ fullPage: true, path: file });
  await testInfo.attach(path.basename(file), { path: file, contentType: "image/png" });
}

type WebSocketFrameCapture = { received: string[]; sent: string[] };

function captureWebSocketFrames(page: Page): WebSocketFrameCapture {
  const capture: WebSocketFrameCapture = { received: [], sent: [] };
  page.on("websocket", (socket) => {
    socket.on("framesent", (event) => capture.sent.push(String(event.payload)));
    socket.on("framereceived", (event) => capture.received.push(String(event.payload)));
  });
  return capture;
}

async function captureWebSocketFramesWithDelayedRpcResult(
  page: Page,
  method: string,
  delayedOccurrence: number
): Promise<WebSocketFrameCapture & { releaseDelayedResponses(): void }> {
  const capture: WebSocketFrameCapture = { received: [], sent: [] };
  const delayedResponses: Array<() => void> = [];
  let matchingRequestCount = 0;
  await page.routeWebSocket(/\/ws(?:\?.*)?$/, (pageSocket) => {
    const serverSocket = pageSocket.connectToServer();
    const delayedRequestIds = new Set<string>();
    pageSocket.onMessage((message) => {
      const payload = String(message);
      capture.sent.push(payload);
      try {
        const request = JSON.parse(payload) as { id?: unknown; method?: string };
        if (request.method === method && request.id != null) {
          matchingRequestCount += 1;
          if (matchingRequestCount === delayedOccurrence) {
            delayedRequestIds.add(String(request.id));
          }
        }
      } catch {
        // Non-JSON frames are forwarded unchanged.
      }
      serverSocket.send(message);
    });
    serverSocket.onMessage((message) => {
      const payload = String(message);
      let delayed = false;
      try {
        const response = JSON.parse(payload) as { id?: unknown };
        delayed = response.id != null && delayedRequestIds.delete(String(response.id));
      } catch {
        // Non-JSON frames are forwarded unchanged.
      }
      const forward = () => {
        capture.received.push(payload);
        pageSocket.send(message);
      };
      if (delayed) {
        delayedResponses.push(forward);
      } else {
        forward();
      }
    });
  });
  return {
    ...capture,
    releaseDelayedResponses() {
      delayedResponses.splice(0).forEach((forward) => forward());
    }
  };
}

function rpcFrameProof(capture: WebSocketFrameCapture) {
  const relevantMethods = new Set([
    "thread/context/read",
    "thread/control/set",
    "thread/history/read",
    "turn/start"
  ]);
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
  return { sent, received };
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

function rpcRequestsForMethod(
  capture: WebSocketFrameCapture,
  method: string
): Array<{ id?: unknown; method?: string; params?: unknown }> {
  return capture.sent.flatMap((payload) => {
    try {
      const message = JSON.parse(payload) as { id?: unknown; method?: string; params?: unknown };
      return message.method === method ? [message] : [];
    } catch {
      return [];
    }
  });
}

async function gatewayRequest(page: Page, method: string, params: unknown): Promise<unknown> {
  return page.evaluate(async ({ method, params }) => await new Promise((resolve, reject) => {
    const url = new URL("/ws", window.location.origin);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    const socket = new WebSocket(url);
    const id = `agent-acp-live-${method}-${Date.now()}`;
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
