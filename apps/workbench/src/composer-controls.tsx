import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { Check, GitBranch, Pin, ShieldCheck, ShieldPlus, X } from "lucide-react";
import type {
  ContextReadResult,
  InitializeResult,
  PendingClarify,
  PendingPermission,
  PermissionDecision,
  SessionUsageSummaryView,
  SettingsReadResult
} from "@psychevo/protocol";
import { SessionUsageGrid, normalizedPercent } from "./right-workspace";

export function ComposerRequests({
  clarifies,
  permissions,
  onClarify,
  onPermission
}: {
  clarifies: PendingClarify[];
  permissions: PendingPermission[];
  onClarify(request: PendingClarify, answers: string[][] | null, cancel: boolean): void;
  onPermission(request: PendingPermission, decision: PermissionDecision): void;
}) {
  if (permissions.length === 0 && clarifies.length === 0) {
    return null;
  }
  return (
    <div className="composerRequests" aria-label="Pending requests">
      {permissions.map((permission) => (
        <div className="composerRequest" key={permission.requestId}>
          <div className="composerRequestHeader">
            <strong>{permission.toolName}</strong>
            {permission.timeoutSecs ? <span>{permission.timeoutSecs}s</span> : null}
          </div>
          <p>{permission.summary || permission.reason}</p>
          {permission.summary && permission.reason && permission.summary !== permission.reason ? (
            <p>{permission.reason}</p>
          ) : null}
          {(permission.matchedRule || permission.suggestedRule) ? (
            <div className="composerRequestMeta">
              {permission.matchedRule ? <code>{permission.matchedRule}</code> : null}
              {permission.suggestedRule ? <code>{permission.suggestedRule}</code> : null}
            </div>
          ) : null}
          <div className="composerRequestActions">
            <button onClick={() => onPermission(permission, "allowOnce")} type="button">
              <Check size={14} />
              <span>Once</span>
            </button>
            <button onClick={() => onPermission(permission, "allowSession")} type="button">
              <ShieldCheck size={14} />
              <span>Session</span>
            </button>
            {permission.allowAlways ? (
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
        <ClarifyComposerRequest key={clarify.requestId} request={clarify} onSubmit={onClarify} />
      ))}
    </div>
  );
}

function ClarifyComposerRequest({
  request,
  onSubmit
}: {
  request: PendingClarify;
  onSubmit(request: PendingClarify, answers: string[][] | null, cancel: boolean): void;
}) {
  const questions = useMemo(() => parseClarifyQuestions(request.raw), [request.raw]);
  const [answers, setAnswers] = useState<ClarifyAnswerState[]>(() => initialClarifyAnswers(questions));
  const [fallbackAnswer, setFallbackAnswer] = useState("");

  useEffect(() => {
    setAnswers(initialClarifyAnswers(questions));
    setFallbackAnswer("");
  }, [questions, request.requestId]);

  const resolvedAnswers = questions.map((question, index) => {
    const answer = answers[index] ?? defaultClarifyAnswer(question);
    return answer.kind === "other" ? answer.custom.trim() : answer.value;
  });
  const canSubmit = questions.length === 0
    ? fallbackAnswer.trim().length > 0
    : resolvedAnswers.every((answer) => answer.trim().length > 0);

  function submitClarify() {
    if (!canSubmit) {
      return;
    }
    if (questions.length === 0) {
      onSubmit(request, [[fallbackAnswer.trim()]], false);
      setFallbackAnswer("");
      return;
    }
    onSubmit(request, resolvedAnswers.map((answer) => [answer]), false);
  }

  return (
    <div className="composerRequest">
      <div className="composerRequestHeader">
        <strong>Clarify</strong>
        {request.turnId ? <span>{request.turnId}</span> : null}
      </div>
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
              <fieldset className="composerClarifyQuestion" key={`${request.requestId}:${questionIndex}`}>
                <legend>{question.question}</legend>
                {[...question.options, OTHER_OPTION].map((option) => {
                  const isOther = option.label === OTHER_OPTION.label;
                  const checked = isOther
                    ? answer.kind === "other"
                    : answer.kind === "option" && answer.value === option.label;
                  return (
                    <label className="composerClarifyOption" key={option.label}>
                      <input
                        checked={checked}
                        name={`${request.requestId}:${questionIndex}`}
                        type="radio"
                        onChange={() => {
                          setAnswers((current) => replaceClarifyAnswer(
                            current,
                            questionIndex,
                            isOther
                              ? { kind: "other", value: OTHER_OPTION.label, custom: "" }
                              : { kind: "option", value: option.label, custom: "" }
                          ));
                        }}
                      />
                      <span>
                        <strong>{option.label}</strong>
                        {option.description ? <small>{option.description}</small> : null}
                      </span>
                    </label>
                  );
                })}
                {answer.kind === "other" ? (
                  <input
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
};

type ClarifyQuestion = {
  question: string;
  options: ClarifyOption[];
};

type ClarifyAnswerState =
  | { kind: "option"; value: string; custom: string }
  | { kind: "other"; value: string; custom: string };

const OTHER_OPTION: ClarifyOption = {
  label: "Other",
  description: ""
};

function parseClarifyQuestions(raw: unknown): ClarifyQuestion[] {
  const record = asRecord(raw);
  const questions = Array.isArray(record.questions) ? record.questions : [];
  return questions.slice(0, 3).flatMap((value): ClarifyQuestion[] => {
    const question = asRecord(value);
    const text = typeof question.question === "string" ? question.question.trim() : "";
    const options = Array.isArray(question.options)
      ? question.options.slice(0, 3).flatMap((option): ClarifyOption[] => {
          const optionRecord = asRecord(option);
          const label = typeof optionRecord.label === "string" ? optionRecord.label.trim() : "";
          if (!label) {
            return [];
          }
          return [{
            label,
            description: typeof optionRecord.description === "string" ? optionRecord.description.trim() : ""
          }];
        })
      : [];
    if (!text || options.length < 2) {
      return [];
    }
    return [{ question: text, options }];
  });
}

function initialClarifyAnswers(questions: ClarifyQuestion[]): ClarifyAnswerState[] {
  return questions.map(defaultClarifyAnswer);
}

function defaultClarifyAnswer(question: ClarifyQuestion): ClarifyAnswerState {
  return {
    kind: "option",
    value: question.options[0]?.label ?? "",
    custom: ""
  };
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

export function ComposerSubmitControls({
  context,
  controls,
  usage,
  model,
  variant,
  onModelChange,
  onVariantChange
}: {
  context: ContextReadResult | null;
  controls: SettingsReadResult["controls"];
  usage: SessionUsageSummaryView | null;
  model: string | null;
  variant: string;
  onModelChange(value: string | null): void;
  onVariantChange(value: string): void;
}) {
  const contextPercent = normalizedPercent(context?.percent);
  const [contextOpen, setContextOpen] = useState(false);
  const contextPopoverRef = useRef<HTMLDivElement | null>(null);

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
      <StatusSelect
        label="Model"
        value={model ?? ""}
        values={modelSelectValues(model, controls?.modelOptions ?? [])}
        optionLabels={{ "": emptyModelOptionLabel(controls) }}
        renderDisplayValue={(value) => modelDisplayValue(value, controls)}
        onChange={(value) => onModelChange(value || null)}
      />
      <StatusSelect
        label="Variant"
        optionLabels={{ none: "default" }}
        renderDisplayValue={(value) => value === "none" ? "default" : value || "variant"}
        value={variant}
        values={controls?.variantOptions ?? ["none"]}
        onChange={onVariantChange}
      />
      <div className="composerStatusContext" ref={contextPopoverRef}>
        <button
          aria-label="Context usage"
          aria-expanded={contextOpen}
          className="contextStatusButton"
          onClick={() => {
            setContextOpen((value) => !value);
          }}
          title={context?.label ?? "No active context"}
          type="button"
        >
          <span style={{ "--pevo-context-percent": `${contextPercent}%` } as CSSProperties} />
        </button>
        {contextOpen && (
          <div className="composerContextPopover" role="dialog" aria-label="Context usage">
            <div className="composerContextSummary">
              <span style={{ "--pevo-context-percent": `${contextPercent}%` } as CSSProperties}>
                {context?.available ? `${Math.round(contextPercent)}%` : "0%"}
              </span>
              <div>
                <strong>{context?.label ?? "No active context"}</strong>
                <small>{context?.status ?? "unavailable"}</small>
              </div>
            </div>
            {usage?.available && (
              <SessionUsageGrid compact usage={usage} />
            )}
            {!context?.available && (
              <p>No session context is active.</p>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

export function ComposerStatusLine({
  branch,
  controls,
  path,
  permissionMode,
  profile,
  onBranchClick,
  onPathClick,
  onPermissionModeChange
}: {
  branch: string | null;
  controls: SettingsReadResult["controls"];
  path: string;
  permissionMode: string;
  profile: InitializeResult["profile"] | null;
  onBranchClick(): void;
  onPathClick(): void;
  onPermissionModeChange(value: string): void;
}) {
  const profileLabel = profile && !profile.default ? profile.name : null;
  return (
    <div className="composerStatusLine" aria-label="Composer status">
      <StatusSelect label="Permission mode" value={permissionMode} values={controls?.permissionModeOptions ?? ["default"]} onChange={onPermissionModeChange} />
      {profileLabel ? (
        <span className="profileStatusPill" title={profile?.home ?? profileLabel}>
          <Pin size={12} />
          <span>{profileLabel}</span>
        </span>
      ) : null}
      <button className="pathStatusButton" onClick={onPathClick} title={path} type="button">{path || "workspace"}</button>
      {branch?.trim() ? (
        <button className="branchStatusButton" onClick={onBranchClick} type="button">
          <GitBranch size={13} />
          <span>{branch}</span>
        </button>
      ) : null}
    </div>
  );
}

export function StatusSelect({
  label,
  optionLabels,
  renderDisplayValue,
  value,
  values,
  onChange
}: {
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
      data-status={label.toLowerCase().replace(/\s+/g, "-")}
      style={valueWidth ? { "--pevo-status-select-value-width": valueWidth } as CSSProperties : undefined}
      title={label}
    >
      {displayValue && <span aria-hidden="true" className="statusSelectDisplay">{displayValue}</span>}
      <select aria-label={label} title={value || label} value={value} onChange={(event) => onChange(event.target.value)}>
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

function modelSelectValues(model: string | null, options: string[]): string[] {
  const values = ["", ...options];
  const selected = model?.trim();
  if (selected && !values.includes(selected)) {
    values.splice(1, 0, selected);
  }
  return values;
}

function modelDisplayValue(
  value: string,
  controls: SettingsReadResult["controls"]
): string {
  const trimmed = value.trim();
  if (trimmed) {
    return trimmed;
  }
  return emptyModelOptionLabel(controls);
}

function emptyModelOptionLabel(controls: SettingsReadResult["controls"]): string {
  if (controls?.modelStatus === "error") {
    return "Model unavailable";
  }
  return "Select model";
}
