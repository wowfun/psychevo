import { cleanup } from "@testing-library/react";
import { afterEach, vi } from "vitest";
import { gatewayMock } from "./gateway-mock";

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
  gatewayMock.slashSettings = {
    scope: "global",
    cwd: gatewayMock.scope.cwd,
    leaderKey: "ctrl+x",
    leaderTimeoutMs: 2000,
    aliases: [],
    keybinds: [],
    diagnostics: []
  };
  gatewayMock.endpoint = { wsUrl: "ws://127.0.0.1/test", baseUrl: "http://127.0.0.1/test" };
  gatewayMock.model = "xiaomi/xiaomi-token-high";
  gatewayMock.modelVariant = "none";
  gatewayMock.modelOverride = null;
  gatewayMock.modelVariantOverride = null;
  gatewayMock.modelError = null;
  gatewayMock.modelStatus = "resolved";
  gatewayMock.modelSettings = {
    scope: "global",
    cwd: gatewayMock.scope.cwd,
    defaultModel: "xiaomi/xiaomi-token-high",
    defaultReasoningEffort: null,
    providers: [
      {
        id: "opencode-zen",
        label: "OpenCode Zen",
        builtIn: true,
        configured: false,
        baseUrl: "https://opencode.ai/zen/v1",
        apiKeyEnv: "OPENCODE_ZEN_API_KEY",
        credentialStatus: "notRequired",
        noAuth: true,
        canFetchModels: true,
        unavailableReason: null
      },
      {
        id: "xiaomi-token-plan",
        label: "Xiaomi Token Plan",
        builtIn: true,
        configured: true,
        baseUrl: "https://token-plan-cn.xiaomimimo.com/v1",
        apiKeyEnv: "XIAOMI_TOKEN_PLAN_API_KEY",
        credentialStatus: "present",
        noAuth: false,
        canFetchModels: true,
        unavailableReason: null
      },
      {
        id: "custom",
        label: "Custom",
        builtIn: false,
        configured: false,
        baseUrl: null,
        apiKeyEnv: null,
        credentialStatus: "missing",
        noAuth: false,
        canFetchModels: false,
        unavailableReason: "requires provider setup"
      }
    ],
    auxiliary: [
      { task: "title_generation", label: "Title generation", provider: "auto", model: "", reasoningEffort: null, effectiveModel: null },
      { task: "compression", label: "Context compression", provider: "xiaomi-token-plan", model: "mimo-v2.5-pro", reasoningEffort: null, effectiveModel: "xiaomi-token-plan/mimo-v2.5-pro" }
    ],
    modelOptions: [
      { provider: "xiaomi-token-plan", id: "mimo-v2.5-pro", value: "xiaomi-token-plan/mimo-v2.5-pro", label: null, providerLabel: "Xiaomi Token Plan", free: false, contextLimit: 1048576, reasoningSupported: true, reasoningEfforts: ["none", "low", "medium", "high"] },
      { provider: "xiaomi", id: "xiaomi-token-high", value: "xiaomi/xiaomi-token-high", label: null, providerLabel: "Xiaomi", free: false, contextLimit: null, reasoningSupported: true, reasoningEfforts: ["none", "low", "medium", "high"] }
    ]
  };
  gatewayMock.modelCatalog = [
    { provider: "opencode-zen", id: "mimo-v2.5-free", value: "opencode-zen/mimo-v2.5-free", label: null, providerLabel: "OpenCode Zen", free: true, contextLimit: null, reasoningSupported: true, reasoningEfforts: ["none", "low", "medium", "high"] },
    { provider: "opencode-zen", id: "deepseek-v4-pro", value: "opencode-zen/deepseek-v4-pro", label: null, providerLabel: "OpenCode Zen", free: false, contextLimit: null, reasoningSupported: true, reasoningEfforts: ["none", "low", "medium", "high"] }
  ];
  gatewayMock.observabilityRead = null;
  gatewayMock.usageRead = null;
  gatewayMock.wechatQrPoll = null;
  gatewayMock.permissionRespond = () => ({ accepted: true });
  gatewayMock.clarifyRespond = () => ({ accepted: true });
  gatewayMock.openDownloadLog.length = 0;
  gatewayMock.optimisticLog.length = 0;
  gatewayMock.projectBranch = "main";
  gatewayMock.requestLog.length = 0;
  gatewayMock.subscribers = [];
  gatewayMock.archivedSessionSummaries = [];
  gatewayMock.browserWorkspaces = null;
  gatewayMock.agentRecords = [];
  gatewayMock.backendRecords = [];
  gatewayMock.automationRecords = [];
  gatewayMock.channelRecords = [
    {
      id: "release",
      channel: "telegram",
      domain: null,
      enabled: true,
      label: "Release Bot",
      transport: "polling",
      cwd: null,
      model: "xiaomi/xiaomi-token-high",
      permissionMode: null,
      requireMention: true,
      credential: { env: "TELEGRAM_BOT_TOKEN", status: "present" },
      account: null,
      baseUrl: null,
      appId: null,
      allowlist: { users: ["12345"], groups: [], status: "present" },
      runtimeStatus: "ready",
      runner: {
        state: "running",
        reason: "polling_empty",
        lastPollAtMs: Date.now(),
        lastHealthyPollAtMs: Date.now(),
        lastInboundAtMs: null,
        lastOutboundAtMs: null,
        lastIlinkErrcode: null,
        lastError: null
      }
    },
    {
      id: "ops-lark",
      channel: "lark",
      domain: "lark",
      enabled: false,
      label: "Ops Lark",
      transport: "long_connection",
      cwd: "/tmp/project",
      model: null,
      permissionMode: "default",
      requireMention: true,
      credential: { env: "LARK_APP_SECRET", status: "missing" },
      account: null,
      baseUrl: null,
      appId: { env: "LARK_APP_ID", status: "missing" },
      allowlist: { users: [], groups: [], status: "missing" },
      runtimeStatus: "disabled",
      runner: {
        state: "stopped",
        reason: null,
        lastPollAtMs: null,
        lastHealthyPollAtMs: null,
        lastInboundAtMs: null,
        lastOutboundAtMs: null,
        lastIlinkErrcode: null,
        lastError: null
      }
    }
  ];
  gatewayMock.sessionSummaries = [];
  gatewayMock.snapshot.thread = {
    id: "thread-1",
    backend: { kind: "psychevo" as const, nativeId: "thread-1" },
    sourceKey: "source-key"
  };
  gatewayMock.snapshot.pendingPermissions = [];
  gatewayMock.snapshot.pendingClarifies = [];
  gatewayMock.snapshot.entries = [];
  gatewayMock.snapshot.activity = { running: false, activeTurnId: null, queuedTurns: 0 };
  gatewayMock.workspaceDiffResult = {
    isGitRepo: true,
    files: [],
    unifiedDiff: "",
    truncation: { truncated: false, maxBytes: 0, maxLines: 0, omittedBytes: 0, omittedLines: 0 },
    selectedPath: null
  };
  gatewayMock.workspaceFileReadResults.clear();
  gatewayMock.workspaceFilesResult = {
    root: gatewayMock.scope.cwd,
    entries: [],
    truncated: false
  };
  gatewayMock.workspaceChangesResult = { groups: [] };
  window.localStorage.clear();
});
