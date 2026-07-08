import { vi } from "vitest";
import { gatewayMock } from "./gateway-mock";

function normalizeNullableString(value: string | null | undefined): string | null {
  const normalized = value?.trim() ?? "";
  return normalized ? normalized : null;
}

function normalizePermissionMode(value: string | null | undefined): string | null {
  const normalized = normalizeNullableString(value);
  return normalized && normalized !== "default" ? normalized : null;
}

function normalizeEnvRecord(value: string | null | undefined, fallback: string | null): string | null {
  return normalizeNullableString(value) ?? fallback;
}

function currentEnvRecord(
  channel: Record<string, unknown>,
  field: "credential" | "account" | "baseUrl" | "appId",
  fallback: string | null
): string | null {
  const record = channel[field] as { env?: string | null } | null | undefined;
  return record?.env ?? fallback;
}

function uniqueList(values: string[]): string[] {
  const seen = new Set<string>();
  const next: string[] = [];
  for (const value of values) {
    const item = value.trim();
    if (!item || seen.has(item)) {
      continue;
    }
    seen.add(item);
    next.push(item);
  }
  return next;
}

function defaultAgentRecord(): Record<string, unknown> {
  return managedAgentRecord({
    name: "translate",
    description: "Translate user messages",
    target: "project",
    enabled: true,
    instructions: "Translate user messages."
  });
}

function managedAgentRecord(input: {
  name?: string;
  description?: string;
  target?: "project" | "profile";
  enabled?: boolean;
  instructions?: string;
  backend?: { ref?: string } | null;
  entrypoints?: string[];
  tools?: string[];
  mcpServers?: string[];
  rawMarkdown?: string;
}): Record<string, unknown> {
  const target = input.target ?? "project";
  const name = input.name ?? "agent";
  const description = input.description ?? "";
  const enabled = input.enabled !== false;
  const path = target === "project"
    ? `/tmp/project/.psychevo/agents/${name}.md`
    : `/tmp/profile/agents/${name}.md`;
  const entrypoints = input.entrypoints ?? ["subagent"];
  const instructions = input.instructions ?? "";
  return {
    name,
    description,
    enabled,
    source: target,
    sourceLabel: target === "project" ? "Project" : "Profile",
    generated: false,
    target,
    mutable: true,
    path,
    backend: input.backend ?? null,
    entrypoints,
    tools: input.tools ?? [],
    mcpServers: input.mcpServers ?? [],
    diagnostics: [],
    instructions,
    rawMarkdown: input.rawMarkdown ?? renderMockAgentMarkdown(name, description, enabled, entrypoints, instructions)
  };
}

function renderMockAgentMarkdown(
  name: string,
  description: string,
  enabled: boolean,
  entrypoints: string[],
  instructions: string
): string {
  return [
    "---",
    `name: ${name}`,
    `description: ${description}`,
    `enabled: ${enabled ? "true" : "false"}`,
    `entrypoints: [${entrypoints.join(", ")}]`,
    "---",
    instructions
  ].join("\n");
}

function managedTeamRecord(input: {
  name?: string;
  description?: string;
  target?: "project" | "profile";
  enabled?: boolean;
  leader?: string;
  members?: Array<Record<string, unknown>>;
  maxParallelAgents?: number;
  instructions?: string;
  rawMarkdown?: string;
}): Record<string, unknown> {
  const target = input.target ?? "project";
  const name = input.name ?? "team";
  const description = input.description ?? "";
  const enabled = input.enabled !== false;
  const leader = input.leader ?? "general";
  const members = input.members ?? [{ id: "researcher", agent: "general" }];
  const maxParallelAgents = input.maxParallelAgents ?? 4;
  const instructions = input.instructions ?? "";
  const path = target === "project"
    ? `/tmp/project/.psychevo/teams/${name}.md`
    : `/tmp/profile/teams/${name}.md`;
  return {
    name,
    description,
    enabled,
    source: target,
    sourceLabel: target === "project" ? "Project" : "Profile",
    target,
    mutable: true,
    path,
    leader,
    members,
    maxParallelAgents,
    diagnostics: [],
    instructions,
    rawMarkdown: input.rawMarkdown ?? renderMockTeamMarkdown(name, description, enabled, leader, members, maxParallelAgents, instructions)
  };
}

function renderMockTeamMarkdown(
  name: string,
  description: string,
  enabled: boolean,
  leader: string,
  members: Array<Record<string, unknown>>,
  maxParallelAgents: number,
  instructions: string
): string {
  return [
    "---",
    `name: ${name}`,
    `description: ${description}`,
    `enabled: ${enabled ? "true" : "false"}`,
    `leader: ${leader}`,
    `maxParallelAgents: ${maxParallelAgents}`,
    "members:",
    ...members.map((member) => `  - id: ${member.id}\n    agent: ${member.agent}`),
    "---",
    instructions
  ].join("\n");
}

function findMockAgent(name: string | undefined, target: string | undefined): Record<string, unknown> | null {
  const all = [
    ...gatewayMock.agentRecords,
    ...gatewayMock.shadowedAgentRecords,
    ...gatewayMock.disabledAgentRecords
  ];
  return all.find((agent) => agent.name === name && (!target || agent.target === target)) ?? null;
}

function findMockTeam(name: string | undefined, target: string | undefined): Record<string, unknown> | null {
  const all = [
    ...gatewayMock.teamRecords,
    ...gatewayMock.shadowedTeamRecords,
    ...gatewayMock.disabledTeamRecords
  ];
  return all.find((team) => team.name === name && (!target || team.target === target)) ?? null;
}

function upsertMockAgent(agent: Record<string, unknown>) {
  const sameIdentity = (item: Record<string, unknown>) => item.name === agent.name && item.target === agent.target;
  gatewayMock.agentRecords = gatewayMock.agentRecords.filter((item) => !sameIdentity(item));
  gatewayMock.shadowedAgentRecords = gatewayMock.shadowedAgentRecords.filter((item) => !sameIdentity(item));
  gatewayMock.disabledAgentRecords = gatewayMock.disabledAgentRecords.filter((item) => !sameIdentity(item));
  if (agent.enabled === false) {
    gatewayMock.disabledAgentRecords = [...gatewayMock.disabledAgentRecords, agent];
  } else {
    gatewayMock.agentRecords = [...gatewayMock.agentRecords, agent];
  }
}

