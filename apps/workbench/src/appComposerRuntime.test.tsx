// @vitest-environment jsdom

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import {
  agentRecord,
  deferred,
  gatewayMock,
  openAgentRuntimePopover,
  sessionSummary
} from "./appComposerAgent.fixture";
import { App } from "./App";

type FirstClassProfileRef = "native" | "codex" | "opencode";

describe("Workbench first-class Agent runtime controls", () => {
  it("discovers Native, Codex ACP, and OpenCode ACP from compatible Thread targets", async () => {
    useFirstClassContexts();

    render(<App />);

    const popover = await openAgentRuntimePopover();
    const targets = within(popover).getByRole("radiogroup", { name: "Agent target" });
    expect(within(targets).getByRole("radio", { name: "Psychevo · Psychevo (Native)" })).toBeTruthy();
    expect(within(targets).getByRole("radio", { name: "Codex · Codex (ACP)" })).toBeTruthy();
    expect(within(targets).getByRole("radio", { name: "OpenCode · OpenCode (ACP)" })).toBeTruthy();
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/context/read",
        params: expect.objectContaining({
          target: { agentRef: null, runtimeProfileRef: "native" }
        })
      });
    });
  });

  it.each([
    ["codex", "Codex (ACP)"],
    ["opencode", "OpenCode (ACP)"]
  ] as const)("atomically pairs the %s profile from the Default Native target", async (runtimeProfileRef, label) => {
    useFirstClassContexts();
    render(<App />);

    const popover = await openAgentRuntimePopover();
    const targetLabel = `${label.replace(" (ACP)", "")} · ${label}`;
    const profile = within(popover).getByRole("radio", { name: targetLabel }) as HTMLButtonElement;
    expect(profile.disabled).toBe(false);
    fireEvent.click(profile);

    await waitFor(() => {
      const entry = screen.getByRole("button", { name: "Agent target" });
      expect(entry.textContent).toBe(label);
    });
    const nextPopover = await openAgentRuntimePopover();
    expect(within(nextPopover).getByRole("radio", { name: targetLabel }).getAttribute("aria-checked")).toBe("true");
  });

  it("keeps Thread context sendable when the Agent changes inside one ACP profile", async () => {
    gatewayMock.agentRecords = [
      agentRecord("codex", ["peer"], "codex"),
      agentRecord("reviewer", ["peer"], "codex")
    ];
    gatewayMock.runtimeContextRead = (params) => firstClassContext(requestedProfile(params), {
      compatibleTargets: [
        {
          targetId: "target:default:native",
          agentRef: null,
          runtimeProfileRef: "native",
          agentLabel: "Psychevo",
          profileLabel: "Psychevo (Native)",
          label: "Psychevo · Psychevo (Native)",
          ready: true,
          unavailableReason: null
        },
        {
          targetId: "target:codex:codex",
          agentRef: "codex",
          runtimeProfileRef: "codex",
          agentLabel: "codex",
          profileLabel: "Codex (ACP)",
          label: "codex · Codex (ACP)",
          ready: true,
          unavailableReason: null
        },
        {
          targetId: "target:reviewer:codex",
          agentRef: "reviewer",
          runtimeProfileRef: "codex",
          agentLabel: "reviewer",
          profileLabel: "Codex (ACP)",
          label: "reviewer · Codex (ACP)",
          ready: true,
          unavailableReason: null
        }
      ]
    });
    render(<App />);

    const runtimePopover = await openAgentRuntimePopover();
    fireEvent.click(within(runtimePopover).getByRole("radio", { name: "codex · Codex (ACP)" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Agent target" })).toBeNull();
      expect(screen.getByRole("button", { name: "Agent target" }).textContent).toContain("codex (ACP)");
    });
    const agentPopover = await openAgentRuntimePopover();
    const reviewer = within(agentPopover).getByRole("radio", { name: "reviewer · Codex (ACP)" }) as HTMLButtonElement;
    expect(reviewer.disabled).toBe(false);
    expect(reviewer.getAttribute("aria-checked")).toBe("false");
    fireEvent.click(reviewer);
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Agent target" })).toBeNull();
      expect(screen.getByRole("button", { name: "Agent target" }).textContent).toContain("reviewer");
    });
    fireEvent.change(screen.getByPlaceholderText("Ask Psychevo..."), {
      target: { value: "review with the selected Agent" }
    });
    const send = screen.getByRole("button", { name: "Send message" }) as HTMLButtonElement;
    await waitFor(() => expect(send.disabled).toBe(false));
    fireEvent.click(send);

    expect(await turnStartParams()).toEqual(expect.objectContaining({
      target: { agentRef: "reviewer", runtimeProfileRef: "codex" }
    }));
  });

  it("sets a Native mode through the Thread control interface before starting a turn", async () => {
    useFirstClassContexts();
    render(<App />);

    await screen.findByRole("button", { name: "Agent target" });
    fireEvent.change(await screen.findByRole("combobox", { name: "Mode" }), { target: { value: "1" } });
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/control/set",
        params: expect.objectContaining({
          targetId: "target:default:native",
          controlId: "mode",
          value: "plan"
        })
      });
    });
    await new Promise((resolve) => window.setTimeout(resolve, 0));
    fireEvent.change(screen.getByPlaceholderText("Ask Psychevo..."), {
      target: { value: "plan through the native Agent" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    const params = await turnStartParams();
    expect(params).toEqual(expect.objectContaining({
      target: { agentRef: null, runtimeProfileRef: "native" },
      input: [{ type: "text", text: "plan through the native Agent" }],
      mentions: [],
      turnOverrides: {},
      expectedContextRevision: "101",
      expectedControlRevision: "201"
    }));
    expect(Object.keys(params).sort()).toEqual([
      "expectedContextRevision",
      "expectedControlRevision",
      "input",
      "mentions",
      "scope",
      "target",
      "threadId",
      "turnOverrides"
    ]);
  });

  it.each([
    ["codex", "Codex (ACP)"],
    ["opencode", "OpenCode (ACP)"]
  ] as const)("starts %s through the same ACP target and control interface", async (runtimeProfileRef, label) => {
    useFirstClassContexts();
    render(<App />);

    const popover = await openAgentRuntimePopover();
    fireEvent.click(within(popover).getByRole("radio", { name: `${label.replace(" (ACP)", "")} · ${label}` }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/draft/prepare",
        params: expect.objectContaining({
          targetId: `target:${runtimeProfileRef}:${runtimeProfileRef}`
        })
      });
    });

    const model = await screen.findByRole("button", { name: "Model" });
    fireEvent.click(model);
    const modelPicker = await screen.findByRole("dialog", { name: "Model and reasoning" });
    fireEvent.click(within(modelPicker).getByRole("radio", { name: "Model B" }));
    fireEvent.click(within(modelPicker).getByRole("radio", { name: "High" }));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toEqual(expect.arrayContaining([
        {
          method: "thread/control/set",
          params: expect.objectContaining({ targetId: `target:${runtimeProfileRef}:${runtimeProfileRef}`, controlId: "model" })
        },
        {
          method: "thread/control/set",
          params: expect.objectContaining({ targetId: `target:${runtimeProfileRef}:${runtimeProfileRef}`, controlId: "reasoning" })
        }
      ]));
    });
    await new Promise((resolve) => window.setTimeout(resolve, 0));
    fireEvent.change(screen.getByPlaceholderText("Ask Psychevo..."), {
      target: { value: `run through ${runtimeProfileRef}` }
    });
    const send = screen.getByRole("button", { name: "Send message" }) as HTMLButtonElement;
    await waitFor(() => expect(send.disabled).toBe(false));
    fireEvent.click(send);

    const params = await turnStartParams();
    expect(params).toEqual(expect.objectContaining({
      target: { agentRef: runtimeProfileRef, runtimeProfileRef },
      input: [{ type: "text", text: `run through ${runtimeProfileRef}` }],
      mentions: [],
      turnOverrides: {},
      expectedContextRevision: runtimeProfileRef === "codex" ? "102" : "103",
      expectedControlRevision: runtimeProfileRef === "codex" ? "202" : "203"
    }));
    expect(Object.keys(params).sort()).toEqual([
      "expectedContextRevision",
      "expectedControlRevision",
      "input",
      "mentions",
      "scope",
      "target",
      "threadId",
      "turnOverrides"
    ]);
    expect(gatewayMock.requestLog).toEqual(expect.arrayContaining([
      {
        method: "thread/control/set",
        params: expect.objectContaining({
          targetId: `target:${runtimeProfileRef}:${runtimeProfileRef}`,
          controlId: "model",
          value: `${runtimeProfileRef}/model-b`
        })
      },
      {
        method: "thread/control/set",
        params: expect.objectContaining({
          targetId: `target:${runtimeProfileRef}:${runtimeProfileRef}`,
          controlId: "reasoning",
          value: "high"
        })
      }
    ]));
  });

  it("honors Thread sendability instead of inferring readiness from ACP branding", async () => {
    gatewayMock.runtimeContextRead = (params) => {
      const requested = requestedProfile(params);
      return firstClassContext(requested, requested === "opencode" ? {
        compatibleTargets: [{
          targetId: "target:opencode:opencode",
          agentRef: "opencode",
          runtimeProfileRef: "opencode",
          agentLabel: "OpenCode",
          profileLabel: "OpenCode (ACP)",
          label: "OpenCode · OpenCode (ACP)",
          ready: false,
          unavailableReason: "OpenCode ACP is under maintenance."
        }],
        sendability: {
          allowed: false,
          reason: "OpenCode ACP is under maintenance.",
          recoveryAction: "backend/doctor"
        }
      } : {});
    };
    render(<App />);

    const popover = await openAgentRuntimePopover();
    const opencode = within(popover).getByRole("radio", { name: "OpenCode · OpenCode (ACP)" }) as HTMLButtonElement;
    expect(opencode.disabled).toBe(false);
    fireEvent.click(opencode);
    await screen.findByRole("button", { name: "Model" });
    fireEvent.change(screen.getByPlaceholderText("Ask Psychevo..."), {
      target: { value: "must stay local" }
    });

    const send = screen.getByRole("button", { name: "Send message" }) as HTMLButtonElement;
    await waitFor(() => expect(send.disabled).toBe(true));
    expect(send.title).toContain("OpenCode ACP is under maintenance");
    expect(gatewayMock.requestLog.some((entry) => entry.method === "turn/start")).toBe(false);
  });

  it("applies a bound ACP session control with capability, binding, context, and control revisions", async () => {
    gatewayMock.runtimeContextRead = () => firstClassContext("codex", {
      binding: {
        threadId: "thread-1",
        agentRef: "codex",
        agentFingerprint: "codex-fingerprint",
        runtimeRef: "codex",
        backendKind: "acp",
        nativeKind: null,
        sessionHandle: "codex-session",
        cwd: "/tmp/project",
        profileFingerprint: "codex-fingerprint",
        ownership: "readWrite",
        bindingRevision: 17
      },
      controls: [control({
        id: "mode",
        label: "Mode",
        surfaceRole: "mode",
        effectiveValue: "review",
        choices: [
          { value: "review", label: "Review", description: null },
          { value: "plan", label: "Plan", description: null }
        ],
        applyScope: "session",
        capabilityRevision: "9007199254740993"
      })],
      selectionState: "bound"
    });
    render(<App />);

    const mode = await screen.findByRole("combobox", { name: "Mode" });
    fireEvent.change(mode, { target: { value: "1" } });

    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/control/set",
        params: expect.objectContaining({
          threadId: null,
          targetId: "target:codex:codex",
          controlId: "mode",
          value: "plan",
          expectedCapabilityRevision: "9007199254740993",
          expectedBindingRevision: 17,
          expectedContextRevision: "102",
          expectedControlRevision: "202"
        })
      });
    });
  });

  it("restores the immutable Agent from the bound Thread context after reload", async () => {
    gatewayMock.agentRecords = [agentRecord("reviewer", ["peer"], "codex")];
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Bound reviewer")];
    gatewayMock.runtimeContextRead = (params) => {
      const request = params as {
        threadId?: string | null;
        target?: { agentRef?: string | null; runtimeProfileRef?: string } | null;
      };
      if (request.threadId !== "thread-1" || request.target !== null) {
        return firstClassContext("native");
      }
      return firstClassContext("codex", {
      targetId: "target:reviewer:codex",
      binding: {
        threadId: "thread-1",
        agentRef: "reviewer",
        agentFingerprint: "reviewer-fingerprint",
        runtimeRef: "codex",
        backendKind: "acp",
        nativeKind: "acp",
        sessionHandle: "codex-session",
        cwd: "/tmp/project",
        profileFingerprint: "codex-fingerprint",
        ownership: "readWrite",
        bindingRevision: 17
      },
      compatibleTargets: [
        {
          targetId: "target:default:native",
          agentRef: null,
          runtimeProfileRef: "native",
          agentLabel: "Psychevo",
          profileLabel: "Psychevo (Native)",
          label: "Psychevo · Psychevo (Native)",
          ready: true,
          unavailableReason: null
        },
        {
          targetId: "target:reviewer:codex",
          agentRef: "reviewer",
          runtimeProfileRef: "codex",
          agentLabel: "reviewer",
          profileLabel: "Codex (ACP)",
          label: "reviewer · Codex (ACP)",
          ready: true,
          unavailableReason: null
        }
      ],
      selectionState: "bound"
    });
    };

    render(<App />);

    fireEvent.click(await screen.findByText("Bound reviewer"));

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Agent target" }).textContent).toContain("reviewer");
    });
    expect(gatewayMock.requestLog).toContainEqual({
      method: "thread/context/read",
      params: expect.objectContaining({ threadId: "thread-1", target: null })
    });
    expect(gatewayMock.requestLog.some((entry) => {
      if (entry.method !== "thread/context/read") return false;
      const params = entry.params as { threadId?: string | null; target?: unknown };
      return params.threadId === "thread-1" && params.target !== null;
    })).toBe(false);
    expect(gatewayMock.requestLog.filter((entry) => (
      entry.method === "thread/context/read"
      && (entry.params as { threadId?: string | null }).threadId === "thread-1"
    ))).toHaveLength(1);
    expect(screen.queryByText("Default Agent is not compatible with this Runtime Profile.")).toBeNull();
  });

  it("changes a bound Agent/Profile target through exactly one new thread", async () => {
    const preparedScope = {
      cwd: "/tmp/project",
      source: {
        kind: "web",
        rawId: "draft:new-source",
        lifetime: "persistent",
        rawIdentity: {
          kind: "web",
          rawId: "draft:new-source",
          canonicalRawId: "new-source",
          cwd: "/tmp/project",
          draft: true
        },
        visibleName: "project"
      }
    } as const;
    const delayedSettings = deferred<Record<string, unknown>>();
    let delayAuxiliaryRefresh = false;
    let starts = 0;
    gatewayMock.threadStart = () => {
      starts += 1;
      return {
        ...gatewayMock.snapshot,
        scope: starts === 1 ? gatewayMock.scope : preparedScope,
        thread: starts === 1 ? gatewayMock.snapshot.thread : null,
        entries: [],
        activity: { ...gatewayMock.snapshot.activity }
      };
    };
    gatewayMock.runtimeContextRead = (params) => {
      if (requestedProfile(params) === "opencode") return firstClassContext("opencode");
      return firstClassContext("codex", {
        binding: {
          threadId: "thread-1",
          agentRef: "codex",
          agentFingerprint: "codex-fingerprint",
          runtimeRef: "codex",
          backendKind: "acp",
          nativeKind: null,
          sessionHandle: "codex-session",
          cwd: "/tmp/project",
          profileFingerprint: "codex-fingerprint",
          ownership: "readWrite",
          bindingRevision: 17
        },
        selectionState: "bound"
      });
    };
    gatewayMock.settingsRead = () => delayAuxiliaryRefresh
      ? delayedSettings.promise
      : gatewayMock.settingsResult(null);
    render(<App />);
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Agent target" }).textContent).toContain("Codex");
    });
    const startsBefore = gatewayMock.requestLog.filter((entry) => entry.method === "thread/start").length;
    const requestsBefore = gatewayMock.requestLog.length;

    const popover = await openAgentRuntimePopover();
    delayAuxiliaryRefresh = true;
    fireEvent.click(within(popover).getByRole("radio", { name: "Start a new thread with OpenCode · OpenCode (ACP)" }));

    expect(screen.getByRole("button", { name: "Agent target" }).textContent).toContain("OpenCode");
    expect((screen.getByRole("button", { name: "Agent target" }) as HTMLButtonElement).disabled).toBe(true);
    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/start")).toHaveLength(startsBefore + 1);
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/draft/prepare",
        params: {
          scope: preparedScope,
          targetId: "target:opencode:opencode"
        }
      });
    });
    const switchMethods = gatewayMock.requestLog.slice(requestsBefore).map((entry) => entry.method);
    const threadStartIndex = switchMethods.indexOf("thread/start");
    const draftPrepareIndex = switchMethods.indexOf("thread/draft/prepare", threadStartIndex + 1);
    expect(switchMethods.slice(threadStartIndex, draftPrepareIndex + 1)).toEqual([
      "thread/start",
      "thread/draft/prepare"
    ]);
    await waitFor(() => {
      const settingsIndex = gatewayMock.requestLog
        .slice(requestsBefore)
        .findIndex((entry) => entry.method === "settings/read");
      expect(settingsIndex).toBeGreaterThan(draftPrepareIndex);
    });
    delayedSettings.resolve(gatewayMock.settingsResult(null));
    await new Promise((resolve) => window.setTimeout(resolve, 0));
    expect(gatewayMock.requestLog.filter((entry) => entry.method === "thread/start")).toHaveLength(startsBefore + 1);
    await waitFor(() => {
      expect(gatewayMock.requestLog.slice(requestsBefore).some((entry) => entry.method === "command/list"))
        .toBe(true);
    });
    const refreshedCommandScope = gatewayMock.requestLog
      .slice(requestsBefore)
      .filter((entry) => entry.method === "command/list")
      .at(-1)?.params;
    expect(refreshedCommandScope).toEqual({ scope: preparedScope, threadId: null });
    const targetReads = gatewayMock.requestLog
      .slice(requestsBefore)
      .filter((entry) => entry.method === "thread/context/read")
      .map((entry) => (entry.params as { target?: unknown }).target);
    expect(targetReads).not.toContain(null);
    expect(targetReads).toEqual([]);
  });

  it("refreshes authoritative Thread context after a completed turn", async () => {
    useFirstClassContexts();
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Bound ACP thread")];
    render(<App />);

    fireEvent.click(await screen.findByText("Bound ACP thread"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/context/read",
        params: expect.objectContaining({ threadId: "thread-1" })
      });
    });
    const readsBeforeCompletion = gatewayMock.requestLog.filter((entry) => (
      entry.method === "thread/context/read"
      && (entry.params as { threadId?: string | null }).threadId === "thread-1"
    )).length;

    await act(async () => {
      for (const subscriber of gatewayMock.subscribers) {
        subscriber({
          method: "gateway/event",
          params: {
            type: "turnCompleted",
            threadId: "thread-1",
            turnId: "turn-1",
            turn: {
              id: "turn-1",
              threadId: "thread-1",
              status: "completed",
              outcome: "normal",
              error: null,
              startedAtMs: 1,
              completedAtMs: 2
            },
            committedEntries: []
          }
        });
      }
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => (
        entry.method === "thread/context/read"
        && (entry.params as { threadId?: string | null }).threadId === "thread-1"
      )).length).toBeGreaterThan(readsBeforeCompletion);
    });
  });

  it("refreshes authoritative Thread context after shell catalog changes", async () => {
    useFirstClassContexts();
    gatewayMock.sessionSummaries = [sessionSummary("thread-1", "Bound ACP thread")];
    render(<App />);

    fireEvent.click(await screen.findByText("Bound ACP thread"));
    await waitFor(() => {
      expect(gatewayMock.requestLog).toContainEqual({
        method: "thread/context/read",
        params: expect.objectContaining({ threadId: "thread-1" })
      });
    });
    const readsBeforeShell = gatewayMock.requestLog.filter((entry) => (
      entry.method === "thread/context/read"
      && (entry.params as { threadId?: string | null }).threadId === "thread-1"
    )).length;

    act(() => {
      for (const subscriber of gatewayMock.subscribers) {
        subscriber({
          method: "shell/result",
          params: { thread: { id: "thread-1" } }
        });
      }
    });

    await waitFor(() => {
      expect(gatewayMock.requestLog.filter((entry) => (
        entry.method === "thread/context/read"
        && (entry.params as { threadId?: string | null }).threadId === "thread-1"
      )).length).toBeGreaterThan(readsBeforeShell);
    });
  });
});

