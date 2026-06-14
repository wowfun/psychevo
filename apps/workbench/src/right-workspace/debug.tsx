import { Bug, RefreshCw } from "lucide-react";
import { prettyJson, traceEventLabel, traceEventSeq, traceEventTime } from "../data";
import type { DebugEvent, TraceState } from "../types";

export function DebugPanel({
  events,
  onRefreshTrace,
  trace
}: {
  events: DebugEvent[];
  onRefreshTrace(): void;
  trace: TraceState;
}) {
  const traceEvents = trace.result?.events ?? [];
  const traceWarnings = trace.result?.warnings ?? [];
  return (
    <section className="debugPanel" aria-label="Debug event stream">
      <header>
        <Bug size={17} />
        <div>
          <h2>Debug</h2>
          <p>{traceEvents.length} trace events · {events.length} recent notifications</p>
        </div>
        <button aria-label="Refresh Trace" onClick={onRefreshTrace} type="button">
          <RefreshCw size={15} />
        </button>
      </header>
      <div className="debugSection">
        <div className="debugSectionHeader">
          <strong>Trace</strong>
          <span>{trace.loading ? "loading" : trace.result?.available ? "persisted" : "unavailable"}</span>
        </div>
        {trace.error && <p className="debugNotice">{trace.error}</p>}
        {traceWarnings.map((warning) => (
          <p className="debugNotice" key={warning}>{warning}</p>
        ))}
        <div className="debugList">
          {traceEvents.map((event, index) => (
            <details key={`${trace.threadId ?? "trace"}:${traceEventSeq(event) ?? index}`}>
              <summary>
                <code>{traceEventLabel(event)}</code>
                <span>{traceEventTime(event)}</span>
              </summary>
              <pre>{prettyJson(event)}</pre>
            </details>
          ))}
          {traceEvents.length === 0 && <p>No persisted trace events.</p>}
        </div>
      </div>
      <div className="debugSection">
        <div className="debugSectionHeader">
          <strong>Notifications</strong>
          <span>{events.length} recent</span>
        </div>
      <div className="debugList">
        {events.map((event) => (
          <details key={event.id}>
            <summary>
              <code>{event.method}</code>
              <span>{new Date(event.at).toLocaleTimeString()}</span>
            </summary>
            <pre>{prettyJson(event.payload)}</pre>
          </details>
        ))}
        {events.length === 0 && <p>No events yet.</p>}
      </div>
      </div>
    </section>
  );
}
