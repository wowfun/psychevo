import { useEffect, useRef, useState, type RefObject } from "react";
import { Check } from "lucide-react";
import type {
  RunnableTargetView,
  RuntimeBindingView,
  RuntimeProfileView,
  ThreadControlDescriptorView
} from "@psychevo/protocol";
import { StatusSelect } from "./composer-controls";
import {
  runtimeControlDependencyMatches,
  runtimeControlValueLabel,
  runnableTargetUnavailableReason
} from "./runtime-context";
import type { WorkbenchAgent } from "./types";

export function ComposerRuntimeControls({
  binding,
  controls,
  profiles = [],
  targets = [],
  controlValues,
  disabled,
  targetId,
  contextError,
  contextLoading,
  onTargetChange,
  onControlChange
}: {
  binding: RuntimeBindingView | null;
  controls: ThreadControlDescriptorView[];
  profiles?: RuntimeProfileView[];
  targets?: RunnableTargetView[];
  controlValues: Record<string, unknown>;
  disabled: boolean;
  targetId: string;
  contextError: string | null;
  contextLoading: boolean;
  onTargetChange(targetId: string): void;
  onControlChange(control: ThreadControlDescriptorView, value: unknown): void;
}) {
  const modeControls = controls.filter((control) => control.surfaceRole === "mode");
  return (
    <div className="composerRuntimeControls" aria-label="Runtime controls">
      <AgentRuntimeSelector
        binding={binding}
        controls={controls}
        profiles={profiles}
        targets={targets}
        controlValues={controlValues}
        disabled={disabled}
        targetId={targetId}
        contextLoading={contextLoading}
        onTargetChange={onTargetChange}
        onControlChange={onControlChange}
      />
      <RuntimeControlFields
        controls={modeControls}
        disabled={disabled || contextLoading}
        values={controlValues}
        onChange={onControlChange}
      />
      {contextError && <span className="runtimeModeHint is-error" title={contextError}>Runtime context unavailable</span>}
    </div>
  );
}

