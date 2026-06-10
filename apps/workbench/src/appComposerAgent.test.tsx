// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";

const gatewayMock = vi.hoisted(() => {
  const scope = {
    workdir: "/tmp/project",
    source: {
      kind: "web",
      rawId: null,
      lifetime: "persistent" as const,
      rawIdentity: null,
      visibleName: null
    }
  };
  const source = {
    kind: "web",
    rawId: "test-source",
    lifetime: "persistent" as const,
    rawIdentity: null,
    visibleName: null
  };
  const snapshot = {
    source,
    scope,
    thread: {
      id: "thread-1",
      backend: { kind: "psychevo" as const, nativeId: "thread-1" },
      sourceKey: "source-key"
    },
    entries: [],
    activity: { running: false, activeTurnId: null, queuedTurns: 0 },
    pendingPermissions: [],
    pendingClarifies: []
  };
  return {
    commandExecute: ((command: string): unknown => {
      return {
        accepted: false,
        command,
        known: false,
        action: { type: "passThroughPrompt", text: command }
      };
    }),
    commandList: [] as Array<Record<string, unknown>>,
    openDownloadLog: [] as string[],
    optimisticLog: [] as string[],
    requestLog: [] as Array<{ method: string; params: unknown }>,
    scope,
    settingsResult(agent: string | null) {
      return {
        workdir: scope.workdir,
        project: {
          path: scope.workdir,
          displayPath: "/tmp/project",
          branch: "main"
        },
        memoryResources: { mode: "status_only", available: true },
        secrets: { frontendPersistence: "disabled" },
        controls: {
          permissionMode: "default",
          mode: "default",
          agent,
          model: "xiaomi/xiaomi-token-high",
          variant: "none",
          permissionModeOptions: ["default"],
          modeOptions: ["default", "plan"],
          modelOptions: ["xiaomi/xiaomi-token-high", "openai/gpt-4o"],
          variantOptions: ["none"]
        }
      };
    },
    snapshot,
    source
  };
});

vi.mock("@psychevo/client", () => {
  class GatewayClient {
    subscribe = vi.fn();
    close = vi.fn();

    async connect() {
      return undefined;
    }

    async request(method: string, params?: unknown) {
      gatewayMock.requestLog.push({ method, params });
      if (method === "initialize") {
        return {
          server: "test",
          version: "0.0.0",
          cwd: gatewayMock.scope.workdir,
          scope: gatewayMock.scope,
          source: gatewayMock.source,
          capabilities: {}
        };
      }
      if (method === "thread/resume" || method === "thread/read") {
        return gatewayMock.snapshot;
      }
      if (method === "thread/list") {
        return { sessions: [] };
      }
      if (method === "settings/read") {
        return gatewayMock.settingsResult(null);
      }
      if (method === "settings/update") {
        const record = params as { agent?: string | null };
        return gatewayMock.settingsResult(record.agent ?? null);
      }
      if (method === "agent/list") {
        return {
          agents: [
            {
              name: "translate",
              description: "Translate user messages",
              source: "project",
              generated: false,
              path: "/tmp/project/.psychevo/agents/translate.md",
              entrypoints: ["main"]
            }
          ],
          shadowed_agents: []
        };
      }
      if (method === "backend/list") {
        return { backends: [] };
      }
      if (method === "command/list") {
        return { commands: gatewayMock.commandList, hiddenDynamic: 0 };
      }
      if (method === "command/execute") {
        const record = params as { command?: string };
        return gatewayMock.commandExecute(record.command ?? "");
      }
      if (method === "workspace/files") {
        return { root: gatewayMock.scope.workdir, entries: [], truncated: false };
      }
      if (method === "workspace/diff") {
        return {
          isGitRepo: true,
          files: [],
          unifiedDiff: "",
          truncation: { truncated: false, maxBytes: 0, maxLines: 0, omittedBytes: 0, omittedLines: 0 },
          selectedPath: null
        };
      }
      if (method === "context/read") {
        return {
          available: true,
          label: "0 tokens",
          status: "ok",
          usedTokens: 0,
          contextLimit: null,
          percent: 0,
          categories: [],
          advice: []
        };
      }
      if (method === "completion/list") {
        return { items: [], replacement: null };
      }
      if (method === "turn/start") {
        return { accepted: true };
      }
      throw new Error(`unexpected request: ${method}`);
    }
  }

  return {
    GatewayClient,
    appendOptimisticPrompt: (current: unknown, text: string) => {
      gatewayMock.optimisticLog.push(text);
      return current;
    },
    applyLiveTranscriptEvent: (current: unknown) => current,
    parseThreadSnapshot: (value: unknown) => value,
    reconcileThreadSnapshot: (_current: unknown, next: unknown) => next,
    scopeForWorkdir: (workdir: string) => ({ ...gatewayMock.scope, workdir })
  };
});

