// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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
    endpoint: { wsUrl: "ws://127.0.0.1/test", baseUrl: "http://127.0.0.1/test" } as { wsUrl: string; baseUrl: string } | null,
    observabilityRead: null as null | ((params: unknown) => unknown | Promise<unknown>),
    openDownloadLog: [] as string[],
    optimisticLog: [] as string[],
    requestLog: [] as Array<{ method: string; params: unknown }>,
    subscribers: [] as Array<(notification: { method: string; params?: unknown }) => void>,
    archivedSessionSummaries: [] as Array<Record<string, unknown>>,
    backendRecords: [] as Array<Record<string, unknown>>,
    scope,
    sessionSummaries: [] as Array<Record<string, unknown>>,
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
    source,
    workspaceDiffResult: {
      isGitRepo: true,
      files: [] as Array<{
        path: string;
        status: "modified" | "added" | "deleted" | "untracked" | "binary" | "unreadable";
        binary: boolean;
        unreadable: boolean;
        placeholder: string | null;
      }>,
      unifiedDiff: "",
      truncation: { truncated: false, maxBytes: 0, maxLines: 0, omittedBytes: 0, omittedLines: 0 },
      selectedPath: null as string | null
    },
    workspaceFileReadResults: new Map<string, unknown>(),
    workspaceFilesResult: {
      root: scope.workdir,
      entries: [] as Array<{ path: string; name: string; kind: "file" | "directory"; depth: number }>,
      truncated: false
    },
    workspaceChangesResult: {
      groups: [] as Array<unknown>
    }
  };
});

vi.mock("@psychevo/client", () => {
  class GatewayClient {
    subscribe = vi.fn((callback: (notification: { method: string; params?: unknown }) => void) => {
      gatewayMock.subscribers.push(callback);
      return () => {
        gatewayMock.subscribers = gatewayMock.subscribers.filter((item) => item !== callback);
      };
    });
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
        const record = params as { threadId?: string | null } | undefined;
        const threadId = record?.threadId ?? gatewayMock.snapshot.thread?.id ?? null;
        return {
          ...gatewayMock.snapshot,
          thread: threadId
            ? {
                id: threadId,
                backend: { kind: "psychevo" as const, nativeId: threadId },
                sourceKey: `source-${threadId}`
              }
            : null
        };
      }
      if (method === "thread/list") {
        const record = params as { archived?: boolean } | undefined;
        return { sessions: record?.archived ? gatewayMock.archivedSessionSummaries : gatewayMock.sessionSummaries };
      }
      if (method === "thread/start") {
        return {
          ...gatewayMock.snapshot,
          thread: null,
          entries: [],
          activity: { running: false, activeTurnId: null, queuedTurns: 0 },
          pendingPermissions: [],
          pendingClarifies: []
        };
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
          shadowedAgents: [],
          diagnostics: []
        };
      }
      if (method === "agent/status") {
        return {
          agents: [],
          control: {
            spawningPaused: false,
            maxSpawnDepthCap: 3,
            concurrencyCap: null
          }
        };
      }
      if (method === "backend/list") {
        return { backends: gatewayMock.backendRecords };
      }
      if (method === "backend/write") {
        const record = params as {
          id?: string;
          target?: "project" | "profile";
          enabled?: boolean;
          label?: string | null;
          description?: string | null;
          command?: string | null;
          args?: string[];
          cwd?: string | null;
          entrypoints?: string[];
          clientCapabilities?: string[];
          mcpServers?: string[];
        };
        const backend = {
          id: record.id ?? "opencode",
          kind: "acp",
          enabled: record.enabled ?? true,
          label: record.label ?? record.id ?? "OpenCode",
          description: record.description ?? null,
          command: record.command ?? "opencode",
          args: record.args ?? ["acp"],
          cwd: record.cwd ?? "invocation",
          entrypoints: record.entrypoints ?? ["peer", "subagent"],
          clientCapabilities: record.clientCapabilities ?? ["fs.read", "fs.write", "terminal"],
          mcpServers: record.mcpServers ?? [],
          envKeys: [],
          sourceTargets: [record.target ?? "profile"],
          diagnostics: []
        };
        gatewayMock.backendRecords = [
          ...gatewayMock.backendRecords.filter((item) => item.id !== backend.id),
          backend
        ];
        return {
          written: true,
          changed: true,
          path: "/tmp/home/config.toml",
          target: record.target ?? "profile",
          backend
        };
      }
      if (method === "backend/delete") {
        const record = params as { id?: string; target?: "project" | "profile" };
        return {
          deleted: true,
          changed: true,
          id: record.id ?? "opencode",
          path: "/tmp/home/config.toml",
          target: record.target ?? "profile"
        };
      }
      if (method === "backend/doctor") {
        const record = params as { id?: string };
        return {
          id: record.id ?? "opencode",
          kind: "acp",
          ok: true,
          checks: [
            { name: "enabled", ok: true, message: "backend enabled", path: null },
            { name: "command", ok: true, message: "command resolved", path: "/usr/bin/opencode" }
          ]
        };
      }
      if (method === "command/list") {
        return { commands: gatewayMock.commandList, hiddenDynamic: 0 };
      }
      if (method === "command/execute") {
        const record = params as { command?: string };
        return gatewayMock.commandExecute(record.command ?? "");
      }
      if (method === "workspace/files") {
        return gatewayMock.workspaceFilesResult;
      }
      if (method === "workspace/diff") {
        const record = params as { path?: string | null } | undefined;
        const selectedPath = record?.path ?? null;
        if (!selectedPath) {
          return gatewayMock.workspaceDiffResult;
        }
        return {
          ...gatewayMock.workspaceDiffResult,
          files: gatewayMock.workspaceDiffResult.files.filter((file) => file.path === selectedPath),
          unifiedDiff: [
            `diff --git a/${selectedPath} b/${selectedPath}`,
            `--- a/${selectedPath}`,
            `+++ b/${selectedPath}`,
            "@@ -1 +1 @@",
            "-old selected",
            "+new selected"
          ].join("\n"),
          selectedPath
        };
      }
      if (method === "workspace/changes") {
        return gatewayMock.workspaceChangesResult;
      }
      if (method === "workspace/change/accept" || method === "workspace/change/reject") {
        return { accepted: true, changes: gatewayMock.workspaceChangesResult };
      }
      if (method === "workspace/file/read") {
        const record = params as { path?: string | null } | undefined;
        const path = record?.path ?? "";
        return gatewayMock.workspaceFileReadResults.get(path) ?? {
          path,
          content: "",
          binary: false,
          unreadable: null,
          truncated: false
        };
      }
      if (method === "workspace/file/write") {
        const record = params as { content?: string; path?: string };
        return {
          path: record.path ?? "",
          revision: "written",
          sizeBytes: record.content?.length ?? 0,
          lineEnding: "lf"
        };
      }
      if (method === "observability/read") {
        if (gatewayMock.observabilityRead) {
          return gatewayMock.observabilityRead(params);
        }
        const record = params as { threadId?: string | null } | undefined;
        const hasThread = Boolean(record?.threadId);
        return {
          context: {
            available: hasThread,
            label: hasThread ? "200/1.0k (20.0%)" : "No active session",
            status: hasThread ? "provider_usage" : "unavailable",
            usedTokens: hasThread ? 200 : 0,
            contextLimit: hasThread ? 1000 : null,
            percent: hasThread ? 20 : null,
            categories: hasThread ? [
              {
                id: "base_policy",
                label: "Base policy",
                tokens: 20,
                estimated: true,
                status: "estimated",
                percent: 2,
                details: {}
              },
              {
                id: "developer_prompt",
                label: "Developer prompt",
                tokens: 60,
                estimated: true,
                status: "estimated",
                percent: 6,
                details: {
                  skill_count: 1,
                  skill_entries: [{ name: "design", tokens: 42 }]
                }
              },
              {
                id: "history",
                label: "History",
                tokens: 120,
                estimated: true,
                status: "estimated",
                percent: 12,
                details: {
                  roles: {
                    assistant: { count: 1, tokens: 70 },
                    user: { count: 1, tokens: 50 }
                  }
                }
              }
            ] : [],
            advice: []
          },
          usage: {
            available: hasThread,
            sessionId: hasThread ? record?.threadId : null,
            provider: hasThread ? "mock" : null,
            model: hasThread ? "mock-model" : null,
            messageCount: hasThread ? 2 : 0,
            assistantMessageCount: hasThread ? 1 : 0,
            contextInputTokens: hasThread ? 200 : 0,
            billableInputTokens: hasThread ? 150 : 0,
            billableOutputTokens: hasThread ? 50 : 0,
            reasoningTokens: hasThread ? 12 : 0,
            cacheReadTokens: hasThread ? 80 : 0,
            cacheWriteTokens: hasThread ? 10 : 0,
            reportedTotalTokens: hasThread ? 250 : 0,
            estimatedCostNanodollars: hasThread ? 10_000_000 : 0,
            unknownPricingCount: 0,
            cacheReadPercent: hasThread ? 40 : null
          }
        };
      }
      if (method === "completion/list") {
        return { items: [], replacement: null };
      }
      if (method === "turn/start") {
        return { accepted: true };
      }
      if (method === "terminal/start") {
        return { terminalId: "terminal-1", cwd: gatewayMock.scope.workdir, pid: null };
      }
      if (method === "terminal/write" || method === "terminal/resize" || method === "terminal/terminate") {
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
    endpoint: gatewayMock.endpoint,
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

vi.mock("@xterm/xterm", () => {
  class Terminal {
    cols = 80;
    rows = 24;
    options: Record<string, unknown>;

    constructor(options: Record<string, unknown>) {
      this.options = options;
    }

    dispose = vi.fn();
    focus = vi.fn();
    loadAddon = vi.fn();
    onData = vi.fn(() => ({ dispose: vi.fn() }));
    open = vi.fn();
    write = vi.fn();
  }
  return { Terminal };
});

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit = vi.fn();
  }
}));

