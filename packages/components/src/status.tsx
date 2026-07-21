import { ChevronRight, CircleSlash, RefreshCw } from "lucide-react";
import type { CSSProperties } from "react";
import type {
  ContextReadResult,
  ContextUsageCategoryView,
  GatewayActivity,
  SessionUsageSummaryView,
  WorkspaceDiffFileView
} from "@psychevo/protocol";
import { IconButton } from "./primitives";

export interface StatusPanelProps {
  activity?: GatewayActivity | undefined;
  changedFiles?: WorkspaceDiffFileView[] | undefined;
  context?: ContextReadResult | undefined;
  sessionId?: string | null | undefined;
  status: string;
  usage?: SessionUsageSummaryView | undefined;
  onChangedFile?(path: string): void;
  onRefresh(): void;
}

export function StatusPanel(props: StatusPanelProps) {
  const changedFiles = Array.isArray(props.changedFiles) ? props.changedFiles : [];
  const rawContextPercent = props.context?.percent;
  const contextPercent = typeof rawContextPercent === "number"
    ? Math.max(0, Math.min(100, rawContextPercent))
    : 0;
  const contextPercentAvailable = typeof rawContextPercent === "number" && Number.isFinite(rawContextPercent);
  const categories = orderContextCategories(props.context?.categories ?? []);

  return (
    <section className="pevo-panel pevo-utility" aria-label="Status">
      <header className="pevo-panelHeader">
        <div className="pevo-statusTitleBlock">
          <div className="pevo-titleLine">
            <CircleSlash size={17} aria-hidden />
            <h2>Status</h2>
          </div>
          <p className="pevo-statusSessionId">{props.sessionId ?? "draft"}</p>
        </div>
        <IconButton icon={<RefreshCw size={17} />} label="Refresh" onClick={props.onRefresh} />
      </header>

      <div className="pevo-stack">
        <div className="pevo-contextSummary">
          <strong>{props.context?.label ?? "No active context"}</strong>
          <small>{props.context?.status ?? "unavailable"}</small>
        </div>
        {categories.length > 0 ? (
          <PromptTokenStack
            categories={categories}
            contextPercent={contextPercent}
            contextPercentAvailable={contextPercentAvailable}
          />
        ) : null}
        {props.usage?.available ? (
          <div className="pevo-sessionUsageGrid">
            <div>
              <span>Session tokens</span>
              <strong>{formatUsageTotal(props.usage.effectiveTotalTokens, props.usage.totalStatus)}</strong>
            </div>
            <div>
              <span>Cache read</span>
              <strong>{formatPercent(props.usage.cacheReadPercent)}</strong>
            </div>
            <div>
              <span>Cost</span>
              <strong>{formatNanodollars(props.usage.estimatedCostNanodollars)}</strong>
            </div>
            <div>
              <span>Reasoning</span>
              <strong>{formatCompactNumber(props.usage.reasoningTokens)}</strong>
            </div>
          </div>
        ) : null}
        {categories.length > 0 ? (
          <details className="pevo-promptTokensDisclosure">
            <summary>
              <span>Prompt tokens</span>
              <ChevronRight size={13} aria-hidden="true" />
            </summary>
            <div className="pevo-promptTokenCategoryList">
              {categories.map((category) => (
                <PromptTokenCategory category={category} key={category.id} />
              ))}
            </div>
          </details>
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

function PromptTokenStack({
  categories,
  contextPercent,
  contextPercentAvailable
}: {
  categories: ContextUsageCategoryView[];
  contextPercent: number;
  contextPercentAvailable: boolean;
}) {
  const totalTokens = categories.reduce((total, category) => total + Math.max(0, category.tokens), 0);
  const rawSegments = categories.map((category) => {
    const categoryPercent = typeof category.percent === "number" && Number.isFinite(category.percent)
      ? Math.max(0, category.percent)
      : totalTokens > 0
        ? (Math.max(0, category.tokens) / totalTokens) * (contextPercentAvailable ? contextPercent : 100)
        : 0;
    return { category, percent: categoryPercent };
  }).filter((segment) => segment.percent > 0 || segment.category.tokens > 0);
  const rawPercentTotal = rawSegments.reduce((total, segment) => total + segment.percent, 0);
  const scale = rawPercentTotal > 100 ? 100 / rawPercentTotal : 1;
  const displayPercentTotal = Math.min(100, rawPercentTotal * scale);
  const freePercent = contextPercentAvailable ? Math.max(0, 100 - displayPercentTotal) : 0;
  if (rawSegments.length === 0) {
    return <div className="pevo-promptTokenStack is-empty" aria-label="No prompt token categories" />;
  }
  return (
    <div
      className="pevo-promptTokenStack"
      aria-label={`Prompt token categories use ${formatPercentPrecise(displayPercentTotal)} of the context window`}
    >
      {rawSegments.map(({ category, percent }) => {
        const title = `${contextCategoryLabel(category)}: ${formatTokenEstimate(category.tokens, category.estimated)} (${formatPercentPrecise(category.percent ?? percent)})`;
        return (
          <span
            aria-label={title}
            className="pevo-promptTokenSegment"
            key={category.id}
            style={{
              "--pevo-prompt-token-color": promptTokenCategoryColor(category.id),
              "--pevo-prompt-token-width": `${Math.max(0, percent * scale)}%`
            } as CSSProperties}
            title={title}
          />
        );
      })}
      {freePercent > 0 ? (
        <span
          aria-hidden="true"
          className="pevo-promptTokenFreeSpace"
          style={{ "--pevo-prompt-token-width": `${freePercent}%` } as CSSProperties}
        />
      ) : null}
    </div>
  );
}

function PromptTokenCategory({ category }: { category: ContextUsageCategoryView }) {
  const rows = contextCategoryDetailRows(category);
  return (
    <div className="pevo-promptTokenCategory">
      <div className="pevo-promptTokenCategorySummary">
        <span>{contextCategoryLabel(category)}</span>
        <strong>{formatTokenEstimate(category.tokens, category.estimated)}</strong>
        <small>{formatPercentPrecise(category.percent)}</small>
      </div>
      {rows.length > 0 ? (
        <dl className="pevo-promptTokenCategoryDetails">
          {rows.map((row) => (
            <div key={`${row.label}:${row.value}`}>
              <dt>{row.label}</dt>
              <dd>{row.value}</dd>
            </div>
          ))}
        </dl>
      ) : null}
    </div>
  );
}

function orderContextCategories(categories: ContextUsageCategoryView[]): ContextUsageCategoryView[] {
  const order = new Map([
    "base_policy",
    "developer_prompt",
    "project_context",
    "history",
    "turn_context",
    "current_prompt",
    "system_tools"
  ].map((id, index) => [id, index]));
  return [...categories].sort((left, right) => {
    const leftOrder = order.get(left.id) ?? 99;
    const rightOrder = order.get(right.id) ?? 99;
    return leftOrder - rightOrder || contextCategoryLabel(left).localeCompare(contextCategoryLabel(right));
  });
}

function contextCategoryLabel(category: ContextUsageCategoryView): string {
  return category.label || category.id;
}

function promptTokenCategoryColor(categoryId: string): string {
  switch (categoryId) {
    case "base_policy":
      return "var(--pevo-context-base-policy, oklch(66% 0.08 252))";
    case "developer_prompt":
      return "var(--pevo-context-developer-prompt, oklch(68% 0.11 30))";
    case "project_context":
      return "var(--pevo-context-project-context, oklch(67% 0.1 150))";
    case "history":
      return "var(--pevo-context-history, oklch(72% 0.11 82))";
    case "turn_context":
      return "var(--pevo-context-turn-context, oklch(66% 0.09 305))";
    case "current_prompt":
      return "var(--pevo-context-current-prompt, oklch(70% 0.08 195))";
    case "system_tools":
      return "var(--pevo-context-system-tools, oklch(64% 0.08 16))";
    default:
      return "var(--pevo-accent)";
  }
}

function formatTokenEstimate(value: number, estimated = false): string {
  return `${estimated ? "~" : ""}${formatCompactNumber(value)}`;
}

function formatCompactNumber(value: number): string {
  if (!Number.isFinite(value)) {
    return "0";
  }
  if (value >= 1_000_000) {
    return `${(value / 1_000_000).toFixed(1)}M`;
  }
  if (value >= 1_000) {
    return `${(value / 1_000).toFixed(1)}k`;
  }
  return `${Math.max(0, Math.round(value))}`;
}

function formatUsageTotal(value: number | null, status: string): string {
  if (value === null) {
    return "Unavailable";
  }
  return `${status === "partial" ? "≥" : ""}${formatCompactNumber(value)}`;
}

function formatPercent(value: number | null | undefined): string {
  return typeof value === "number" && Number.isFinite(value) ? `${Math.round(value)}%` : "none";
}

function formatPercentPrecise(value: number | null | undefined): string {
  return typeof value === "number" && Number.isFinite(value) ? `${value.toFixed(1)}%` : "none";
}

function formatNanodollars(value: number): string {
  if (!Number.isFinite(value) || value <= 0) {
    return "$0.000000";
  }
  return `$${(value / 1_000_000_000).toFixed(6)}`;
}

function contextCategoryDetailRows(category: ContextUsageCategoryView): Array<{ label: string; value: string }> {
  const rows: Array<{ label: string; value: string }> = [];
  const details = asOptionalRecord(category.details);
  if (!details) {
    return rows;
  }
  if (category.id === "developer_prompt") {
    const entries = Array.isArray(details.skill_entries) ? details.skill_entries : [];
    const skillRows = entries
      .map(asOptionalRecord)
      .filter((entry): entry is Record<string, unknown> => Boolean(entry))
      .map((entry) => ({
        name: optionalStringField(entry.name) ?? "skill",
        tokens: numericDetail(entry.tokens)
      }))
      .filter((entry) => entry.tokens > 0)
      .sort((left, right) => right.tokens - left.tokens || left.name.localeCompare(right.name));
    for (const entry of skillRows) {
      rows.push({ label: entry.name, value: formatTokenEstimate(entry.tokens, true) });
    }
    const skillCount = numericDetail(details.skill_count);
    if (skillRows.length === 0 && skillCount > 0) {
      rows.push({ label: "Skills", value: `${skillCount}` });
    }
  }
  if (category.id === "history") {
    const roles = asOptionalRecord(details.roles);
    for (const [role, value] of Object.entries(roles ?? {}).sort(([left], [right]) => left.localeCompare(right))) {
      const record = asOptionalRecord(value);
      if (!record) {
        continue;
      }
      const count = numericDetail(record.count);
      const tokens = numericDetail(record.tokens);
      rows.push({
        label: role,
        value: `${count} ${count === 1 ? "msg" : "msgs"}, ${formatTokenEstimate(tokens, true)}`
      });
    }
  }
  if (category.id === "project_context") {
    const count = numericDetail(details.count);
    if (count > 0) {
      rows.push({ label: "project_context", value: `${count} ${count === 1 ? "msg" : "msgs"}` });
    }
  }
  if (category.id === "turn_context") {
    const count = numericDetail(details.selected_skill_context_count);
    const tokens = numericDetail(details.selected_skill_context_tokens);
    if (count > 0 || tokens > 0) {
      rows.push({
        label: "selected_skill_context",
        value: `${count} ${count === 1 ? "msg" : "msgs"}, ${formatTokenEstimate(tokens, true)}`
      });
    }
  }
  if (category.id === "system_tools") {
    const count = numericDetail(details.tool_count);
    if (count > 0) {
      rows.push({ label: "tools", value: `${count}` });
    }
  }
  return rows;
}

function asOptionalRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function optionalStringField(value: unknown): string | null {
  return typeof value === "string" && value.trim() !== "" ? value : null;
}

function numericDetail(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? Math.max(0, value) : 0;
}