vi.mock("@psychevo/host", () => ({
  createBrowserHost: () => ({
    endpoint: { wsUrl: "ws://127.0.0.1/test", baseUrl: "http://127.0.0.1/test" },
    storage: {
      getJson: (_key: string, fallback: unknown) => fallback,
      setJson: vi.fn()
    },
    clipboard: { writeText: vi.fn(async () => ({ ok: true })) },
    files: { pickFile: vi.fn(async () => ({ ok: false })) },
    open: { openDownload: vi.fn((url: string) => gatewayMock.openDownloadLog.push(url)) }
  }),
  downloadUrl: () => "http://127.0.0.1/download"
}));

Object.defineProperty(HTMLElement.prototype, "scrollTo", {
  configurable: true,
  value: vi.fn()
});

afterEach(() => {
  cleanup();
  gatewayMock.commandExecute = (command: string) => ({
    accepted: false,
    command,
    known: false,
    action: { type: "passThroughPrompt", text: command }
  });
  gatewayMock.commandList = [];
  gatewayMock.openDownloadLog.length = 0;
  gatewayMock.optimisticLog.length = 0;
  gatewayMock.requestLog.length = 0;
});

function commandItem(
  name: string,
  presentationKind: string,
  destination: string,
  summary = `${name} summary`
): Record<string, unknown> {
  return {
    name,
    slash: `/${name}`,
    usage: `/${name}`,
    summary,
    aliases: [],
    argumentKind: "none",
    source: "core",
    presentationKind,
    destination,
    feedbackAnchor: "commandsPanel",
    alternateAction: null
  };
}

function workspaceDiffAction() {
  return {
    type: "workspaceDiff",
    diff: {
      isGitRepo: true,
      files: [],
      unifiedDiff: "diff --git a/src/main.rs b/src/main.rs\n",
      truncation: { truncated: false, maxBytes: 0, maxLines: 0, omittedBytes: 0, omittedLines: 0 },
      selectedPath: null
    }
  };
}

