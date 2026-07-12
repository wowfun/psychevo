import { cleanup } from "@testing-library/react";
import { afterEach, vi } from "vitest";
import { gatewayMock } from "./gateway-mock";

const localStorageItems = new Map<string, string>();

function defaultSkillRecords(): Array<Record<string, unknown>> {
  return [
    {
      id: "/tmp/profile/skills/review/SKILL.md",
      name: "review",
      description: "Review code changes",
      location: "/tmp/profile/skills/review/SKILL.md",
      skill_dir: "/tmp/profile/skills/review",
      source: "global",
      category: "engineering",
      enabled: true,
      prompt_visible: true,
      readiness_status: "available",
      supported_on_current_platform: true,
      disable_model_invocation: false,
      tags: ["review"],
      missing_required_environment_variables: [],
      missing_credential_files: [],
      required_tools: ["shell"],
      required_toolsets: [],
      issues: []
    },
    {
      id: "/tmp/profile/skills/imagegen/SKILL.md",
      name: "imagegen",
      description: "Generate bitmap assets",
      location: "/tmp/profile/skills/imagegen/SKILL.md",
      skill_dir: "/tmp/profile/skills/imagegen",
      source: "global",
      category: "media",
      enabled: true,
      prompt_visible: true,
      readiness_status: "available",
      supported_on_current_platform: true,
      disable_model_invocation: false,
      tags: [],
      missing_required_environment_variables: [],
      missing_credential_files: [],
      required_tools: [],
      required_toolsets: [],
      issues: []
    },
    {
      id: "/tmp/profile/skills/root-note.md",
      name: "root-note",
      description: "Root markdown skill",
      location: "/tmp/profile/skills/root-note.md",
      skill_dir: "/tmp/profile/skills",
      source: "global",
      category: "notes",
      enabled: true,
      prompt_visible: true,
      readiness_status: "available",
      supported_on_current_platform: true,
      disable_model_invocation: false,
      tags: ["notes"],
      missing_required_environment_variables: [],
      missing_credential_files: [],
      required_tools: [],
      required_toolsets: [],
      issues: []
    },
    {
      id: "/tmp/project/.psychevo/skills/deploy/SKILL.md",
      name: "deploy",
      description: "Deploy with release checks",
      location: "/tmp/project/.psychevo/skills/deploy/SKILL.md",
      skill_dir: "/tmp/project/.psychevo/skills/deploy",
      source: "project",
      category: "operations",
      enabled: false,
      prompt_visible: false,
      readiness_status: "setup_needed",
      supported_on_current_platform: true,
      disable_model_invocation: false,
      tags: ["release"],
      missing_required_environment_variables: ["DEPLOY_TOKEN"],
      missing_credential_files: ["secrets/deploy.json"],
      required_tools: ["shell"],
      required_toolsets: ["web"],
      issues: ["disabled", "missing environment variables: DEPLOY_TOKEN"]
    }
  ];
}

function defaultRuntimeProfileRecords(): Array<Record<string, unknown>> {
  return [
    {
      id: "native",
      runtime: "native",
      enabled: true,
      label: "Psychevo (Native)",
      generated: true,
      configured: false,
      backendRef: null,
      provenance: "Native",
      profileRevision: "1",
      capabilityRevision: "1",
      defaultModel: null,
      defaultMode: "default",
      defaultAgent: null,
      approvalMode: null,
      sandbox: null,
      workspaceRoots: [],
      sourceTargets: [],
      health: { status: "ready", summary: "Built in runtime", commandPath: null, checkedAtMs: null },
      readinessStages: [{ id: "configuration", status: "ready", summary: "Built in", observedAtMs: null }],
      diagnostics: []
    },
    {
      id: "codex",
      runtime: "acp",
      enabled: true,
      label: "Codex (ACP)",
      generated: true,
      configured: false,
      backendRef: "codex",
      provenance: "ACP",
      profileRevision: "2",
      capabilityRevision: "2",
      defaultModel: null,
      defaultMode: "auto-review",
      defaultAgent: null,
      approvalMode: null,
      sandbox: null,
      workspaceRoots: [],
      optionKeys: ["mode"],
      sourceTargets: [],
      health: { status: "warning", summary: "Command not checked", commandPath: null, checkedAtMs: null },
      readinessStages: [{ id: "executable", status: "unchecked", summary: "Not checked", observedAtMs: null }],
      diagnostics: []
    },
    {
      id: "opencode",
      runtime: "acp",
      enabled: true,
      label: "OpenCode (ACP)",
      generated: true,
      configured: false,
      backendRef: "opencode",
      provenance: "ACP",
      profileRevision: "3",
      capabilityRevision: "3",
      defaultModel: null,
      defaultMode: "build",
      defaultAgent: null,
      approvalMode: null,
      sandbox: null,
      workspaceRoots: [],
      optionKeys: ["mode"],
      sourceTargets: [],
      health: { status: "warning", summary: "Command not checked", commandPath: null, checkedAtMs: null },
      readinessStages: [{ id: "executable", status: "unchecked", summary: "Not checked", observedAtMs: null }],
      diagnostics: []
    }
  ];
}

