import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { expect, test } from "@playwright/test";
import {
  beginBrowserJourneySample,
  installJourneyWebSocketProbe,
  readBrowserJourneyMarks,
  readBrowserJourneyRunnerMarks,
  resetBrowserJourneySample,
  waitForBrowserJourneyMark
} from "./journey-websocket-probe";
import {
  classifyDeterministicNativeRequestPurpose,
  prepareDeterministicAcpAgent,
  startDeterministicNativeModel
} from "./runtime-live.support";

test.describe("critical journey deterministic fixture controls", () => {
  test.beforeEach(({ isMobile }) => {
    test.skip(isMobile, "Node fixture controls only need one Playwright project");
  });

  test("holds Native first output and completion behind separate visual gates", async () => {
    const fixture = await startDeterministicNativeModel({ journeyMode: "visual" });
    const control = fixture.journey;
    expect(control).not.toBeNull();
    try {
      const responsePromise = fetch(`${fixture.baseUrl}/chat/completions`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ model: "default", messages: [{ role: "user", content: "private" }] })
      });
      await control?.waitFor("request_received");
      const response = await responsePromise;
      expect(response.ok).toBe(true);
      expect(control?.events().map((event) => event.event)).toEqual(["request_received"]);

      control?.releaseFirstOutput();
      await control?.waitFor("first_output_emitted");
      expect(control?.events().map((event) => event.event)).toEqual([
        "request_received",
        "first_output_emitted"
      ]);

      control?.releaseCompletion();
      await control?.waitFor("completion_emitted");
      const wire = await response.text();
      expect(wire).toContain(fixture.expectedAnswer.slice(0, 5));
      expect(wire).toContain("data: [DONE]");
      expect(control?.events().map((event) => event.event)).toEqual([
        "request_received",
        "first_output_emitted",
        "completion_emitted"
      ]);
      expect(JSON.stringify(control?.events())).not.toContain("private");
      expect(fixture.requests()).toHaveLength(1);
    } finally {
      await fixture.stop();
    }
  });

  test("lets Native profiling flow freely while retaining receiver evidence", async () => {
    const fixture = await startDeterministicNativeModel({ journeyMode: "profile" });
    try {
      const response = await fetch(`${fixture.baseUrl}/chat/completions`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ model: "default", messages: [] })
      });
      expect(await response.text()).toContain("data: [DONE]");
      await fixture.journey?.waitFor("completion_emitted");
      expect(fixture.journey?.events().map((event) => event.event)).toEqual([
        "request_received",
        "first_output_emitted",
        "completion_emitted"
      ]);
      expect(fixture.journey?.events().map((event) => event.plannedDelayMs)).toEqual([
        0,
        32,
        32
      ]);
    } finally {
      await fixture.stop();
    }
  });

  test("classifies Native main turns separately from asynchronous title requests", async () => {
    expect(classifyDeterministicNativeRequestPurpose({
      messages: [{ role: "user", content: "same profiling prompt" }]
    })).toBe("main_turn");
    expect(classifyDeterministicNativeRequestPurpose({
      messages: [{
        role: "system",
        content: [{
          type: "text",
          text: "Generate a concise title for this coding-agent session. Return only the title."
        }]
      }]
    })).toBe("async_title");

    const fixture = await startDeterministicNativeModel({ journeyMode: "profile" });
    try {
      const main = fetch(`${fixture.baseUrl}/chat/completions`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ messages: [{ role: "user", content: "same profiling prompt" }] })
      });
      const title = fetch(`${fixture.baseUrl}/chat/completions`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ messages: [{
          role: "system",
          content: "Generate a concise title for this coding-agent session. Return only the title."
        }] })
      });
      const mainReceived = await fixture.journey?.waitFor(
        "request_received",
        { purpose: "main_turn", sequence: 1 }
      );
      const titleReceived = await fixture.journey?.waitFor(
        "request_received",
        { purpose: "async_title", sequence: 1 }
      );
      expect(mainReceived?.purposeSequence).toBe(1);
      expect(titleReceived?.purposeSequence).toBe(1);
      expect(mainReceived?.requestIndex).not.toBe(titleReceived?.requestIndex);
      await Promise.all([
        main.then((response) => response.text()),
        title.then((response) => response.text())
      ]);
    } finally {
      await fixture.stop();
    }
  });

  test("resets first-paint and runner observations for every browser sample", async ({ page }) => {
    await installJourneyWebSocketProbe(page);
    await page.goto("data:text/html,<main class='appShell' data-turn-state='running'></main>");

    for (const sampleIndex of [1, 2]) {
      await beginBrowserJourneySample(page, sampleIndex);
      await page.evaluate(() => {
        const assistant = document.createElement("div");
        assistant.className = "pevo-message is-assistant";
        assistant.textContent = "deterministic output";
        document.body.append(assistant);
      });
      await waitForBrowserJourneyMark(page, "first_output_visible", 10_000, sampleIndex);
      await expect.poll(() => readBrowserJourneyRunnerMarks(page).some((mark) => (
        mark.id === "first_output_visible" && mark.sampleIndex === sampleIndex
      ))).toBe(true);
      await resetBrowserJourneySample(page);
    }

    const visible = (await readBrowserJourneyMarks(page)).filter((mark) => (
      mark.id === "first_output_visible"
    ));
    expect(visible.map((mark) => mark.sampleIndex)).toEqual([1, 2]);
    await expect(page.locator(".pevo-message.is-assistant")).toHaveCount(2);
  });

  test("gates ACP prompt output and keeps its persisted journey evidence content-free", async () => {
    const root = mkdtempSync(path.join(tmpdir(), "psychevo-acp-journey-"));
    const fixture = prepareDeterministicAcpAgent(
      "codex",
      root,
      "critical_journey",
      { journeyMode: "visual" }
    );
    const control = fixture.journey;
    expect(control).not.toBeNull();
    const peer = startJsonRpcPeer(fixture.command, fixture.args);
    const privatePrompt = "private ACP journey prompt";
    try {
      peer.send({
        jsonrpc: "2.0",
        id: 1,
        method: "initialize",
        params: { protocolVersion: 1, clientCapabilities: {} }
      });
      await peer.waitFor((message) => message.id === 1 && message.result != null);
      peer.send({
        jsonrpc: "2.0",
        id: 2,
        method: "session/new",
        params: { cwd: root, mcpServers: [] }
      });
      const opened = await peer.waitFor((message) => message.id === 2 && message.result != null);
      const sessionId = String((opened.result as { sessionId?: unknown }).sessionId ?? "");
      expect(sessionId).not.toBe("");

      peer.send({
        jsonrpc: "2.0",
        id: 3,
        method: "session/prompt",
        params: {
          sessionId,
          prompt: [{ type: "text", text: privatePrompt }]
        }
      });
      await control?.waitFor("request_received");
      expect(peer.messages.some(isAgentMessageChunk)).toBe(false);
      expect(peer.messages.some((message) => message.id === 3 && message.result != null)).toBe(false);

      control?.releaseFirstOutput();
      await control?.waitFor("first_output_emitted");
      await peer.waitFor(isAgentMessageChunk);
      expect(peer.messages.some((message) => message.id === 3 && message.result != null)).toBe(false);

      control?.releaseCompletion();
      await control?.waitFor("completion_emitted");
      await peer.waitFor((message) => message.id === 3 && message.result != null);
      expect(control?.events().map((event) => event.event)).toEqual([
        "request_received",
        "first_output_emitted",
        "completion_emitted"
      ]);
      expect(control?.events().every((event) => event.sessionId === sessionId)).toBe(true);
      expect(readFileSync(fixture.logPath, "utf8")).not.toContain(privatePrompt);
      expect(readFileSync(fixture.statePath, "utf8")).not.toContain(privatePrompt);
      expect(JSON.stringify(control?.events())).not.toContain(privatePrompt);
    } finally {
      await peer.stop();
      rmSync(root, { force: true, recursive: true });
    }
  });
});

