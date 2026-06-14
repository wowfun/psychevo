// @vitest-environment jsdom

import { cleanup, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, vi } from "vitest";

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
    pendingPermissions: [] as Array<Record<string, unknown>>,
    pendingClarifies: [] as Array<Record<string, unknown>>
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
    completionResult: { items: [], replacement: null } as Record<string, unknown>,
    commandList: [] as Array<Record<string, unknown>>,
    endpoint: { wsUrl: "ws://127.0.0.1/test", baseUrl: "http://127.0.0.1/test" } as { wsUrl: string; baseUrl: string } | null,
    observabilityRead: null as null | ((params: unknown) => unknown | Promise<unknown>),
    permissionRespond: (() => ({ accepted: true })) as (params: unknown) => unknown | Promise<unknown>,
    openDownloadLog: [] as string[],
    optimisticLog: [] as string[],
    projectBranch: "main" as string | null,
    requestLog: [] as Array<{ method: string; params: unknown }>,
    subscribers: [] as Array<(notification: { method: string; params?: unknown }) => void>,
    archivedSessionSummaries: [] as Array<Record<string, unknown>>,
    agentRecords: [] as Array<Record<string, unknown>>,
    backendRecords: [] as Array<Record<string, unknown>>,
    scope,
    sessionSummaries: [] as Array<Record<string, unknown>>,
    settingsResult(agent: string | null) {
      return {
        workdir: scope.workdir,
        project: {
          path: scope.workdir,
          displayPath: "/tmp/project",
          branch: gatewayMock.projectBranch
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
          variantOptions: ["none"],
          runtimeRef: "native"
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

export { gatewayMock };

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
      if (method === "thread/browser") {
        return {
          workspaces: [
            {
              workdir: gatewayMock.scope.workdir,
              project: {
                workdir: gatewayMock.scope.workdir,
                label: "project",
                displayPath: "/tmp/project"
              },
              sessions: gatewayMock.sessionSummaries,
              hiddenCount: 0,
              nextCursor: null
            }
          ]
        };
      }
      if (method === "thread/start") {
        return {
          ...gatewayMock.snapshot,
          thread: null,
          entries: [],
          activity: { running: false, activeTurnId: null, queuedTurns: 0 }
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
          agents: gatewayMock.agentRecords.length > 0 ? gatewayMock.agentRecords : [
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
      if (method === "runtime/options") {
        const record = params as { runtimeRef?: string | null; runtimeSessionId?: string | null } | undefined;
        const runtimeRef = record?.runtimeRef?.trim() || "native";
        return {
          runtimeRef,
          runtimeSessionId: record?.runtimeSessionId ?? `${runtimeRef}-session`,
          options: runtimeRef === "native"
            ? [
                {
                  id: "mode",
                  name: "Mode",
                  description: null,
                  category: "mode",
                  type: "select",
                  currentValue: "default",
                  values: [
                    { value: "default", name: "default", description: null },
                    { value: "plan", name: "plan", description: null }
                  ]
                }
              ]
            : [
                {
                  id: "mode",
                  name: "Mode",
                  description: "OpenCode mode",
                  category: "mode",
                  type: "select",
                  currentValue: "build",
                  values: [
                    { value: "build", name: "build", description: null },
                    { value: "plan", name: "plan", description: null },
                    { value: "review", name: "Review", description: null }
                  ]
                }
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
        return gatewayMock.completionResult;
      }
      if (method === "turn/start") {
        return { accepted: true };
      }
      if (method === "permission/respond") {
        return gatewayMock.permissionRespond(params);
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
  gatewayMock.completionResult = { items: [], replacement: null };
  gatewayMock.commandList = [];
  gatewayMock.endpoint = { wsUrl: "ws://127.0.0.1/test", baseUrl: "http://127.0.0.1/test" };
  gatewayMock.observabilityRead = null;
  gatewayMock.permissionRespond = () => ({ accepted: true });
  gatewayMock.openDownloadLog.length = 0;
  gatewayMock.optimisticLog.length = 0;
  gatewayMock.projectBranch = "main";
  gatewayMock.requestLog.length = 0;
  gatewayMock.subscribers = [];
  gatewayMock.archivedSessionSummaries = [];
  gatewayMock.agentRecords = [];
  gatewayMock.backendRecords = [];
  gatewayMock.sessionSummaries = [];
  gatewayMock.snapshot.thread = {
    id: "thread-1",
    backend: { kind: "psychevo" as const, nativeId: "thread-1" },
    sourceKey: "source-key"
  };
  gatewayMock.snapshot.pendingPermissions = [];
  gatewayMock.snapshot.pendingClarifies = [];
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

export function commandItem(
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

export function sessionSummary(id: string, title: string): Record<string, unknown> {
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

export function agentRecord(
  name: string,
  entrypoints: string[],
  backendRef: string | null = null
): Record<string, unknown> {
  return {
    name,
    description: `${name} agent`,
    source: backendRef ? "generated" : "project",
    generated: Boolean(backendRef),
    path: backendRef ? null : `/tmp/project/.psychevo/agents/${name}.md`,
    backend: backendRef ? { ref: backendRef } : null,
    entrypoints
  };
}

export function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

export function observabilityResult(threadId: string | null, peer = false): Record<string, unknown> {
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

export function workspaceDiffAction() {
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

export async function openAgentRuntimePopover() {
  const existing = screen.queryByRole("dialog", { name: "Agent and runtime" });
  if (existing) {
    return existing;
  }
  fireEvent.click(await screen.findByRole("button", { name: "Agent" }));
  return await screen.findByRole("dialog", { name: "Agent and runtime" });
}

export async function selectMainAgent(value: string) {
  const popover = await openAgentRuntimePopover();
  const label = value || "Default Agent";
  fireEvent.click(within(popover).getByRole("radio", { name: label }));
  return popover;
}

export async function selectRuntime(value: string) {
  const popover = await openAgentRuntimePopover();
  const label = value === "native"
    ? "Native Runtime"
    : value === "opencode"
      ? "OpenCode"
      : value;
  fireEvent.click(within(popover).getByRole("radio", { name: label }));
  return popover;
}