function upsertMockTeam(team: Record<string, unknown>) {
  const sameIdentity = (item: Record<string, unknown>) => item.name === team.name && item.target === team.target;
  gatewayMock.teamRecords = gatewayMock.teamRecords.filter((item) => !sameIdentity(item));
  gatewayMock.shadowedTeamRecords = gatewayMock.shadowedTeamRecords.filter((item) => !sameIdentity(item));
  gatewayMock.disabledTeamRecords = gatewayMock.disabledTeamRecords.filter((item) => !sameIdentity(item));
  if (team.enabled === false) {
    gatewayMock.disabledTeamRecords = [...gatewayMock.disabledTeamRecords, team];
  } else {
    gatewayMock.teamRecords = [...gatewayMock.teamRecords, team];
  }
}

function deleteMockAgent(name: string | undefined, target: string | undefined) {
  const keep = (agent: Record<string, unknown>) => !(agent.name === name && (!target || agent.target === target));
  gatewayMock.agentRecords = gatewayMock.agentRecords.filter(keep);
  gatewayMock.shadowedAgentRecords = gatewayMock.shadowedAgentRecords.filter(keep);
  gatewayMock.disabledAgentRecords = gatewayMock.disabledAgentRecords.filter(keep);
}

function deleteMockTeam(name: string | undefined, target: string | undefined) {
  const keep = (team: Record<string, unknown>) => !(team.name === name && (!target || team.target === target));
  gatewayMock.teamRecords = gatewayMock.teamRecords.filter(keep);
  gatewayMock.shadowedTeamRecords = gatewayMock.shadowedTeamRecords.filter(keep);
  gatewayMock.disabledTeamRecords = gatewayMock.disabledTeamRecords.filter(keep);
}

function defaultCredentialEnv(channel: string): string | null {
  switch (channel) {
    case "wechat":
      return "WECHAT_BOT_TOKEN";
    case "telegram":
      return "TELEGRAM_BOT_TOKEN";
    case "feishu":
      return "FEISHU_APP_SECRET";
    case "lark":
      return "LARK_APP_SECRET";
    default:
      return null;
  }
}

function defaultAccountEnv(channel: string): string | null {
  return channel === "wechat" ? "WECHAT_ACCOUNT_ID" : null;
}

function defaultBaseUrlEnv(channel: string): string | null {
  return channel === "wechat" ? "WECHAT_ILINK_BASE_URL" : null;
}

function defaultAppIdEnv(channel: string): string | null {
  switch (channel) {
    case "feishu":
      return "FEISHU_APP_ID";
    case "lark":
      return "LARK_APP_ID";
    default:
      return null;
  }
}


function usageReadResult(): Record<string, unknown> {
  const days = Array.from({ length: 365 }, (_, index) => {
    const date = new Date(Date.UTC(2026, 0, 1 + index));
    const tokens = index % 8 === 0 ? 0 : 100 + (index % 17) * 50;
    return {
      date: date.toISOString().slice(0, 10),
      sessionCount: tokens > 0 ? 1 : 0,
      messageCount: tokens > 0 ? 2 : 0,
      reportedTotalTokens: tokens,
      contextInputTokens: Math.round(tokens * 0.7),
      cacheReadTokens: Math.round(tokens * 0.25),
      cacheWriteTokens: Math.round(tokens * 0.05),
      estimatedCostNanodollars: tokens * 1000,
      costStatus: tokens > 0 ? "estimated" : "unknown",
      estimatedPricingCount: tokens > 0 ? 1 : 0,
      freePricingCount: 0,
      includedPricingCount: 0,
      unknownPricingCount: 0
    };
  });
  const window = (id: string, label: string, reportedTotalTokens: number, cacheReadPercent: number) => ({
    id,
    label,
    sinceMs: id === "all" ? null : 1_767_225_600_000,
    sessionCount: id === "all" ? 8 : 3,
    messageCount: id === "all" ? 42 : 12,
    assistantMessageCount: id === "all" ? 20 : 6,
    contextInputTokens: Math.round(reportedTotalTokens * 0.7),
    billableInputTokens: Math.round(reportedTotalTokens * 0.45),
    billableOutputTokens: Math.round(reportedTotalTokens * 0.25),
    reasoningTokens: Math.round(reportedTotalTokens * 0.04),
    cacheReadTokens: Math.round(reportedTotalTokens * 0.25),
    cacheWriteTokens: Math.round(reportedTotalTokens * 0.02),
    reportedTotalTokens,
    estimatedCostNanodollars: reportedTotalTokens * 1000,
    costStatus: "estimated",
    estimatedPricingCount: 6,
    freePricingCount: 0,
    includedPricingCount: 0,
    unknownPricingCount: id === "all" ? 1 : 0,
    cacheReadPercent
  });
  return {
    generatedAtMs: 1_798_650_000_000,
    windows: [
      window("all", "All time", 125_000, 35),
      window("30d", "Last 30 days", 38_000, 42),
      window("7d", "Last 7 days", 9_200, 47)
    ],
    activity: {
      startDate: days[0]?.date ?? "",
      endDate: days.at(-1)?.date ?? "",
      days
    }
  };
}

