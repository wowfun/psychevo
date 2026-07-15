// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { parseThreadContext } from "./runtime-context";
import { ComposerRuntimeControls, RuntimeControlFields } from "./runtime-controls";

afterEach(cleanup);

describe("runtime composer controls", () => {
  it("presents Native, Codex ACP, and OpenCode ACP through one compatible-target selector", () => {
    const context = firstClassContext("codex");
    render(
      <ComposerRuntimeControls
        binding={null}
        controls={context.controls}
        profiles={context.profiles}
        targets={context.compatibleTargets}
        controlValues={{}}
        disabled={false}
        targetId="target:codex:codex"
        contextError={null}
        contextLoading={false}
        onTargetChange={vi.fn()}
        onControlChange={vi.fn()}
      />
    );

    expect(screen.queryByRole("combobox", { name: "Permission mode" })).toBeNull();
    expect(screen.getByRole("combobox", { name: "Mode" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Agent target" }));

    const dialog = screen.getByRole("dialog", { name: "Agent target" });
    const targets = within(dialog).getByRole("radiogroup", { name: "Agent target" });
    expect(within(targets).getByRole("radio", { name: "Psychevo · Psychevo (Native)" })).toBeTruthy();
    expect(within(targets).getByRole("radio", { name: "Codex · Codex (ACP)" })).toBeTruthy();
    expect(within(targets).getByRole("radio", { name: "OpenCode · OpenCode (ACP)" })).toBeTruthy();
    expect(within(targets).getByRole("radio", { name: "Codex · Codex (ACP)" }).textContent).toContain("Codex (ACP)");
    expect(within(targets).getByRole("radio", { name: "Codex · Codex (ACP)" }).textContent).not.toContain("Codex ·");
    expect(within(dialog).queryByText("Agent target")).toBeNull();
    expect(within(dialog).queryByText("Manage Agent targets")).toBeNull();
    expect(within(dialog).queryByRole("combobox", { name: "Permission mode" })).toBeNull();
    expect(within(dialog).getByLabelText("Approval: ask (read-only)")).toBeTruthy();
  });

  it.each(["native", "codex", "opencode"] as const)(
    "shows the same descriptor-driven Mode draft for %s",
    (runtimeProfileRef) => {
      const context = firstClassContext(runtimeProfileRef);
      render(
        <ComposerRuntimeControls
          binding={null}
          controls={context.controls}
          profiles={context.profiles}
          targets={context.compatibleTargets}
          controlValues={{ mode: "plan" }}
          disabled={false}
          targetId={`target:${runtimeProfileRef === "native" ? "default" : runtimeProfileRef}:${runtimeProfileRef}`}
          contextError={null}
          contextLoading={false}
          onTargetChange={vi.fn()}
          onControlChange={vi.fn()}
        />
      );

      expect((screen.getByRole("combobox", { name: "Mode" }) as HTMLSelectElement).value).toBe("1");
    }
  );

  it.each(["native", "codex", "opencode"] as const)(
    "projects %s model and reasoning through the same control descriptors",
    (runtimeProfileRef) => {
      const context = firstClassContext(runtimeProfileRef);
      const onChange = vi.fn();
      render(
        <RuntimeControlFields
          controls={context.controls.filter((control) => (
            control.surfaceRole === "model" || control.surfaceRole === "reasoning"
          ))}
          dependencyControls={context.controls}
          disabled={false}
          values={{}}
          onChange={onChange}
        />
      );

      expect((screen.getByRole("combobox", { name: "Model" }) as HTMLSelectElement).value).toBe("0");
      expect(screen.getByLabelText("Reasoning: high (read-only)")).toBeTruthy();
      fireEvent.change(screen.getByRole("combobox", { name: "Model" }), { target: { value: "1" } });
      expect(onChange).toHaveBeenCalledWith(
        context.controls.find((control) => control.id === "model"),
        `${runtimeProfileRef}/model-b`
      );
    }
  );
});

function firstClassContext(runtimeProfileRef: "native" | "codex" | "opencode") {
  return parseThreadContext({
    targetId: `target:${runtimeProfileRef === "native" ? "default" : runtimeProfileRef}:${runtimeProfileRef}`,
    runtimeProfileRef,
    selectionState: "draft",
    profiles: [
      {
        id: "native",
        runtime: "native",
        label: "Psychevo (Native)",
        enabled: true,
        provenance: "Native",
        health: { status: "ready", summary: "Ready" }
      },
      {
        id: "codex",
        runtime: "acp",
        label: "Codex",
        backendRef: "codex",
        enabled: true,
        provenance: "ACP",
        health: { status: "ready", summary: "Ready" }
      },
      {
        id: "opencode",
        runtime: "acp",
        label: "OpenCode",
        backendRef: "opencode",
        enabled: true,
        provenance: "ACP",
        health: { status: "ready", summary: "Ready" }
      }
    ],
    binding: null,
    controls: [
      {
        id: "mode",
        label: "Mode",
        surfaceRole: "mode",
        mutability: "selectable",
        enabled: true,
        required: false,
        unavailableReason: null,
        effectiveValue: "default",
        effectiveSource: "runtimeDefault",
        isDefault: true,
        choices: [
          { value: "default", label: "Default" },
          { value: "plan", label: "Plan" }
        ],
        applyScope: "turnDraft",
        stability: "stable",
        channelSafe: true,
        capabilityRevision: "11"
      },
      {
        id: "model",
        label: "Model",
        surfaceRole: "model",
        mutability: "selectable",
        enabled: true,
        required: true,
        unavailableReason: null,
        effectiveValue: `${runtimeProfileRef}/model-a`,
        effectiveSource: "runtimeObserved",
        isDefault: false,
        choices: [
          { value: `${runtimeProfileRef}/model-a`, label: "Model A" },
          { value: `${runtimeProfileRef}/model-b`, label: "Model B" }
        ],
        applyScope: "session",
        stability: "stable",
        channelSafe: false,
        capabilityRevision: "12"
      },
      {
        id: "reasoning",
        label: "Reasoning",
        surfaceRole: "reasoning",
        mutability: "readOnly",
        enabled: true,
        required: false,
        unavailableReason: null,
        effectiveValue: "high",
        effectiveSource: "runtimeObserved",
        isDefault: false,
        choices: [],
        applyScope: "session",
        stability: "stable",
        channelSafe: false,
        capabilityRevision: "13"
      },
      {
        id: "approval",
        label: "Approval",
        surfaceRole: "advanced",
        mutability: "readOnly",
        enabled: true,
        required: false,
        unavailableReason: null,
        effectiveValue: "ask",
        effectiveSource: "profileDefault",
        isDefault: true,
        choices: [],
        applyScope: "session",
        stability: "stable",
        channelSafe: false,
        capabilityRevision: "14"
      },
      {
        id: "permissionMode",
        label: "Permission mode",
        surfaceRole: "advanced",
        mutability: "selectable",
        enabled: true,
        required: false,
        unavailableReason: null,
        effectiveValue: "default",
        effectiveSource: "runtimeDefault",
        isDefault: true,
        choices: [
          { value: "default", label: "default" },
          { value: "acceptEdits", label: "acceptEdits" },
          { value: "dontAsk", label: "dontAsk" },
          { value: "bypassPermissions", label: "bypassPermissions" }
        ],
        applyScope: "turnDraft",
        stability: "stable",
        channelSafe: true,
        capabilityRevision: "15"
      }
    ],
    compatibleTargets: [
      {
        targetId: "target:default:native",
        agentRef: null,
        runtimeProfileRef: "native",
        agentLabel: "Psychevo",
        profileLabel: "Psychevo (Native)",
        label: "Psychevo · Psychevo (Native)",
        ready: true
      },
      {
        targetId: "target:codex:codex",
        agentRef: "codex",
        runtimeProfileRef: "codex",
        agentLabel: "Codex",
        profileLabel: "Codex (ACP)",
        label: "Codex · Codex (ACP)",
        ready: true
      },
      {
        targetId: "target:opencode:opencode",
        agentRef: "opencode",
        runtimeProfileRef: "opencode",
        agentLabel: "OpenCode",
        profileLabel: "OpenCode (ACP)",
        label: "OpenCode · OpenCode (ACP)",
        ready: true
      }
    ],
    inputCapabilities: [
      { kind: "text", enabled: true },
      { kind: "agentMention", enabled: true }
    ],
    actions: [],
    sendability: { allowed: true },
    history: { owner: runtimeProfileRef === "native" ? "psychevo" : "agent", fidelity: "full" },
    pendingInteractions: [],
    contextRevision: "21",
    controlRevision: "22"
  });
}