type JsonRpcMessage = {
  id?: number;
  jsonrpc?: "2.0";
  method?: string;
  params?: unknown;
  result?: unknown;
  error?: unknown;
};

function startJsonRpcPeer(command: string, args: string[]): {
  child: ChildProcessWithoutNullStreams;
  messages: JsonRpcMessage[];
  send(message: JsonRpcMessage): void;
  stop(): Promise<void>;
  waitFor(predicate: (message: JsonRpcMessage) => boolean, timeoutMs?: number): Promise<JsonRpcMessage>;
} {
  const child = spawn(command, args, { stdio: "pipe" });
  const messages: JsonRpcMessage[] = [];
  let buffer = "";
  let stderr = "";
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk: string) => {
    buffer += chunk;
    while (buffer.includes("\n")) {
      const newline = buffer.indexOf("\n");
      const line = buffer.slice(0, newline).trim();
      buffer = buffer.slice(newline + 1);
      if (line) messages.push(JSON.parse(line) as JsonRpcMessage);
    }
  });
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk: string) => {
    stderr += chunk;
  });
  return {
    child,
    messages,
    send(message) {
      child.stdin.write(`${JSON.stringify(message)}\n`);
    },
    async stop() {
      if (child.exitCode !== null) return;
      await new Promise<void>((resolve) => {
        const timeout = setTimeout(resolve, 1_000);
        child.once("exit", () => {
          clearTimeout(timeout);
          resolve();
        });
        child.kill();
      });
    },
    async waitFor(predicate, timeoutMs = 10_000) {
      const deadline = Date.now() + timeoutMs;
      while (Date.now() < deadline) {
        const message = messages.find(predicate);
        if (message) return message;
        if (child.exitCode !== null) {
          throw new Error(`ACP fixture exited with ${child.exitCode}: ${stderr}`);
        }
        await new Promise((resolve) => setTimeout(resolve, 10));
      }
      throw new Error(`timed out waiting for ACP fixture message: ${stderr}`);
    }
  };
}

function isAgentMessageChunk(message: JsonRpcMessage): boolean {
  const params = message.params as { update?: { sessionUpdate?: unknown } } | undefined;
  return message.method === "session/update" && params?.update?.sessionUpdate === "agent_message_chunk";
}