Object.defineProperty(HTMLElement.prototype, "scrollTo", {
  configurable: true,
  value: vi.fn()
});

Object.defineProperty(window, "matchMedia", {
  configurable: true,
  value: vi.fn((query: string) => ({
    addEventListener: vi.fn(),
    addListener: vi.fn(),
    dispatchEvent: vi.fn(),
    matches: false,
    media: query,
    onchange: null,
    removeEventListener: vi.fn(),
    removeListener: vi.fn()
  }))
});

Object.defineProperty(HTMLCanvasElement.prototype, "getContext", {
  configurable: true,
  value: vi.fn(() => ({
    clearRect: vi.fn(),
    fillRect: vi.fn(),
    getImageData: vi.fn(() => ({ data: new Uint8ClampedArray([0, 0, 0, 255]) })),
    fillStyle: ""
  }))
});

const localStorageItems = new Map<string, string>();

Object.defineProperty(window, "localStorage", {
  configurable: true,
  value: {
    clear: vi.fn(() => localStorageItems.clear()),
    getItem: vi.fn((key: string) => localStorageItems.get(key) ?? null),
    key: vi.fn((index: number) => Array.from(localStorageItems.keys())[index] ?? null),
    removeItem: vi.fn((key: string) => {
      localStorageItems.delete(key);
    }),
    setItem: vi.fn((key: string, value: string) => {
      localStorageItems.set(key, value);
    }),
    get length() {
      return localStorageItems.size;
    }
  }
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  gatewayMock.commandExecute = (command: string) => ({
    accepted: false,
    command,
    known: false,
    action: { type: "passThroughPrompt", text: command }
  });
  gatewayMock.commandList = [];
  gatewayMock.endpoint = { wsUrl: "ws://127.0.0.1/test", baseUrl: "http://127.0.0.1/test" };
  gatewayMock.observabilityRead = null;
  gatewayMock.openDownloadLog.length = 0;
  gatewayMock.optimisticLog.length = 0;
  gatewayMock.requestLog.length = 0;
  gatewayMock.subscribers = [];
  gatewayMock.archivedSessionSummaries = [];
  gatewayMock.backendRecords = [];
  gatewayMock.sessionSummaries = [];
  gatewayMock.snapshot.thread = {
    id: "thread-1",
    backend: { kind: "psychevo" as const, nativeId: "thread-1" },
    sourceKey: "source-key"
  };
  gatewayMock.workspaceDiffResult = {
    isGitRepo: true,
    files: [],
    unifiedDiff: "",
    truncation: { truncated: false, maxBytes: 0, maxLines: 0, omittedBytes: 0, omittedLines: 0 },
    selectedPath: null
  };
  gatewayMock.workspaceFileReadResults.clear();
  gatewayMock.workspaceFilesResult = {
    root: gatewayMock.scope.workdir,
    entries: [],
    truncated: false
  };
  gatewayMock.workspaceChangesResult = { groups: [] };
  window.localStorage.clear();
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

function sessionSummary(id: string, title: string): Record<string, unknown> {
  return {
    id,
    workdir: gatewayMock.scope.workdir,
    project: {
      workdir: gatewayMock.scope.workdir,
      label: "project",
      displayPath: "/tmp/project"
    },
    model: null,
    provider: null,
    startedAtMs: 1,
    updatedAtMs: 2,
    endedAtMs: null,
    endReason: null,
    archivedAtMs: null,
    messageCount: 1,
    toolCallCount: 0,
    visibleEntryCount: 1,
    activity: { running: false, activeTurnId: null, queuedTurns: 0 },
    title,
    displayTitle: title,
    preview: "session preview"
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function observabilityResult(threadId: string | null, peer = false): Record<string, unknown> {
  const hasThread = Boolean(threadId);
  return {
    context: {
      available: hasThread,
      label: hasThread ? (peer ? "8.0k/200.0k (4.0%)" : "200/1.0k (20.0%)") : "No active session",
      status: hasThread ? (peer ? "reported by ACP peer" : "provider_usage") : "unavailable",
      usedTokens: hasThread ? (peer ? 8_000 : 200) : 0,
      contextLimit: hasThread ? (peer ? 200_000 : 1000) : null,
      percent: hasThread ? (peer ? 4 : 20) : null,
      categories: [],
      advice: []
    },
    usage: {
      available: hasThread,
      sessionId: hasThread ? threadId : null,
      provider: hasThread ? "mock" : null,
      model: hasThread ? "mock-model" : null,
      messageCount: hasThread ? 2 : 0,
      assistantMessageCount: hasThread ? 1 : 0,
      contextInputTokens: hasThread ? (peer ? 8_000 : 200) : 0,
      billableInputTokens: hasThread ? (peer ? 6_100 : 150) : 0,
      billableOutputTokens: hasThread ? (peer ? 356 : 50) : 0,
      reasoningTokens: hasThread ? (peer ? 18 : 12) : 0,
      cacheReadTokens: hasThread ? (peer ? 6_200 : 80) : 0,
      cacheWriteTokens: hasThread ? 10 : 0,
      reportedTotalTokens: hasThread ? (peer ? 8_000 : 250) : 0,
      estimatedCostNanodollars: hasThread ? (peer ? 0 : 10_000_000) : 0,
      unknownPricingCount: 0,
      cacheReadPercent: hasThread ? (peer ? 50 : 40) : null
    }
  };
}

function workspaceDiffAction() {
  return {
    type: "workspaceDiff",
    diff: {
      isGitRepo: true,
      files: [
        { path: "src/main.rs", status: "modified", binary: false, unreadable: false, placeholder: null }
      ],
      unifiedDiff: [
        "diff --git a/src/main.rs b/src/main.rs",
        "--- a/src/main.rs",
        "+++ b/src/main.rs",
        "@@ -1 +1 @@",
        "-old main",
        "+new main"
      ].join("\n"),
      truncation: { truncated: false, maxBytes: 0, maxLines: 0, omittedBytes: 0, omittedLines: 0 },
      selectedPath: null
    }
  };
}

describe("Workbench composer agent wiring", () => {
  it("starts in a hidden draft without rendering a history draft row", async () => {
    const { container } = render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    expect((container.querySelector(".workbench") as HTMLElement | null)?.style.getPropertyValue("--right-column-width")).toBe("520px");
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/start",
        params: expect.objectContaining({ scope: gatewayMock.scope })
      });
    });
    expect(container.querySelectorAll(".pevo-sessionRow.is-draft")).toHaveLength(0);
    expect(screen.queryByRole("region", { name: "Workspace status" })).toBeNull();
  });

  it("opens right workspace tabs from Home and the add menu", async () => {
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    expect(within(home).queryByText("local PTY")).toBeNull();
    expect(within(home).queryByText("workspace tree")).toBeNull();
    fireEvent.click(within(home).getByRole("button", { name: /Review/ }));
    expect(await screen.findByRole("region", { name: "Review" })).toBeTruthy();

    fireEvent.click(document.querySelector(".rightAddMenu summary") as HTMLElement);
    const addMenuFiles = screen.getAllByRole("menuitem", { name: "Files" }).at(-1);
    expect(addMenuFiles).toBeTruthy();
    fireEvent.click(addMenuFiles!);
    expect(await screen.findByRole("region", { name: "Workspace files" })).toBeTruthy();

    fireEvent.click(screen.getByLabelText("Workspace home"));
    const visibleHome = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(visibleHome).getByRole("button", { name: /Terminal/ }));
    expect(await screen.findByRole("region", { name: "Terminal" })).toBeTruthy();
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "terminal/start")).toBe(true);
    });
  });

  it("closes the right workspace add menu on outside click and item activation", async () => {
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: /Review/ }));
    expect(await screen.findByRole("region", { name: "Review" })).toBeTruthy();

    const trigger = document.querySelector(".rightAddMenu summary") as HTMLElement | null;
    const menu = trigger!.closest("details") as HTMLDetailsElement | null;
    fireEvent.click(trigger!);
    await waitFor(() => expect(menu?.open).toBe(true));
    fireEvent.mouseDown(screen.getByRole("region", { name: "Transcript" }));
    await waitFor(() => expect(menu?.open).toBe(false));

    fireEvent.click(trigger!);
    await waitFor(() => expect(menu?.open).toBe(true));
    fireEvent.click(screen.getByRole("menuitem", { name: "Files" }));
    expect(await screen.findByRole("region", { name: "Workspace files" })).toBeTruthy();
    await waitFor(() => expect(menu?.open).toBe(false));

    fireEvent.click(trigger!);
    await waitFor(() => expect(menu?.open).toBe(true));
    fireEvent.click(screen.getByRole("menuitem", { name: "Terminal" }));
    expect(await screen.findByRole("region", { name: "Terminal" })).toBeTruthy();
    await waitFor(() => expect(menu?.open).toBe(false));
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "terminal/start")).toBe(true);
    });
  });

  it("restores and clamps the right workspace width preference", async () => {
    window.localStorage.setItem("psychevo.workbench.v0.prefs", JSON.stringify({
      appearance: "dark",
      debug: false,
      rightWidthPx: 9999
    }));
    const { container } = render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    const workbench = container.querySelector(".workbench") as HTMLElement | null;
    expect(workbench?.style.getPropertyValue("--right-column-width")).toBe("1200px");
  });

  it("toggles Review changed files and scopes the diff preview", async () => {
    gatewayMock.workspaceDiffResult = {
      isGitRepo: true,
      files: [
        { path: "docs/api.md", status: "modified", binary: false, unreadable: false, placeholder: null },
        { path: "src/main.rs", status: "modified", binary: false, unreadable: false, placeholder: null }
      ],
      unifiedDiff: [
        "diff --git a/docs/api.md b/docs/api.md",
        "--- a/docs/api.md",
        "+++ b/docs/api.md",
        "@@ -1 +1 @@",
        "-old docs",
        "+new docs",
        "diff --git a/src/main.rs b/src/main.rs",
        "--- a/src/main.rs",
        "+++ b/src/main.rs",
        "@@ -1 +1 @@",
        "-old main",
        "+new main"
      ].join("\n"),
      truncation: { truncated: false, maxBytes: 0, maxLines: 0, omittedBytes: 0, omittedLines: 0 },
      selectedPath: null
    };
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Review" }));
    const review = await screen.findByRole("region", { name: "Review" });
    expect(within(review).getByText("docs/api.md")).toBeTruthy();
    expect(within(review).getAllByText("M↓").length).toBeGreaterThan(0);
    expect(within(review).getAllByLabelText("1 additions, 1 deletions").length).toBeGreaterThan(0);
    expect(within(review).queryByText("diff --git a/docs/api.md b/docs/api.md")).toBeNull();

    fireEvent.click(within(review).getByRole("button", { name: "Show changed files" }));
    expect(within(review).getByLabelText("Filter changed files")).toBeTruthy();
    fireEvent.change(within(review).getByLabelText("Filter changed files"), { target: { value: "main" } });
    expect(within(review).getByRole("treeitem", { name: /main\.rs/ })).toBeTruthy();
    expect(within(review).queryByRole("treeitem", { name: /api\.md/ })).toBeNull();

    fireEvent.click(within(review).getByRole("treeitem", { name: /main\.rs/ }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "workspace/diff",
        params: expect.objectContaining({ path: "src/main.rs" })
      });
    });
    expect(await within(review).findByText("new selected")).toBeTruthy();
    expect(within(review).getByText("src/main.rs")).toBeTruthy();
  });

  it("rejects turn-scoped Review files through workspace change RPCs", async () => {
    gatewayMock.workspaceChangesResult = {
      groups: [
        {
          turnId: "turn-1",
          threadId: "thread-1",
          createdAtMs: 1,
          completedAtMs: 2,
          files: [
            {
              path: "docs/api.md",
              status: "modified",
              binary: false,
              unreadable: false,
              reviewStatus: "pending",
              canReject: true,
              message: null
            }
          ]
        }
      ]
    };
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Review" }));
    const review = await screen.findByRole("region", { name: "Review" });

    expect(within(review).getByText("docs/api.md")).toBeTruthy();
    fireEvent.click(within(review).getByLabelText("Reject docs/api.md"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "workspace/change/reject",
        params: expect.objectContaining({ path: "docs/api.md", turnId: "turn-1" })
      });
    });
  });

  it("renders Markdown file previews from the shared Markdown component", async () => {
    gatewayMock.workspaceFilesResult = {
      root: gatewayMock.scope.workdir,
      entries: [
        { path: "docs", name: "docs", kind: "directory", depth: 0 },
        { path: "docs/README.md", name: "README.md", kind: "file", depth: 1 },
        { path: "src", name: "src", kind: "directory", depth: 0 },
        { path: "src/main.rs", name: "main.rs", kind: "file", depth: 1 }
      ],
      truncated: false
    };
    gatewayMock.workspaceFileReadResults.set("docs/README.md", {
      path: "docs/README.md",
      content: "# API Notes\n\n- supports markdown",
      binary: false,
      unreadable: null,
      truncated: false
    });
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Files" }));
    const files = await screen.findByRole("region", { name: "Workspace files" });
    expect(within(files).getByLabelText("Filter workspace files")).toBeTruthy();
    expect(files.querySelector("header p")).toBeNull();

    fireEvent.click(within(files).getByRole("treeitem", { name: /README\.md/ }));
    expect(await within(files).findByText("/tmp/project/docs/README.md")).toBeTruthy();
    expect(await within(files).findByRole("heading", { name: "API Notes" })).toBeTruthy();
    expect(within(files).getByText("supports markdown")).toBeTruthy();
  });

  it("saves text edits manually without entering the Review queue", async () => {
    gatewayMock.workspaceFilesResult = {
      root: gatewayMock.scope.workdir,
      entries: [
        { path: "docs", name: "docs", kind: "directory", depth: 0 },
        { path: "docs/README.md", name: "README.md", kind: "file", depth: 1 }
      ],
      truncated: false
    };
    gatewayMock.workspaceFileReadResults.set("docs/README.md", {
      path: "docs/README.md",
      content: "before\n",
      binary: false,
      editable: true,
      editableReason: null,
      revision: "r1",
      sizeBytes: 7,
      lineEnding: "lf",
      unreadable: null,
      truncated: false
    });
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Files" }));
    const files = await screen.findByRole("region", { name: "Workspace files" });
    fireEvent.click(within(files).getByRole("treeitem", { name: /README\.md/ }));
    fireEvent.click(await within(files).findByLabelText("Edit docs/README.md"));
    const editor = within(files).getByLabelText("Edit docs/README.md");
    fireEvent.change(editor, { target: { value: "after\n" } });
    fireEvent.click(within(files).getByLabelText("Save file"));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "workspace/file/write",
        params: expect.objectContaining({
          path: "docs/README.md",
          content: "after\n",
          expectedRevision: "r1",
          force: false
        })
      });
    });
    expect(gatewayMock.requestLog.some((entry) => entry.method.startsWith("workspace/change/"))).toBe(false);
  });

  it("renders code previews with absolute paths, syntax tokens, and escaped source text", async () => {
    gatewayMock.workspaceFilesResult = {
      root: gatewayMock.scope.workdir,
      entries: [
        { path: "src", name: "src", kind: "directory", depth: 0 },
        { path: "src/main.py", name: "main.py", kind: "file", depth: 1 }
      ],
      truncated: false
    };
    gatewayMock.workspaceFileReadResults.set("src/main.py", {
      path: "src/main.py",
      content: "def greet():\n    return \"<script>alert(1)</script>\"\n",
      binary: false,
      unreadable: null,
      truncated: false
    });
    const { container } = render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Files" }));
    const files = await screen.findByRole("region", { name: "Workspace files" });

    fireEvent.click(within(files).getByRole("treeitem", { name: /main\.py/ }));
    expect(await within(files).findByText("/tmp/project/src/main.py")).toBeTruthy();
    const preview = container.querySelector(".rightCodePreview") as HTMLElement | null;
    expect(preview?.dataset.lang).toBe("python");
    expect(preview?.querySelector(".hljs-keyword, .hljs-title")).toBeTruthy();
    expect(preview?.querySelector("script")).toBeNull();
    expect(preview?.innerHTML).toContain("&lt;script&gt;");
  });

  it("keeps Terminal interactive without the persistent title and state header", async () => {
    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    fireEvent.click(within(home).getByRole("button", { name: "Terminal" }));
    const terminal = await screen.findByRole("region", { name: "Terminal" });
    await waitFor(() => {
      expect(gatewayMock.requestLog.some((entry) => entry.method === "terminal/start")).toBe(true);
    });

    expect(within(terminal).queryByRole("heading", { name: "project" })).toBeNull();
    expect(within(terminal).queryByText("/tmp/project")).toBeNull();
    expect(within(terminal).queryByText("running")).toBeNull();
  });

  it("keeps concrete draft agent selection and submits the selected agent", async () => {
    render(<App />);

    const agentSelect = await screen.findByRole("combobox", { name: "Agent" });
    expect(screen.getByRole("option", { name: "Default Agent" })).toBeTruthy();
    expect(screen.getByRole("option", { name: "Default Permission" })).toBeTruthy();
    expect(await screen.findByRole("option", { name: "translate" })).toBeTruthy();
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
    expect(screen.queryByRole("button", { name: "Artifacts" })).toBeNull();
    expect(screen.queryByText("Pinned")).toBeNull();
    expect(screen.queryByText("Sessions")).toBeNull();
    expect(screen.getByRole("button", { name: "Settings" })).toBeTruthy();
  });

  it("shows an explicit Settings return action", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    expect(settingsRegion).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Appearance" }).getAttribute("aria-current")).toBe("page");
    expect(within(settingsRegion).getByRole("heading", { name: "Appearance" })).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Archived sessions" })).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Debug" })).toBeTruthy();
    expect(within(settingsRegion).getByRole("button", { name: "Agents" })).toBeTruthy();
    for (const removed of ["General", "Session", "Session history", "Commands", "Integrations", "Diagnostics"]) {
      expect(within(settingsRegion).queryByRole("button", { name: removed })).toBeNull();
    }
    expect(within(settingsRegion).getByRole("searchbox", { name: "Search settings" })).toBeTruthy();
    expect(within(settingsRegion).queryByText("/tmp/project")).toBeNull();
    expect(within(settingsRegion).queryByRole("heading", { name: "Settings" })).toBeNull();
    expect(within(settingsRegion).queryByRole("button", { name: "Back to transcript" })).toBeNull();

    const backButton = within(settingsRegion).getByRole("button", { name: "Back to app" });

    fireEvent.click(backButton);
    expect(await screen.findByRole("region", { name: "Transcript" })).toBeTruthy();
  });

  it("switches Settings sections while keeping session controls in the composer", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Agents" }));

    expect(within(settingsRegion).getByRole("button", { name: "Agents" }).getAttribute("aria-current")).toBe("page");
    expect(within(settingsRegion).getByRole("region", { name: "Agents" })).toBeTruthy();
    expect(within(settingsRegion).getByText("Profile ACP Backends")).toBeTruthy();
    expect(within(settingsRegion).queryByText("Translate user messages")).toBeNull();
    expect(within(settingsRegion).queryByRole("combobox", { name: "Agent" })).toBeNull();
    expect(within(settingsRegion).queryByRole("combobox", { name: "Model" })).toBeNull();
    expect(within(settingsRegion).queryByRole("combobox", { name: "Permission mode" })).toBeNull();

    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Back to app" }));
    expect(await screen.findByRole("combobox", { name: "Agent" })).toBeTruthy();
  });

  it("shows archived sessions from Settings without turning the sidebar into an archive filter", async () => {
    gatewayMock.sessionSummaries = [sessionSummary("active-thread", "Active session")];
    gatewayMock.archivedSessionSummaries = [sessionSummary("archived-thread", "Archived session")];

    render(<App />);

    expect(await screen.findByText("Active session")).toBeTruthy();
    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Archived sessions" }));

    const archivedPanel = await within(settingsRegion).findByRole("region", { name: "Archived sessions" });
    expect(await within(archivedPanel).findByText("Archived session")).toBeTruthy();
    expect(within(settingsRegion).queryByText("Active session")).toBeNull();
    expect(gatewayMock.requestLog).toContainEqual({
      method: "thread/list",
      params: expect.objectContaining({ archived: true })
    });
  });

  it("renders provider-qualified model options while keeping a compact selected indicator", async () => {
    render(<App />);

    const modelSelect = await screen.findByRole("combobox", { name: "Model" }) as HTMLSelectElement;
    expect(modelSelect.selectedOptions[0]?.textContent).toBe("xiaomi/xiaomi-token-high");
    expect(modelSelect.title).toBe("xiaomi/xiaomi-token-high");
    expect(screen.getByRole("option", { name: "openai/gpt-4o" })).toBeTruthy();
    expect(screen.getByRole("option", { name: "xiaomi/xiaomi-token-high" })).toBeTruthy();
    expect(screen.queryByRole("option", { name: "xiaomi-token-high" })).toBeNull();
    expect(screen.getByText("xiaomi-token-high")).toBeTruthy();
    expect(modelSelect.closest(".statusSelect")?.getAttribute("style")).toContain("--pevo-status-select-value-width: 18ch");
  });

  it("renders the full session id in the Status panel", async () => {
    const longSessionId = "019ebc20-1234-5678-9abc-def0123492dd";
    gatewayMock.sessionSummaries = [sessionSummary(longSessionId, "Long session")];

    render(<App />);

    fireEvent.click(await screen.findByText("Long session"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: longSessionId })
      });
    });
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "observability/read",
        params: expect.objectContaining({ threadId: longSessionId })
      });
    });
    fireEvent.click(await screen.findByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    expect(await within(home).findByText(longSessionId)).toBeTruthy();
    expect(within(home).queryByText("/tmp/project")).toBeNull();
    expect(home.querySelector(".rightStatusMetrics")).toBeNull();
    expect(within(home).queryByText("Session")).toBeNull();
    expect(within(home).queryByText("Connection")).toBeNull();
    expect(within(home).queryByText("Turn")).toBeNull();
    expect(within(home).queryByText("Queued")).toBeNull();
    expect(within(home).getByText("Session tokens")).toBeTruthy();
    expect(within(home).getByText("250")).toBeTruthy();
    expect(within(home).getByText("40%")).toBeTruthy();
    expect(within(home).getByText("$0.010000")).toBeTruthy();
    expect(within(home).queryByText("Messages")).toBeNull();
    expect(within(home).queryByText("Provider")).toBeNull();
    expect(within(home).queryByText("Model")).toBeNull();
    expect(home.querySelector(".sessionContextRing")).toBeNull();
    const stack = home.querySelector(".promptTokenStack");
    expect(stack).toBeTruthy();
    expect(stack?.querySelectorAll(".promptTokenSegment").length).toBe(3);
    expect(stack?.querySelector('[title^="Developer prompt:"]')).toBeTruthy();
    const promptTokens = within(home).getByText("Prompt tokens").closest("details") as HTMLDetailsElement | null;
    expect(promptTokens).toBeTruthy();
    expect(promptTokens?.classList.contains("promptTokensDisclosure")).toBe(true);
    expect(promptTokens?.open).toBe(false);
    const promptSummary = promptTokens!.querySelector("summary") as HTMLElement;
    expect(within(promptSummary).queryByText("3")).toBeNull();
    expect(promptTokens?.querySelectorAll(".promptTokenCategory details").length).toBe(0);
    fireEvent.click(promptSummary);
    expect(promptTokens?.open).toBe(true);
    expect(within(promptTokens as HTMLElement).getByText("Developer prompt")).toBeTruthy();
    expect(within(promptTokens as HTMLElement).getByText("design")).toBeTruthy();
    expect(screen.queryByText("019ebc20...92dd")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Context usage" }));
    const contextPopover = await screen.findByRole("dialog", { name: "Context usage" });
    expect(within(contextPopover).getByText("Session tokens")).toBeTruthy();
    expect(within(contextPopover).getByText("Cache read")).toBeTruthy();
    expect(within(contextPopover).getByText("Cost")).toBeTruthy();
    expect(within(contextPopover).queryByText("Developer prompt")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "New Session" }));
    await waitFor(() => {
      expect(within(home).getByText("No session usage yet.")).toBeTruthy();
    });
  });

  it("does not apply stale session observability after creating a new draft", async () => {
    const staleObservability = deferred<Record<string, unknown>>();
    gatewayMock.sessionSummaries = [sessionSummary("old-thread", "Old session")];
    gatewayMock.observabilityRead = (params: unknown) => {
      const threadId = (params as { threadId?: string | null } | undefined)?.threadId ?? null;
      if (threadId === "old-thread") {
        return staleObservability.promise;
      }
      return observabilityResult(threadId);
    };

    render(<App />);

    fireEvent.click(await screen.findByText("Old session"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "old-thread" })
      });
    });
    fireEvent.click(await screen.findByLabelText("Show right inspector"));
    fireEvent.click(screen.getByLabelText("New Session"));

    const home = await screen.findByRole("region", { name: "Workspace status" });
    await waitFor(() => {
      expect(within(home).getByText("draft")).toBeTruthy();
      expect(within(home).getByText("No active session")).toBeTruthy();
    });

    await act(async () => {
      staleObservability.resolve(observabilityResult("old-thread", true));
      await staleObservability.promise;
    });

    expect(within(home).getByText("No active session")).toBeTruthy();
    expect(within(home).queryByText("reported by ACP peer")).toBeNull();
    expect(within(home).queryByText("8.0k/200.0k (4.0%)")).toBeNull();
  });

  it("ignores late completed-turn observability refreshes for a previous session", async () => {
    gatewayMock.observabilityRead = (params: unknown) => {
      const threadId = (params as { threadId?: string | null } | undefined)?.threadId ?? null;
      return observabilityResult(threadId, threadId === "old-thread");
    };

    render(<App />);

    expect(await screen.findByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "New Session" }));
    fireEvent.click(await screen.findByLabelText("Show right inspector"));
    const home = await screen.findByRole("region", { name: "Workspace status" });
    await waitFor(() => {
      expect(within(home).getByText("draft")).toBeTruthy();
      expect(within(home).getByText("No active session")).toBeTruthy();
    });

    await act(async () => {
      for (const subscriber of gatewayMock.subscribers) {
        subscriber({
          method: "gateway/event",
          params: {
            type: "turnCompleted",
            threadId: "old-thread",
            turnId: "old-turn",
            outcome: "normal",
            committedEntries: []
          }
        });
      }
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "observability/read",
        params: expect.objectContaining({ threadId: "old-thread" })
      });
    });
    expect(within(home).getByText("No active session")).toBeTruthy();
    expect(within(home).queryByText("reported by ACP peer")).toBeNull();
    expect(within(home).queryByText("8.0k/200.0k (4.0%)")).toBeNull();
  });

  it("opens the composer context usage popover without revealing Status", async () => {
    render(<App />);

    expect(screen.queryByRole("region", { name: "Workspace status" })).toBeNull();
    fireEvent.click(await screen.findByRole("button", { name: "Context usage" }));

    const contextPopover = await screen.findByRole("dialog", { name: "Context usage" });
    expect(within(contextPopover).getByText("No session context is active.")).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Workspace status" })).toBeNull();
    expect(screen.getByLabelText("Show right inspector")).toBeTruthy();
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

    expect(await screen.findByRole("region", { name: "Commands overlay" })).toBeTruthy();
    expect(await screen.findByRole("region", { name: "Commands" })).toBeTruthy();
    expect(screen.getByRole("region", { name: "Transcript" })).toBeTruthy();
    expect(screen.getByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    for (const heading of ["Navigate", "Inspect", "Control", "Submit", "Export", "Extensions"]) {
      expect(screen.getByText(heading)).toBeTruthy();
    }
    expect(screen.getByRole("button", { name: /\/diff/ })).toBeTruthy();
    expect(screen.getByText("Preview")).toBeTruthy();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);

    fireEvent.click(screen.getByRole("button", { name: "Close Commands" }));
    expect(screen.queryByRole("region", { name: "Commands overlay" })).toBeNull();
    expect(screen.getByPlaceholderText("Ask Psychevo...")).toBeTruthy();
  });

  it("opens commands slash results as transcript overlays", async () => {
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
    fireEvent.change(textarea, { target: { value: "/commands" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByRole("region", { name: "Commands overlay" })).toBeTruthy();
    expect(screen.getByRole("region", { name: "Transcript" })).toBeTruthy();
    expect(screen.getByPlaceholderText("Ask Psychevo...")).toBeTruthy();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
  });

  it("does not expose /agents as a GUI command surface", async () => {
    gatewayMock.commandList = [
      commandItem("commands", "navigate", "commands")
    ];
    gatewayMock.commandExecute = (command: string) => ({
      accepted: false,
      command,
      known: true,
      message: "/agents is managed by the Workbench agent selector and Settings Agents.",
      feedbackAnchor: "composer",
      action: null
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/agents" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(screen.getByText("/agents is managed by the Workbench agent selector and Settings Agents.")).toBeTruthy();
    });
    expect(screen.queryByRole("region", { name: "Commands overlay" })).toBeNull();
    expect(screen.queryByRole("region", { name: "Agents overlay" })).toBeNull();
    expect(screen.queryByRole("region", { name: "Settings" })).toBeNull();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
  });

  it("creates a Profile ACP backend from the generic Settings Agents add action", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Agents" }));

    const agentsPanel = await within(settingsRegion).findByRole("region", { name: "Agents" });
    const addButton = within(agentsPanel).getByRole("button", { name: "Add ACP backend" });
    expect(addButton.textContent).toBe("");
    fireEvent.click(addButton);
    const form = await within(agentsPanel).findByRole("form", { name: "Profile ACP backend" });
    expect(within(form).queryByLabelText("Target")).toBeNull();
    expect((within(form).getByLabelText("ID") as HTMLInputElement).value).toBe("");
    const commandJson = within(form).getByLabelText("Command JSON") as HTMLTextAreaElement;
    expect(commandJson.value).toBe("");
    expect(commandJson.placeholder).toContain("\"command\": \"opencode\"");
    expect(commandJson.placeholder).toContain("\"args\": [\"acp\"]");
    expect(within(form).queryByLabelText("Command")).toBeNull();
    expect(within(form).queryByLabelText("Args")).toBeNull();
    expect(within(form).queryByLabelText("Env")).toBeNull();
    expect((within(form).getByLabelText("CWD") as HTMLInputElement).value).toBe("");
    expect(within(form).getByLabelText("Label").closest("label")?.textContent).toContain("Optional");
    expect(within(form).getByLabelText("Description").closest("label")?.textContent).toContain("Optional");
    expect(within(form).getByText(/Resolves to \/tmp\/project$/)).toBeTruthy();
    expect(within(form).queryByLabelText("Enabled")).toBeNull();
    expect(within(form).queryByText("Entrypoints")).toBeNull();
    fireEvent.change(within(form).getByLabelText("CWD"), { target: { value: "agents" } });
    expect(within(form).getByText(/Resolves to \/tmp\/project\/agents$/)).toBeTruthy();
    fireEvent.change(within(form).getByLabelText("CWD"), { target: { value: "/opt/acp" } });
    expect(within(form).getByText(/Resolves to \/opt\/acp$/)).toBeTruthy();
    fireEvent.change(within(form).getByLabelText("CWD"), { target: { value: "" } });
    fireEvent.change(within(form).getByLabelText("ID"), { target: { value: "local-acp" } });
    fireEvent.change(commandJson, { target: { value: "{" } });
    expect(within(form).getByText("Command JSON must be valid JSON.")).toBeTruthy();
    expect((within(form).getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.change(commandJson, {
      target: {
        value: JSON.stringify({
          command: "local-agent",
          args: ["acp", "--stdio"],
          env: { ACP_LOG: "debug" }
        }, null, 2)
      }
    });
    fireEvent.click(within(form).getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "backend/write",
        params: expect.objectContaining({
          id: "local-acp",
          target: "profile",
          label: null,
          description: null,
          command: "local-agent",
          args: ["acp", "--stdio"],
          env: { ACP_LOG: "debug" },
          cwd: "invocation",
          entrypoints: ["peer", "subagent"],
          clientCapabilities: ["fs.read", "fs.write", "terminal"]
        })
      });
    });
  });

  it("updates Profile ACP backend enabled and entrypoints from Settings Agents rows", async () => {
    gatewayMock.backendRecords = [
      {
        id: "opencode",
        kind: "acp",
        enabled: true,
        label: "OpenCode",
        description: null,
        command: "opencode",
        args: ["acp"],
        cwd: "invocation",
        entrypoints: ["peer", "subagent"],
        clientCapabilities: ["fs.read", "fs.write", "terminal"],
        mcpServers: [],
        envKeys: [],
        sourceTargets: ["profile"],
        diagnostics: []
      }
    ];

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Settings" }));
    const settingsRegion = await screen.findByRole("region", { name: "Settings" });
    fireEvent.click(within(settingsRegion).getByRole("button", { name: "Agents" }));
    const agentsPanel = await within(settingsRegion).findByRole("region", { name: "Agents" });

    fireEvent.click(await within(agentsPanel).findByRole("switch", { name: "Disable opencode" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "backend/write",
        params: expect.objectContaining({
          id: "opencode",
          target: "profile",
          enabled: false,
          entrypoints: ["peer", "subagent"]
        })
      });
    });

    fireEvent.click(await within(agentsPanel).findByLabelText("opencode peer entrypoint"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "backend/write",
        params: expect.objectContaining({
          id: "opencode",
          target: "profile",
          enabled: false,
          entrypoints: ["subagent"]
        })
      });
    });
  });

  it("routes commands clicked inside the overlay without submitting transcript turns", async () => {
    gatewayMock.commandList = [
      commandItem("status", "inspect", "status")
    ];
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: command === "/status" ? "inspect" : "navigate",
      feedbackAnchor: command === "/status" ? "status" : "commandsPanel",
      action: { type: "showPanel", panel: command === "/status" ? "status" : "commands" }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/help" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByRole("region", { name: "Commands overlay" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /\/status/ }));

    expect(await screen.findByRole("region", { name: "Workspace status" })).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Commands overlay" })).toBeNull();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
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

  it("reveals workspace status and shows local feedback for composer-entered status commands", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "inspect",
      feedbackAnchor: "status",
      action: { type: "showPanel", panel: "status" }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/status" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByRole("region", { name: "Workspace status" })).toBeTruthy();
    expect(await screen.findByText("Opened Status.")).toBeTruthy();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
  });

  it("auto-dismisses successful inspect command feedback", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "inspect",
      feedbackAnchor: "status",
      action: { type: "showPanel", panel: "status" }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/context" } });
    vi.useFakeTimers();
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.getByText("Opened Status.")).toBeTruthy();
    await act(async () => {
      vi.advanceTimersByTime(2_999);
    });
    expect(screen.getByText("Opened Status.")).toBeTruthy();
    await act(async () => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.queryByText("Opened Status.")).toBeNull();
  });

  it("shows sandbox status feedback near the composer while revealing workspace status", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      message: "sandbox: workspace-write",
      presentationKind: "inspect",
      feedbackAnchor: "status",
      action: null
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/sandbox" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByRole("region", { name: "Workspace status" })).toBeTruthy();
    expect(await screen.findByText("sandbox: workspace-write")).toBeTruthy();
    fireEvent.mouseDown(document.body);
    await waitFor(() => {
      expect(screen.queryByText("sandbox: workspace-write")).toBeNull();
    });
    expect(screen.getByRole("region", { name: "Workspace status" })).toBeTruthy();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
  });

  it("reveals collapsed History for composer-entered sessions commands", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "navigate",
      feedbackAnchor: "commandsPanel",
      action: { type: "showPanel", panel: "history" }
    });

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Collapse left sidebar" }));
    expect(screen.queryByText("Sessions")).toBeNull();

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/sessions" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByText("Sessions")).toBeTruthy();
    expect(await screen.findByText("Opened History.")).toBeTruthy();
  });

  it("keeps idle steer errors local to the composer", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "control",
      feedbackAnchor: "composer",
      action: { type: "steerPrompt", text: "hello" }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/steer hello" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByText("/steer is only available while a turn is running.")).toBeTruthy();
    expect(screen.getByRole("region", { name: "Transcript" })).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Commands overlay" })).toBeNull();
    expect(screen.queryByRole("region", { name: "Commands" })).toBeNull();
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
  });

  it("clears transient slash feedback after switching sessions", async () => {
    gatewayMock.sessionSummaries = [
      sessionSummary("thread-1", "First session"),
      sessionSummary("thread-2", "Second session")
    ];
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "inspect",
      feedbackAnchor: "status",
      action: { type: "showPanel", panel: "status" }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/usage" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByText("Opened Status.")).toBeTruthy();
    fireEvent.click(await screen.findByText("Second session"));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/resume",
        params: expect.objectContaining({ threadId: "thread-2" })
      });
    });
    await waitFor(() => {
      expect(screen.queryByText("Opened Status.")).toBeNull();
    });
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

  it("submits queued slash payloads while displaying the original slash line", async () => {
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "control",
      feedbackAnchor: "composer",
      action: { type: "queuePrompt", text: "hello", displayText: command }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/queue hello" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "turn/start",
        params: expect.objectContaining({
          input: [{ type: "text", text: "hello" }]
        })
      });
    });
    expect(gatewayMock.optimisticLog).toContain("/queue hello");
  });

  it("shows a bounded export error instead of opening downloads without a host endpoint", async () => {
    gatewayMock.endpoint = null;
    gatewayMock.commandExecute = (command: string) => ({
      accepted: true,
      command,
      known: true,
      presentationKind: "export",
      feedbackAnchor: "trigger",
      action: { type: "downloadSession", kind: "export", threadId: "thread-1" }
    });

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...");
    fireEvent.change(textarea, { target: { value: "/export" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByText("Export is not available for this session.")).toBeTruthy();
    expect(gatewayMock.openDownloadLog).toEqual([]);
  });

  it("routes session undo and redo without submitting transcript turns", async () => {
    gatewayMock.commandExecute = (command: string) => {
      if (command === "/undo") {
        return {
          accepted: true,
          command,
          known: true,
          presentationKind: "control",
          feedbackAnchor: "composer",
          message: "undone 2 messages; prompt restored",
          action: {
            type: "sessionUndo",
            threadId: "thread-1",
            prompt: "second prompt",
            revertedMessages: 2
          }
        };
      }
      return {
        accepted: true,
        command,
        known: true,
        presentationKind: "control",
        feedbackAnchor: "composer",
        message: "redone 2 messages; complete",
        action: {
          type: "sessionRedo",
          threadId: "thread-1",
          restoredMessages: 2,
          complete: true
        }
      };
    };

    render(<App />);

    const textarea = await screen.findByPlaceholderText("Ask Psychevo...") as HTMLTextAreaElement;
    const beforeUndo = gatewayMock.requestLog.length;
    fireEvent.change(textarea, { target: { value: "/undo" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(textarea.value).toBe("second prompt");
    });
    expect(await screen.findByText("undone 2 messages; prompt restored")).toBeTruthy();
    const undoMethods = gatewayMock.requestLog.slice(beforeUndo).map((entry) => entry.method);
    expect(undoMethods).toContain("thread/read");
    expect(undoMethods).toContain("thread/list");
    expect(undoMethods).toContain("workspace/diff");
    expect(undoMethods).toContain("observability/read");
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);

    const beforeRedo = gatewayMock.requestLog.length;
    fireEvent.change(textarea, { target: { value: "/redo" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(textarea.value).toBe("");
    });
    expect(await screen.findByText("redone 2 messages; complete")).toBeTruthy();
    const redoMethods = gatewayMock.requestLog.slice(beforeRedo).map((entry) => entry.method);
    expect(redoMethods).toContain("thread/read");
    expect(redoMethods).toContain("thread/list");
    expect(redoMethods).toContain("workspace/diff");
    expect(redoMethods).toContain("observability/read");
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
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

    expect(await screen.findByRole("region", { name: "Review" })).toBeTruthy();
    expect(screen.queryByLabelText("Inline preview")).toBeNull();
    expect(screen.getAllByText("src/main.rs").length).toBeGreaterThan(0);
    expect(screen.getAllByText("M↓").length).toBeGreaterThan(0);
    expect(screen.getAllByText("+1").length).toBeGreaterThan(0);
    expect(screen.getAllByText("-1").length).toBeGreaterThan(0);
    expect(screen.queryByText("diff --git a/src/main.rs b/src/main.rs")).toBeNull();

    fireEvent.change(textarea, { target: { value: "/export" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(gatewayMock.openDownloadLog).toContain("http://127.0.0.1/download");
    });
    expect(await screen.findByText("Export download opened.")).toBeTruthy();
  });
});