gatewayMock.skillRecords = defaultSkillRecords();

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
  gatewayMock.scope.cwd = "/tmp/project";
  gatewayMock.commandExecute = (command: string) => ({
    accepted: false,
    command,
    known: false,
    action: { type: "passThroughPrompt", text: command }
  });
  gatewayMock.completionResult = { items: [], replacement: null };
  gatewayMock.threadActionRun = null;
  gatewayMock.threadStart = null;
  gatewayMock.turnStart = null;
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
  gatewayMock.acpChannelModelSafe = true;
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
        name: "OpenCode Zen",
        builtIn: true,
        configured: false,
        api: "https://opencode.ai/zen/v1",
        apiKeyEnv: "OPENCODE_ZEN_API_KEY",
        credentialStatus: "notRequired",
        noAuth: true,
        canFetchModels: true,
        unavailableReason: null
      },
      {
        id: "xiaomi-token-plan",
        name: "Xiaomi Token Plan",
        builtIn: true,
        configured: true,
        api: "https://token-plan-cn.xiaomimimo.com/v1",
        apiKeyEnv: "XIAOMI_TOKEN_PLAN_API_KEY",
        credentialStatus: "present",
        noAuth: false,
        canFetchModels: true,
        unavailableReason: null
      },
      {
        id: "custom",
        name: "Custom",
        builtIn: false,
        configured: false,
        api: null,
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
      { provider: "xiaomi-token-plan", id: "mimo-v2.5-pro", value: "xiaomi-token-plan/mimo-v2.5-pro", name: null, providerName: "Xiaomi Token Plan", free: false, limit: { context: 1048576, output: null }, reasoningSupported: true, reasoningEfforts: ["none", "low", "medium", "high"] },
      { provider: "xiaomi", id: "xiaomi-token-high", value: "xiaomi/xiaomi-token-high", name: null, providerName: "Xiaomi", free: false, limit: { context: null, output: null }, reasoningSupported: true, reasoningEfforts: ["none", "low", "medium", "high"] }
    ]
  };
  gatewayMock.modelCatalog = [
    { provider: "opencode-zen", id: "mimo-v2.5-free", value: "opencode-zen/mimo-v2.5-free", name: null, providerName: "OpenCode Zen", free: true, limit: { context: null, output: null }, reasoningSupported: true, reasoningEfforts: ["none", "low", "medium", "high"] },
    { provider: "opencode-zen", id: "deepseek-v4-pro", value: "opencode-zen/deepseek-v4-pro", name: null, providerName: "OpenCode Zen", free: false, limit: { context: null, output: null }, reasoningSupported: true, reasoningEfforts: ["none", "low", "medium", "high"] }
  ];
  gatewayMock.observabilityRead = null;
  gatewayMock.usageRead = null;
  gatewayMock.wechatQrPoll = null;
  gatewayMock.threadInteractionRespond = (params: unknown) => ({
    accepted: true,
    interactionId: (params as { interactionId?: string }).interactionId ?? "interaction-1",
    outcome: "accepted"
  });
  gatewayMock.clipboardWriteLog.length = 0;
  gatewayMock.openDownloadLog.length = 0;
  gatewayMock.optimisticLog.length = 0;
  gatewayMock.projectBranch = "main";
  gatewayMock.requestLog.length = 0;
  gatewayMock.xtermTerminalOptions.length = 0;
  gatewayMock.subscribers = [];
  gatewayMock.archivedSessionSummaries = [];
  gatewayMock.browserWorkspaces = null;
  gatewayMock.agentRecords = [];
  gatewayMock.shadowedAgentRecords = [];
  gatewayMock.disabledAgentRecords = [];
  gatewayMock.teamRecords = [];
  gatewayMock.shadowedTeamRecords = [];
  gatewayMock.disabledTeamRecords = [];
  gatewayMock.teamStatusResult = null;
  gatewayMock.backendRecords = [];
  gatewayMock.runtimeContextRead = null;
  gatewayMock.runtimeProfileRecords = defaultRuntimeProfileRecords();
  gatewayMock.skillRecords = defaultSkillRecords();
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
      runtimeRef: "native",
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
      runtimeRef: "opencode",
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
  gatewayMock.agentRecords = [];
  gatewayMock.shadowedAgentRecords = [];
  gatewayMock.disabledAgentRecords = [];
  gatewayMock.teamRecords = [];
  gatewayMock.shadowedTeamRecords = [];
  gatewayMock.disabledTeamRecords = [];
  gatewayMock.teamStatusResult = null;
  gatewayMock.backendRecords = [];
  gatewayMock.sessionSummaries = [];
  gatewayMock.snapshot.thread = {
    id: "thread-1",
    backend: { kind: "native" as const, sessionHandle: "thread-1", runtimeRef: "native" },
    sourceKey: "source-key"
  };
  gatewayMock.snapshot.pendingActions = [];
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
