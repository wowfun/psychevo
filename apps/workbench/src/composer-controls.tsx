import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { Check, ShieldCheck, ShieldPlus, X } from "lucide-react";
import type {
  ContextReadResult,
  PendingActionView,
  PermissionDecision,
  SessionUsageSummaryView,
  SettingsReadResult,
  ThreadControlDescriptorView
} from "@psychevo/protocol";
import { ModelReasoningSelector, modelOptionsForThreadControl } from "./model-picker";
import { SessionUsageGrid, normalizedPercent } from "./right-workspace/usage";

export function ComposerRequests({
  clarifies,
  permissions,
  onClarify,
  onPermission
}: {
  clarifies: PendingActionView[];
  permissions: PendingActionView[];
  onClarify(request: PendingActionView, answers: string[][] | null, cancel: boolean): void;
  onPermission(request: PendingActionView, decision: PermissionDecision): void;
}) {
  if (permissions.length === 0 && clarifies.length === 0) {
    return null;
  }
  return (
    <div className="composerRequests" aria-label="Pending requests">
      {permissions.map((permission) => (
        <div className="composerRequest" key={permission.actionId}>
          <div className="composerRequestHeader">
            <strong>{permissionTitle(permission)}</strong>
            {permissionTimeoutSecs(permission) ? <span>{permissionTimeoutSecs(permission)}s</span> : null}
          </div>
          <AttentionProvenance request={permission} />
          <p>{permissionSummary(permission)}</p>
          {permissionReason(permission) && permissionSummary(permission) !== permissionReason(permission) ? (
            <p>{permissionReason(permission)}</p>
          ) : null}
          {(permissionMatchedRule(permission) || permissionSuggestedRule(permission)) ? (
            <div className="composerRequestMeta">
              {permissionMatchedRule(permission) ? <code>{permissionMatchedRule(permission)}</code> : null}
              {permissionSuggestedRule(permission) ? <code>{permissionSuggestedRule(permission)}</code> : null}
            </div>
          ) : null}
          <div className="composerRequestActions">
            <button onClick={() => onPermission(permission, "allowOnce")} type="button">
              <Check size={14} />
              <span>Once</span>
            </button>
            {permissionAllowSession(permission) ? (
              <button onClick={() => onPermission(permission, "allowSession")} type="button">
                <ShieldCheck size={14} />
                <span>Session</span>
              </button>
            ) : null}
            {permissionAllowAlways(permission) ? (
              <button onClick={() => onPermission(permission, "allowAlways")} type="button">
                <ShieldPlus size={14} />
                <span>Always</span>
              </button>
            ) : null}
            <button onClick={() => onPermission(permission, "deny")} type="button">
              <X size={14} />
              <span>Deny</span>
            </button>
          </div>
        </div>
      ))}
      {clarifies.map((clarify) => (
        <ClarifyComposerRequest key={clarify.actionId} request={clarify} onSubmit={onClarify} />
      ))}
    </div>
  );
}

