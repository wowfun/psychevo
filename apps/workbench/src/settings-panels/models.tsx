import { useEffect, useMemo, useState } from "react";
import { RotateCcw, Save, Search } from "lucide-react";
import type { GatewayClient } from "@psychevo/client";
import type { AuxiliaryModelAssignmentView, ModelOptionView, ModelProviderView, ModelSettingsResult } from "@psychevo/protocol";
import { ModelReasoningSelector, reasoningEffortsForModelOption } from "../model-picker";
import { errorMessage } from "./common";

type ProviderDraft = {
  providerId: string;
  label: string;
  baseUrl: string;
  apiKeyEnv: string;
  apiKey: string;
  noAuth: boolean;
};

type AssignmentDraft = {
  model: string;
  reasoningEffort: string;
};

export function ModelsSettingsPanel({
  client,
  disabled,
  onModelAssignmentSaved,
  onModelCatalogLoaded,
  cwd
}: {
  client: GatewayClient | null;
  disabled: boolean;
  onModelAssignmentSaved(): Promise<void>;
  onModelCatalogLoaded(options: ModelOptionView[]): void;
  cwd: string;
}) {
  const [settings, setSettings] = useState<ModelSettingsResult | null>(null);
  const [providerDrafts, setProviderDrafts] = useState<Record<string, ProviderDraft>>({});
  const [catalog, setCatalog] = useState<Record<string, ModelOptionView[]>>({});
  const [defaultDraft, setDefaultDraft] = useState<AssignmentDraft>({ model: "", reasoningEffort: "none" });
  const [auxDrafts, setAuxDrafts] = useState<Record<string, AssignmentDraft>>({});
  const [loading, setLoading] = useState(false);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  async function loadModelSettings() {
    if (!client) {
      setSettings(null);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const result = await client.request("model/settings/read", {
        scope: "global",
        cwd: cwd
      });
      setSettings(result);
      setDefaultDraft({
        model: result.defaultModel ?? "",
        reasoningEffort: result.defaultReasoningEffort ?? "none"
      });
      setAuxDrafts(Object.fromEntries(result.auxiliary.map((item) => {
        const model = item.effectiveModel ?? (item.provider !== "auto" && item.model ? `${item.provider}/${item.model}` : "");
        return [
          item.task,
          {
            model,
            reasoningEffort: item.reasoningEffort ?? "none"
          }
        ];
      })));
      setProviderDrafts((current) => mergeProviderDrafts(current, result.providers));
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void loadModelSettings();
  }, [client, cwd]);

  const modelOptions = useMemo(() => {
    const merged = new Map<string, ModelOptionView>();
    for (const option of settings?.modelOptions ?? []) {
      merged.set(option.value, option);
    }
    for (const options of Object.values(catalog)) {
      for (const option of options) {
        merged.set(option.value, option);
      }
    }
    return [...merged.values()].sort((left, right) => left.value.localeCompare(right.value));
  }, [catalog, settings]);
  const freeSelection = modelOptions.find((option) => option.value === defaultDraft.model && option.free && option.provider === "opencode-zen")
    ?? Object.values(auxDrafts)
      .map((value) => modelOptions.find((option) => option.value === value.model && option.free && option.provider === "opencode-zen"))
      .find(Boolean);

  async function fetchProviderCatalog(provider: ModelProviderView) {
    if (!client) return;
    setBusyKey(`catalog:${provider.id}`);
    setError(null);
    setNotice(null);
    try {
      const result = await client.request("model/provider/catalog", {
        scope: "global",
        providerId: provider.id,
        refresh: true,
        cwd: cwd
      });
      setCatalog((current) => ({ ...current, [result.providerId]: result.models }));
      onModelCatalogLoaded(result.models);
      setNotice(`${provider.label}: catalog updated`);
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusyKey(null);
    }
  }

  async function saveProvider(provider: ModelProviderView) {
    if (!client) return;
    const draft = providerDrafts[provider.id];
    if (!draft) return;
    setBusyKey(`provider:${provider.id}`);
    setError(null);
    setNotice(null);
    try {
      const result = await client.request("model/provider/save", {
        scope: "global",
        providerId: draft.providerId,
        label: draft.label,
        baseUrl: draft.baseUrl,
        apiKeyEnv: draft.noAuth ? null : draft.apiKeyEnv || null,
        apiKey: draft.noAuth ? null : draft.apiKey || null,
        noAuth: draft.noAuth
      });
      setSettings(result);
      setProviderDrafts((current) => mergeProviderDrafts(current, result.providers));
      const savedProvider = result.providers.find((item) => item.id === draft.providerId || item.id === provider.id);
      if (savedProvider?.id === "opencode-zen" && savedProvider.canFetchModels) {
        const catalogResult = await client.request("model/provider/catalog", {
          scope: "global",
          providerId: savedProvider.id,
          refresh: true,
          cwd: cwd
        });
        setCatalog((current) => ({ ...current, [catalogResult.providerId]: catalogResult.models }));
        onModelCatalogLoaded(catalogResult.models);
        setNotice(`${draft.label}: saved; catalog updated`);
      } else {
        setNotice(`${draft.label}: saved`);
      }
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusyKey(null);
    }
  }

  async function saveAssignment(target: "default" | "auxiliary", draft: AssignmentDraft, task?: string) {
    if (!client) return;
    const split = splitModelValue(draft.model);
    if (!split && target === "default") {
      setError("Default model must use provider/model");
      return;
    }
    setBusyKey(target === "default" ? "assignment:default" : `assignment:${task}`);
    setError(null);
    setNotice(null);
    try {
      await client.request("model/assignment/set", {
        scope: "global",
        target,
        task: task ?? null,
        provider: split?.provider ?? "auto",
        model: split?.model ?? "",
        reasoningEffort: draft.reasoningEffort || "none"
      });
      setNotice(target === "default" ? "Default model saved" : `${formatAuxTaskLabel(task ?? "")}: saved`);
      await loadModelSettings();
      if (target === "default") {
        await onModelAssignmentSaved();
      }
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusyKey(null);
    }
  }

  return (
    <section className="modelsSettingsPanel" aria-label="Models">
      <div className="modelSettingsToolbar">
        <button
          aria-label="Refresh model settings"
          disabled={disabled || loading || !client}
          onClick={() => void loadModelSettings()}
          title="Refresh model settings"
          type="button"
        >
          <RotateCcw size={13} />
          <span>Refresh</span>
        </button>
      </div>
      {error && <div className="modelSettingsMessage is-error" role="alert">{error}</div>}
      {notice && <div className="modelSettingsMessage">{notice}</div>}
      {freeSelection && (
        <div className="modelSettingsMessage is-warning">
          OpenCode Zen free models may route data through free endpoints with different retention policies.
        </div>
      )}
      <section className="modelAssignmentPanel" aria-label="Model assignments">
        <ModelAssignmentRow
          busy={busyKey === "assignment:default"}
          disabled={disabled || !client}
          label="Default model"
          options={modelOptions}
          value={defaultDraft}
          onChange={setDefaultDraft}
          onSave={() => void saveAssignment("default", defaultDraft)}
        />
        {(settings?.auxiliary ?? defaultAuxiliaryAssignments()).map((assignment) => (
          <ModelAssignmentRow
            busy={busyKey === `assignment:${assignment.task}`}
            disabled={disabled || !client}
            key={assignment.task}
            label={assignment.label}
            options={modelOptions}
            resetLabel="Inherit default"
            value={auxDrafts[assignment.task] ?? { model: "", reasoningEffort: "none" }}
            onChange={(value) => setAuxDrafts((current) => ({ ...current, [assignment.task]: value }))}
            onSave={() => void saveAssignment("auxiliary", auxDrafts[assignment.task] ?? { model: "", reasoningEffort: "none" }, assignment.task)}
          />
        ))}
      </section>
      <section className="modelProvidersPanel" aria-label="Providers">
        {(settings?.providers ?? []).map((provider) => (
          <ProviderSettingsRow
            busy={busyKey === `provider:${provider.id}` || busyKey === `catalog:${provider.id}`}
            catalogCount={catalog[provider.id]?.length ?? 0}
            disabled={disabled || !client}
            draft={providerDrafts[provider.id] ?? providerDraftFromView(provider)}
            key={provider.id}
            provider={provider}
            onDraftChange={(draft) => setProviderDrafts((current) => ({ ...current, [provider.id]: draft }))}
            onFetch={() => void fetchProviderCatalog(provider)}
            onSave={() => void saveProvider(provider)}
          />
        ))}
        {!settings && !loading && <div className="modelSettingsMessage">Model settings unavailable</div>}
      </section>
    </section>
  );
}

