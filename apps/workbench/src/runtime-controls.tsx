import { useEffect, useRef, useState } from "react";
import { Check, ChevronDown } from "lucide-react";
import type { RuntimeConfigOptionValueView, RuntimeConfigOptionView } from "@psychevo/protocol";
import { StatusSelect } from "./composer-controls";
import type { WorkbenchAgent, WorkbenchBackend } from "./types";

export function ComposerRuntimeControls({
  agents,
  runtimeBackends,
  disabled,
  agentValue,
  runtimeValue,
  runtimeModeValue,
  runtimeModeOption,
  runtimeModeValues,
  runtimeModeError,
  runtimeModeUnavailable,
  agentPersonaEnabled,
  onAgentChange,
  onRuntimeChange,
  onRuntimeModeChange
}: {
  agents: WorkbenchAgent[];
  runtimeBackends: WorkbenchBackend[];
  disabled: boolean;
  agentValue: string;
  runtimeValue: string;
  runtimeModeValue: string;
  runtimeModeOption: RuntimeConfigOptionView | null;
  runtimeModeValues: RuntimeConfigOptionValueView[];
  runtimeModeError: string | null;
  runtimeModeUnavailable: boolean;
  agentPersonaEnabled: boolean;
  onAgentChange(value: string): void;
  onRuntimeChange(value: string): void;
  onRuntimeModeChange(value: string): void;
}) {
  const selectedBackend = runtimeBackends.find((backend) => backend.id === runtimeValue) ?? null;
  const runtimeLabel = selectedBackend ? backendRuntimeLabel(selectedBackend) : "Native";
  const hasBaseMode = runtimeModeOption ? projectRuntimeModeOption(runtimeModeOption).supportsPlan : false;
  const modeValues = hasBaseMode ? ["", ...runtimeModeValues.map((option) => option.value)] : runtimeModeValues.map((option) => option.value);
  const modeLabels: Record<string, string> = {
    ...(hasBaseMode ? { "": "Default/Plan" } : {}),
    ...Object.fromEntries(runtimeModeValues.map((option) => [option.value, option.name]))
  };
  return (
    <div className="composerRuntimeControls" aria-label="Runtime controls">
      <AgentRuntimeSelector
        agents={agents}
        runtimeBackends={runtimeBackends}
        disabled={disabled}
        agentPersonaEnabled={agentPersonaEnabled}
        value={agentValue}
        runtimeValue={runtimeValue}
        onChange={onAgentChange}
        onRuntimeChange={onRuntimeChange}
      />
      {runtimeValue !== "native" && runtimeModeOption && runtimeModeValues.length > 0 && (
        <StatusSelect
          label={`${runtimeLabel} mode`}
          optionLabels={modeLabels}
          renderDisplayValue={(value) => modeLabels[value] ?? value}
          value={hasBaseMode ? runtimeModeValue : runtimeModeValue || modeValues[0] || ""}
          values={modeValues}
          onChange={onRuntimeModeChange}
        />
      )}
      {runtimeValue !== "native" && runtimeModeError && (
        <span className="runtimeModeHint is-error" title={runtimeModeError}>Mode unavailable</span>
      )}
      {runtimeValue !== "native" && runtimeModeUnavailable && (
        <span className="runtimeModeHint">No modes</span>
      )}
    </div>
  );
}