function useFirstClassContexts() {
  gatewayMock.runtimeContextRead = (params) => firstClassContext(requestedProfile(params));
}

function requestedProfile(params: unknown): FirstClassProfileRef {
  const value = (params as { target?: { runtimeProfileRef?: unknown } | null } | null)?.target?.runtimeProfileRef;
  return value === "codex" || value === "opencode" ? value : "native";
}

function firstClassContext(
  runtimeProfileRef: FirstClassProfileRef,
  overrides: Record<string, unknown> = {}
): Record<string, unknown> {
  const revision = runtimeProfileRef === "native" ? 1 : runtimeProfileRef === "codex" ? 2 : 3;
  return {
    runtimeProfileRef,
    selectionState: "draft",
    profiles: gatewayMock.runtimeProfileRecords,
    binding: null,
    controls: [
      control({
        id: "mode",
        label: "Mode",
        surfaceRole: "mode",
        effectiveValue: "default",
        choices: [
          { value: "default", label: "Default", description: null },
          { value: "plan", label: "Plan", description: null }
        ],
        capabilityRevision: String(revision)
      }),
      control({
        id: "model",
        label: "Model",
        surfaceRole: "model",
        effectiveValue: `${runtimeProfileRef}/model-a`,
        choices: [
          { value: `${runtimeProfileRef}/model-a`, label: "Model A", description: null },
          { value: `${runtimeProfileRef}/model-b`, label: "Model B", description: null }
        ],
        capabilityRevision: String(10 + revision)
      }),
      control({
        id: "reasoning",
        label: "Reasoning",
        surfaceRole: "reasoning",
        effectiveValue: "medium",
        choices: [
          { value: "medium", label: "Medium", description: null },
          { value: "high", label: "High", description: null }
        ],
        capabilityRevision: String(20 + revision)
      })
    ],
    stability: "stable",
    capabilities: [{
      id: "turn.start",
      enabled: true,
      stability: "stable",
      unavailableReason: null
    }],
    compatibleTargets: [
      {
        targetId: "target:default:native",
        agentRef: null,
        runtimeProfileRef: "native",
        agentLabel: "Psychevo",
        profileLabel: "Psychevo (Native)",
        label: "Psychevo · Psychevo (Native)",
        ready: true,
        unavailableReason: null
      },
      {
        targetId: "target:codex:codex",
        agentRef: "codex",
        runtimeProfileRef: "codex",
        agentLabel: "Codex",
        profileLabel: "Codex (ACP)",
        label: "Codex · Codex (ACP)",
        ready: true,
        unavailableReason: null
      },
      {
        targetId: "target:opencode:opencode",
        agentRef: "opencode",
        runtimeProfileRef: "opencode",
        agentLabel: "OpenCode",
        profileLabel: "OpenCode (ACP)",
        label: "OpenCode · OpenCode (ACP)",
        ready: true,
        unavailableReason: null
      }
    ],
    inputCapabilities: [
      { kind: "text", enabled: true, unavailableReason: null },
      { kind: "agentMention", enabled: true, unavailableReason: null }
    ],
    actions: [],
    sendability: { allowed: true, reason: null, recoveryAction: null },
    history: {
      owner: runtimeProfileRef === "native" ? "psychevo" : "agent",
      fidelity: "full",
      cursor: null,
      hint: null
    },
    pendingInteractions: [],
    contextRevision: String(100 + revision),
    controlRevision: String(200 + revision),
    ...overrides
  };
}

function control(overrides: Record<string, unknown>): Record<string, unknown> {
  return {
    id: "model",
    label: "Model",
    surfaceRole: "model",
    mutability: "selectable",
    enabled: true,
    required: false,
    unavailableReason: null,
    effectiveValue: null,
    effectiveSource: "runtimeDefault",
    isDefault: false,
    choices: [],
    dependsOn: null,
    applyScope: "turnDraft",
    stability: "stable",
    channelSafe: true,
    capabilityRevision: "1",
    ...overrides
  };
}

async function turnStartParams(): Promise<Record<string, unknown>> {
  await waitFor(() => {
    expect(gatewayMock.requestLog.map((entry) => entry.method)).toContain("turn/start");
  });
  const entry = gatewayMock.requestLog.filter((candidate) => candidate.method === "turn/start").at(-1);
  return entry?.params as Record<string, unknown>;
}
