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
        return { commands: [], hiddenDynamic: 0 };
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
    appendOptimisticPrompt: (current: unknown) => current,
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
    open: { openDownload: vi.fn() }
  }),
  downloadUrl: () => "http://127.0.0.1/download"
}));

Object.defineProperty(HTMLElement.prototype, "scrollTo", {
  configurable: true,
  value: vi.fn()
});

afterEach(() => {
  cleanup();
  gatewayMock.requestLog.length = 0;
});

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
});
