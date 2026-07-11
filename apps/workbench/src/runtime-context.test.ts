import { describe, expect, it } from "vitest";
import {
  agentPairingUnavailableReason,
  parseRuntimeContext,
  registerRuntimeContextChildTabs,
  runtimeControlSelections,
  runtimeProfileCapsuleLabel,
  runtimeProfileDisplayLabel,
  unsupportedRequiredAgentContributions
} from "./runtime-context";
import type { AgentContribution, WorkbenchAgent } from "./types";

function agentWith(
  contributions: AgentContribution[],
  optionalContributions: AgentContribution[] = []
): WorkbenchAgent {
  return {
    name: "reviewer",
    description: "Review changes",
    enabled: true,
    source: "project",
    sourceLabel: "Project",
    generated: false,
    mutable: true,
    entrypoints: ["subagent"],
    tools: [],
    mcpServers: [],
    contributions,
    optionalContributions,
    diagnostics: []
  };
}

describe("runtime context projection", () => {
  it("parses Profile, binding, control, and session fields without trusting raw payloads", () => {
    const context = parseRuntimeContext({
      runtimeRef: "codex",
      selectionState: "bound",
      profiles: [{
        id: "codex",
        runtime: "codex",
        enabled: true,
        label: "Codex",
        generated: true,
        configured: false,
        provenance: "Direct",
        capabilityRevision: "9007199254740993",
        profileRevision: "18446744073709551615",
        stability: "stable",
        capabilities: [{ id: "turn.start", enabled: true, stability: "stable" }],
        health: { status: "ready", summary: "Ready", checkedAtMs: 100 },
        readinessStages: [{ id: "transport", status: "ready", summary: "Connected", observedAtMs: 100 }]
      }],
      binding: {
        threadId: "thread-1",
        runtimeRef: "codex",
        backendKind: "runtime",
        cwd: "/tmp/project",
        profileFingerprint: "fingerprint",
        ownership: "readWrite",
        bindingRevision: 2
      },
      controls: [{
        id: "mode",
        label: "Mode",
        state: "readOnlyCurrent",
        currentValue: "review",
        choices: [],
        channelSafe: true,
        capabilityRevision: "9007199254740993"
      }],
      stability: "stable",
      capabilities: [{ id: "turn.start", enabled: true, stability: "stable" }],
      activeSession: {
        sessionHandle: "rts_parent",
        dedupKey: "rtd_parent",
        fidelity: "summary",
        ownership: "readOnly",
        actions: ["fork"]
      },
      children: [{
        sessionHandle: "rts_child",
        threadId: "thread-child",
        parentThreadId: "thread-1",
        title: "Runtime review",
        status: "idle",
        dedupKey: "rtd_child",
        fidelity: "partial",
        ownership: "readOnly",
        actions: ["read", "fork"]
      }],
      goal: {
        objective: "Ship evidence",
        status: "active",
        tokenBudget: 20_000,
        tokensUsed: 500,
        timeUsedSeconds: 40,
        createdAt: 10,
        updatedAt: 20,
        nativeThreadId: "native-thread-secret"
      },
      accountRateLimits: {
        rateLimits: {
          limitId: "codex",
          limitName: "Codex",
          primary: { usedPercent: 25, windowDurationMins: 300, resetsAt: 9_000 },
          secondary: null,
          credits: { hasCredits: true, unlimited: false, balance: "12.50" },
          individualLimit: null,
          planType: "pro",
          rateLimitReachedType: null,
          nativeAccountId: "account-secret"
        },
        rateLimitsByLimitId: {
          codex: {
            limitId: "codex",
            primary: { usedPercent: 25, windowDurationMins: 300, resetsAt: 9_000 }
          }
        },
        resetCreditsAvailable: 2,
        nativeAccountId: "account-secret"
      }
    });

    expect(context.binding).toMatchObject({ runtimeRef: "codex", bindingRevision: 2 });
    expect(context.profiles[0]).toMatchObject({
      profileRevision: "18446744073709551615",
      capabilityRevision: "9007199254740993"
    });
    expect(context.controls[0]).toMatchObject({
      id: "mode",
      state: "readOnlyCurrent",
      currentValue: "review",
      capabilityRevision: "9007199254740993"
    });
    expect(context.activeSession).toMatchObject({ dedupKey: "rtd_parent", fidelity: "summary", ownership: "readOnly" });
    expect(context.stability).toBe("stable");
    expect(context.capabilities).toEqual([{ id: "turn.start", enabled: true, stability: "stable" }]);
    expect(context.children[0]).toMatchObject({
      threadId: "thread-child",
      sessionHandle: "rts_child",
      status: "idle"
    });
    expect(context.goal).toEqual({
      objective: "Ship evidence",
      status: "active",
      tokenBudget: 20_000,
      tokensUsed: 500,
      timeUsedSeconds: 40,
      createdAt: 10,
      updatedAt: 20
    });
    expect(context.accountRateLimits).toEqual({
      rateLimits: {
        limitId: "codex",
        limitName: "Codex",
        primary: { usedPercent: 25, windowDurationMins: 300, resetsAt: 9_000 },
        secondary: null,
        credits: { hasCredits: true, unlimited: false, balance: "12.50" },
        individualLimit: null,
        planType: "pro",
        rateLimitReachedType: null
      },
      rateLimitsByLimitId: {
        codex: {
          limitId: "codex",
          limitName: null,
          primary: { usedPercent: 25, windowDurationMins: 300, resetsAt: 9_000 },
          secondary: null,
          credits: null,
          individualLimit: null,
          planType: null,
          rateLimitReachedType: null
        }
      },
      resetCreditsAvailable: 2
    });
    expect(JSON.stringify(context)).not.toContain("native-thread-secret");
    expect(JSON.stringify(context)).not.toContain("account-secret");
    expect(registerRuntimeContextChildTabs([], context)).toEqual([
      expect.objectContaining({
        kind: "agentSession",
        threadId: "thread-child",
        parentThreadId: "thread-1",
        title: "Runtime review",
        historyFidelity: "partial",
        runtimeStatus: "idle"
      })
    ]);
    expect(runtimeProfileCapsuleLabel(context.profiles[0]!)).toBe("Codex · Direct");
    expect(agentPairingUnavailableReason(agentWith(["instructions"]), context.profiles[0]!)).toBeNull();
  });

  it("validates direct Runtime Profile pairing per required Agent Definition contribution", () => {
    const profile = parseRuntimeContext({
      runtimeRef: "opencode",
      profiles: [{ id: "opencode", runtime: "opencode", label: "OpenCode" }]
    }).profiles[0]!;

    expect(unsupportedRequiredAgentContributions(
      agentWith(["instructions", "tools", "mcp", "skills"], ["mcp", "skills"]),
      profile
    )).toEqual(["tools"]);
    expect(agentPairingUnavailableReason(
      agentWith(["instructions", "tools", "mcp", "skills"], ["mcp", "skills"]),
      profile
    )).toBe(
      "OpenCode cannot faithfully apply the required Agent Definition tool policy contribution. Mark tool policy optional or choose Native."
    );
    expect(agentPairingUnavailableReason(
      agentWith(["instructions", "tools", "mcp", "skills"], ["tools", "mcp", "skills"]),
      profile
    )).toBeNull();
  });

  it("never coerces public runtime revisions through JavaScript numbers", () => {
    const context = parseRuntimeContext({
      runtimeRef: "codex",
      profiles: [{
        id: "codex",
        runtime: "codex",
        label: "Codex",
        profileRevision: 9_007_199_254_740_993,
        capabilityRevision: "018",
        health: { status: "ready", summary: "Ready" }
      }],
      controls: [{
        id: "mode",
        label: "Mode",
        state: "selectable",
        capabilityRevision: "18446744073709551616"
      }]
    });

    expect(context.profiles[0]?.profileRevision).toBe("0");
    expect(context.profiles[0]?.capabilityRevision).toBe("0");
    expect(context.controls[0]?.capabilityRevision).toBe("0");
  });

  it("drops malformed runtime-child status instead of forwarding native payload text", () => {
    const context = parseRuntimeContext({
      children: [{
        sessionHandle: "rts_child",
        threadId: "thread-child",
        parentThreadId: "thread-1",
        status: "idle; nativeSession=secret",
        fidelity: "partial",
        ownership: "readOnly"
      }]
    });

    expect(context.children[0]?.status).toBeNull();
  });

  it("requires an ACP profile to use its matching backend-backed Agent Definition", () => {
    const profile = parseRuntimeContext({
      runtimeRef: "acp:cursor",
      profiles: [{
        id: "acp:cursor",
        runtime: "acp",
        label: "Cursor (ACP)",
        backendRef: "cursor"
      }]
    }).profiles[0]!;
    const plain = agentWith(["instructions"]);
    expect(agentPairingUnavailableReason(plain, profile)).toContain(
      "can only use the Agent Definition backed by cursor"
    );
    const matching = { ...plain, backend: { ref: "cursor" } };
    expect(agentPairingUnavailableReason(matching, profile)).toBeNull();
  });

  it("normalizes ACP suffixes and fails malformed enums to conservative defaults", () => {
    const context = parseRuntimeContext({
      profiles: [{
        id: "acp:cursor",
        runtime: "acp",
        label: "Cursor (ACP)",
        health: { status: "unchecked", summary: "Not checked" }
      }],
      binding: { ownership: "unexpected" },
      controls: [{ id: "mode", state: "unexpected" }]
    });

    expect(context.runtimeRef).toBe("acp:cursor");
    expect(runtimeProfileDisplayLabel(context.profiles[0]!)).toBe("Cursor (ACP)");
    expect(context.binding?.ownership).toBe("readOnly");
    expect(context.controls[0]?.state).toBe("runtimeDefault");
    expect(context.goal).toBeNull();
    expect(context.accountRateLimits).toBeNull();
  });

  it("fails malformed goal and account metadata closed instead of inventing status", () => {
    const malformedGoal = parseRuntimeContext({
      goal: {
        objective: "Ship evidence",
        status: "native_future_status",
        tokenBudget: 100,
        tokensUsed: 1,
        timeUsedSeconds: 2,
        createdAt: 3,
        updatedAt: 4
      },
      accountRateLimits: {
        rateLimits: {
          primary: { usedPercent: "25", windowDurationMins: 300, resetsAt: 9_000 }
        },
        rateLimitsByLimitId: {},
        resetCreditsAvailable: 2
      }
    });

    expect(malformedGoal.goal).toBeNull();
    expect(malformedGoal.accountRateLimits).toBeNull();
  });

  it("serializes every selected observed runtime control by descriptor id", () => {
    const controls = parseRuntimeContext({
      controls: [
        { id: "mode", state: "selectable", choices: [{ value: "plan", label: "Plan" }] },
        { id: "effort", state: "selectable", choices: [{ value: "high", label: "High" }] },
        { id: "feature.fast", state: "selectable", choices: [{ value: true, label: "Fast" }] },
        { id: "model", state: "readOnlyCurrent", currentValue: "observed/model" }
      ]
    }).controls;

    expect(runtimeControlSelections(controls, {
      mode: "plan",
      effort: "high",
      "feature.fast": true,
      model: "ignored/model"
    })).toEqual({
      mode: "plan",
      effort: "high",
      "feature.fast": "true"
    });
  });

  it("drops dependent selections when their catalog model no longer matches", () => {
    const controls = parseRuntimeContext({
      controls: [
        {
          id: "model",
          state: "selectable",
          choices: [
            { value: "model-a", label: "Model A" },
            { value: "model-b", label: "Model B" }
          ]
        },
        {
          id: "effort",
          state: "selectable",
          choices: [{ value: "high", label: "High" }],
          dependsOn: { controlId: "model", value: "model-a" }
        }
      ]
    }).controls;

    expect(controls[1]?.dependsOn).toEqual({ controlId: "model", value: "model-a" });
    expect(runtimeControlSelections(controls, {
      model: "model-b",
      effort: "high"
    })).toEqual({ model: "model-b" });
    expect(runtimeControlSelections(controls, {
      effort: "high"
    })).toEqual({ effort: "high" });
  });
});
