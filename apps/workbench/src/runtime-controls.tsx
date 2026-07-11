import { useEffect, useRef, useState, type RefObject } from "react";
import { Check, ChevronDown } from "lucide-react";
import type {
  RuntimeBindingView,
  RuntimeConfigOptionValueView,
  RuntimeConfigOptionView,
  RuntimeControlDescriptorView,
  RuntimeProfileView
} from "@psychevo/protocol";
import { StatusSelect } from "./composer-controls";
import {
  agentPairingUnavailableReason,
  runtimeControlDependencyMatches,
  runtimeControlValueLabel,
  runtimeProfileCapsuleLabel,
  runtimeProfileDisplayLabel,
  runtimeProfileProvenance,
  runtimeProfileSourceLabel,
  runtimeProfileUnavailableReason
} from "./runtime-context";
import type { WorkbenchAgent, WorkbenchBackend } from "./types";

export function ComposerRuntimeControls({
  agents,
  profiles,
  binding,
  controls,
  controlValues,
  disabled,
  agentValue,
  runtimeValue,
  contextError,
  contextLoading,
  onAgentChange,
  onRuntimeChange,
  onControlChange,
  onManageRuntimes
}: {
  agents: WorkbenchAgent[];
  profiles: RuntimeProfileView[];
  binding: RuntimeBindingView | null;
  controls: RuntimeControlDescriptorView[];
  controlValues: Record<string, unknown>;
  disabled: boolean;
  agentValue: string;
  runtimeValue: string;
  contextError: string | null;
  contextLoading: boolean;
  onAgentChange(value: string): void;
  onRuntimeChange(value: string): void;
  onControlChange(control: RuntimeControlDescriptorView, value: unknown): void;
  onManageRuntimes(): void;
}) {
  const selectedProfile = profiles.find((profile) => profile.id === runtimeValue) ?? null;
  const nativeProfileSelected = runtimeValue === "native" || selectedProfile?.runtime === "native";
  const visibleControls = nativeProfileSelected
    ? controls.filter((control) => control.id !== "mode")
    : controls;
  return (
    <div className="composerRuntimeControls" aria-label="Runtime controls">
      <AgentDefinitionSelector
        agents={agents}
        binding={binding}
        disabled={disabled}
        profile={selectedProfile}
        value={agentValue}
        onChange={onAgentChange}
      />
      <RuntimeProfileSelector
        agentValue={agentValue}
        agents={agents}
        binding={binding}
        disabled={disabled || contextLoading}
        profiles={profiles}
        value={runtimeValue}
        onRuntimeChange={onRuntimeChange}
        onManageRuntimes={onManageRuntimes}
      />
      <RuntimeControlFields
        controls={visibleControls}
        disabled={disabled}
        emptyStateVisible={!nativeProfileSelected}
        values={controlValues}
        onChange={onControlChange}
      />
      {contextError && <span className="runtimeModeHint is-error" title={contextError}>Runtime context unavailable</span>}
    </div>
  );
}

