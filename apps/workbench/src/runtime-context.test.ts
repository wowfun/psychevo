import { describe, expect, it } from "vitest";
import {
  parseThreadContext as parseRawThreadContext,
  runnableTargetUnavailableReason,
  runtimeControlSelections,
  runtimeProfileCapsuleLabel,
  runtimeProfileDisplayLabel,
  shouldRetainFirstTurnDraftContext
} from "./runtime-context";

function parseThreadContext(value: Record<string, unknown>) {
  return parseRawThreadContext({
    selectedTargetId: "target:test",
    suggestedTargetId: null,
    ...value
  });
}
describe("runtime context projection", () => {
  it("parses the Thread Context interface without trusting raw payloads", () => {
    const context = parseThreadContext({
      runtimeProfileRef: "codex",
      selectionState: "bound",
      profiles: [{
        id: "codex",
        runtime: "acp",
        enabled: true,
        label: "Codex",
        generated: true,
        configured: false,
        backendRef: "codex",
        provenance: "ACP",
        capabilityRevision: "9007199254740993",
        profileRevision: "18446744073709551615",
        stability: "stable",
        capabilities: [{
          id: "turn.start",
          enabled: true,
          stability: "stable",
          unavailableReason: null
        }],
        health: { status: "ready", summary: "Ready", checkedAtMs: 100 },
        readinessStages: [{ id: "transport", status: "ready", summary: "Connected", observedAtMs: 100 }]
      }],
      binding: {
        threadId: "thread-1",
        runtimeRef: "codex",
        backendKind: "acp",
        nativeKind: null,
        sessionHandle: "codex-session",
        cwd: "/tmp/project",
        profileFingerprint: "fingerprint",
        ownership: "readWrite",
        bindingRevision: 2
      },
      controls: [{
        id: "mode",
        label: "Mode",
        surfaceRole: "mode",
        mutability: "readOnly",
        enabled: true,
        required: false,
        unavailableReason: null,
        effectiveValue: "review",
        effectiveSource: "runtimeObserved",
        isDefault: false,
        choices: [],
        applyScope: "session",
        stability: "stable",
        channelSafe: true,
        capabilityRevision: "9007199254740993"
      }],
      stability: "stable",
      capabilities: [{
        id: "turn.start",
        enabled: true,
        stability: "stable",
        unavailableReason: "A newer capability-pack schema is required."
      }],
      compatibleTargets: [{
        targetId: "target:codex:codex",
        agentRef: "codex",
        runtimeProfileRef: "codex",
        agentLabel: "Codex",
        profileLabel: "Codex (ACP)",
        label: "Codex · Codex (ACP)",
        ready: true,
        unavailableReason: null
      }],
      inputCapabilities: [{ kind: "text", enabled: true, unavailableReason: null }],
      actions: [{
        id: "session.fork",
        label: "Fork",
        enabled: true,
        stability: "stable",
        channelSafe: false,
        unavailableReason: null
      }, {
        id: "compact",
        label: "Compact",
        enabled: true,
        stability: "stable",
        channelSafe: true,
        unavailableReason: null
      }],
      sendability: { allowed: true, reason: null, recoveryAction: null },
      history: { owner: "agent", fidelity: "summary", cursor: "next", hint: "Agent history" },
      pendingInteractions: [],
      contextRevision: "7",
      controlRevision: "9",
      ignoredNativeSecret: "must-not-cross-interface"
    });

    expect(context.binding).toMatchObject({ runtimeRef: "codex", bindingRevision: 2 });
    expect(context.profiles[0]).toMatchObject({
      profileRevision: "18446744073709551615",
      capabilityRevision: "9007199254740993"
    });
    expect(context.controls[0]).toMatchObject({
      id: "mode",
      surfaceRole: "mode",
      mutability: "readOnly",
      effectiveValue: "review",
      effectiveSource: "runtimeObserved",
      applyScope: "session",
      capabilityRevision: "9007199254740993"
    });
    expect(context.stability).toBe("stable");
    expect(context.capabilities).toEqual([{
      id: "turn.start",
      enabled: true,
      stability: "stable",
      unavailableReason: "A newer capability-pack schema is required."
    }]);
    expect(context.compatibleTargets[0]).toMatchObject({ runtimeProfileRef: "codex", ready: true });
    expect(context.inputCapabilities).toEqual([{ kind: "text", enabled: true, unavailableReason: null }]);
    expect(context.actions).toEqual([{
      id: "compact",
      label: "Compact",
      enabled: true,
      stability: "stable",
      channelSafe: true,
      unavailableReason: null
    }]);
    expect(context.sendability).toEqual({ allowed: true, reason: null, recoveryAction: null });
    expect(context.history).toEqual({ owner: "agent", fidelity: "summary", cursor: "next", hint: "Agent history" });
    expect(context.contextRevision).toBe("7");
    expect(context.controlRevision).toBe("9");
    expect(JSON.stringify(context)).not.toContain("must-not-cross-interface");
    expect(runtimeProfileCapsuleLabel(context.profiles[0]!)).toBe("Codex (ACP) · ACP");
  });

  it("uses the compatible target reason without inferring from implementation kind", () => {
    expect(runnableTargetUnavailableReason({
      targetId: "target:reviewer:opaque",
      agentRef: "reviewer",
      runtimeProfileRef: "opaque-profile",
      agentLabel: "Reviewer",
      profileLabel: "Opaque",
      label: "Reviewer",
      ready: false,
      unavailableReason: "The projected target is not ready."
    })).toBe("The projected target is not ready.");
  });

  it("preserves every typed history action while rejecting opaque action ids", () => {
    const context = parseThreadContext({
      actions: [
        "fork",
        "forkBefore",
        "revertConversation",
        "unrevertConversation",
        "opaque.history.action"
      ].map((id) => ({
        id,
        label: id,
        enabled: true,
        stability: "stable",
        channelSafe: false,
        unavailableReason: null
      }))
    });

    expect(context.actions.map((action) => action.id)).toEqual([
      "fork",
      "forkBefore",
      "revertConversation",
      "unrevertConversation"
    ]);
  });

  it("never coerces public runtime revisions through JavaScript numbers", () => {
    const context = parseThreadContext({
      runtimeProfileRef: "codex",
      profiles: [{
        id: "codex",
        runtime: "acp",
        label: "Codex",
        profileRevision: 9_007_199_254_740_993,
        capabilityRevision: "018",
        health: { status: "ready", summary: "Ready" }
      }],
      controls: [{
        id: "mode",
        label: "Mode",
        surfaceRole: "mode",
        mutability: "selectable",
        enabled: true,
        required: false,
        capabilityRevision: "5db92a55f2f24d87"
      }]
    });

    expect(context.profiles[0]?.profileRevision).toBe("0");
    expect(context.profiles[0]?.capabilityRevision).toBe("018");
    expect(context.controls[0]?.capabilityRevision).toBe("5db92a55f2f24d87");
  });

  it("keeps discovery unselected when the context has no selected target", () => {
    const context = parseRawThreadContext({
      profiles: [{ id: "opaque-profile", runtime: "acp", label: "Opaque" }]
    });
    expect(context.selectedTargetId).toBeNull();
    expect(context.suggestedTargetId).toBeNull();
  });

  it("rejects only the non-effective context observed after an exact first-turn selection", () => {
    const current = parseThreadContext({
      selectionState: "prospective",
      compatibleTargets: [{
        targetId: "target:test",
        agentRef: null,
        runtimeProfileRef: "native",
        ready: true
      }]
    });
    const observed = parseRawThreadContext({
      selectedTargetId: null,
      suggestedTargetId: "target:test",
      selectionState: "default",
      binding: null,
      compatibleTargets: current.compatibleTargets
    });

    expect(shouldRetainFirstTurnDraftContext(current, observed, true, "thread-1")).toBe(true);
    expect(shouldRetainFirstTurnDraftContext(current, observed, false, "thread-1")).toBe(false);
    expect(shouldRetainFirstTurnDraftContext(current, {
      ...observed,
      selectedTargetId: "target:test",
      suggestedTargetId: null,
      selectionState: "bound",
      binding: {
        threadId: "thread-1",
        agentRef: null,
        agentFingerprint: "native-agent-fingerprint",
        runtimeRef: "native",
        backendKind: "native",
        nativeKind: "native",
        sessionHandle: "native-session",
        cwd: "/tmp/project",
        profileFingerprint: "native-fingerprint",
        ownership: "readWrite",
        bindingRevision: 1
      }
    }, true, "thread-1")).toBe(false);
  });

  it("normalizes ACP suffixes and fails malformed enums to conservative defaults", () => {
    const context = parseThreadContext({
      profiles: [{
        id: "acp:cursor",
        runtime: "acp",
        label: "Cursor (ACP)",
        health: { status: "unchecked", summary: "Not checked" }
      }],
      binding: { ownership: "unexpected" },
      controls: [{ id: "mode", surfaceRole: "unexpected", mutability: "unexpected" }],
      history: { owner: "runtime" }
    });

    expect(context.runtimeProfileRef).toBe("");
    expect(runtimeProfileDisplayLabel(context.profiles[0]!)).toBe("Cursor (ACP)");
    expect(context.binding?.ownership).toBe("readOnly");
    expect(context.history.owner).toBe("psychevo");
    expect(context.controls[0]).toMatchObject({
      surfaceRole: "advanced",
      mutability: "readOnly",
      effectiveSource: "runtimeDefault",
      applyScope: "turnDraft",
      stability: "unavailable"
    });
  });

  it("serializes only explicit selectable runtime drafts by descriptor id", () => {
    const controls = parseThreadContext({
      controls: [
        { id: "mode", mutability: "selectable", enabled: true, choices: [{ value: "plan", label: "Plan" }] },
        { id: "effort", mutability: "selectable", enabled: true, choices: [{ value: "high", label: "High" }] },
        { id: "feature.fast", mutability: "selectable", enabled: true, choices: [{ value: true, label: "Fast" }] },
        { id: "model", mutability: "readOnly", enabled: true, effectiveValue: "observed/model", effectiveSource: "runtimeObserved" }
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
      "feature.fast": true
    });
    expect(runtimeControlSelections(controls, {})).toEqual({});
  });

  it("never serializes a disabled control draft", () => {
    const controls = parseThreadContext({
      controls: [{
        id: "opaque.control",
        label: "Opaque control",
        mutability: "selectable",
        enabled: false,
        required: true,
        unavailableReason: "The Adapter has not implemented this control.",
        choices: [{ value: "on", label: "On" }]
      }]
    }).controls;

    expect(runtimeControlSelections(controls, { "opaque.control": "on" })).toEqual({});
    expect(controls[0]).toMatchObject({
      enabled: false,
      required: true,
      unavailableReason: "The Adapter has not implemented this control."
    });
  });

  it("drops dependent selections when their catalog model no longer matches", () => {
    const controls = parseThreadContext({
      controls: [
        {
          id: "model",
          surfaceRole: "model",
          mutability: "selectable",
          enabled: true,
          effectiveValue: "model-a",
          choices: [
            { value: "model-a", label: "Model A" },
            { value: "model-b", label: "Model B" }
          ]
        },
        {
          id: "effort",
          surfaceRole: "reasoning",
          mutability: "selectable",
          enabled: true,
          effectiveValue: "high",
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
