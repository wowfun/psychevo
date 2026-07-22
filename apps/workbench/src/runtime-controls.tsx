import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { Check } from "lucide-react";
import type {
  RunnableTargetView,
  RuntimeBindingView,
  RuntimeProfileView,
  ThreadControlDescriptorView
} from "@psychevo/protocol";
import { usePopoverDismiss } from "./popover-dismiss";
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
  preparing = false,
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
  preparing?: boolean;
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
        preparing={preparing}
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
  preparing,
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
  preparing: boolean;
  onTargetChange(targetId: string): void;
  onControlChange(control: ThreadControlDescriptorView, value: unknown): void;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const selectedTarget = targets.find((target) => target.targetId === targetId) ?? null;
  const agentLabel = preparing ? "Preparing…" : selectedTarget?.agentLabel || "Select Agent";
  const profileLabel = preparing ? "Preparing…" : selectedTarget?.profileLabel || "Runtime Profile";
  const displayLabel = preparing
    ? "Preparing…"
    : selectedTarget ? agentTargetDisplayLabel(selectedTarget, profiles) : agentLabel;
  const optionControls = controls.filter((control) => (
    control.surfaceRole === "advanced" && control.id !== "permissionMode"
  ));
  const selectedPairingReason = runnableTargetUnavailableReason(selectedTarget);
  const startsNewBoundThread = binding != null;
  usePopoverDismiss(open, rootRef, triggerRef, () => setOpen(false));

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
        ref={triggerRef}
        title={`${agentLabel} · ${profileLabel}`}
        type="button"
      >
        <span>{displayLabel}</span>
      </button>
      {open && (
        <div className="agentRuntimePopover agentDefinitionPopover pevo-controlPopover" role="dialog" aria-label="Agent target">
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
      ...Object.fromEntries(control.choices.map((choice, index) => [
        String(index),
        runtimeControlChoiceLabel(control, choice.label, choice.value)
      ]))
    };
    return (
      <RuntimeControlSelect
        disabled={disabled}
        key={control.id}
        label={control.label}
        optionLabels={optionLabels}
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

function RuntimeControlSelect({
  disabled,
  label,
  optionLabels,
  value,
  values,
  onChange
}: {
  disabled: boolean;
  label: string;
  optionLabels: Record<string, string>;
  value: string;
  values: string[];
  onChange(value: string): void;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const listRef = useRef<HTMLDivElement | null>(null);
  const listboxId = useId();
  const displayValue = optionLabels[value] ?? value;
  const initialFocusValue = values.includes(value) && value !== "__unavailable__"
    ? value
    : values.find((option) => option !== "__unavailable__") ?? null;
  usePopoverDismiss(open, rootRef, triggerRef, () => setOpen(false));

  useEffect(() => {
    if (!open) return;
    const options = Array.from(listRef.current?.querySelectorAll<HTMLButtonElement>('[role="option"]') ?? []);
    const selected = options.find((option) => option.getAttribute("aria-selected") === "true" && !option.disabled);
    const initial = selected ?? options.find((option) => !option.disabled);
    for (const option of options) option.tabIndex = option === initial ? 0 : -1;
    initial?.focus();
  }, [open, initialFocusValue]);

  function moveOptionFocus(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp" && event.key !== "Home" && event.key !== "End") {
      return;
    }
    const options = Array.from(listRef.current?.querySelectorAll<HTMLButtonElement>('[role="option"]:not(:disabled)') ?? []);
    if (options.length === 0) return;
    event.preventDefault();
    const focusedIndex = options.findIndex((option) => option === document.activeElement);
    const nextIndex = event.key === "Home"
      ? 0
      : event.key === "End"
        ? options.length - 1
        : event.key === "ArrowDown"
          ? (focusedIndex + 1 + options.length) % options.length
          : (focusedIndex - 1 + options.length) % options.length;
    for (const option of options) option.tabIndex = -1;
    const next = options[nextIndex];
    if (next) {
      next.tabIndex = 0;
      next.focus();
    }
  }

  return (
    <div
      className="runtimeControlSelect"
      data-status={label.toLowerCase().replace(/\s+/g, "-")}
      ref={rootRef}
    >
      <button
        aria-controls={open ? listboxId : undefined}
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-label={label}
        className="runtimeControlButton"
        disabled={disabled}
        onClick={() => setOpen((current) => !current)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            setOpen(true);
          }
        }}
        ref={triggerRef}
        role="combobox"
        title={displayValue}
        type="button"
      >
        <span>{displayValue}</span>
      </button>
      {open ? (
        <div
          aria-label={label}
          className="runtimeControlPopover pevo-controlPopover"
          id={listboxId}
          onKeyDown={moveOptionFocus}
          ref={listRef}
          role="listbox"
        >
          {values.map((option) => {
            const selected = option === value;
            const optionLabel = optionLabels[option] ?? option;
            return (
              <button
                aria-selected={selected}
                className={`runtimeControlOption pevo-controlPopoverRow ${selected ? "is-selected" : ""}`}
                disabled={option === "__unavailable__"}
                key={option}
                onClick={() => {
                  setOpen(false);
                  onChange(option);
                  triggerRef.current?.focus();
                }}
                role="option"
                tabIndex={option === initialFocusValue ? 0 : -1}
                title={optionLabel}
                type="button"
              >
                <span>{optionLabel}</span>
                {selected ? <Check aria-hidden="true" size={13} /> : null}
              </button>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}

function runtimeControlChoiceLabel(
  control: ThreadControlDescriptorView,
  label: string,
  value: unknown
): string {
  if (control.id !== "permissionMode") return label;
  switch (typeof value === "string" ? value : label) {
    case "default": return "Default Permission";
    case "acceptEdits": return "Accept Edits";
    case "dontAsk": return "Don't Ask";
    case "bypassPermissions": return "Bypass Permissions";
    default: return label;
  }
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
      className={`agentRuntimeRow pevo-controlPopoverRow ${selected ? "is-selected" : ""}`}
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
