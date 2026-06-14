import { useEffect, useRef, useState, type CSSProperties } from "react";
import { GitBranch, Pin } from "lucide-react";
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
  onClarify(requestId: string, answer: string): void;
  onPermission(requestId: string, decision: PermissionDecision): void;
}) {
  if (permissions.length === 0 && clarifies.length === 0) {
    return null;
  }
  return (
    <div className="composerRequests" aria-label="Pending requests">
      {permissions.map((permission) => (
        <div className="composerRequest" key={permission.requestId}>
          <strong>{permission.toolName}</strong>
          <p>{permission.reason}</p>
          <div>
            <button onClick={() => onPermission(permission.requestId, "allowOnce")} type="button">Once</button>
            <button onClick={() => onPermission(permission.requestId, "allowSession")} type="button">Session</button>
            <button onClick={() => onPermission(permission.requestId, "deny")} type="button">Deny</button>
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
  onSubmit(requestId: string, answer: string): void;
}) {
  const [answer, setAnswer] = useState("");
  return (
    <form
      className="composerRequest"
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit(request.requestId, answer);
        setAnswer("");
      }}
    >
      <strong>Clarify</strong>
      <pre>{JSON.stringify(request.raw, null, 2)}</pre>
      <div>
        <input value={answer} onChange={(event) => setAnswer(event.target.value)} />
        <button type="submit">Submit</button>
      </div>
    </form>
  );
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
        values={["", ...(controls?.modelOptions ?? [])]}
        renderDisplayValue={compactModelLabel}
        onChange={(value) => onModelChange(value || null)}
      />
      <StatusSelect
        label="Variant"
        renderDisplayValue={(value) => value || "variant"}
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
      <button className="branchStatusButton" onClick={onBranchClick} type="button">
        <GitBranch size={13} />
        <span>{branch || "no-branch"}</span>
      </button>
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

function compactModelLabel(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) {
    return "model";
  }
  const slash = trimmed.lastIndexOf("/");
  const label = slash >= 0 ? trimmed.slice(slash + 1).trim() : trimmed;
  return label || trimmed;
}