function ClarifyComposerRequest({
  request,
  onSubmit
}: {
  request: PendingActionView;
  onSubmit(request: PendingActionView, answers: string[][] | null, cancel: boolean): void;
}) {
  const questions = useMemo(
    () => parseClarifyQuestions(clarifyRawPayload(request)),
    [request.actionId]
  );
  const url = useMemo(() => clarifyUrl(request), [request.actionId]);
  const [answers, setAnswers] = useState<ClarifyAnswerState[]>(() => initialClarifyAnswers(questions));
  const [fallbackAnswer, setFallbackAnswer] = useState("");

  useEffect(() => {
    setAnswers(initialClarifyAnswers(questions));
    setFallbackAnswer("");
  }, [questions, request.actionId]);

  const resolvedAnswers = questions.map((question, index) => resolvedClarifyAnswer(
    answers[index] ?? defaultClarifyAnswer(question)
  ));
  const canSubmit = questions.length === 0
    ? fallbackAnswer.trim().length > 0
    : resolvedAnswers.every((answer, index) => {
        const state = answers[index] ?? defaultClarifyAnswer(questions[index]!);
        return !questions[index]!.required
          || (answer.length > 0 && (!state.customSelected || state.custom.trim().length > 0));
      });

  function submitClarify() {
    if (!canSubmit) {
      return;
    }
    if (questions.length === 0) {
      onSubmit(request, [[fallbackAnswer.trim()]], false);
      setFallbackAnswer("");
      return;
    }
    onSubmit(request, resolvedAnswers, false);
  }

  return (
    <div className="composerRequest">
      <div className="composerRequestHeader">
        <strong>Clarify</strong>
        {request.turnId ? <span>{request.turnId}</span> : null}
      </div>
      <AttentionProvenance request={request} />
      {url ? (
        <a className="composerRequestLink" href={url} rel="noopener noreferrer" target="_blank">
          Open Codex App link
        </a>
      ) : null}
      {questions.length === 0 ? (
        <input
          value={fallbackAnswer}
          onChange={(event) => setFallbackAnswer(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              submitClarify();
            }
          }}
        />
      ) : (
        <div className="composerClarifyQuestions">
          {questions.map((question, questionIndex) => {
            const answer = answers[questionIndex] ?? defaultClarifyAnswer(question);
            return (
              <fieldset className="composerClarifyQuestion" key={`${request.actionId}:${questionIndex}`}>
                <legend>{question.question}</legend>
                {[
                  ...question.options,
                  ...(question.custom && question.options.length > 0 ? [OTHER_OPTION] : [])
                ].map((option, optionIndex) => {
                  const isOther = option.label === OTHER_OPTION.label;
                  const checked = isOther
                    ? answer.customSelected
                    : answer.selected.includes(option.label);
                  return (
                    <label className="composerClarifyOption" key={`${option.label}:${optionIndex}`}>
                      <input
                        checked={checked}
                        name={`${request.actionId}:${questionIndex}`}
                        type={question.multiple ? "checkbox" : "radio"}
                        onChange={() => {
                          setAnswers((current) => toggleClarifyAnswer(
                            current,
                            questionIndex,
                            question,
                            option.label,
                            isOther
                          ));
                        }}
                      />
                      <span>
                        {option.image ? (
                          <img alt="" className="composerClarifyOptionImage" src={option.image} />
                        ) : null}
                        <strong>{option.label}</strong>
                        {option.description ? <small>{option.description}</small> : null}
                      </span>
                    </label>
                  );
                })}
                {answer.customSelected ? (
                  <input
                    aria-label={`${question.question} custom answer`}
                    type={question.secret ? "password" : "text"}
                    value={answer.custom}
                    onChange={(event) => {
                      setAnswers((current) => replaceClarifyAnswer(current, questionIndex, {
                        ...answer,
                        custom: event.target.value
                      }));
                    }}
                  />
                ) : null}
              </fieldset>
            );
          })}
        </div>
      )}
      <div className="composerRequestActions">
        <button disabled={!canSubmit} onClick={submitClarify} type="button">Submit</button>
        <button onClick={() => onSubmit(request, null, true)} type="button">Cancel</button>
      </div>
    </div>
  );
}

type ClarifyOption = {
  label: string;
  description: string;
  image?: string;
};

type ClarifyQuestion = {
  header: string;
  question: string;
  options: ClarifyOption[];
  multiple: boolean;
  custom: boolean;
  secret: boolean;
  required: boolean;
};

type ClarifyAnswerState = {
  selected: string[];
  customSelected: boolean;
  custom: string;
};

const OTHER_OPTION: ClarifyOption = {
  label: "Other",
  description: ""
};