function AgentDefinitionSelector({
  agents,
  binding,
  disabled,
  profile,
  value,
  onChange
}: {
  agents: WorkbenchAgent[];
  binding: RuntimeBindingView | null;
  disabled: boolean;
  profile: RuntimeProfileView | null;
  value: string;
  onChange(value: string): void;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const selectedAgent = agents.find((agent) => agentOptionValue(agent) === value) ?? null;
  const agentLabel = selectedAgent?.name ?? "Default Agent";
  const selectedPairingReason = agentPairingUnavailableReason(selectedAgent, profile);
  const startsNewDirectThread = binding?.backendKind === "runtime";
  usePopoverDismiss(open, rootRef, () => setOpen(false));

  return (
    <div ref={rootRef} className="agentRuntimeSelector" onKeyDown={(event) => {
      if (event.key === "Escape") setOpen(false);
    }}>
      <button
        aria-expanded={open}
        aria-haspopup="dialog"
        aria-label="Agent"
        className="agentRuntimeButton"
        disabled={disabled}
        onClick={() => setOpen((current) => !current)}
        title={agentLabel}
        type="button"
      >
        <span>{agentLabel}</span>
        <ChevronDown aria-hidden="true" size={13} />
      </button>
      {open && (
        <div className="agentRuntimePopover agentDefinitionPopover" role="dialog" aria-label="Agent Definition">
          <div className="agentRuntimeGroup">
            <div className="agentRuntimeGroupLabel">{startsNewDirectThread ? "Start a new thread" : "Agent Definition"}</div>
            <div className="agentRuntimeRows" role="radiogroup" aria-label="Main agent">
              <AgentRuntimeRow
                ariaLabel={startsNewDirectThread && value !== "" ? "Start a new thread with Default Agent" : "Default Agent"}
                disabled={disabled || (startsNewDirectThread && value === "")}
                label="Default Agent"
                selected={value === ""}
                title={startsNewDirectThread ? "The current thread keeps its existing Agent Definition." : undefined}
                onSelect={() => onChange("")}
              />
              {agents.map((agent) => {
                const optionValue = agentOptionValue(agent);
                const pairingReason = agentPairingUnavailableReason(agent, profile);
                return (
                  <AgentRuntimeRow
                    key={`${agent.source}:${agent.path ?? agent.name}`}
                    ariaLabel={startsNewDirectThread && value !== optionValue ? `Start a new thread with ${agent.name}` : agent.name}
                    disabled={disabled || Boolean(pairingReason) || (startsNewDirectThread && value === optionValue)}
                    label={agent.name}
                    selected={value === optionValue}
                    title={pairingReason ?? (startsNewDirectThread ? "The current thread keeps its existing Agent Definition." : undefined)}
                    onSelect={() => onChange(optionValue)}
                  />
                );
              })}
            </div>
          </div>
          {selectedPairingReason && <div className="agentRuntimeHint is-warning">{selectedPairingReason}</div>}
        </div>
      )}
    </div>
  );
}

function RuntimeProfileSelector({
  agentValue,
  agents,
  binding,
  disabled,
  profiles,
  value,
  onRuntimeChange,
  onManageRuntimes
}: {
  agentValue: string;
  agents: WorkbenchAgent[];
  binding: RuntimeBindingView | null;
  disabled: boolean;
  profiles: RuntimeProfileView[];
  value: string;
  onRuntimeChange(value: string): void;
  onManageRuntimes(): void;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const selected = profiles.find((profile) => profile.id === value) ?? null;
  const selectedAgent = agents.find((agent) => agentOptionValue(agent) === agentValue) ?? null;
  const label = selected
    ? binding ? runtimeProfileCapsuleLabel(selected) : runtimeProfileDisplayLabel(selected)
    : value || "Runtime Profile";
  usePopoverDismiss(open, rootRef, () => setOpen(false));

  return (
    <div ref={rootRef} className="agentRuntimeSelector runtimeProfileSelector" onKeyDown={(event) => {
      if (event.key === "Escape") setOpen(false);
    }}>
      <button
        aria-expanded={open}
        aria-haspopup="dialog"
        aria-label={binding ? `Bound Runtime Profile ${label}` : "Runtime Profile"}
        className={binding ? "agentRuntimeButton runtimeProvenanceCapsule" : "agentRuntimeButton"}
        disabled={disabled}
        onClick={() => setOpen((current) => !current)}
        title={binding ? `${label}. Runtime bindings are immutable.` : label}
        type="button"
      >
        <span>{label}</span>
        <ChevronDown aria-hidden="true" size={13} />
      </button>
      {open && (
        <div className="agentRuntimePopover runtimeProfilePopover" role="dialog" aria-label="Runtime Profile selection">
          <div className="agentRuntimeGroupLabel">{binding ? "Start a new thread" : "Runtime Profile"}</div>
          <div className="agentRuntimeRows" role="radiogroup" aria-label="Runtime">
            {profiles.map((profile) => {
              const pairingReason = agentPairingUnavailableReason(selectedAgent, profile);
              const unavailableReason = runtimeProfileUnavailableReason(profile);
              const reason = pairingReason ?? unavailableReason;
              const displayLabel = runtimeProfileDisplayLabel(profile);
              return (
                <AgentRuntimeRow
                  key={profile.id}
                  ariaLabel={binding && profile.id !== binding.runtimeRef ? `Start a new thread with ${displayLabel}` : displayLabel}
                  description={`${runtimeProfileProvenance(profile)} · ${runtimeProfileSourceLabel(profile)}${profile.health.status === "unchecked" ? " · Unchecked" : ""}`}
                  disabled={disabled || Boolean(reason) || (binding != null && profile.id === binding.runtimeRef)}
                  label={displayLabel}
                  selected={profile.id === value}
                  title={reason ?? (binding ? "The current thread keeps its existing Runtime Profile." : undefined)}
                  onSelect={() => {
                    setOpen(false);
                    onRuntimeChange(profile.id);
                  }}
                />
              );
            })}
          </div>
          <button className="runtimeProfileManageButton" onClick={() => {
            setOpen(false);
            onManageRuntimes();
          }} type="button">
            Manage Runtime Profiles
          </button>
        </div>
      )}
    </div>
  );
}

function RuntimeControlFields({
  controls,
  disabled,
  emptyStateVisible,
  values,
  onChange
}: {
  controls: RuntimeControlDescriptorView[];
  disabled: boolean;
  emptyStateVisible: boolean;
  values: Record<string, unknown>;
  onChange(control: RuntimeControlDescriptorView, value: unknown): void;
}) {
  const runtimeDefaultKey = "__runtime_default__";
  const visibleControls = controls.filter((control) =>
    runtimeControlDependencyMatches(control, controls, values)
  );
  if (visibleControls.length === 0) {
    return emptyStateVisible
      ? <span aria-label="Runtime control state" className="runtimeControlState">Runtime default</span>
      : null;
  }
  return <>{visibleControls.map((control) => {
    const value = Object.prototype.hasOwnProperty.call(values, control.id)
      ? values[control.id]
      : control.currentValue;
    if (control.state === "readOnlyCurrent" || disabled) {
      return (
        <span aria-label={`${control.label}: ${runtimeControlValueLabel(control, value)} (read-only)`} className="runtimeControlState is-readonly" key={control.id} title="Observed current value · read-only">
          {control.label}: {runtimeControlValueLabel(control, value)}
        </span>
      );
    }
    if (control.state === "runtimeDefault" || control.choices.length === 0) {
      return <span aria-label={`${control.label}: Runtime default`} className="runtimeControlState" key={control.id}>Runtime default</span>;
    }
    const optionKeys = [runtimeDefaultKey, ...control.choices.map((_, index) => String(index))];
    const selectedIndex = control.choices.findIndex((choice) => valuesEqual(choice.value, value));
    const selectedKey = selectedIndex < 0 ? runtimeDefaultKey : String(selectedIndex);
    const optionLabels: Record<string, string> = {
      [runtimeDefaultKey]: "Runtime default",
      ...Object.fromEntries(control.choices.map((choice, index) => [String(index), choice.label]))
    };
    return (
      <StatusSelect
        key={control.id}
        label={control.label}
        optionLabels={optionLabels}
        renderDisplayValue={(key) => optionLabels[key] ?? "Runtime default"}
        value={selectedKey}
        values={optionKeys}
        onChange={(key) => {
          if (key === runtimeDefaultKey) {
            onChange(control, undefined);
            return;
          }
          const choice = control.choices[Number(key)];
          if (choice) onChange(control, choice.value);
        }}
      />
    );
  })}</>;
}

function AgentRuntimeRow({
  ariaLabel,
  description,
  disabled,
  label,
  selected,
  title,
  onSelect
}: {
  ariaLabel?: string | undefined;
  description?: string | undefined;
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
      <span className="agentRuntimeRowCopy"><span>{label}</span>{description && <small>{description}</small>}</span>
      {selected && <Check aria-hidden="true" size={13} />}
    </button>
  );
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

export function agentOptionValue(agent: WorkbenchAgent): string {
  return agent.source === "explicit" ? agent.path?.trim() || agent.name : agent.name;
}

export function isComposerRunnableAgent(agent: WorkbenchAgent): boolean {
  if (!agent.name) {
    return false;
  }
  return !agent.backend?.ref;
}

export function isComposerRuntimeBackend(backend: WorkbenchBackend): boolean {
  return backend.enabled
    && Boolean(backend.command?.trim())
    && backend.entrypoints.includes("peer");
}

export function runtimeSupportsAgentPersona(runtimeRef: string): boolean {
  return runtimeRef === "native";
}

export function runtimeControlAsConfigOption(
  control: RuntimeControlDescriptorView | null
): RuntimeConfigOptionView | null {
  if (!control) return null;
  const values = control.choices.flatMap((choice): RuntimeConfigOptionValueView[] => (
    typeof choice.value === "string"
      ? [{ value: choice.value, name: choice.label, description: choice.description, group: null }]
      : []
  ));
  return {
    id: control.id,
    name: control.label,
    description: null,
    category: control.id === "mode" ? "mode" : null,
    type: values.length > 0 ? "select" : "readonly",
    currentValue: typeof control.currentValue === "string" ? control.currentValue : null,
    values
  };
}

export type RuntimeModeProjection = {
  allValues: RuntimeConfigOptionValueView[];
  defaultValue: string;
  extraValues: RuntimeConfigOptionValueView[];
  supportsPlan: boolean;
};

export function projectRuntimeModeOption(option: RuntimeConfigOptionView | null): RuntimeModeProjection {
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

export function resolvePeerRuntimeMode(
  projection: RuntimeModeProjection,
  workMode: string,
  selectedExtraMode: string
): string {
  if (projection.supportsPlan) {
    if (selectedExtraMode && projection.extraValues.some((value) => value.value === selectedExtraMode)) {
      return selectedExtraMode;
    }
    return workMode === "plan" ? "plan" : projection.defaultValue;
  }
  return selectedExtraMode || projection.defaultValue;
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

export function isRuntimeModeOption(option: RuntimeConfigOptionView): boolean {
  return option.id === "mode" || option.category === "mode";
}