function ProviderSettingsRow({
  busy,
  catalogCount,
  disabled,
  draft,
  provider,
  onDraftChange,
  onFetch,
  onSave
}: {
  busy: boolean;
  catalogCount: number;
  disabled: boolean;
  draft: ProviderDraft;
  provider: ModelProviderView;
  onDraftChange(draft: ProviderDraft): void;
  onFetch(): void;
  onSave(): void;
}) {
  const saveDisabled = disabled || busy || !draft.providerId.trim() || !draft.label.trim() || !draft.baseUrl.trim();
  return (
    <div className="modelProviderRow">
      <div className="modelProviderIdentity">
        <strong>{provider.label}</strong>
        {providerSecondaryStatus(provider) && <span>{providerSecondaryStatus(provider)}</span>}
      </div>
      <div className="modelProviderFields">
        {provider.id === "custom" && (
          <label>
            <span>ID</span>
            <input
              disabled={disabled || busy}
              onChange={(event) => onDraftChange({ ...draft, providerId: event.currentTarget.value })}
              value={draft.providerId}
            />
          </label>
        )}
        {provider.id === "custom" && (
          <label>
            <span>Label</span>
            <input
              disabled={disabled || busy}
              onChange={(event) => onDraftChange({ ...draft, label: event.currentTarget.value })}
              value={draft.label}
            />
          </label>
        )}
        <label>
          <span>Base URL</span>
          <input
            disabled={disabled || busy}
            onChange={(event) => onDraftChange({ ...draft, baseUrl: event.currentTarget.value })}
            value={draft.baseUrl}
          />
        </label>
        {!draft.noAuth && (
          <>
            <label>
              <span>API key env</span>
              <input
                disabled={disabled || busy}
                onChange={(event) => onDraftChange({ ...draft, apiKeyEnv: event.currentTarget.value })}
                value={draft.apiKeyEnv}
              />
            </label>
            <label>
              <span>API key</span>
              <input
                disabled={disabled || busy}
                onChange={(event) => onDraftChange({ ...draft, apiKey: event.currentTarget.value })}
                type="password"
                value={draft.apiKey}
              />
            </label>
          </>
        )}
        <label className="modelNoAuthToggle">
          <input
            checked={draft.noAuth}
            disabled={disabled || busy}
            onChange={(event) => onDraftChange({ ...draft, noAuth: event.currentTarget.checked, apiKey: "" })}
            type="checkbox"
          />
          <span>No auth</span>
        </label>
      </div>
      <div className="modelProviderActions">
        <button disabled={disabled || busy || !provider.canFetchModels} onClick={onFetch} type="button">
          <Search size={13} />
          <span>{catalogCount ? `${catalogCount} models` : "Fetch"}</span>
        </button>
        <button disabled={saveDisabled} onClick={onSave} type="button">
          <Save size={13} />
          <span>{busy ? "Saving" : "Save"}</span>
        </button>
      </div>
    </div>
  );
}