function parseClarifyQuestions(raw: unknown): ClarifyQuestion[] {
  const record = asRecord(raw);
  const questions = Array.isArray(record.questions) ? record.questions : [];
  return questions.flatMap((value): ClarifyQuestion[] => {
    const question = asRecord(value);
    const text = typeof question.question === "string" ? question.question.trim() : "";
    const options = Array.isArray(question.options)
      ? question.options.flatMap((option): ClarifyOption[] => {
          const optionRecord = asRecord(option);
          const label = typeof optionRecord.label === "string" ? optionRecord.label.trim() : "";
          if (!label) {
            return [];
          }
          const image = safeImageUrl(optionRecord.image);
          return [{
            label,
            description: typeof optionRecord.description === "string" ? optionRecord.description.trim() : "",
            ...(image ? { image } : {})
          }];
        })
      : [];
    if (!text) {
      return [];
    }
    return [{
      header: typeof question.header === "string" ? question.header.trim() : "",
      question: text,
      options,
      multiple: question.multiple === true,
      custom: typeof question.custom === "boolean" ? question.custom : true,
      secret: question.secret === true,
      required: question.required !== false
    }];
  });
}

function initialClarifyAnswers(questions: ClarifyQuestion[]): ClarifyAnswerState[] {
  return questions.map(defaultClarifyAnswer);
}

function defaultClarifyAnswer(question: ClarifyQuestion): ClarifyAnswerState {
  return {
    selected: question.multiple ? [] : question.options[0] ? [question.options[0].label] : [],
    customSelected: question.custom && question.options.length === 0,
    custom: ""
  };
}

function resolvedClarifyAnswer(answer: ClarifyAnswerState): string[] {
  const custom = answer.custom.trim();
  return [
    ...answer.selected,
    ...(answer.customSelected && custom ? [custom] : [])
  ];
}

function toggleClarifyAnswer(
  answers: ClarifyAnswerState[],
  index: number,
  question: ClarifyQuestion,
  label: string,
  custom: boolean
): ClarifyAnswerState[] {
  const current = answers[index] ?? defaultClarifyAnswer(question);
  if (!question.multiple) {
    return replaceClarifyAnswer(answers, index, {
      selected: custom ? [] : [label],
      customSelected: custom,
      custom: custom ? current.custom : ""
    });
  }
  if (custom) {
    return replaceClarifyAnswer(answers, index, {
      ...current,
      customSelected: !current.customSelected,
      custom: current.customSelected ? "" : current.custom
    });
  }
  return replaceClarifyAnswer(answers, index, {
    ...current,
    selected: current.selected.includes(label)
      ? current.selected.filter((value) => value !== label)
      : [...current.selected, label]
  });
}

function replaceClarifyAnswer(
  answers: ClarifyAnswerState[],
  index: number,
  answer: ClarifyAnswerState
): ClarifyAnswerState[] {
  const next = [...answers];
  next[index] = answer;
  return next;
}

function asRecord(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value) ? value as Record<string, unknown> : {};
}

function safeImageUrl(value: unknown): string | undefined {
  if (typeof value !== "string") {
    return undefined;
  }
  const candidate = value.trim();
  if (/^data:image\/(?:png|jpeg|webp|gif|svg\+xml);base64,/i.test(candidate)) {
    return candidate;
  }
  try {
    const url = new URL(candidate);
    return url.protocol === "https:" ? url.toString() : undefined;
  } catch {
    return undefined;
  }
}

function actionPayload(action: PendingActionView): Record<string, unknown> {
  return asRecord(action.payload);
}

function actionPayloadString(action: PendingActionView, key: string): string {
  const value = actionPayload(action)[key];
  return typeof value === "string" ? value : "";
}

