import { useEffect, useMemo, useState } from "react";
import { Pencil, Plus, RotateCcw, Save, Search, X } from "lucide-react";
import { ActionButton, CreatePanel, DisclosureButton, useActionReceipts } from "@psychevo/components";
import type { GatewayClient } from "@psychevo/client";
import type { AuxiliaryModelAssignmentView, ModelOptionView, ModelProviderView, ModelSettingsResult } from "@psychevo/protocol";
import { ModelReasoningSelector, reasoningEffortsForModelOption } from "../model-picker";
import { errorMessage } from "./common";

type ProviderDraft = {
  sourceProviderId: string;
  providerId: string;
  name: string;
  api: string;
  apiKey: string;
  noAuth: boolean;
  modelId: string;
  modelName: string;
  context: string;
  output: string;
  advancedFormat: "json" | "toml";
  advanced: string;
};

type AssignmentDraft = {
  model: string;
  reasoningEffort: string;
};

type VoiceSettingsRow = {
  key: "asr" | "tts" | "realtime";
  label: string;
  providerLabel: string;
  model: string;
  detail: string;
  credentialStatus: string;
};

type ImageGenerationSettingsRow = {
  providerLabel: string;
  model: string;
  detail: string;
  credentialStatus: string;
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
  const [editingProviderId, setEditingProviderId] = useState<string | null>(null);
  const [addingProvider, setAddingProvider] = useState(false);
  const [addDraft, setAddDraft] = useState<ProviderDraft | null>(null);
  const [loading, setLoading] = useState(false);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const receipts = useActionReceipts();
  const showReceipt = (message: string) => {
    receipts.push({ message });
    setNotice(receipts.available ? null : message);
  };

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
      setProviderDrafts((current) => mergeProviderDrafts(current, result.providers, result.modelOptions));
      setAddDraft((current) => current ?? initialAddDraft(result.providers));
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
  const providerTemplates = settings?.providers ?? [];
  const visibleProviders = providerTemplates.filter(providerIsAvailable);
  const freeSelection = modelOptions.find((option) => option.value === defaultDraft.model && option.free && option.provider === "opencode-zen")
    ?? Object.values(auxDrafts)
      .map((value) => modelOptions.find((option) => option.value === value.model && option.free && option.provider === "opencode-zen"))
      .find(Boolean);

  function patchProviderDraft(providerId: string, patch: Partial<ProviderDraft>) {
    setProviderDrafts((current) => {
      const provider = providerTemplates.find((item) => item.id === providerId);
      const draft = current[providerId] ?? providerDraftFromView(provider ?? customProviderTemplate(), modelOptions);
      return { ...current, [providerId]: { ...draft, ...patch } };
    });
  }

  async function fetchProviderCatalog(draft: ProviderDraft) {
    if (!client) return;
    const providerId = draft.providerId.trim();
    if (!providerId) {
      setError("Provider id is required");
      return;
    }
    setBusyKey(`catalog:${providerId}`);
    setError(null);
    setNotice(null);
    try {
      const result = await client.request("model/provider/catalog", {
        scope: "global",
        providerId,
        refresh: true,
        cwd: cwd
      });
      setCatalog((current) => ({ ...current, [result.providerId]: result.models }));
      onModelCatalogLoaded(result.models);
      showReceipt(`${displayProviderName(draft)} catalog updated`);
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusyKey(null);
    }
  }

  async function saveProvider(draft: ProviderDraft, mode: "add" | "edit") {
    if (!client) return;
    const providerId = draft.providerId.trim();
    const modelId = draft.modelId.trim();
    if (!providerId || !modelId) {
      setError("Provider id and model id are required");
      return;
    }
    setBusyKey(`provider:${providerId}`);
    setError(null);
    setNotice(null);
    try {
      const result = await client.request("model/provider/save", {
        scope: "global",
        providerId,
        name: draft.name.trim() || null,
        api: draft.api.trim(),
        apiKey: draft.noAuth ? null : draft.apiKey.trim() || null,
        noAuth: draft.noAuth,
        model: {
          id: modelId,
          name: draft.modelName.trim() || null,
          limit: {
            context: positiveIntegerOrNull(draft.context),
            output: positiveIntegerOrNull(draft.output)
          },
          advancedFormat: draft.advancedFormat,
          advanced: draft.advanced.trim() || null
        }
      });
      setSettings(result);
      setProviderDrafts((current) => mergeProviderDrafts(current, result.providers, result.modelOptions));
      showReceipt(`${displayProviderName(draft)} saved`);
      if (mode === "add") {
        setAddingProvider(false);
        setAddDraft(initialAddDraft(result.providers));
      } else {
        setEditingProviderId(null);
      }
      await loadModelSettings();
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
      showReceipt(target === "default" ? "Default model saved" : `${formatAuxTaskLabel(task ?? "")} saved`);
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

  const editingProvider = visibleProviders.find((provider) => provider.id === editingProviderId) ?? null;
  const editingDraft = editingProvider ? providerDrafts[editingProvider.id] ?? providerDraftFromView(editingProvider, modelOptions) : null;

  return (
    <section className="modelsSettingsPanel" aria-label="Models">
      <div className="modelSettingsToolbar">
        <ActionButton
          disabled={disabled || loading || !client}
          icon={<RotateCcw size={13} />}
          onClick={() => void loadModelSettings()}
          variant="ghost"
        >
          Refresh
        </ActionButton>
        <DisclosureButton
          controls="model-provider-editor-add"
          expanded={addingProvider}
          label="Connect provider"
          className={`modelProviderAddButton${addingProvider ? " is-active" : ""}`}
          disabled={disabled || loading || !client}
          icon={<Plus size={13} />}
          onExpandedChange={(expanded) => {
            if (!expanded) {
              setAddingProvider(false);
              return;
            }
            setAddingProvider(true);
            setEditingProviderId(null);
            setAddDraft((current) => current ?? initialAddDraft(providerTemplates));
          }}
          variant={addingProvider ? "secondary" : "primary"}
        >
          Connect provider
        </DisclosureButton>
      </div>
      {error && <div className="modelSettingsMessage is-error" role="alert">{error}</div>}
      {notice && <div className="modelSettingsMessage">{notice}</div>}
      {freeSelection && (
        <div className="modelSettingsMessage is-warning">
          OpenCode Zen free models may route data through free endpoints with different retention policies.
        </div>
      )}
      {addingProvider && addDraft && (
        <ProviderEditor
          busy={busyKey === `provider:${addDraft.providerId}` || busyKey === `catalog:${addDraft.providerId}`}
          catalogOptions={catalogOptionsForDraft(addDraft, modelOptions, catalog)}
          disabled={disabled || !client}
          draft={addDraft}
          mode="add"
          providerTemplates={providerTemplates}
          onCancel={() => setAddingProvider(false)}
          onDraftChange={setAddDraft}
          onFetch={() => void fetchProviderCatalog(addDraft)}
          onProviderTemplateChange={(provider) => setAddDraft(providerDraftFromView(provider, modelOptions))}
          onSave={() => void saveProvider(addDraft, "add")}
        />
      )}
      {editingProvider && editingDraft && (
        <ProviderEditor
          busy={busyKey === `provider:${editingDraft.providerId}` || busyKey === `catalog:${editingDraft.providerId}`}
          catalogOptions={catalogOptionsForDraft(editingDraft, modelOptions, catalog)}
          disabled={disabled || !client}
          draft={editingDraft}
          mode="edit"
          providerTemplates={providerTemplates}
          onCancel={() => setEditingProviderId(null)}
          onDraftChange={(nextDraft) => patchProviderDraft(editingProvider.id, nextDraft)}
          onFetch={() => void fetchProviderCatalog(editingDraft)}
          onProviderTemplateChange={(template) => patchProviderDraft(editingProvider.id, providerDraftFromView(template, modelOptions))}
          onSave={() => void saveProvider(editingDraft, "edit")}
        />
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
      <VoiceSettingsPanel voice={settings?.voice ?? null} />
      <ImageGenerationSettingsPanel value={settings?.imageGeneration ?? null} />
      <section className="modelProvidersPanel" aria-label="Available providers">
        <div className="modelProvidersHeader">
          <div>
            <strong>Providers</strong>
            <span>{visibleProviders.length ? `${visibleProviders.length} available` : "No available providers"}</span>
          </div>
        </div>
        {visibleProviders.map((provider) => {
          const editing = editingProviderId === provider.id;
          return (
            <div className="modelProviderStack" key={provider.id}>
              <ProviderSummaryRow
                busy={busyKey === `provider:${provider.id}` || busyKey === `catalog:${provider.id}`}
                disabled={disabled || !client}
                editing={editing}
                provider={provider}
                onEdit={() => {
                  if (editing) {
                    setEditingProviderId(null);
                    return;
                  }
                  setAddingProvider(false);
                  setEditingProviderId(provider.id);
                }}
              />
            </div>
          );
        })}
        {!settings && !loading && <div className="modelSettingsMessage">Model settings unavailable</div>}
      </section>
    </section>
  );
}

function VoiceSettingsPanel({ voice }: { voice: unknown }) {
  const rows = voiceSettingsRows(voice);
  return (
    <section className="voiceSettingsPanel" aria-label="Voice models">
      <div className="modelProvidersHeader">
        <div>
          <strong>Voice</strong>
          <span>ASR, TTS, realtime</span>
        </div>
      </div>
      {rows.map((row) => (
        <div className="voiceSettingsRow" key={row.key}>
          <div className="modelProviderIdentity">
            <strong>{row.label}</strong>
            <span>{row.providerLabel}</span>
          </div>
          <div className="voiceSettingsModel">
            <strong>{row.model}</strong>
            <span>{row.detail}</span>
          </div>
          <div className="modelProviderStatus" data-status={row.credentialStatus}>
            {voiceCredentialStatusLabel(row.credentialStatus)}
          </div>
        </div>
      ))}
    </section>
  );
}

function ImageGenerationSettingsPanel({ value }: { value: unknown }) {
  const row = imageGenerationSettingsRow(value);
  return (
    <section className="imageGenerationSettingsPanel" aria-label="Image generation">
      <div className="modelProvidersHeader">
        <div>
          <strong>Images</strong>
          <span>Generation provider</span>
        </div>
      </div>
      <div className="imageGenerationSettingsRow">
        <div className="modelProviderIdentity">
          <strong>Image generation</strong>
          <span>{row.providerLabel}</span>
        </div>
        <div className="voiceSettingsModel">
          <strong>{row.model}</strong>
          <span>{row.detail}</span>
        </div>
        <div className="modelProviderStatus" data-status={row.credentialStatus}>
          {voiceCredentialStatusLabel(row.credentialStatus)}
        </div>
      </div>
    </section>
  );
}

function ProviderSummaryRow({
  busy,
  disabled,
  editing,
  provider,
  onEdit
}: {
  busy: boolean;
  disabled: boolean;
  editing: boolean;
  provider: ModelProviderView;
  onEdit(): void;
}) {
  return (
    <div className="modelProviderRow">
      <div className="modelProviderIdentity">
        <strong>{provider.name}</strong>
        <span>{provider.id}</span>
      </div>
      <div className="modelProviderStatus" data-status={provider.credentialStatus}>
        {provider.credentialStatus === "notRequired" ? "No auth" : "API key ready"}
      </div>
        <DisclosureButton
          controls={`model-provider-editor-${provider.id}`}
          expanded={editing}
          label={`Edit ${provider.name}`}
          className={editing ? "is-active" : undefined}
          disabled={disabled || busy}
          icon={<Pencil size={13} />}
          onExpandedChange={onEdit}
        >
          Edit
        </DisclosureButton>
    </div>
  );
}

function voiceSettingsRows(value: unknown): VoiceSettingsRow[] {
  const voice = objectValue(value);
  const asr = objectValue(voice?.asr);
  const tts = objectValue(voice?.tts);
  const realtime = objectValue(voice?.realtime);
  return [
    {
      key: "asr",
      label: "ASR",
      providerLabel: voiceProviderLabel(asr),
      model: stringField(asr?.model) ?? "Not configured",
      detail: `Language ${stringField(asr?.language) ?? "auto"}`,
      credentialStatus: stringField(asr?.credentialStatus) ?? "missing"
    },
    {
      key: "tts",
      label: "TTS",
      providerLabel: voiceProviderLabel(tts),
      model: stringField(tts?.model) ?? "Not configured",
      detail: [
        stringField(tts?.voice),
        stringField(tts?.format)
      ].filter(Boolean).join(" / ") || "No voice selected",
      credentialStatus: stringField(tts?.credentialStatus) ?? "missing"
    },
    realtime ? {
      key: "realtime",
      label: "Realtime",
      providerLabel: voiceProviderLabel(realtime),
      model: stringField(realtime.model) ?? "Not configured",
      detail: [
        stringField(realtime.transport),
        stringField(realtime.voice)
      ].filter(Boolean).join(" / ") || "Live audio",
      credentialStatus: stringField(realtime.credentialStatus) ?? "missing"
    } : {
      key: "realtime",
      label: "Realtime",
      providerLabel: "Not configured",
      model: "No default",
      detail: "voice.realtime",
      credentialStatus: "notConfigured"
    }
  ];
}

function voiceProviderLabel(value: Record<string, unknown> | null): string {
  if (!value) {
    return "Not configured";
  }
  return stringField(value.providerLabel) ?? stringField(value.provider) ?? "Unknown provider";
}

function imageGenerationSettingsRow(value: unknown): ImageGenerationSettingsRow {
  const imageGeneration = objectValue(value);
  if (!imageGeneration) {
    return {
      providerLabel: "Not configured",
      model: "No default",
      detail: "image_generation",
      credentialStatus: "notConfigured"
    };
  }
  return {
    providerLabel: stringField(imageGeneration.label) ?? stringField(imageGeneration.provider) ?? "Unknown provider",
    model: stringField(imageGeneration.model) ?? "Not configured",
    detail: [
      stringField(imageGeneration.size),
      stringField(imageGeneration.format)
    ].filter(Boolean).join(" / ") || "Default output",
    credentialStatus: stringField(imageGeneration.credentialStatus) ?? "missing"
  };
}


function voiceCredentialStatusLabel(status: string): string {
  switch (status) {
    case "present":
      return "API key ready";
    case "missing":
      return "Missing key";
    case "notConfigured":
      return "Not configured";
    default:
      return status;
  }
}

function objectValue(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function stringField(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

function ProviderEditor({
  busy,
  catalogOptions,
  disabled,
  draft,
  mode,
  providerTemplates,
  onCancel,
  onDraftChange,
  onFetch,
  onProviderTemplateChange,
  onSave
}: {
  busy: boolean;
  catalogOptions: ModelOptionView[];
  disabled: boolean;
  draft: ProviderDraft;
  mode: "add" | "edit";
  providerTemplates: ModelProviderView[];
  onCancel(): void;
  onDraftChange(draft: ProviderDraft): void;
  onFetch(): void;
  onProviderTemplateChange(provider: ModelProviderView): void;
  onSave(): void;
}) {
  const providerSelectValue = draft.sourceProviderId === "custom" ? "custom" : draft.providerId;
  const saveDisabled = disabled
    || busy
    || !draft.providerId.trim()
    || !draft.api.trim()
    || !draft.modelId.trim()
    || (!draft.noAuth && mode === "add" && !draft.apiKey.trim());
  const modelListId = `model-options-${mode}-${draft.providerId || "custom"}`;
  return (
    <CreatePanel
      className="modelProviderEditor"
      description={mode === "add" ? "Connect a provider and register its first model." : "Update provider connection and model metadata."}
      icon={mode === "add" ? <Plus size={16} /> : <Pencil size={16} />}
      layout="side"
      id={`model-provider-editor-${mode === "add" ? "add" : draft.providerId}`}
      onClose={onCancel}
      title={mode === "add" ? "Connect provider" : `Edit ${displayProviderName(draft)}`}
      footer={
        <>
          <ActionButton disabled={disabled || busy} icon={<X size={13} />} onClick={onCancel} variant="ghost">
            Cancel
          </ActionButton>
          <ActionButton disabled={saveDisabled} icon={<Save size={13} />} onClick={onSave} variant="primary">
            {busy ? "Saving" : "Save provider"}
          </ActionButton>
        </>
      }
    >
      <div className="modelProviderEditorForm">
        <div className="modelProviderEditorRow modelProviderEditorRowProvider">
          {draft.sourceProviderId === "custom" ? (
            <label>
              <span>Provider id</span>
              <input
                className="pevo-fieldControl pevo-fieldControl--compact"
                disabled={disabled || busy}
                onChange={(event) => onDraftChange({ ...draft, providerId: event.currentTarget.value })}
                value={draft.providerId}
              />
            </label>
          ) : (
            <label>
              <span>Provider id</span>
              <select
                className="pevo-fieldControl pevo-fieldControl--compact"
                disabled={disabled || busy || mode === "edit"}
                onChange={(event) => {
                  const selected = providerTemplates.find((provider) => provider.id === event.currentTarget.value) ?? customProviderTemplate();
                  onProviderTemplateChange(selected);
                }}
                value={providerSelectValue}
              >
                {providerTemplates.map((provider) => (
                  <option key={provider.id} value={provider.id}>{provider.id === "custom" ? "Custom" : provider.id}</option>
                ))}
              </select>
            </label>
          )}
          <label>
            <span>Name</span>
            <input
              className="pevo-fieldControl pevo-fieldControl--compact"
              disabled={disabled || busy}
              onChange={(event) => onDraftChange({ ...draft, name: event.currentTarget.value })}
              value={draft.name}
            />
          </label>
        </div>
        <div className="modelProviderEditorRow modelProviderEditorRowAuth">
          <label>
            <span>Base URL</span>
            <input
              className="pevo-fieldControl pevo-fieldControl--compact"
              disabled={disabled || busy}
              onChange={(event) => onDraftChange({ ...draft, api: event.currentTarget.value })}
              value={draft.api}
            />
          </label>
          <label>
            <span>API key</span>
            <input
              className="pevo-fieldControl pevo-fieldControl--secret pevo-fieldControl--compact"
              disabled={disabled || busy || draft.noAuth}
              onChange={(event) => onDraftChange({ ...draft, apiKey: event.currentTarget.value })}
              type="password"
              value={draft.apiKey}
            />
          </label>
          <label>
            <span>API key env</span>
            <input className="pevo-fieldControl pevo-fieldControl--compact" readOnly value={defaultApiKeyEnv(draft.providerId)} />
          </label>
          <label className="modelNoAuthToggle">
            <input
              checked={draft.noAuth}
              className="pevo-choiceControl"
              disabled={disabled || busy}
              onChange={(event) => onDraftChange({ ...draft, noAuth: event.currentTarget.checked, apiKey: "" })}
              type="checkbox"
            />
            <span>No auth</span>
          </label>
        </div>
        <div className="modelProviderEditorRow modelProviderEditorRowModel">
          <label>
            <span>Model id</span>
            <input
              className="pevo-fieldControl pevo-fieldControl--compact"
              disabled={disabled || busy}
              list={modelListId}
              onChange={(event) => onDraftChange({ ...draft, modelId: event.currentTarget.value })}
              value={draft.modelId}
            />
            <datalist id={modelListId}>
              {catalogOptions.map((option) => (
                <option key={option.value} value={option.id}>{option.name ?? option.value}</option>
              ))}
            </datalist>
          </label>
          <ActionButton className="modelProviderFetchButton" disabled={disabled || busy || !draft.providerId.trim()} icon={<Search size={13} />} onClick={onFetch} variant="ghost">
            Fetch models
          </ActionButton>
          <label>
            <span>Name</span>
            <input
              className="pevo-fieldControl pevo-fieldControl--compact"
              disabled={disabled || busy}
              onChange={(event) => onDraftChange({ ...draft, modelName: event.currentTarget.value })}
              value={draft.modelName}
            />
          </label>
        </div>
        <div className="modelProviderEditorRow modelProviderEditorRowLimits">
          <label>
            <span>Context</span>
            <input
              className="pevo-fieldControl pevo-fieldControl--compact"
              disabled={disabled || busy}
              inputMode="numeric"
              onChange={(event) => onDraftChange({ ...draft, context: event.currentTarget.value })}
              value={draft.context}
            />
          </label>
          <label>
            <span>Max output</span>
            <input
              className="pevo-fieldControl pevo-fieldControl--compact"
              disabled={disabled || busy}
              inputMode="numeric"
              onChange={(event) => onDraftChange({ ...draft, output: event.currentTarget.value })}
              value={draft.output}
            />
          </label>
          <label>
            <span>Advanced</span>
            <select
              className="pevo-fieldControl pevo-fieldControl--compact"
              disabled={disabled || busy}
              onChange={(event) => onDraftChange({ ...draft, advancedFormat: event.currentTarget.value === "toml" ? "toml" : "json" })}
              value={draft.advancedFormat}
            >
              <option value="json">JSON</option>
              <option value="toml">TOML</option>
            </select>
          </label>
        </div>
        <div className="modelProviderEditorRow modelProviderEditorRowAdvanced">
          <label>
            <span>Advanced Metadata</span>
            <textarea
              className="pevo-fieldControl pevo-fieldControl--code"
              disabled={disabled || busy}
              onChange={(event) => onDraftChange({ ...draft, advanced: event.currentTarget.value })}
              spellCheck={false}
              value={draft.advanced}
            />
          </label>
        </div>
      </div>
    </CreatePanel>
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

function mergeProviderDrafts(
  current: Record<string, ProviderDraft>,
  providers: ModelProviderView[],
  options: ModelOptionView[]
): Record<string, ProviderDraft> {
  const next = { ...current };
  for (const provider of providers) {
    next[provider.id] = next[provider.id] ?? providerDraftFromView(provider, options);
  }
  return next;
}

function providerDraftFromView(provider: ModelProviderView, options: ModelOptionView[]): ProviderDraft {
  const firstModel = options.find((option) => option.provider === provider.id);
  return {
    sourceProviderId: provider.id,
    providerId: provider.id === "custom" ? "" : provider.id,
    name: provider.id === "custom" ? "" : provider.name,
    api: provider.api ?? (provider.id === "custom" ? "http://127.0.0.1:1234/v1" : ""),
    apiKey: "",
    noAuth: provider.noAuth || provider.credentialStatus === "notRequired",
    modelId: firstModel?.id ?? "",
    modelName: firstModel?.name ?? "",
    context: firstModel?.limit.context ? String(firstModel.limit.context) : "",
    output: firstModel?.limit.output ? String(firstModel.limit.output) : "",
    advancedFormat: "json",
    advanced: ""
  };
}

function customProviderTemplate(): ModelProviderView {
  return {
    id: "custom",
    name: "Custom",
    builtIn: false,
    configured: false,
    api: null,
    apiKeyEnv: null,
    credentialStatus: "missing",
    noAuth: false,
    canFetchModels: false,
    unavailableReason: "requires provider setup"
  };
}

function initialAddDraft(providers: ModelProviderView[]): ProviderDraft {
  const custom = providers.find((provider) => provider.id === "custom") ?? customProviderTemplate();
  return providerDraftFromView(custom, []);
}

function providerIsAvailable(provider: ModelProviderView): boolean {
  if (provider.id === "custom") return false;
  return provider.credentialStatus === "present" || provider.credentialStatus === "notRequired" || provider.noAuth;
}

function catalogOptionsForDraft(
  draft: ProviderDraft,
  options: ModelOptionView[],
  catalog: Record<string, ModelOptionView[]>
): ModelOptionView[] {
  const providerId = draft.providerId.trim();
  const merged = new Map<string, ModelOptionView>();
  for (const option of options) {
    if (option.provider === providerId) {
      merged.set(option.value, option);
    }
  }
  for (const option of catalog[providerId] ?? []) {
    merged.set(option.value, option);
  }
  return [...merged.values()].sort((left, right) => left.id.localeCompare(right.id));
}

function defaultApiKeyEnv(providerId: string): string {
  const normalized = providerId.toUpperCase().replace(/[^A-Z0-9]+/g, "_").replace(/^_+|_+$/g, "");
  return `${normalized || "PROVIDER"}_API_KEY`;
}

function displayProviderName(draft: ProviderDraft): string {
  return draft.name.trim() || draft.providerId.trim() || "Provider";
}

function positiveIntegerOrNull(value: string): number | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const parsed = Number.parseInt(trimmed, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
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