describe("Workbench composer agent wiring", () => {
  it("persists concrete agent selection and submits the selected agent", async () => {
    render(<App />);

    const agentSelect = await screen.findByRole("combobox", { name: "Agent" });
    expect(screen.getByRole("option", { name: "Default Agent" })).toBeTruthy();
    expect(screen.getByRole("option", { name: "Default Permission" })).toBeTruthy();
    fireEvent.change(agentSelect, { target: { value: "translate" } });

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "settings/update",
        params: expect.objectContaining({ agent: "translate", threadId: "thread-1" })
      });
    });

    fireEvent.change(agentSelect, { target: { value: "" } });
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "settings/update",
        params: expect.objectContaining({ agent: null, threadId: "thread-1" })
      });
    });
    fireEvent.change(agentSelect, { target: { value: "translate" } });

    const textarea = screen.getByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "hello" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({ agentName: "translate" })
      });
    });
  });

  it("unmounts hidden left sidebar sections when collapsed", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Collapse left sidebar" }));

    expect(screen.getByRole("button", { name: "New Session" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Search" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Artifacts" })).toBeTruthy();
    expect(screen.queryByText("Pinned")).toBeNull();
    expect(screen.queryByText("Sessions")).toBeNull();
    expect(screen.getByRole("button", { name: "Settings" })).toBeTruthy();
  });

  it("shows an explicit Settings return action", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    expect(await screen.findByRole("region", { name: "Settings" })).toBeTruthy();

    const backButton = screen.getByRole("button", { name: "Back to transcript" });
    expect(backButton.textContent).toBe("");
    expect(backButton.getAttribute("title")).toBe("Back to transcript");

    fireEvent.click(backButton);
    expect(await screen.findByRole("region", { name: "Transcript" })).toBeTruthy();
  });

  it("renders provider-qualified model names as short labels", async () => {
    render(<App />);

    const modelSelect = await screen.findByRole("combobox", { name: "Model" }) as HTMLSelectElement;
    expect(modelSelect.selectedOptions[0]?.textContent).toBe("xiaomi-token-high");
    expect(modelSelect.title).toBe("xiaomi/xiaomi-token-high");
    expect(screen.getByRole("option", { name: "gpt-4o" })).toBeTruthy();
    expect(screen.queryByRole("option", { name: "xiaomi/xiaomi-token-high" })).toBeNull();
  });

  it("groups command panel rows by runtime presentation kind", async () => {
    gatewayMock.commandList = [
      commandItem("sessions", "navigate", "history"),
      commandItem("diff", "inspect", "preview"),
      commandItem("queue", "control", "composer"),
      commandItem("fork", "submit", "composer"),
      commandItem("export", "export", "download"),
      commandItem("x-daily", "extension", "composer", "Fetch X daily posts.")
    ];
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "navigate",
      feedbackAnchor: "commandsPanel",
      action: { type: "showPanel", panel: "commands" }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/help" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByRole("region", { name: "Commands" })).toBeTruthy();
    for (const heading of ["Navigate", "Inspect", "Control", "Submit", "Export", "Extensions"]) {
      expect(screen.getByText(heading)).toBeTruthy();
    }
    expect(screen.getByRole("button", { name: /\/diff/ })).toBeTruthy();
    expect(screen.getByText("Preview")).toBeTruthy();
  });

  it("shows composer feedback for known unsupported slash commands without submitting a turn", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: false,
      command,
      known: true,
      message: "/model is managed by the Workbench model controls.",
      presentationKind: "control",
      feedbackAnchor: "composer",
      alternateAction: { type: "openComposerControl", target: "model", label: "Open model controls" },
      action: null
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/model" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByText("/model is managed by the Workbench model controls.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Open model controls" })).toBeTruthy();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
  });

  it("submits unknown slash input as prompt text", async () => {
    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/tmp/output.txt" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          input: [{ type: "text", text: "/tmp/output.txt" }]
        })
      });
    });
  });

  it("submits dynamic slash payloads while displaying the original slash line", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "extension",
      feedbackAnchor: "composer",
      action: { type: "submitPrompt", text: "$x-daily latest", displayText: command }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/x-daily latest" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          input: [{ type: "text", text: "$x-daily latest" }]
        })
      });
    });
    expect(gatewayMock.optimisticLog).toContain("/x-daily latest");
  });

  it("routes diff previews and artifact downloads from structured slash actions", async () => {
    gatewayMock.commandExecute = (command: string) => {
      if (command === "/diff") {
        return {
          accepted: true,
          command,
          known: true,
          presentationKind: "inspect",
          feedbackAnchor: "trigger",
          action: workspaceDiffAction()
        };
      }
      return {
        accepted: true,
        command,
        known: true,
        presentationKind: "export",
        feedbackAnchor: "trigger",
        action: { type: "downloadSession", kind: "export", threadId: "thread-1" }
      };
    };

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/diff" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByLabelText("Inline preview")).toBeTruthy();
    expect(screen.getByText("Workspace Diff")).toBeTruthy();

    fireEvent.change(textarea, { target: { value: "/export" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.openDownloadLog).toContain("http://127.0.0.1/download");
    });
    expect(await screen.findByText("Export download opened.")).toBeTruthy();
  });
});
