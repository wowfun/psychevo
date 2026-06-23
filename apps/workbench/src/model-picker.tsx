import { type CSSProperties, useEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronDown } from "lucide-react";
import type { ModelOptionView, SettingsReadResult } from "@psychevo/protocol";

const DEFAULT_REASONING_EFFORTS = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];

const REASONING_LABELS: Record<string, string> = {
  none: "Default",
  minimal: "Minimal",
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "XHigh",
  max: "Max"
};

export function ModelReasoningSelector({
  ariaLabel = "Model",
  className = "",
  disabled = false,
  emptyLabel = "Select model",
  model,
  options,
  placement = "top-end",
  recentModels = [],
  resetLabel,
  variant,
  variantOptions,
  onModelChange,
  onSelectionChange,
  onVariantChange
}: {
  ariaLabel?: string;
  className?: string;
  disabled?: boolean;
  emptyLabel?: string;
  model: string | null;
  options: ModelOptionView[];
  placement?: "top-end" | "bottom-start";
  recentModels?: string[];
  resetLabel?: string | undefined;
  variant: string;
  variantOptions?: string[];
  onModelChange(value: string | null): void;
  onSelectionChange?: ((model: string | null, variant: string) => void) | undefined;
  onVariantChange(value: string): void;
}) {
  const [open, setOpen] = useState(false);
  const [modelFilter, setModelFilter] = useState("");
  const [optimisticRecentModels, setOptimisticRecentModels] = useState<string[]>(recentModels);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const searchRef = useRef<HTMLInputElement | null>(null);
  const selected = model ? options.find((option) => option.value === model) ?? fallbackModelOption(model) : null;
  const recentModelsKey = recentModels.join("\u0000");
  const reasoningValues = reasoningEffortsForModelOption(selected, variantOptions);
  const selectedReasoning = reasoningValues.includes(variant) ? variant : "none";
  const filteredOptions = useMemo(
    () => filterAndOrderModelOptions(options, model, optimisticRecentModels, modelFilter),
    [model, modelFilter, options, optimisticRecentModels]
  );
  const modelGroups = useMemo(() => groupAdjacentModelOptions(filteredOptions), [filteredOptions]);
  const popoverStyle = useMemo(() => ({
    "--pevo-model-picker-popover-width": `${popoverWidthCharacters({
      emptyLabel,
      filteredOptions,
      modelGroups,
      reasoningValues,
      resetLabel
    })}ch`
  }) as CSSProperties, [emptyLabel, filteredOptions, modelGroups, reasoningValues, resetLabel]);
  const resetSelected = Boolean(resetLabel) && !selected && !model;
  const displayLabel = selected
    ? `${modelShortLabel(selected)} ${reasoningLabel(selectedReasoning)}`
    : resetSelected
      ? resetLabel
      : emptyLabel;
  const title = selected
    ? `${selected.value} / ${reasoningLabel(selectedReasoning)}`
    : resetSelected
      ? resetLabel
      : emptyLabel;

  useEffect(() => {
    setOptimisticRecentModels(recentModels);
  }, [recentModelsKey]);

  useEffect(() => {
    if (!open) {
      return;
    }
    const focusTimer = window.setTimeout(() => searchRef.current?.focus(), 0);
    const closeOnOutsidePointer = (event: PointerEvent) => {
      const target = event.target;
      if (target instanceof Node && rootRef.current?.contains(target)) {
        return;
      }
      setOpen(false);
    };
    document.addEventListener("pointerdown", closeOnOutsidePointer);
    return () => {
      window.clearTimeout(focusTimer);
      document.removeEventListener("pointerdown", closeOnOutsidePointer);
    };
  }, [open]);

  useEffect(() => {
    if (selected) {
      if (!reasoningValues.includes(variant)) {
        onVariantChange("none");
      }
      return;
    }
    if (variant !== "none") {
      onVariantChange("none");
    }
  }, [onVariantChange, reasoningValues, selected, variant]);

  function selectModel(option: ModelOptionView) {
    const nextReasoningValues = reasoningEffortsForModelOption(option, variantOptions);
    const nextVariant = nextReasoningValues.includes(variant) ? variant : "none";
    setOptimisticRecentModels((current) => recordRecentModel(option.value, current));
    if (onSelectionChange) {
      onSelectionChange(option.value, nextVariant);
      return;
    }
    if (nextVariant !== variant) {
      onVariantChange(nextVariant);
    }
    onModelChange(option.value);
  }

  function resetModel() {
    if (onSelectionChange) {
      onSelectionChange(null, "none");
      return;
    }
    if (variant !== "none") {
      onVariantChange("none");
    }
    onModelChange(null);
  }

  return (
    <div
      ref={rootRef}
      className={`modelReasoningSelector is-${placement} ${className}`.trim()}
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
        aria-label={ariaLabel}
        className="modelReasoningButton"
        disabled={disabled}
        title={title ?? emptyLabel}
        onClick={() => setOpen((current) => !current)}
      >
        <span>{displayLabel}</span>
        <ChevronDown size={13} aria-hidden="true" />
      </button>
      {open && (
        <div
          className="modelReasoningPopover"
          role="dialog"
          aria-label={`${ariaLabel} and reasoning`}
          style={popoverStyle}
        >
          <div className="modelReasoningGroup">
            <div className="modelReasoningGroupLabel">Model</div>
            <input
              ref={searchRef}
              aria-label={`${ariaLabel} filter`}
              className="modelReasoningSearch"
              onChange={(event) => setModelFilter(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key !== "Escape") {
                  event.stopPropagation();
                }
              }}
              placeholder="Filter models"
              type="search"
              value={modelFilter}
            />
            <div className="modelReasoningRows modelReasoningModelRows" role="radiogroup" aria-label={ariaLabel}>
              {resetLabel && (
                <ModelReasoningRow
                  checked={resetSelected}
                  label={resetLabel}
                  onSelect={resetModel}
                />
              )}
              {modelGroups.length > 0 ? modelGroups.map((group, groupIndex) => (
                <div className="modelReasoningProviderGroup" key={`${group.provider}:${groupIndex}`}>
                  <div className="modelReasoningProviderHeading">{group.label}</div>
                  {group.options.map((option) => (
                    <ModelReasoningRow
                      key={option.value}
                      checked={model === option.value}
                      free={option.free}
                      label={modelShortLabel(option)}
                      value={option.value}
                      onSelect={() => selectModel(option)}
                    />
                  ))}
                </div>
              )) : (
                <div className="modelReasoningHint">{modelFilter.trim() ? "No matching models" : emptyLabel}</div>
              )}
            </div>
          </div>
          <div className="modelReasoningDivider" />
          <div className="modelReasoningGroup">
            <div className="modelReasoningGroupLabel">Reasoning</div>
            {selected ? (
              <div className="modelReasoningRows" role="radiogroup" aria-label={ariaLabel === "Model" ? "Reasoning" : `${ariaLabel} reasoning`}>
                {reasoningValues.map((value) => (
                  <ModelReasoningRow
                    key={value}
                    checked={selectedReasoning === value}
                    label={reasoningLabel(value)}
                    onSelect={() => {
                      if (onSelectionChange) {
                        onSelectionChange(model, value);
                        return;
                      }
                      onVariantChange(value);
                    }}
                  />
                ))}
              </div>
            ) : (
              <div className="modelReasoningHint">Select a model first</div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function ModelReasoningRow({
  checked,
  free = false,
  label,
  value,
  onSelect
}: {
  checked: boolean;
  free?: boolean;
  label: string;
  value?: string;
  onSelect(): void;
}) {
  return (
    <button
      type="button"
      className={`modelReasoningRow ${checked ? "is-selected" : ""}`}
      aria-checked={checked}
      data-model-free={free ? "true" : undefined}
      data-model-value={value}
      onClick={onSelect}
      role="radio"
    >
      <span>
        <strong>{label}</strong>
        {free && <span className="modelReasoningFreeBadge">Free</span>}
      </span>
      {checked && <Check size={13} aria-hidden="true" />}
    </button>
  );
}

export function modelOptionsForControls(
  controls: SettingsReadResult["controls"],
  model: string | null
): ModelOptionView[] {
  const details = controls?.modelDetails?.length
    ? controls.modelDetails
    : (controls?.modelOptions ?? []).map(fallbackModelOption);
  const selected = model?.trim();
  if (!selected || details.some((option) => option.value === selected)) {
    return details;
  }
  return [fallbackModelOption(selected), ...details];
}

function filterAndOrderModelOptions(
  options: ModelOptionView[],
  selectedModel: string | null,
  recentModels: string[],
  query: string
): ModelOptionView[] {
  const filter = query.trim().toLocaleLowerCase();
  const filtered = filter
    ? options.filter((option) => modelSearchText(option).includes(filter))
    : options;
  const recentRank = new Map<string, number>();
  for (const value of [selectedModel, ...recentModels]) {
    const key = value?.trim();
    if (key && !recentRank.has(key)) {
      recentRank.set(key, recentRank.size);
    }
  }
  return filtered
    .map((option, index) => ({ option, index }))
    .sort((left, right) => {
      const leftRank = recentRank.get(left.option.value);
      const rightRank = recentRank.get(right.option.value);
      if (leftRank !== undefined || rightRank !== undefined) {
        return (leftRank ?? Number.MAX_SAFE_INTEGER) - (rightRank ?? Number.MAX_SAFE_INTEGER);
      }
      return left.index - right.index;
    })
    .map((item) => item.option);
}

function modelSearchText(option: ModelOptionView): string {
  return [
    option.value,
    option.id,
    option.label,
    option.provider,
    option.providerLabel
  ].filter(Boolean).join(" ").toLocaleLowerCase();
}

type ModelOptionGroup = {
  provider: string;
  label: string;
  options: ModelOptionView[];
};

function groupAdjacentModelOptions(options: ModelOptionView[]): ModelOptionGroup[] {
  const groups: ModelOptionGroup[] = [];
  for (const option of options) {
    const provider = option.provider?.trim() || "";
    const label = option.providerLabel?.trim() || provider || "Unknown provider";
    const last = groups.at(-1);
    if (last && last.provider === provider) {
      last.options.push(option);
      continue;
    }
    groups.push({
      provider,
      label,
      options: [option]
    });
  }
  return groups;
}

function popoverWidthCharacters({
  emptyLabel,
  filteredOptions,
  modelGroups,
  reasoningValues,
  resetLabel
}: {
  emptyLabel: string;
  filteredOptions: ModelOptionView[];
  modelGroups: ModelOptionGroup[];
  reasoningValues: string[];
  resetLabel?: string | undefined;
}): number {
  const modelWidths = filteredOptions.map((option) => (
    modelShortLabel(option).length + (option.free ? 7 : 0)
  ));
  const values = [
    "Filter models".length,
    "Reasoning".length,
    emptyLabel.length,
    resetLabel?.length ?? 0,
    ...modelWidths,
    ...modelGroups.map((group) => group.label.length),
    ...reasoningValues.map((value) => reasoningLabel(value).length)
  ];
  return Math.max(24, Math.min(54, Math.max(...values) + 8));
}

function fallbackModelOption(value: string): ModelOptionView {
  const { provider, id } = splitProviderModel(value);
  return {
    provider,
    id,
    value,
    label: null,
    providerLabel: null,
    free: false,
    contextLimit: null,
    reasoningSupported: null,
    reasoningEfforts: []
  };
}

function splitProviderModel(value: string): { provider: string; id: string } {
  const trimmed = value.trim();
  const slash = trimmed.indexOf("/");
  if (slash > 0 && slash < trimmed.length - 1) {
    return {
      provider: trimmed.slice(0, slash),
      id: trimmed.slice(slash + 1)
    };
  }
  return {
    provider: "",
    id: trimmed
  };
}

function modelShortLabel(option: ModelOptionView): string {
  return option.label?.trim() || option.id || splitProviderModel(option.value).id || option.value;
}

export function reasoningEffortsForModelOption(
  option: ModelOptionView | null,
  variantOptions?: string[]
): string[] {
  if (!option) {
    return ["none"];
  }
  if (option.reasoningEfforts?.length) {
    return normalizeReasoningEfforts(option.reasoningEfforts);
  }
  if (option.reasoningSupported === false) {
    return ["none"];
  }
  return normalizeReasoningEfforts(variantOptions?.length ? variantOptions : DEFAULT_REASONING_EFFORTS);
}

function normalizeReasoningEfforts(values: string[]): string[] {
  const seen = new Set<string>();
  const normalized = values
    .map((value) => value.trim())
    .filter(Boolean)
    .filter((value) => {
      if (seen.has(value)) {
        return false;
      }
      seen.add(value);
      return true;
    });
  return normalized.includes("none") ? normalized : ["none", ...normalized];
}

export function reasoningLabel(value: string): string {
  return REASONING_LABELS[value] ?? value;
}

function recordRecentModel(value: string, current: string[]): string[] {
  const trimmed = value.trim();
  if (!trimmed) {
    return current;
  }
  return [trimmed, ...current.filter((item) => item !== trimmed)].slice(0, 8);
}