function ModelAssignmentRow({
  busy,
  disabled,
  label,
  options,
  resetLabel,
  value,
  onChange,
  onSave
}: {
  busy: boolean;
  disabled: boolean;
  label: string;
  options: ModelOptionView[];
  resetLabel?: string;
  value: AssignmentDraft;
  onChange(value: AssignmentDraft): void;
  onSave(): void;
}) {
  const selectedOption = options.find((option) => option.value === value.model) ?? null;
  const reasoningOptions = reasoningEffortsForModelOption(selectedOption);
  const reasoningEffort = reasoningOptions.includes(value.reasoningEffort) ? value.reasoningEffort : "none";
  const modelDisabled = disabled || busy;
  function updateModel(model: string | null) {
    const nextModel = model ?? "";
    const nextOption = options.find((option) => option.value === nextModel) ?? null;
    const nextReasoningOptions = reasoningEffortsForModelOption(nextOption);
    onChange({
      model: nextModel,
      reasoningEffort: nextReasoningOptions.includes(value.reasoningEffort) ? value.reasoningEffort : "none"
    });
  }
  return (
    <div className="modelAssignmentRow">
      <div>
        <strong>{label}</strong>
      </div>
      <div className="modelAssignmentControls">
        <ModelReasoningSelector
          ariaLabel={label}
          className="modelAssignmentPicker"
          disabled={modelDisabled}
          emptyLabel="Select model"
          model={value.model || null}
          options={options}
          placement="bottom-start"
          resetLabel={resetLabel}
          variant={reasoningEffort}
          onModelChange={updateModel}
          onVariantChange={(nextReasoning) => onChange({ ...value, reasoningEffort: nextReasoning })}
        />
        <button disabled={disabled || busy || (!resetLabel && !value.model.trim())} onClick={onSave} type="button">
          <Save size={13} />
          <span>{busy ? "Saving" : "Save"}</span>
        </button>
      </div>
    </div>
  );
}