function AgentRuntimeSelector({
  binding,
  controls,
  profiles,
  targets,
  controlValues,
  disabled,
  targetId,
  contextLoading,
  onTargetChange,
  onControlChange
}: {
  binding: RuntimeBindingView | null;
  controls: ThreadControlDescriptorView[];
  profiles: RuntimeProfileView[];
  targets: RunnableTargetView[];
  controlValues: Record<string, unknown>;
  disabled: boolean;
  targetId: string;
  contextLoading: boolean;
  onTargetChange(targetId: string): void;
  onControlChange(control: ThreadControlDescriptorView, value: unknown): void;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const selectedTarget = targets.find((target) => target.targetId === targetId) ?? null;
  const agentLabel = selectedTarget?.agentLabel || "Select Agent";
  const profileLabel = selectedTarget?.profileLabel || "Runtime Profile";
  const displayLabel = selectedTarget ? agentTargetDisplayLabel(selectedTarget, profiles) : agentLabel;
  const optionControls = controls.filter((control) => control.surfaceRole === "advanced");
  const selectedPairingReason = runnableTargetUnavailableReason(selectedTarget);
  const startsNewBoundThread = binding != null;
  usePopoverDismiss(open, rootRef, () => setOpen(false));

  return (
    <div ref={rootRef} className="agentRuntimeSelector" onKeyDown={(event) => {
      if (event.key === "Escape") setOpen(false);
    }}>
      <button
        aria-expanded={open}
        aria-haspopup="dialog"
        aria-label="Agent target"
        className="agentRuntimeButton"
        disabled={disabled || contextLoading}
        onClick={() => setOpen((current) => !current)}
        title={`${agentLabel} · ${profileLabel}`}
        type="button"
      >
        <span>{displayLabel}</span>
      </button>
      {open && (
        <div className="agentRuntimePopover agentDefinitionPopover" role="dialog" aria-label="Agent target">
          <div className="agentRuntimeGroup">
            <div className="agentRuntimeRows" role="radiogroup" aria-label="Agent target">
              {targets.map((target) => {
                const selected = target.targetId === targetId;
                return (
                  <AgentRuntimeRow
                    key={target.targetId}
                    ariaLabel={startsNewBoundThread && !selected ? `Start a new thread with ${target.label}` : target.label}
                    disabled={disabled || !target.ready || (startsNewBoundThread && selected)}
                    label={agentTargetDisplayLabel(target, profiles)}
                    selected={selected}
                    title={!target.ready
                      ? target.unavailableReason ?? `${target.label} is not ready.`
                      : startsNewBoundThread
                        ? selected
                          ? "Immutable provenance for the current Thread."
                          : "Starts a new Thread; the current Thread remains bound to its existing Agent target."
                        : target.label}
                    onSelect={() => {
                      setOpen(false);
                      onTargetChange(target.targetId);
                    }}
                  />
                );
              })}
            </div>
          </div>
          {selectedPairingReason && <div className="agentRuntimeHint is-warning">{selectedPairingReason}</div>}
          {optionControls.length > 0 && (
            <div className="agentRuntimeOptions">
              <div className="agentRuntimeDivider" />
              <div className="agentRuntimeGroupLabel">Runtime Options</div>
              <RuntimeControlFields
                controls={optionControls}
                dependencyControls={controls}
                disabled={disabled || contextLoading}
                values={controlValues}
                onChange={onControlChange}
              />
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export function RuntimeControlFields({
  controls,
  dependencyControls = controls,
  disabled,
  hideDefaults = false,
  values,
  onChange
}: {
  controls: ThreadControlDescriptorView[];
  dependencyControls?: ThreadControlDescriptorView[];
  disabled: boolean;
  hideDefaults?: boolean;
  values: Record<string, unknown>;
  onChange(control: ThreadControlDescriptorView, value: unknown): void;
}) {
  const visibleControls = controls.filter((control) =>
    runtimeControlDependencyMatches(control, dependencyControls, values)
    && !(
      hideDefaults
      && control.isDefault
      && valuesEqual(
        Object.prototype.hasOwnProperty.call(values, control.id) ? values[control.id] : control.effectiveValue,
        control.effectiveValue
      )
    )
  );
  if (visibleControls.length === 0) return null;
  return <>{visibleControls.map((control) => {
    const value = Object.prototype.hasOwnProperty.call(values, control.id)
      ? values[control.id]
      : control.effectiveValue;
    const valueLabel = runtimeControlValueLabel(control, value);
    if (!control.enabled || control.mutability === "readOnly" || control.choices.length === 0) {
      return (
        <span
          aria-label={`${control.label}: ${valueLabel} (${control.enabled ? "read-only" : "unavailable"})`}
          className={`runtimeControlState is-readonly ${value == null ? "is-unavailable" : ""}`}
          key={control.id}
          title={control.unavailableReason ?? `${runtimeControlSourceLabel(control)} · read-only`}
        >
          {control.label}: {valueLabel}
        </span>
      );
    }
    const unavailableKey = "__unavailable__";
    const selectedIndex = control.choices.findIndex((choice) => valuesEqual(choice.value, value));
    const selectedKey = selectedIndex < 0 ? unavailableKey : String(selectedIndex);
    const optionKeys = selectedIndex < 0
      ? [unavailableKey, ...control.choices.map((_, index) => String(index))]
      : control.choices.map((_, index) => String(index));
    const optionLabels: Record<string, string> = {
      [unavailableKey]: control.surfaceRole === "model" ? "Unavailable" : `Choose ${control.label}`,
      ...Object.fromEntries(control.choices.map((choice, index) => [String(index), choice.label]))
    };
    return (
      <StatusSelect
        disabled={disabled}
        key={control.id}
        label={control.label}
        optionLabels={optionLabels}
        renderDisplayValue={(key) => optionLabels[key] ?? valueLabel}
        value={selectedKey}
        values={optionKeys}
        onChange={(key) => {
          if (key === unavailableKey) return;
          const choice = control.choices[Number(key)];
          if (choice) onChange(control, choice.value);
        }}
      />
    );
  })}</>;
}

function runtimeControlSourceLabel(control: ThreadControlDescriptorView): string {
  switch (control.effectiveSource) {
    case "profileDefault": return "Profile default";
    case "sourceDraft": return "Source draft";
    case "threadPreference": return "Thread preference";
    case "turnOverride": return "Turn override";
    case "runtimeObserved": return "Observed runtime value";
    case "runtimeDefault": return "Runtime default";
  }
}

function AgentRuntimeRow({
  ariaLabel,
  disabled,
  label,
  selected,
  title,
  onSelect
}: {
  ariaLabel?: string | undefined;
  disabled: boolean;
  label: string;
  selected: boolean;
  title?: string | undefined;
  onSelect(): void;
}) {
  return (
    <button
      aria-checked={selected}
      aria-label={ariaLabel}
      className={`agentRuntimeRow ${selected ? "is-selected" : ""}`}
      disabled={disabled}
      onClick={onSelect}
      role="radio"
      title={title}
      type="button"
    >
      <span>{label}</span>
      {selected && <Check aria-hidden="true" size={13} />}
    </button>
  );
}

function agentTargetDisplayLabel(target: RunnableTargetView, profiles: RuntimeProfileView[]): string {
  const profile = profiles.find((candidate) => candidate.id === target.runtimeProfileRef) ?? null;
  return profile?.runtime.toLowerCase() === "acp"
    ? `${target.agentLabel} (ACP)`
    : target.agentLabel;
}

function usePopoverDismiss(open: boolean, rootRef: RefObject<HTMLDivElement | null>, close: () => void) {
  useEffect(() => {
    if (!open) return;
    const closeOnOutsidePointer = (event: PointerEvent) => {
      const target = event.target;
      if (target instanceof Node && rootRef.current?.contains(target)) return;
      close();
    };
    document.addEventListener("pointerdown", closeOnOutsidePointer);
    return () => document.removeEventListener("pointerdown", closeOnOutsidePointer);
  }, [close, open, rootRef]);
}

function valuesEqual(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true;
  try {
    return JSON.stringify(left) === JSON.stringify(right);
  } catch {
    return false;
  }
}

function hasReadyTargetForAgent(
  targets: RunnableTargetView[],
  agent: WorkbenchAgent | null,
  selectedAgentRef: string
): boolean {
  if (targets.length === 0) return false;
  if (!selectedAgentRef) {
    return targets.some((target) => target.agentRef == null && target.ready);
  }
  const refs = new Set([selectedAgentRef, agent?.name].filter((value): value is string => Boolean(value)));
  return targets.some((target) => (
    target.ready && target.agentRef != null && refs.has(target.agentRef)
  ));
}

export function agentOptionValue(agent: WorkbenchAgent): string {
  return agent.source === "explicit" ? agent.path?.trim() || agent.name : agent.name;
}

export function runtimeControlAsConfigOption(
  control: ThreadControlDescriptorView | null
): RuntimeModeOption | null {
  if (!control) return null;
  const values = control.choices.flatMap((choice): RuntimeModeValue[] => (
    typeof choice.value === "string"
      ? [{ value: choice.value, name: choice.label, description: choice.description, group: null }]
      : []
  ));
  return {
    id: control.id,
    name: control.label,
    description: null,
    category: control.surfaceRole === "mode" ? "mode" : null,
    type: values.length > 0 ? "select" : "readonly",
    currentValue: typeof control.effectiveValue === "string" ? control.effectiveValue : null,
    values
  };
}

export type RuntimeModeProjection = {
  allValues: RuntimeModeValue[];
  defaultValue: string;
  extraValues: RuntimeModeValue[];
  supportsPlan: boolean;
};

export type RuntimeModeValue = {
  value: string;
  name: string;
  description: string | null;
  group: string | null;
};

export type RuntimeModeOption = {
  id: string;
  name: string;
  description: string | null;
  category: string | null;
  type: "select" | "readonly";
  currentValue: string | null;
  values: RuntimeModeValue[];
};

export function projectRuntimeModeOption(option: RuntimeModeOption | null): RuntimeModeProjection {
  const allValues = option?.values ?? [];
  const valueSet = new Set(allValues.map((value) => value.value));
  const currentValue = option?.currentValue && valueSet.has(option.currentValue)
    ? option.currentValue
    : "";
  const supportsPlan = valueSet.has("plan");
  const defaultValue = valueSet.has("default")
    ? "default"
    : currentValue || allValues.find((value) => value.value !== "plan")?.value || allValues[0]?.value || "";
  const extraValues = supportsPlan
    ? allValues.filter((value) => value.value !== "plan" && value.value !== "default" && value.value !== defaultValue)
    : allValues;
  return {
    allValues,
    defaultValue,
    extraValues,
    supportsPlan
  };
}

export function runtimeModeCommandValues(projection: RuntimeModeProjection): string[] {
  if (projection.supportsPlan) {
    return [
      projection.defaultValue,
      "plan",
      ...projection.extraValues.map((value) => value.value)
    ].filter((value, index, values) => value && values.indexOf(value) === index);
  }
  return projection.allValues.map((value) => value.value);
}

export function normalizeRequestedRuntimeMode(projection: RuntimeModeProjection, requested: string): string | null {
  if (!requested) {
    return null;
  }
  if (projection.supportsPlan && requested === "default") {
    return projection.defaultValue || null;
  }
  return requested;
}

export function formatRuntimeModeValues(projection: RuntimeModeProjection): string {
  if (projection.supportsPlan) {
    const defaultLabel = projection.defaultValue && projection.defaultValue !== "default"
      ? `default (${projection.defaultValue})`
      : "default";
    const labels = [
      defaultLabel,
      "plan",
      ...projection.extraValues.map((value) => value.value)
    ].filter(Boolean);
    return labels.join(", ") || "none";
  }
  return projection.allValues.map((value) => value.value).join(", ") || "none";
}

export function isRuntimeModeOption(option: RuntimeModeOption): boolean {
  return option.id === "mode" || option.category === "mode";
}