function actionPayloadNumber(action: PendingActionView, key: string): number {
  const value = actionPayload(action)[key];
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

function actionPayloadBool(action: PendingActionView, key: string): boolean {
  return actionPayload(action)[key] === true;
}

function actionPayloadRecord(action: PendingActionView, key: string): Record<string, unknown> {
  return asRecord(actionPayload(action)[key]);
}

function AttentionProvenance({ request }: { request: PendingActionView }) {
  const runtimeRef = actionPayloadString(request, "runtimeRef");
  const runtimeKind = runtimeKindLabel(actionPayloadString(request, "runtimeKind"));
  const profileLabel = actionPayloadString(request, "profileLabel");
  const origin = actionPayloadRecord(request, "origin");
  const parentThreadId = typeof origin.parentThreadId === "string" ? origin.parentThreadId : "";
  const childThreadId = typeof origin.childThreadId === "string" ? origin.childThreadId : "";
  const sessionLifetime = actionPayloadString(request, "authorizationLifetime");
  const alwaysLifetime = actionPayloadString(request, "alwaysAuthorizationLifetime");
  const hasProvenance = Boolean(runtimeRef || runtimeKind || profileLabel || parentThreadId || childThreadId);
  if (!hasProvenance && request.kind !== "permission") {
    return null;
  }
  return (
    <div className="composerRequestAttention" aria-label="Shared Attention context">
      {runtimeRef || runtimeKind || profileLabel ? (
        <span>{`${runtimeKind || "Runtime"} · ${profileLabel || runtimeRef}${runtimeRef ? ` (${runtimeRef})` : ""}`}</span>
      ) : null}
      {childThreadId ? (
        <span>{`Child ${childThreadId}${parentThreadId ? ` · Parent ${parentThreadId}` : ""}`}</span>
      ) : parentThreadId ? <span>{`Parent ${parentThreadId}`}</span> : null}
      {request.kind === "permission" ? <span>Once · this request only</span> : null}
      {request.kind === "permission" && permissionAllowSession(request) ? (
        <span>{`Session · ${authorizationLifetimeLabel(sessionLifetime)}`}</span>
      ) : null}
      {request.kind === "permission" && permissionAllowAlways(request) ? (
        <span>{`Always · ${authorizationLifetimeLabel(alwaysLifetime)}`}</span>
      ) : null}
    </div>
  );
}

function runtimeKindLabel(value: string): string {
  const normalized = value.trim();
  return normalized.toLowerCase() === "acp" ? "ACP" : normalized;
}

function authorizationLifetimeLabel(value: string): string {
  switch (value) {
    case "until_runtime_instance_restarts": return "until the runtime instance restarts";
    case "permanent": return "permanent";
    default:
      return value.endsWith("_session")
        ? "current Agent session"
        : value
          ? value.replaceAll("_", " ")
          : "adapter-declared scope";
  }
}

function permissionTitle(permission: PendingActionView): string {
  return permission.title ?? (actionPayloadString(permission, "toolName") || "permission");
}

function permissionSummary(permission: PendingActionView): string {
  return permission.summary ?? (actionPayloadString(permission, "summary") || permissionReason(permission));
}

function permissionReason(permission: PendingActionView): string {
  return actionPayloadString(permission, "reason");
}

function permissionMatchedRule(permission: PendingActionView): string {
  return actionPayloadString(permission, "matchedRule");
}

function permissionSuggestedRule(permission: PendingActionView): string {
  return actionPayloadString(permission, "suggestedRule");
}

function permissionAllowAlways(permission: PendingActionView): boolean {
  return actionPayloadBool(permission, "allowAlways")
    && actionPayloadString(permission, "alwaysAuthorizationLifetime") === "permanent";
}

function permissionAllowSession(permission: PendingActionView): boolean {
  return actionPayloadBool(permission, "allowSession")
    && Boolean(actionPayloadString(permission, "authorizationLifetime"));
}

function permissionTimeoutSecs(permission: PendingActionView): number {
  return actionPayloadNumber(permission, "timeoutSecs");
}

function clarifyRawPayload(action: PendingActionView): unknown {
  const payload = actionPayload(action);
  return payload.raw ?? action.payload;
}

function clarifyUrl(action: PendingActionView): string {
  const raw = asRecord(clarifyRawPayload(action));
  const value = typeof raw.url === "string" ? raw.url.trim() : "";
  if (!value) {
    return "";
  }
  try {
    const url = new URL(value);
    return url.protocol === "https:" || url.protocol === "http:" ? url.toString() : "";
  } catch {
    return "";
  }
}

export function ComposerSubmitControls({
  context,
  controls,
  usage,
  controlValues,
  disabled,
  modelControl,
  reasoningControl,
  onContextOpen,
  onControlChange
}: {
  context: ContextReadResult | null;
  controls: SettingsReadResult["controls"];
  usage: SessionUsageSummaryView | null;
  controlValues: Record<string, unknown>;
  disabled: boolean;
  modelControl: ThreadControlDescriptorView | null;
  reasoningControl: ThreadControlDescriptorView | null;
  onContextOpen?(): void;
  onControlChange(control: ThreadControlDescriptorView, value: unknown): void;
}) {
  const contextPercent = normalizedPercent(context?.percent);
  const contextHasLimit = context?.available === true && context.contextLimit != null;
  const contextSummaryValue = context?.available
    ? contextHasLimit
      ? `${Math.round(contextPercent)}%`
      : compactTokenCount(context.usedTokens)
    : "—";
  const [contextOpen, setContextOpen] = useState(false);
  const contextPopoverRef = useRef<HTMLDivElement | null>(null);
  const model = controlStringValue(modelControl, controlValues);
  const explicitReasoning = controlStringValue(reasoningControl, controlValues);
  const reasoning = explicitReasoning;
  const reasoningSelectable = reasoningControl?.enabled === true
    && reasoningControl.mutability === "selectable"
    && reasoningControl.choices.some((choice) => typeof choice.value === "string");
  const reasoningPresentation = reasoningSelectable
    ? "selectable"
    : explicitReasoning
      ? "readOnly"
      : "hidden";
  const reasoningValues = reasoningControl?.choices.flatMap((choice): string[] => (
    typeof choice.value === "string" ? [choice.value] : []
  )) ?? [];
  const richModelControl = modelControl?.mutability === "selectable"
    && modelControl.enabled
    && modelControl.choices.some((choice) => typeof choice.value === "string");

  useEffect(() => {
    if (!contextOpen) {
      return;
    }
    function onPointerDown(event: MouseEvent) {
      if (contextPopoverRef.current?.contains(event.target as Node)) {
        return;
      }
      setContextOpen(false);
    }
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setContextOpen(false);
      }
    }
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [contextOpen]);

  return (
    <div className="composerSubmitControls" aria-label="Composer submit controls">
      {richModelControl && modelControl && (
        <ModelReasoningSelector
          disabled={disabled || !modelControl.enabled}
          emptyLabel={emptyModelOptionLabel(controls)}
          model={model}
          options={modelOptionsForThreadControl(modelControl, controls, model)}
          recentModels={controls?.recentModels ?? []}
          reasoningPresentation={reasoningPresentation}
          reasoningValues={reasoningValues}
          showChevron={false}
          variant={reasoning}
          onModelChange={(value) => {
            if (value != null) onControlChange(modelControl, value);
          }}
          onSelectionChange={(value, nextReasoning) => {
            if (value != null && value !== model) {
              onControlChange(modelControl, value);
              return;
            }
            if (reasoningSelectable && reasoningControl && nextReasoning !== reasoning) {
              onControlChange(reasoningControl, nextReasoning);
            }
          }}
          onVariantChange={(value) => {
            if (reasoningSelectable && reasoningControl) onControlChange(reasoningControl, value);
          }}
        />
      )}
      {modelControl && !richModelControl && (
        <span
          aria-label={`Model: ${model ?? (modelControl.enabled ? "selection required" : "unavailable")} (${modelControl.enabled ? "read-only" : "unavailable"})`}
          className={`runtimeControlState is-readonly ${model == null ? "is-unavailable" : ""}`}
          title={modelControl.unavailableReason ?? "Model is not selectable for this Agent target."}
        >
          {model ?? (modelControl.enabled ? "Select model" : "Model unavailable")}
        </span>
      )}
      <div className="composerStatusContext" ref={contextPopoverRef}>
        <button
          aria-label="Context usage"
          aria-expanded={contextOpen}
          className="contextStatusButton"
          onClick={() => {
            setContextOpen((value) => {
              if (!value) onContextOpen?.();
              return !value;
            });
          }}
          title={context?.label ?? "No active context"}
          type="button"
        >
          <span
            className={contextHasLimit ? undefined : "is-limitUnavailable"}
            style={{ "--pevo-context-percent": `${contextPercent}%` } as CSSProperties}
          />
        </button>
        {contextOpen && (
          <div className="composerContextPopover" role="dialog" aria-label="Context usage">
            <div className="composerContextSummary">
              <span
                className={contextHasLimit ? undefined : "is-limitUnavailable"}
                style={{ "--pevo-context-percent": `${contextPercent}%` } as CSSProperties}
              >
                {contextSummaryValue}
              </span>
              <div>
                <strong>{context?.label ?? "No active context"}</strong>
                <small>{context?.status ?? "unavailable"}</small>
              </div>
            </div>
            {usage?.available && (
              <SessionUsageGrid compact usage={usage} />
            )}
            {context?.available && context.contextLimit == null && (
              <p>{context.usedTokens.toLocaleString()} tokens · Limit unavailable</p>
            )}
            {!context?.available && (
              <p>Context unavailable.</p>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function controlStringValue(
  control: ThreadControlDescriptorView | null,
  values: Record<string, unknown>
): string | null {
  if (!control) return null;
  const value = Object.prototype.hasOwnProperty.call(values, control.id)
    ? values[control.id]
    : control.effectiveValue;
  return typeof value === "string" && value.trim() ? value : null;
}

function compactTokenCount(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "0";
  if (value < 1_000) return Math.round(value).toLocaleString();
  const compact = new Intl.NumberFormat(undefined, {
    notation: "compact",
    maximumFractionDigits: 1
  }).format(value);
  return compact.toLowerCase();
}

export function StatusSelect({
  disabled = false,
  label,
  optionLabels,
  renderDisplayValue,
  value,
  values,
  onChange
}: {
  disabled?: boolean;
  label: string;
  optionLabels?: Record<string, string>;
  renderDisplayValue?(value: string): string;
  value: string;
  values: string[];
  onChange(value: string): void;
}) {
  const displayValue = renderDisplayValue?.(value);
  const valueWidth = displayValue ? `${Math.max(5, displayValue.length + 1)}ch` : undefined;
  return (
    <label
      className={`statusSelect ${displayValue ? "has-displayValue" : ""}`}
      data-disabled={disabled ? "true" : undefined}
      data-status={label.toLowerCase().replace(/\s+/g, "-")}
      style={valueWidth ? { "--pevo-status-select-value-width": valueWidth } as CSSProperties : undefined}
      title={label}
    >
      {displayValue && <span aria-hidden="true" className="statusSelectDisplay">{displayValue}</span>}
      <select disabled={disabled} aria-label={label} title={value || label} value={value} onChange={(event) => onChange(event.target.value)}>
        {values.map((option) => (
          <option key={option || "default"} value={option}>{optionLabels?.[option] ?? defaultStatusSelectValue(label, option)}</option>
        ))}
      </select>
    </label>
  );
}

function defaultStatusSelectValue(label: string, value: string): string {
  if (label === "Permission mode" && value === "default") {
    return "Default Permission";
  }
  return value || label.toLowerCase();
}

function emptyModelOptionLabel(controls: SettingsReadResult["controls"]): string {
  if (controls?.modelStatus === "error") {
    return "Model unavailable";
  }
  return "Select model";
}
