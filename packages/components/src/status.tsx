import { CircleSlash, RefreshCw } from "lucide-react";
import type { CSSProperties } from "react";
import type { ContextReadResult, GatewayActivity, WorkspaceDiffFileView } from "@psychevo/protocol";
import { IconButton } from "./primitives";
import { normalizeActivity } from "./shared";

export interface StatusPanelProps {
  activity?: GatewayActivity | undefined;
  changedFiles?: WorkspaceDiffFileView[] | undefined;
  context?: ContextReadResult | undefined;
  sessionId?: string | null | undefined;
  status: string;
  onChangedFile?(path: string): void;
  onRefresh(): void;
}

export function StatusPanel(props: StatusPanelProps) {
  const activity = normalizeActivity(props.activity);
  const changedFiles = Array.isArray(props.changedFiles) ? props.changedFiles : [];
  const contextPercent = typeof props.context?.percent === "number"
    ? Math.max(0, Math.min(100, props.context.percent))
    : 0;

  return (
    <section className="pevo-panel pevo-utility" aria-label="Status">
      <header className="pevo-panelHeader">
        <div className="pevo-titleLine">
          <CircleSlash size={17} aria-hidden />
          <h2>Status</h2>
        </div>
        <IconButton title="Refresh" onClick={props.onRefresh}>
          <RefreshCw size={17} />
        </IconButton>
      </header>

      <dl className="pevo-statusGrid">
        {props.sessionId && (
          <div className="pevo-statusMetric is-session">
            <dt>Session</dt>
            <dd>{props.sessionId}</dd>
          </div>
        )}
        <div className="pevo-statusMetric">
          <dt>Connection</dt>
          <dd>{props.status}</dd>
        </div>
        <div className="pevo-statusMetric">
          <dt>Turn</dt>
          <dd>{activity.running ? "running" : "idle"}</dd>
        </div>
        <div className="pevo-statusMetric">
          <dt>Queued</dt>
          <dd>{activity.queuedTurns}</dd>
        </div>
      </dl>

      <div className="pevo-stack">
        <h3>Context</h3>
        <div className="pevo-contextLedger">
          <span className="pevo-contextRing" style={{ "--pevo-context-percent": `${contextPercent}%` } as CSSProperties}>
            <span>{props.context?.available ? `${Math.round(contextPercent)}%` : "0%"}</span>
          </span>
          <div>
            <strong>{props.context?.label ?? "No active context"}</strong>
            <small>{props.context?.status ?? "unavailable"}</small>
          </div>
        </div>
        {props.context?.categories?.length ? (
          <div className="pevo-contextBars">
            {props.context.categories.slice(0, 6).map((category) => (
              <div className="pevo-contextBar" key={category.id}>
                <span>{category.label}</span>
                <meter max={100} min={0} value={category.percent ?? 0} />
              </div>
            ))}
          </div>
        ) : null}
      </div>

      <div className="pevo-stack">
        <h3>Changed files</h3>
        {changedFiles.length === 0 ? (
          <p className="pevo-muted">No changes</p>
        ) : (
          <div className="pevo-changedFiles">
            {changedFiles.map((file) => (
              <button key={file.path} onClick={() => props.onChangedFile?.(file.path)} type="button">
                <code>{file.path}</code>
                <span>{file.status}</span>
              </button>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}