function AgentRuntimeSelector({
  agents,
  runtimeBackends,
  disabled,
  agentPersonaEnabled,
  value,
  runtimeValue,
  onChange,
  onRuntimeChange
}: {
  agents: WorkbenchAgent[];
  runtimeBackends: WorkbenchBackend[];
  disabled: boolean;
  agentPersonaEnabled: boolean;
  value: string;
  runtimeValue: string;
  onChange: (value: string) => void;
  onRuntimeChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const selectedAgent = agents.find((agent) => agentOptionValue(agent) === value) ?? null;
  const selectedRuntime = runtimeValue === "native"
    ? null
    : runtimeBackends.find((backend) => backend.id === runtimeValue) ?? null;
  const agentLabel = agentPersonaEnabled ? selectedAgent?.name ?? "Default Agent" : "Agent unavailable";
  const runtimeLabel = selectedRuntime ? backendRuntimeLabel(selectedRuntime) : "Native";
  const displayLabel = runtimeValue === "native"
    ? agentLabel
    : agentPersonaEnabled
      ? `${agentLabel} · ${runtimeLabel}`
      : runtimeLabel;

  useEffect(() => {
    if (!open) {
      return;
    }
    const closeOnOutsidePointer = (event: PointerEvent) => {
      const target = event.target;
      if (target instanceof Node && rootRef.current?.contains(target)) {
        return;
      }
      setOpen(false);
    };
    document.addEventListener("pointerdown", closeOnOutsidePointer);
    return () => document.removeEventListener("pointerdown", closeOnOutsidePointer);
  }, [open]);

  return (
    <div
      ref={rootRef}
      className="agentRuntimeSelector"
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          setOpen(false);
        }
      }}
    >
      <button
        type="button"
        aria-expanded={open}
        aria-haspopup="dialog"
        aria-label="Agent"
        className="agentRuntimeButton"
        disabled={disabled}
        title={`${agentPersonaEnabled ? agentLabel : "Agent unavailable"} / ${runtimeLabel}`}
        onClick={() => setOpen((current) => !current)}
      >
        <span>{displayLabel}</span>
        <ChevronDown size={13} aria-hidden="true" />
      </button>
      {open && (
        <div className="agentRuntimePopover" role="dialog" aria-label="Agent and runtime">
          <div className="agentRuntimeGroup">
            <div className="agentRuntimeGroupLabel">Main agent</div>
            <div className="agentRuntimeRows" role="radiogroup" aria-label="Main agent">
              <AgentRuntimeRow
                disabled={disabled || !agentPersonaEnabled}
                label="Default Agent"
                selected={agentPersonaEnabled && value === ""}
                onSelect={() => onChange("")}
              />
              {agents.map((agent) => {
                const optionValue = agentOptionValue(agent);
                return (
                  <AgentRuntimeRow
                    key={`${agent.source}:${agent.path ?? agent.name}`}
                    disabled={disabled || !agentPersonaEnabled}
                    label={agent.name}
                    selected={agentPersonaEnabled && value === optionValue}
                    onSelect={() => onChange(optionValue)}
                  />
                );
              })}
            </div>
          </div>
          {!agentPersonaEnabled && (
            <div className="agentRuntimeHint">This runtime uses its own persona.</div>
          )}
          <div className="agentRuntimeDivider" />
          <div className="agentRuntimeGroup">
            <div className="agentRuntimeGroupLabel">Runtime</div>
            <div className="agentRuntimeRows" role="radiogroup" aria-label="Runtime">
              <AgentRuntimeRow
                disabled={disabled}
                label="Native Runtime"
                selected={runtimeValue === "native"}
                onSelect={() => onRuntimeChange("native")}
              />
              {runtimeBackends.map((backend) => (
                <AgentRuntimeRow
                  key={backend.id}
                  disabled={disabled}
                  label={backendRuntimeLabel(backend)}
                  selected={runtimeValue === backend.id}
                  onSelect={() => onRuntimeChange(backend.id)}
                />
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function AgentRuntimeRow({
  disabled,
  label,
  selected,
  onSelect
}: {
  disabled: boolean;
  label: string;
  selected: boolean;
  onSelect(): void;
}) {
  return (
    <button
      type="button"
      className={`agentRuntimeRow ${selected ? "is-selected" : ""}`}
      aria-checked={selected}
      disabled={disabled}
      onClick={onSelect}
      role="radio"
    >
      <span>{label}</span>
      {selected && <Check size={13} aria-hidden="true" />}
    </button>
  );
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

function backendRuntimeLabel(backend: WorkbenchBackend): string {
  return backend.label?.trim() || backend.id;
}

export function isRuntimeModeOption(option: RuntimeConfigOptionView): boolean {
  return option.id === "mode" || option.category === "mode";
}
