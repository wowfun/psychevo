import type {
  ModelOptionView,
  SettingsReadResult
} from "@psychevo/protocol";

export function mergeModelCatalogOptionsIntoSettings(
  current: SettingsReadResult | undefined,
  options: ModelOptionView[]
): SettingsReadResult | undefined {
  if (!current?.controls || options.length === 0) {
    return current;
  }
  const modelDetails = new Map<string, ModelOptionView>();
  for (const option of current.controls.modelDetails ?? []) {
    modelDetails.set(option.value, option);
  }
  for (const option of options) {
    modelDetails.set(option.value, option);
  }
  const modelOptions = new Set(current.controls.modelOptions ?? []);
  for (const option of options) {
    modelOptions.add(option.value);
  }
  return {
    ...current,
    controls: {
      ...current.controls,
      modelOptions: [...modelOptions].sort(),
      modelDetails: [...modelDetails.values()].sort((left, right) => left.value.localeCompare(right.value))
    }
  };
}

export function modelTurnBlockReasonForControls(controls: SettingsReadResult["controls"]): string {
  if (controls?.modelStatus === "error" && controls.modelError?.trim()) {
    return `Model unavailable: ${controls.modelError.trim()}`;
  }
  return "Select a provider/model before starting a conversation.";
}
