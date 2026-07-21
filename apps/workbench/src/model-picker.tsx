import { type CSSProperties, useEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronDown } from "lucide-react";
import type {
  ModelOptionView,
  SettingsReadResult,
  ThreadControlDescriptorView
} from "@psychevo/protocol";
import { usePopoverDismiss } from "./popover-dismiss";

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
  reasoningPresentation = "selectable",
  reasoningValues: projectedReasoningValues,
  showChevron = true,
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
  reasoningPresentation?: "selectable" | "readOnly" | "hidden";
  reasoningValues?: string[] | undefined;
  showChevron?: boolean;
  variant: string | null;
  variantOptions?: string[];
  onModelChange(value: string | null): void;
  onSelectionChange?: ((model: string | null, variant: string) => void) | undefined;
  onVariantChange(value: string): void;
}) {
  const [open, setOpen] = useState(false);
  const [modelFilter, setModelFilter] = useState("");
  const [optimisticRecentModels, setOptimisticRecentModels] = useState<string[]>(recentModels);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const searchRef = useRef<HTMLInputElement | null>(null);
  const selected = model ? options.find((option) => option.value === model) ?? fallbackModelOption(model) : null;
  const recentModelsKey = recentModels.join("\u0000");
  const reasoningValues = reasoningPresentation === "selectable"
    ? projectedReasoningValues === undefined
      ? reasoningEffortsForModelOption(selected, variantOptions)
      : authoritativeReasoningEfforts(projectedReasoningValues)
    : reasoningPresentation === "readOnly" && variant
      ? [variant]
      : [];
  const selectedReasoning = variant && reasoningValues.includes(variant) ? variant : null;
  const showReasoning = reasoningPresentation !== "hidden";
  const popoverReasoningValues = reasoningPresentation === "readOnly"
    ? selectedReasoning ? [selectedReasoning] : []
    : reasoningValues;
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
      reasoningValues: popoverReasoningValues,
      resetLabel
    })}ch`
  }) as CSSProperties, [emptyLabel, filteredOptions, modelGroups, popoverReasoningValues, resetLabel]);
  const resetSelected = Boolean(resetLabel) && !selected && !model;
  const displayLabel = selected
    ? `${modelShortLabel(selected)}${reasoningDisplaySuffix(showReasoning, selectedReasoning)}`
    : resetSelected
      ? resetLabel
      : emptyLabel;
  const title = selected
    ? `${selected.value}${showReasoning ? ` / ${selectedReasoning ? reasoningLabel(selectedReasoning) : "Reasoning unavailable"}` : ""}`
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
    return () => {
      window.clearTimeout(focusTimer);
    };
  }, [open]);

  usePopoverDismiss(open, rootRef, triggerRef, () => setOpen(false));

  useEffect(() => {
    if (reasoningPresentation !== "selectable") {
      return;
    }
    if (selected) {
      if (projectedReasoningValues === undefined && (!variant || !reasoningValues.includes(variant))) {
        const fallback = reasoningValues[0];
        if (fallback !== undefined) {
          onVariantChange(fallback);
        }
      }
      return;
    }
    if (projectedReasoningValues === undefined && variant !== "none") {
      onVariantChange("none");
    }
  }, [onVariantChange, reasoningPresentation, reasoningValues, selected, variant]);

  function selectModel(option: ModelOptionView) {
    const nextReasoningValues = reasoningPresentation === "selectable"
      ? projectedReasoningValues === undefined
        ? reasoningEffortsForModelOption(option, variantOptions)
        : authoritativeReasoningEfforts(projectedReasoningValues)
      : [];
    const nextVariant = reasoningPresentation === "selectable"
      ? variant && nextReasoningValues.includes(variant) ? variant : nextReasoningValues[0] ?? "none"
      : variant ?? "none";
    setOptimisticRecentModels((current) => recordRecentModel(option.value, current));
    if (onSelectionChange) {
      onSelectionChange(option.value, nextVariant);
      return;
    }
    if (reasoningPresentation === "selectable" && nextVariant !== variant) {
      onVariantChange(nextVariant);
    }
    onModelChange(option.value);
  }

  function resetModel() {
    if (onSelectionChange) {
      onSelectionChange(null, "none");
      return;
    }
    if (reasoningPresentation === "selectable" && variant !== "none") {
      onVariantChange("none");
    }
    onModelChange(null);
  }

  return (
    <div
      ref={rootRef}
      className={`modelReasoningSelector is-${placement} ${className}`.trim()}
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
        ref={triggerRef}
      >
        <span>{displayLabel}</span>
        {showChevron && <ChevronDown size={13} aria-hidden="true" />}
      </button>
      {open && (
        <div
          className="modelReasoningPopover pevo-controlPopover"
          role="dialog"
          aria-label={`${ariaLabel} and reasoning`}
          style={popoverStyle}
        >
          <div className="modelReasoningGroup">
            <div className="modelReasoningGroupLabel">Model</div>
            <input
              ref={searchRef}
              aria-label={`${ariaLabel} filter`}
              className="modelReasoningSearch pevo-fieldControl pevo-fieldControl--search pevo-fieldControl--compact"
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
          {showReasoning && <div className="modelReasoningDivider" />}
          {showReasoning && (
            <div className="modelReasoningGroup">
              <div className="modelReasoningGroupLabel">Reasoning</div>
              {selected ? reasoningPresentation === "selectable" ? (
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
                <div
                  aria-label={`Reasoning: ${selectedReasoning ? reasoningLabel(selectedReasoning) : "Unavailable"} (read-only)`}
                  className="modelReasoningReadOnly"
                >
                  <strong>{selectedReasoning ? reasoningLabel(selectedReasoning) : "Unavailable"}</strong>
                  {selectedReasoning && <Check size={13} aria-hidden="true" />}
                </div>
              ) : (
                <div className="modelReasoningHint">Select a model first</div>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function authoritativeReasoningEfforts(values: string[]): string[] {
  return [...new Set(values.map((value) => value.trim()).filter(Boolean))];
}

function reasoningDisplaySuffix(showReasoning: boolean, value: string | null): string {
  if (!showReasoning) return "";
  return value ? ` ${reasoningLabel(value)}` : " Unavailable";
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
      className={`modelReasoningRow pevo-controlPopoverRow ${checked ? "is-selected" : ""}`}
      aria-checked={checked}
      data-model-free={free ? "true" : undefined}
      data-model-value={value}
      onClick={onSelect}
      role="radio"
      title={label}
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

export function modelOptionsForThreadControl(
  control: ThreadControlDescriptorView,
  controls: SettingsReadResult["controls"],
  model: string | null
): ModelOptionView[] {
  const metadata = new Map(
    modelOptionsForControls(controls, model).map((option) => [option.value, option])
  );
  return control.choices.flatMap((choice): ModelOptionView[] => {
    if (typeof choice.value !== "string" || !choice.value.trim()) return [];
    const value = choice.value.trim();
    const existing = metadata.get(value);
    if (existing) return [existing];
    const fallback = fallbackModelOption(value);
    return [{
      ...fallback,
      name: choice.label.trim() && choice.label.trim() !== value
        ? choice.label.trim()
        : fallback.name
    }];
  });
}

export function modelLabelForThreadControl(
  control: ThreadControlDescriptorView,
  controls: SettingsReadResult["controls"],
  model: string
): string {
  const metadata = modelOptionsForControls(controls, model)
    .find((option) => option.value === model);
  const metadataName = metadata?.name?.trim();
  if (metadataName) {
    return metadataName;
  }
  const choice = control.choices.find((option) => option.value === model);
  const choiceLabel = choice?.label.trim();
  if (choiceLabel && choiceLabel !== model) {
    return choiceLabel;
  }
  return modelShortLabel(metadata ?? fallbackModelOption(model));
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
    option.name,
    option.provider,
    option.providerName
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
    const label = option.providerName?.trim() || provider || "Unknown provider";
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
    name: null,
    providerName: null,
    free: false,
    limit: { context: null, output: null },
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
  return option.name?.trim() || option.id || splitProviderModel(option.value).id || option.value;
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
