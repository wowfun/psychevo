import { vi } from "vitest";

const gatewayMock = vi.hoisted(() => {
  const scope = {
    cwd: "/tmp/project",
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
    pendingActions: [] as Array<Record<string, unknown>>
  };
  function mergeMockModelOptions(
    current: Array<Record<string, unknown>>,
    next: Array<Record<string, unknown>>
  ): Array<Record<string, unknown>> {
    const merged = new Map<string, Record<string, unknown>>();
    for (const option of current) {
      const value = option.value;
      if (typeof value === "string") {
        merged.set(value, option);
      }
    }
    for (const option of next) {
      const value = option.value;
      if (typeof value === "string") {
        merged.set(value, option);
      }
    }
    return [...merged.values()];
  }
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
    slashSettings: {
      scope: "global",
      cwd: scope.cwd,
      leaderKey: "ctrl+x",
      leaderTimeoutMs: 2000,
      aliases: [] as Array<Record<string, unknown>>,
      keybinds: [] as Array<Record<string, unknown>>,
      diagnostics: [] as string[]
    } as Record<string, unknown>,
    endpoint: { wsUrl: "ws://127.0.0.1/test", baseUrl: "http://127.0.0.1/test" } as { wsUrl: string; baseUrl: string } | null,
    observabilityRead: null as null | ((params: unknown) => unknown | Promise<unknown>),
    usageRead: null as null | ((params: unknown) => unknown | Promise<unknown>),
    wechatQrPoll: null as null | ((params: unknown) => unknown | Promise<unknown>),
    permissionRespond: (() => ({ accepted: true })) as (params: unknown) => unknown | Promise<unknown>,
    clarifyRespond: (() => ({ accepted: true })) as (params: unknown) => unknown | Promise<unknown>,
    clipboardWriteLog: [] as string[],
    openDownloadLog: [] as string[],
    optimisticLog: [] as string[],
    projectBranch: "main" as string | null,
    requestLog: [] as Array<{ method: string; params: unknown }>,
    xtermTerminalOptions: [] as Array<Record<string, unknown>>,
    subscribers: [] as Array<(notification: { method: string; params?: unknown }) => void>,
    archivedSessionSummaries: [] as Array<Record<string, unknown>>,
    browserWorkspaces: null as Array<Record<string, unknown>> | null,
    agentRecords: [] as Array<Record<string, unknown>>,
    shadowedAgentRecords: [] as Array<Record<string, unknown>>,
    disabledAgentRecords: [] as Array<Record<string, unknown>>,
    backendRecords: [] as Array<Record<string, unknown>>,
    skillRecords: [] as Array<Record<string, unknown>>,
    channelRecords: [
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
    ] as Array<Record<string, unknown>>,
    scope,
    sessionSummaries: [] as Array<Record<string, unknown>>,
    automationRecords: [] as Array<Record<string, unknown>>,
    model: "xiaomi/xiaomi-token-high" as string | null,
    modelVariant: "none",
    modelOverride: null as string | null,
    modelVariantOverride: null as string | null,
    recentModels: [] as string[],
    modelError: null as string | null,
    modelStatus: "resolved" as "resolved" | "unconfigured" | "error",
    modelSettings: {
      scope: "global",
      cwd: scope.cwd,
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
    } as Record<string, unknown>,
    modelCatalog: [
      { provider: "opencode-zen", id: "mimo-v2.5-free", value: "opencode-zen/mimo-v2.5-free", name: null, providerName: "OpenCode Zen", free: true, limit: { context: null, output: null }, reasoningSupported: true, reasoningEfforts: ["none", "low", "medium", "high"] },
      { provider: "opencode-zen", id: "deepseek-v4-pro", value: "opencode-zen/deepseek-v4-pro", name: null, providerName: "OpenCode Zen", free: false, limit: { context: null, output: null }, reasoningSupported: true, reasoningEfforts: ["none", "low", "medium", "high"] }
    ] as Array<Record<string, unknown>>,
    mergeModelOptions: mergeMockModelOptions,
    modelSettingsResult() {
      return {
        ...gatewayMock.modelSettings,
        providers: [...(gatewayMock.modelSettings.providers as Array<Record<string, unknown>>)],
        auxiliary: [...(gatewayMock.modelSettings.auxiliary as Array<Record<string, unknown>>)],
        modelOptions: [...(gatewayMock.modelSettings.modelOptions as Array<Record<string, unknown>>)]
      };
    },
    slashSettingsResult() {
      return {
        ...gatewayMock.slashSettings,
        aliases: [...(gatewayMock.slashSettings.aliases as Array<Record<string, unknown>>)],
        keybinds: [...(gatewayMock.slashSettings.keybinds as Array<Record<string, unknown>>)]
      };
    },
    settingsResult(agent: string | null) {
      const fetchedCatalogOptions = (gatewayMock.modelSettings.modelOptions as Array<Record<string, unknown>>)
        .filter((option) => option.provider === "opencode-zen");
      const modelDetails = mergeMockModelOptions([
        {
          provider: "xiaomi",
          id: "xiaomi-token-high",
          value: "xiaomi/xiaomi-token-high",
          name: null,
          providerName: "Xiaomi",
          free: false,
          limit: { context: null, output: null },
          reasoningSupported: true,
          reasoningEfforts: ["none", "low", "medium", "high"]
        },
        {
          provider: "openai",
          id: "gpt-4o",
          value: "openai/gpt-4o",
          name: null,
          providerName: "OpenAI",
          free: false,
          limit: { context: null, output: null },
          reasoningSupported: false,
          reasoningEfforts: ["none"]
        },
        {
          provider: "xiaomi",
          id: "xiaomi-token-low",
          value: "xiaomi/xiaomi-token-low",
          name: null,
          providerName: "Xiaomi",
          free: false,
          limit: { context: null, output: null },
          reasoningSupported: true,
          reasoningEfforts: ["none", "low", "medium"]
        }
      ], fetchedCatalogOptions);
      return {
        cwd: scope.cwd,
        project: {
          path: scope.cwd,
          displayPath: "/tmp/project",
          branch: gatewayMock.projectBranch
        },
        channels: { channels: gatewayMock.channelRecords },
        memoryResources: { mode: "status_only", available: true },
        secrets: { frontendPersistence: "disabled" },
        controls: {
          permissionMode: "default",
          mode: "default",
          agent,
          model: gatewayMock.modelOverride ?? gatewayMock.model,
          modelStatus: gatewayMock.modelStatus,
          modelError: gatewayMock.modelError,
          variant: gatewayMock.modelVariantOverride ?? gatewayMock.modelVariant,
          permissionModeOptions: ["default", "acceptEdits", "dontAsk", "bypassPermissions"],
          modeOptions: ["default", "plan"],
          modelOptions: modelDetails
            .map((option) => option.value)
            .filter((value): value is string => typeof value === "string"),
          modelDetails,
          recentModels: gatewayMock.recentModels,
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
      root: scope.cwd,
      entries: [] as Array<{ path: string; name: string; kind: "file" | "directory"; depth: number }>,
      truncated: false
    },
    workspaceChangesResult: {
      groups: [] as Array<unknown>
    }
  };
});


export { gatewayMock };