function mergeProviderDrafts(current: Record<string, ProviderDraft>, providers: ModelProviderView[]): Record<string, ProviderDraft> {
  const next = { ...current };
  for (const provider of providers) {
    next[provider.id] = next[provider.id] ?? providerDraftFromView(provider);
  }
  return next;
}

function providerDraftFromView(provider: ModelProviderView): ProviderDraft {
  return {
    providerId: provider.id === "custom" ? "" : provider.id,
    label: provider.id === "custom" ? "" : provider.label,
    baseUrl: provider.baseUrl ?? (provider.id === "custom" ? "http://127.0.0.1:1234/v1" : ""),
    apiKeyEnv: provider.apiKeyEnv ?? defaultApiKeyEnv(provider.id),
    apiKey: "",
    noAuth: provider.noAuth
  };
}

function defaultApiKeyEnv(providerId: string): string {
  return `${providerId.toUpperCase().replace(/[^A-Z0-9]+/g, "_").replace(/^_+|_+$/g, "")}_API_KEY`;
}

function splitModelValue(value: string): { provider: string; model: string } | null {
  const trimmed = value.trim();
  const index = trimmed.indexOf("/");
  if (index <= 0 || index === trimmed.length - 1) return null;
  return {
    provider: trimmed.slice(0, index),
    model: trimmed.slice(index + 1)
  };
}

function defaultAuxiliaryAssignments(): AuxiliaryModelAssignmentView[] {
  return [
    { task: "title_generation", label: "Title generation", provider: "auto", model: "", reasoningEffort: null, effectiveModel: null },
    { task: "compression", label: "Context compression", provider: "auto", model: "", reasoningEffort: null, effectiveModel: null }
  ];
}

function formatAuxTaskLabel(task: string): string {
  switch (task) {
    case "title_generation":
      return "Title generation";
    case "compression":
      return "Context compression";
    default:
      return task;
  }
}

function providerSecondaryStatus(provider: ModelProviderView): string | null {
  return provider.configured ? "Configured" : null;
}
