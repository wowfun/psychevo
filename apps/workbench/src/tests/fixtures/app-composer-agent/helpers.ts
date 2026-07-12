import { fireEvent, screen, within } from "@testing-library/react";
import { gatewayMock } from "./gateway-mock";

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
    expandsTo: null,
    presentationKind,
    destination,
    feedbackAnchor: "commandsPanel",
    alternateAction: null
  };
}

export function sessionSummary(id: string, title: string, cwd = gatewayMock.scope.cwd): Record<string, unknown> {
  return {
    id,
    cwd,
    project: {
      cwd,
      label: cwd.split("/").filter(Boolean).at(-1) ?? "project",
      displayPath: cwd
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
    enabled: true,
    source: backendRef ? "generated" : "project",
    sourceLabel: backendRef ? "Generated" : "Project",
    generated: Boolean(backendRef),
    target: backendRef ? null : "project",
    mutable: !backendRef,
    path: backendRef ? null : `/tmp/project/.psychevo/agents/${name}.md`,
    backend: backendRef ? { ref: backendRef } : null,
    entrypoints,
    tools: [],
    mcpServers: [],
    contributions: ["instructions"],
    optionalContributions: [],
    diagnostics: []
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
      status: hasThread ? "exact" : "unavailable",
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
      costStatus: hasThread ? (peer ? "free" : "estimated") : "unknown",
      estimatedPricingCount: hasThread && !peer ? 1 : 0,
      freePricingCount: hasThread && peer ? 1 : 0,
      includedPricingCount: 0,
      unknownPricingCount: 0,
      cacheReadPercent: hasThread ? (peer ? 50 : 40) : null
    }
  };
}

export function usageReadResult(): Record<string, unknown> {
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
  const existing = screen.queryByRole("dialog", { name: "Agent target" });
  if (existing) {
    return existing;
  }
  fireEvent.click(await screen.findByRole("button", { name: "Agent target" }));
  return await screen.findByRole("dialog", { name: "Agent target" });
}

export async function openRuntimeProfilePopover() {
  const existing = screen.queryByRole("dialog", { name: "Agent target" });
  if (existing) return existing;
  fireEvent.click(await screen.findByRole("button", { name: "Agent target" }));
  return await screen.findByRole("dialog", { name: "Agent target" });
}

export async function selectMainAgent(value: string) {
  const popover = await openAgentRuntimePopover();
  const label = value || "Default Agent";
  fireEvent.click(within(popover).getByRole("radio", { name: label }));
  return popover;
}

export async function selectRuntime(value: string) {
  const popover = await openRuntimeProfilePopover();
  const label = value === "native"
    ? "Psychevo (Native)"
    : value === "opencode"
      ? "OpenCode (ACP)"
      : value === "codex" || value === "Codex"
        ? "Codex (ACP)"
        : value;
  const target = within(popover).getAllByRole("radio").find((radio) => (
    radio.getAttribute("aria-label")?.endsWith(` · ${label}`)
  ));
  if (!target) throw new Error(`expected an Agent target for Runtime Profile ${label}`);
  fireEvent.click(target);
  return popover;
}