vi.mock("@psychevo/client", async () => {
  const actual = await vi.importActual<typeof import("@psychevo/client")>("@psychevo/client");

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
          cwd: gatewayMock.scope.cwd,
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
          workspaces: gatewayMock.browserWorkspaces ?? [
            {
              cwd: gatewayMock.scope.cwd,
              project: {
                cwd: gatewayMock.scope.cwd,
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
      if (method === "model/settings/read") {
        return gatewayMock.modelSettingsResult();
      }
      if (method === "slash/settings/read") {
        return gatewayMock.slashSettingsResult();
      }
      if (method === "slash/settings/update") {
        const record = params as {
          leaderKey?: string | null;
          leaderTimeoutMs?: number | null;
          aliases?: Array<Record<string, unknown>>;
          keybinds?: Array<Record<string, unknown>>;
          cwd?: string | null;
        };
        gatewayMock.slashSettings = {
          ...gatewayMock.slashSettings,
          cwd: record.cwd ?? gatewayMock.scope.cwd,
          leaderKey: record.leaderKey ?? gatewayMock.slashSettings.leaderKey,
          leaderTimeoutMs: record.leaderTimeoutMs ?? gatewayMock.slashSettings.leaderTimeoutMs,
          aliases: (record.aliases ?? []).map((entry) => ({
            alias: entry.alias,
            target: entry.target,
            targetSummary: entry.targetSummary ?? "show local status"
          })),
          keybinds: (record.keybinds ?? []).map((entry) => ({
            shortcut: entry.shortcut,
            target: entry.target,
            targetSummary: entry.targetSummary ?? "show local status"
          }))
        };
        return gatewayMock.slashSettingsResult();
      }
      if (method === "model/provider/catalog") {
        const record = params as { providerId?: string };
        gatewayMock.modelSettings = {
          ...gatewayMock.modelSettings,
          modelOptions: gatewayMock.mergeModelOptions(
            gatewayMock.modelSettings.modelOptions as Array<Record<string, unknown>>,
            gatewayMock.modelCatalog
          )
        };
        return {
          providerId: record?.providerId ?? "opencode-zen",
          models: gatewayMock.modelCatalog
        };
      }
      if (method === "model/provider/save") {
        const record = params as {
          providerId: string;
          name?: string | null;
          api: string;
          noAuth?: boolean;
          model?: {
            id: string;
            name?: string | null;
            limit?: { context?: number | null; output?: number | null };
          } | null;
        };
        const providerName = record.name ?? record.providerId;
        const existingProviders = gatewayMock.modelSettings.providers as Array<Record<string, unknown>>;
        const nextProvider = {
          id: record.providerId,
          name: providerName,
          builtIn: existingProviders.some((provider) => provider.id === record.providerId && provider.builtIn === true),
          configured: true,
          api: record.api,
          apiKeyEnv: record.noAuth ? null : `${record.providerId.toUpperCase().replace(/[^A-Z0-9]+/g, "_").replace(/^_+|_+$/g, "")}_API_KEY`,
          credentialStatus: record.noAuth ? "notRequired" : "present",
          noAuth: Boolean(record.noAuth),
          canFetchModels: true,
          unavailableReason: null
        };
        const updatedProviders = existingProviders.some((provider) => provider.id === record.providerId)
          ? existingProviders.map((provider) => (
            provider.id === record.providerId ? { ...provider, ...nextProvider } : provider
          ))
          : [...existingProviders.filter((provider) => provider.id !== "custom"), nextProvider, ...existingProviders.filter((provider) => provider.id === "custom")];
        gatewayMock.modelSettings = {
          ...gatewayMock.modelSettings,
          providers: updatedProviders,
          modelOptions: record.model?.id
            ? gatewayMock.mergeModelOptions(
                gatewayMock.modelSettings.modelOptions as Array<Record<string, unknown>>,
                [{
                  provider: record.providerId,
                  id: record.model.id,
                  value: `${record.providerId}/${record.model.id}`,
                  name: record.model.name ?? null,
                  providerName,
                  free: Boolean(record.noAuth && record.providerId === "opencode-zen"),
                  limit: {
                    context: record.model.limit?.context ?? null,
                    output: record.model.limit?.output ?? null
                  },
                  reasoningSupported: true,
                  reasoningEfforts: ["none", "low", "medium", "high"]
                }]
              )
            : gatewayMock.modelSettings.modelOptions
        };
        return gatewayMock.modelSettingsResult();
      }
      if (method === "model/state/set") {
        const record = params as {
          threadId?: string | null;
          cwd?: string | null;
          model: string;
          reasoningEffort?: string | null;
        };
        gatewayMock.model = record.model;
        gatewayMock.modelVariant = record.reasoningEffort && record.reasoningEffort !== "none"
          ? record.reasoningEffort
          : "none";
        gatewayMock.recentModels = [
          record.model,
          ...gatewayMock.recentModels.filter((model) => model !== record.model)
        ].slice(0, 8);
        return {
          cwd: record.cwd ?? gatewayMock.scope.cwd,
          threadId: record.threadId ?? null,
          model: gatewayMock.model,
          reasoningEffort: gatewayMock.modelVariant === "none" ? null : gatewayMock.modelVariant,
          recentModels: gatewayMock.recentModels
        };
      }
      if (method === "model/assignment/set") {
        const record = params as {
          target: "default" | "auxiliary";
          task?: string | null;
          provider: string;
          model: string;
          reasoningEffort?: string | null;
        };
        if (record.target === "default") {
          const defaultModel = `${record.provider}/${record.model}`;
          gatewayMock.modelSettings = {
            ...gatewayMock.modelSettings,
            defaultModel,
            defaultReasoningEffort: record.reasoningEffort && record.reasoningEffort !== "none"
              ? record.reasoningEffort
              : null
          };
          if (gatewayMock.modelOverride == null) {
            gatewayMock.model = defaultModel;
            gatewayMock.modelVariant = "none";
          }
        } else {
          gatewayMock.modelSettings = {
            ...gatewayMock.modelSettings,
            auxiliary: (gatewayMock.modelSettings.auxiliary as Array<Record<string, unknown>>).map((item) => (
              item.task === record.task
                ? {
                    ...item,
                    provider: record.provider,
                    model: record.model,
                    reasoningEffort: record.reasoningEffort && record.reasoningEffort !== "none"
                      ? record.reasoningEffort
                      : null,
                    effectiveModel: record.model ? `${record.provider}/${record.model}` : null
                  }
                : item
            ))
          };
        }
        return { ok: true, target: record.target, task: record.task ?? null, provider: record.provider, model: record.model };
      }
      if (method === "agent/list") {
        return {
          agents: gatewayMock.agentRecords.length > 0 ? gatewayMock.agentRecords : [defaultAgentRecord()],
          shadowedAgents: gatewayMock.shadowedAgentRecords,
          disabledAgents: gatewayMock.disabledAgentRecords,
          diagnostics: []
        };
      }
      if (method === "agent/read") {
        const record = params as { name?: string; target?: "project" | "profile" | null };
        const agent = findMockAgent(record.name, record.target ?? undefined) ?? defaultAgentRecord();
        return {
          agent,
          instructions: typeof agent.instructions === "string" ? agent.instructions : "",
          rawMarkdown: typeof agent.rawMarkdown === "string" ? agent.rawMarkdown : ""
        };
      }
      if (method === "agent/write") {
        const record = params as {
          name?: string;
          description?: string;
          target?: "project" | "profile" | null;
          enabled?: boolean | null;
          instructions?: string;
          backend?: { ref?: string } | null;
          entrypoints?: string[];
          tools?: string[];
          mcpServers?: string[];
          rawMarkdown?: string | null;
        };
        if (record.rawMarkdown?.includes("invalid-frontmatter")) {
          throw new Error("agent markdown is invalid");
        }
        const agent = managedAgentRecord({
          name: record.name ?? "agent",
          description: record.description ?? "",
          target: record.target ?? "project",
          enabled: record.enabled !== false,
          instructions: record.instructions ?? "",
          backend: record.backend ?? null,
          entrypoints: record.entrypoints ?? [],
          tools: record.tools ?? [],
          mcpServers: record.mcpServers ?? [],
          ...(record.rawMarkdown ? { rawMarkdown: record.rawMarkdown } : {})
        });
        upsertMockAgent(agent);
        return {
          written: true,
          name: agent.name,
          path: agent.path,
          target: agent.target,
          agent
        };
      }
      if (method === "agent/setEnabled") {
        const record = params as { name?: string; target?: "project" | "profile" | null; enabled?: boolean };
        const existing = findMockAgent(record.name, record.target ?? undefined) ?? managedAgentRecord({ name: record.name ?? "agent", target: record.target ?? "project" });
        const agent: Record<string, unknown> = { ...existing, enabled: record.enabled === true };
        upsertMockAgent(agent);
        return {
          written: true,
          name: agent.name,
          path: agent.path,
          target: agent.target,
          agent
        };
      }
      if (method === "agent/delete") {
        const record = params as { name?: string; target?: "project" | "profile" | null };
        const agent = findMockAgent(record.name, record.target ?? undefined) ?? managedAgentRecord({ name: record.name ?? "agent", target: record.target ?? "project" });
        deleteMockAgent(record.name, record.target ?? undefined);
        return {
          deleted: true,
          name: agent.name,
          path: agent.path,
          target: agent.target
        };
      }
      if (method === "team/list") {
        return {
          teams: gatewayMock.teamRecords,
          shadowedTeams: gatewayMock.shadowedTeamRecords,
          disabledTeams: gatewayMock.disabledTeamRecords,
          diagnostics: []
        };
      }
      if (method === "team/read") {
        const record = params as { name?: string; target?: "project" | "profile" | null };
        const team = findMockTeam(record.name, record.target ?? undefined) ?? managedTeamRecord({ name: record.name ?? "team", target: record.target ?? "project" });
        return {
          team,
          instructions: typeof team.instructions === "string" ? team.instructions : "",
          rawMarkdown: typeof team.rawMarkdown === "string" ? team.rawMarkdown : ""
        };
      }
      if (method === "team/write") {
        const record = params as {
          name?: string;
          description?: string;
          target?: "project" | "profile" | null;
          enabled?: boolean | null;
          leader?: string;
          members?: Array<Record<string, unknown>>;
          maxParallelAgents?: number | null;
          instructions?: string;
          rawMarkdown?: string | null;
        };
        const team = managedTeamRecord({
          name: record.name ?? "team",
          description: record.description ?? "",
          target: record.target ?? "project",
          enabled: record.enabled !== false,
          leader: record.leader ?? "general",
          members: record.members ?? [{ id: "researcher", agent: "general" }],
          maxParallelAgents: record.maxParallelAgents ?? 4,
          instructions: record.instructions ?? "",
          ...(record.rawMarkdown ? { rawMarkdown: record.rawMarkdown } : {})
        });
        upsertMockTeam(team);
        return {
          written: true,
          name: team.name,
          path: team.path,
          target: team.target,
          team
        };
      }
      if (method === "team/setEnabled") {
        const record = params as { name?: string; target?: "project" | "profile" | null; enabled?: boolean };
        const existing = findMockTeam(record.name, record.target ?? undefined) ?? managedTeamRecord({ name: record.name ?? "team", target: record.target ?? "project" });
        const team: Record<string, unknown> = { ...existing, enabled: record.enabled === true };
        upsertMockTeam(team);
        return {
          written: true,
          name: team.name,
          path: team.path,
          target: team.target,
          team
        };
      }
      if (method === "team/delete") {
        const record = params as { name?: string; target?: "project" | "profile" | null };
        const team = findMockTeam(record.name, record.target ?? undefined) ?? managedTeamRecord({ name: record.name ?? "team", target: record.target ?? "project" });
        deleteMockTeam(record.name, record.target ?? undefined);
        return {
          deleted: true,
          name: team.name,
          path: team.path,
          target: team.target
        };
      }
      if (method === "team/status") {
        return gatewayMock.teamStatusResult ?? {
          team: null,
          mission: null,
          agents: [],
          control: {
            spawningPaused: false,
            maxSpawnDepthCap: 3,
            concurrencyCap: null
          }
        };
      }
      if (method === "agent/control") {
        return {
          accepted: true,
          agent: null,
          control: {
            spawningPaused: false,
            maxSpawnDepthCap: 3,
            concurrencyCap: null
          }
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
      if (method === "channel/list") {
        return { channels: gatewayMock.channelRecords };
      }
      if (method === "channel/show") {
        const record = params as { id?: string };
        return {
          channel: gatewayMock.channelRecords.find((channel) => channel.id === record.id) ?? gatewayMock.channelRecords[0]
        };
      }
      if (method === "channel/enable") {
        const record = params as { id?: string; enabled?: boolean };
        gatewayMock.channelRecords = gatewayMock.channelRecords.map((channel) => {
          if (channel.id !== record.id) {
            return channel;
          }
          const enabled = record.enabled === true;
          const blocked = channel.credential &&
            typeof channel.credential === "object" &&
            "status" in channel.credential &&
            channel.credential.status !== "present";
          return {
            ...channel,
            enabled,
            runtimeStatus: enabled ? blocked ? "blocked" : "ready" : "disabled",
            runner: {
              ...(channel.runner as Record<string, unknown> | undefined),
              state: enabled ? blocked ? "blocked" : "running" : "stopped",
              reason: enabled ? blocked ? "blocked_allowlist" : "polling_empty" : null,
              lastError: blocked ? "credential env is missing" : null
            }
          };
        });
        return {
          channel: gatewayMock.channelRecords.find((channel) => channel.id === record.id) ?? gatewayMock.channelRecords[0]
        };
      }
      if (method === "channel/update") {
        const record = params as {
          id?: string;
          label?: string | null;
          enabled?: boolean | null;
          cwd?: string | null;
          model?: string | null;
          permissionMode?: string | null;
          requireMention?: boolean | null;
          credentialEnv?: string | null;
          accountEnv?: string | null;
          baseUrlEnv?: string | null;
          appIdEnv?: string | null;
          allowUsers?: string[] | null;
          allowGroups?: string[] | null;
        };
        let updated = gatewayMock.channelRecords[0];
        gatewayMock.channelRecords = gatewayMock.channelRecords.map((channel) => {
          if (channel.id !== record.id) {
            return channel;
          }
          const channelName = String(channel.channel ?? "");
          const channelRecord = channel as Record<string, unknown>;
          const credentialEnv = "credentialEnv" in record
            ? normalizeEnvRecord(record.credentialEnv, defaultCredentialEnv(channelName))
            : currentEnvRecord(channelRecord, "credential", defaultCredentialEnv(channelName));
          const accountEnv = "accountEnv" in record
            ? normalizeEnvRecord(record.accountEnv, defaultAccountEnv(channelName))
            : currentEnvRecord(channelRecord, "account", defaultAccountEnv(channelName));
          const baseUrlEnv = "baseUrlEnv" in record
            ? normalizeEnvRecord(record.baseUrlEnv, defaultBaseUrlEnv(channelName))
            : currentEnvRecord(channelRecord, "baseUrl", defaultBaseUrlEnv(channelName));
          const appIdEnv = "appIdEnv" in record
            ? normalizeEnvRecord(record.appIdEnv, defaultAppIdEnv(channelName))
            : currentEnvRecord(channelRecord, "appId", defaultAppIdEnv(channelName));
          const allowUsers = record.allowUsers ? uniqueList(record.allowUsers) : (channel.allowlist as { users?: string[] }).users ?? [];
          const allowGroups = record.allowGroups ? uniqueList(record.allowGroups) : (channel.allowlist as { groups?: string[] }).groups ?? [];
          const enabled = record.enabled ?? Boolean(channel.enabled);
          const missingCredential = !credentialEnv || (channel.credential as { status?: string }).status !== "present";
          const missingAllowlist = allowUsers.length === 0 && allowGroups.length === 0;
          updated = {
            ...channel,
            label: normalizeNullableString(record.label) ?? channel.label,
            enabled,
            cwd: normalizeNullableString(record.cwd),
            model: normalizeNullableString(record.model),
            permissionMode: normalizePermissionMode(record.permissionMode),
            requireMention: record.requireMention ?? channel.requireMention,
            credential: {
              env: credentialEnv,
              status: (channel.credential as { status?: string }).status ?? "missing"
            },
            account: accountEnv ? { env: accountEnv, status: (channel.account as { status?: string } | null)?.status ?? "missing" } : null,
            baseUrl: baseUrlEnv ? { env: baseUrlEnv, status: (channel.baseUrl as { status?: string } | null)?.status ?? "default" } : null,
            appId: appIdEnv ? { env: appIdEnv, status: (channel.appId as { status?: string } | null)?.status ?? "missing" } : null,
            allowlist: {
              users: allowUsers,
              groups: allowGroups,
              status: missingAllowlist ? "missing" : "present"
            },
            runtimeStatus: enabled ? (missingCredential || missingAllowlist ? "blocked" : "ready") : "disabled",
            runner: {
              ...(channel.runner as Record<string, unknown> | undefined),
              state: enabled ? (missingCredential || missingAllowlist ? "blocked" : "running") : "stopped",
              reason: enabled ? (missingCredential || missingAllowlist ? "blocked_allowlist" : "polling_empty") : null
            }
          };
          return updated;
        });
        return { channel: updated };
      }
      if (method === "channel/delete") {
        const record = params as { id?: string };
        gatewayMock.channelRecords = gatewayMock.channelRecords.filter((channel) => channel.id !== record.id);
        return { channels: gatewayMock.channelRecords };
      }
      if (method === "channel/source/list") {
        const record = params as { id?: string };
        return {
          sources: [
            {
              sourceKey: `im.${record.id ?? "channel"}:source-hash`,
              connectionId: record.id ?? "release",
              platform: "telegram",
              domain: "telegram",
              chatType: "dm",
              chatLabel: "ra...3456",
              userLabel: "ra...4321",
              visibleName: "telegram dm chat ra...3456 user ra...4321",
              threadId: "thread-channel-source",
              threadTitle: "Channel lane",
              cwd: "/tmp/project",
              activityStatus: "idle",
              queuedTurns: 0,
              updatedAtMs: Date.now()
            }
          ]
        };
      }
      if (method === "channel/doctor") {
        const record = params as { id?: string | null } | undefined;
        const selected = record?.id
          ? gatewayMock.channelRecords.filter((channel) => channel.id === record.id)
          : gatewayMock.channelRecords;
        return {
          live: false,
          channels: selected.map((channel) => ({
            id: channel.id,
            channel: channel.channel,
            enabled: channel.enabled,
            runtimeStatus: channel.runtimeStatus,
            runner: channel.runner,
            checks: [
              {
                name: "credential",
                status: (channel.credential as { status?: string }).status === "present" ? "ok" : "fail",
                message: "credential env check"
              },
              {
                name: "allowlist",
                status: (channel.allowlist as { status?: string }).status === "present" ? "ok" : "fail",
                message: "allowlist check"
              },
              { name: "live", status: "skipped", message: "local check only" }
            ]
          }))
        };
      }
      if (method === "channel/wechat-qr/start") {
        return {
          sessionId: "wechat-session",
          qrUrl: "data:image/png;base64,wechat-qr-image",
          qrImage: "data:image/png;base64,wechat-qr-image",
          qrSvg: null,
          status: "wait",
          message: "Scan with WeChat to connect this channel.",
          intervalMs: 3000,
          expiresAtMs: Date.now() + 120000
        };
      }
      if (method === "channel/wechat-qr/poll") {
        if (gatewayMock.wechatQrPoll) {
          return gatewayMock.wechatQrPoll(params);
        }
        const channel = {
          id: "wechat",
          channel: "wechat",
          domain: "wechat",
          enabled: true,
          label: "WeChat",
          transport: "polling",
          cwd: null,
          model: null,
          permissionMode: null,
          requireMention: true,
          credential: { env: "WECHAT_BOT_TOKEN", status: "present" },
          account: { env: "WECHAT_ACCOUNT_ID", status: "present" },
          baseUrl: { env: "WECHAT_ILINK_BASE_URL", status: "present" },
          appId: null,
          allowlist: { users: ["wx-user"], groups: [], status: "present" },
          runtimeStatus: "ready",
          runner: {
            state: "running",
            reason: "qr_login_pending",
            lastPollAtMs: null,
            lastHealthyPollAtMs: null,
            lastInboundAtMs: null,
            lastOutboundAtMs: null,
            lastIlinkErrcode: null,
            lastError: null
          }
        };
        gatewayMock.channelRecords = [
          ...gatewayMock.channelRecords.filter((item) => item.id !== "wechat"),
          channel
        ];
        return {
          done: true,
          status: "qr_login_pending",
          message: "WeChat credentials saved. Gateway is starting polling.",
          channel,
          expiresAtMs: null
        };
      }
      if (method === "skill/list") {
        return {
          skills: gatewayMock.skillRecords,
          count: gatewayMock.skillRecords.length,
          diagnostics: [],
          collisions: {}
        };
      }
      if (method === "skill/read") {
        const record = params as { name?: string; path?: string | null } | undefined;
        const skill = gatewayMock.skillRecords.find((item) => item.name === record?.name && (!record?.path || item.location === record.path))
          ?? gatewayMock.skillRecords.find((item) => item.name === record?.name);
        if (!skill) throw new Error("skill not found");
        const body = `# ${skill.name} workflow\n\nFollow the ${skill.name} workflow.\n\n- Confirm prerequisites\n\n\`\`\`sh\npevo ${skill.name}\n\`\`\``;
        const rawMarkdown = typeof skill.rawMarkdown === "string" ? skill.rawMarkdown : null;
        return {
          ...skill,
          success: true,
          content: rawMarkdown ?? body,
          preview_content: rawMarkdown ?? `---\nname: ${skill.name}\ndescription: ${skill.description}\nallowed-tools:\n  - Bash\n  - Read\n---\n${body}`,
          linked_files: skill.name === "imagegen" ? {} : { references: ["references/guide.md"] }
        };
      }
      if (method === "skill/write") {
        const record = params as { name?: string; path?: string | null; target?: string | null; rawMarkdown?: string } | undefined;
        const skill = gatewayMock.skillRecords.find((item) => item.name === record?.name && (!record?.path || item.location === record.path))
          ?? gatewayMock.skillRecords.find((item) => item.name === record?.name);
        if (!skill) throw new Error("skill not found");
        skill.rawMarkdown = record?.rawMarkdown ?? "";
        return {
          written: true,
          name: skill.name,
          path: skill.location,
          target: record?.target ?? "global"
        };
      }
      if (method === "skill/setEnabled") {
        const record = params as { name?: string; enabled?: boolean } | undefined;
        gatewayMock.skillRecords = gatewayMock.skillRecords.map((item) => {
          if (item.name !== record?.name) return item;
          const enabled = record?.enabled === true;
          return {
            ...item,
            enabled,
            prompt_visible: enabled && item.disable_model_invocation !== true && item.supported_on_current_platform !== false,
            issues: enabled ? [] : ["disabled"]
          };
        });
        return { success: true, name: record?.name, enabled: record?.enabled === true };
      }
      if (method === "plugin/list") {
        return {
          plugins: [
            {
              name: "writer-kit",
              description: "Writing helper plugin",
              source_id: "local:/plugins/writer-kit",
              source: "writer-kit",
              source_kind: "local",
              scope: "global",
              manifest_kind: "psychevo",
              enabled: true,
              status: "Installed",
              readiness: "Installed",
              adapter_mode: "manifest_only",
              package_fingerprint: "abc123",
              trust: { required: false, status: "not_required", fingerprint: "abc123" },
              diagnostics: []
            }
          ],
          count: 1
        };
      }
      if (method === "plugin/import/inspect") {
        return {
          success: true,
          inspection: {
            name: "writer-kit",
            framework: "codex",
            status: "Available",
            source_kind: "local",
            adapter_mode: "manifest_only",
            target_lanes: ["skills", "mcp"],
            unsupported_lanes: ["apps"],
            stages: [
              { stage: "resolve/fetch", status: "ok", message: "resolved local source" },
              { stage: "inspect manifest", status: "ok", message: "loaded codex manifest" }
            ]
          }
        };
      }
      if (method === "plugin/setTrust") {
        return { success: true, trusted: true };
      }
      if (method === "mcp/list") {
        return {
          servers: [
            {
              name: "docs",
              enabled: true,
              required: false,
              sourceKind: "profile",
              transport: {
                kind: "streamable_http",
                url: "https://docs.example.test/mcp",
                auth: {
                  bearerTokenEnvVar: "DOCS_MCP_TOKEN",
                  scopes: ["docs.read"],
                  oauthClientId: "psychevo",
                  oauthConfigured: true,
                  storedOAuthToken: false
                }
              },
              policy: {
                enabledTools: null,
                disabledTools: ["delete"],
                supportsParallelToolCalls: false,
                startupTimeoutSecs: null,
                toolTimeoutSecs: null
              }
            }
          ],
          count: 1
        };
      }
      if (method === "tool/list") {
        return {
          modes: {
            default: { enabled_toolsets: ["coding-core"], disabled_toolsets: [], effective_tools: ["read", "write", "edit", "exec_command", "write_stdin"] },
            plan: { enabled_toolsets: ["coding-core", "web"], disabled_toolsets: [], effective_tools: ["read", "exec_command", "write_stdin", "web_fetch"] }
          },
          toolsets: [
            {
              name: "coding-core",
              source: "built_in",
              description: "Local coding tools",
              tools: ["read", "write", "edit", "exec_command", "write_stdin"],
              includes: [],
              unknown_tools: [],
              mode_mutable: false,
              removable: false
            },
            {
              name: "web",
              source: "built_in",
              description: "Web tools",
              tools: ["web_fetch"],
              includes: [],
              unknown_tools: [],
              mode_mutable: true,
              removable: false
            }
          ]
        };
      }
      if (
        method.startsWith("skill/")
        || method.startsWith("plugin/")
        || method.startsWith("mcp/")
        || method.startsWith("tool/")
      ) {
        return { success: true, status: "succeeded", sessionId: "oauth-session", authorizationUrl: "https://auth.example.test/authorize" };
      }
      if (method === "automation/list") {
        return { automations: gatewayMock.automationRecords };
      }
      if (method === "automation/draft") {
        const record = params as {
          request?: string;
          scope?: { cwd?: string | null } | null;
          currentThreadId?: string | null;
        };
        const threadRequested = Boolean(record.currentThreadId) && /thread|heartbeat|continue/i.test(record.request ?? "");
        return {
          draft: {
            target: threadRequested
              ? { kind: "threadHeartbeat", threadId: record.currentThreadId }
              : { kind: "project" },
            title: threadRequested ? "Thread follow-up" : "Morning repository check",
            prompt: threadRequested
              ? "Continue this thread with a concise status check."
              : "Review current repository state and summarize risky work before standup.",
            schedule: threadRequested
              ? { kind: "interval", everyMinutes: 30 }
              : { kind: "daily", time: "09:00" },
            execution: { policy: "autoSandbox" },
            model: null,
            reasoningEffort: null
          }
        };
      }
      if (method === "automation/write") {
        const record = params as {
          automationId?: string | null;
          scope?: { cwd?: string | null } | null;
          target?: { kind?: string; threadId?: string | null };
          title?: string;
          prompt?: string;
          schedule?: Record<string, unknown>;
          execution?: { policy?: string } | null;
          model?: string | null;
          reasoningEffort?: string | null;
        };
        const now = Date.now();
        const id = record.automationId ?? `automation-${gatewayMock.automationRecords.length + 1}`;
        const existing = gatewayMock.automationRecords.find((automation) => automation.id === id);
        const kind = record.target?.kind === "threadHeartbeat" ? "threadHeartbeat" : "project";
        const targetThreadId = kind === "threadHeartbeat" ? record.target?.threadId ?? "thread-1" : null;
        const automation = {
          id,
          cwd: record.scope?.cwd ?? gatewayMock.scope.cwd,
          kind,
          targetThreadId,
          title: record.title ?? "Project check",
          prompt: record.prompt ?? "Check the project.",
          schedule: record.schedule ?? { kind: "interval", everyMinutes: 60 },
          enabled: existing?.enabled ?? true,
          execution: record.execution ?? { policy: "autoSandbox" },
          model: record.model ?? null,
          reasoningEffort: record.reasoningEffort ?? null,
          sourceKey: kind === "threadHeartbeat" ? `thread:${targetThreadId}` : `automation:${id}`,
          createdAtMs: typeof existing?.createdAtMs === "number" ? existing.createdAtMs : now,
          updatedAtMs: now,
          lastRunAtMs: existing?.lastRunAtMs ?? null,
          nextRunAtMs: existing?.enabled === false ? null : now + 3_600_000,
          lastStatus: existing?.lastStatus ?? null,
          lastError: null,
          runs: Array.isArray(existing?.runs) ? existing.runs : []
        };
        gatewayMock.automationRecords = [
          ...gatewayMock.automationRecords.filter((item) => item.id !== id),
          automation
        ];
        return { automation };
      }
      if (method === "automation/pause" || method === "automation/resume") {
        const record = params as { automationId?: string };
        const now = Date.now();
        const id = record.automationId ?? "automation-1";
        const enabled = method === "automation/resume";
        const existing = gatewayMock.automationRecords.find((automation) => automation.id === id) ?? {
          id,
          cwd: gatewayMock.scope.cwd,
          kind: "project",
          targetThreadId: null,
          title: "Project check",
          prompt: "Check the project.",
          schedule: { kind: "interval", everyMinutes: 60 },
          enabled: true,
          execution: { policy: "autoSandbox" },
          model: null,
          reasoningEffort: null,
          sourceKey: `automation:${id}`,
          createdAtMs: now,
          updatedAtMs: now,
          lastRunAtMs: null,
          nextRunAtMs: now + 3_600_000,
          lastStatus: null,
          lastError: null,
          runs: []
        };
        const automation = {
          ...existing,
          enabled,
          updatedAtMs: now,
          nextRunAtMs: enabled ? now + 3_600_000 : null
        };
        gatewayMock.automationRecords = [
          ...gatewayMock.automationRecords.filter((item) => item.id !== id),
          automation
        ];
        return { automation };
      }
      if (method === "automation/run") {
        const record = params as { automationId?: string; trigger?: string | null };
        const now = Date.now();
        const id = record.automationId ?? "automation-1";
        const existing = gatewayMock.automationRecords.find((automation) => automation.id === id) ?? {
          id,
          cwd: gatewayMock.scope.cwd,
          kind: "project",
          targetThreadId: null,
          title: "Project check",
          prompt: "Check the project.",
          schedule: { kind: "interval", everyMinutes: 60 },
          enabled: true,
          execution: { policy: "autoSandbox" },
          model: null,
          reasoningEffort: null,
          sourceKey: `automation:${id}`,
          createdAtMs: now,
          updatedAtMs: now,
          lastRunAtMs: null,
          nextRunAtMs: now + 3_600_000,
          lastStatus: null,
          lastError: null,
          runs: []
        };
        const run = {
          id: `run-${id}-${now}`,
          automationId: id,
          trigger: record.trigger ?? "manual",
          status: "running",
          startedAtMs: now,
          completedAtMs: null,
          threadId: existing.targetThreadId ?? "thread-automation",
          sourceKey: existing.sourceKey ?? `automation:${id}`,
          error: null,
          metadata: null
        };
        const automation = {
          ...existing,
          updatedAtMs: now,
          lastRunAtMs: now,
          nextRunAtMs: now + 3_600_000,
          lastStatus: "running",
          lastError: null,
          runs: [run, ...(Array.isArray(existing.runs) ? existing.runs : [])].slice(0, 5)
        };
        gatewayMock.automationRecords = [
          ...gatewayMock.automationRecords.filter((item) => item.id !== id),
          automation
        ];
        return { accepted: true, automation, run };
      }
      if (method === "automation/delete") {
        const record = params as { automationId?: string };
        const automationId = record.automationId ?? "automation-1";
        gatewayMock.automationRecords = gatewayMock.automationRecords.filter((automation) => automation.id !== automationId);
        return { deleted: true, automationId };
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
            costStatus: hasThread ? "estimated" : "unknown",
            estimatedPricingCount: hasThread ? 1 : 0,
            freePricingCount: 0,
            includedPricingCount: 0,
            unknownPricingCount: 0,
            cacheReadPercent: hasThread ? 40 : null
          }
        };
      }
      if (method === "usage/read") {
        if (gatewayMock.usageRead) {
          return gatewayMock.usageRead(params);
        }
        return usageReadResult();
      }
      if (method === "completion/list") {
        return gatewayMock.completionResult;
      }
      if (method === "turn/start") {
        const record = params && typeof params === "object" && !Array.isArray(params)
          ? params as { threadId?: string | null }
          : {};
        return { accepted: true, threadId: record.threadId ?? "thread-1" };
      }
      if (method === "permission/respond") {
        return gatewayMock.permissionRespond(params);
      }
      if (method === "clarify/respond") {
        return gatewayMock.clarifyRespond(params);
      }
      if (method === "terminal/start") {
        return { terminalId: "terminal-1", cwd: gatewayMock.scope.cwd, pid: null };
      }
      if (method === "terminal/write" || method === "terminal/resize" || method === "terminal/terminate") {
        return { accepted: true };
      }
      throw new Error(`unexpected request: ${method}`);
    }
  }

  return {
    GatewayClient,
    acceptThreadTurn: actual.acceptThreadTurn,
    appendOptimisticPrompt: (current: unknown, text: string) => {
      gatewayMock.optimisticLog.push(text);
      return current;
    },
    applyGatewayEventToThreadSnapshot: actual.applyGatewayEventToThreadSnapshot,
    applyLiveTranscriptEvent: actual.applyLiveTranscriptEvent,
    applyTurnResultToThreadSnapshot: actual.applyTurnResultToThreadSnapshot,
    bindThreadSnapshot: actual.bindThreadSnapshot,
    emptyThreadSnapshot: actual.emptyThreadSnapshot,
    latestAssistantTranscriptText: actual.latestAssistantTranscriptText,
    parseThreadSnapshot: (value: unknown) => value,
    prepareThreadTurn: (current: unknown, text: string, requestedThreadId?: string | null) => {
      gatewayMock.optimisticLog.push(text);
      return {
        requestedThreadId: requestedThreadId ?? null,
        snapshot: current
      };
    },
    reconcileThreadSnapshot: (_current: unknown, next: unknown) => next,
    scopeForCwd: (cwd: string) => ({ ...gatewayMock.scope, cwd }),
    turnCompletedEventFromResult: actual.turnCompletedEventFromResult,
    threadTurnStartParams: actual.threadTurnStartParams,
    turnResultThreadId: actual.turnResultThreadId
  };
});

vi.mock("@psychevo/host", () => {
  const downloadUrl = (_endpoint: unknown, threadId: string, kind: string, options: Record<string, unknown> = {}) => {
    const url = new URL(`http://127.0.0.1/download/session/${threadId}/${kind}`);
    if (typeof options.format === "string" && options.format) {
      url.searchParams.set("format", options.format);
    }
    if (Array.isArray(options.include) && options.include.length > 0) {
      url.searchParams.set("include", options.include.join(","));
    }
    if (typeof options.filename === "string" && options.filename) {
      url.searchParams.set("filename", options.filename);
    }
    return url.toString();
  };
  return {
    createBrowserHost: () => ({
      endpoint: gatewayMock.endpoint,
      storage: {
        getJson: (_key: string, fallback: unknown) => fallback,
        setJson: vi.fn()
      },
      clipboard: {
        writeText: vi.fn(async (text: string) => {
          gatewayMock.clipboardWriteLog.push(text);
          return { ok: true };
        })
      },
      files: { pickFile: vi.fn(async () => ({ ok: false })) },
      open: {
        downloadSession: vi.fn((endpoint: unknown, threadId: string, kind: string, options: Record<string, unknown> = {}) => {
          gatewayMock.openDownloadLog.push(downloadUrl(endpoint, threadId, kind, options));
          return Promise.resolve({ ok: true, value: undefined });
        }),
        openDownload: vi.fn((url: string) => gatewayMock.openDownloadLog.push(url))
      }
    }),
    downloadUrl
  };
});

vi.mock("@xterm/xterm", () => {
  class Terminal {
    cols = 80;
    rows = 24;
    options: Record<string, unknown>;

    constructor(options: Record<string, unknown>) {
      this.options = options;
      gatewayMock.xtermTerminalOptions.push(options);
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
