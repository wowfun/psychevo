import { type CSSProperties } from "react";
import { ChevronRight } from "lucide-react";
import type { ContextReadResult, SessionUsageSummaryView } from "@psychevo/protocol";
import { asOptionalRecord, optionalStringField } from "../data";
import type { ContextUsageCategory } from "../types";

const CONTEXT_CATEGORY_ORDER = [
  "base_policy",
  "developer_prompt",
  "project_context",
  "history",
  "turn_context",
  "current_prompt",
  "system_tools"
];

export function SessionObservability({
  context,
  hasActiveSession = true,
  usage,
  showCategories = false
}: {
  context: ContextReadResult | null;
  hasActiveSession?: boolean;
  usage: SessionUsageSummaryView | null;
  showCategories?: boolean;
}) {
  const contextPercent = normalizedPercent(context?.percent);
  const orderedCategories = orderedContextCategories(context?.categories ?? []);
  const rawContextPercent = context?.percent;
  const contextPercentAvailable = typeof rawContextPercent === "number" && Number.isFinite(rawContextPercent);
  const summaryLabel = hasActiveSession ? context?.label ?? "No active context" : "No active session";
  const summaryStatus = hasActiveSession ? context?.status ?? "unavailable" : "unbound";
  return (
    <section className="sessionObservability" aria-label="Session observability">
      <div className="sessionObservabilitySummary">
        <strong>{summaryLabel}</strong>
        <small>{summaryStatus}</small>
      </div>
      {showCategories && orderedCategories.length > 0 && (
        <PromptTokenStack
          categories={orderedCategories}
          contextPercent={contextPercent}
          contextPercentAvailable={contextPercentAvailable}
        />
      )}
      {usage?.available ? (
        <SessionUsageGrid usage={usage} />
      ) : (
        <p className="sessionObservabilityEmpty">No session usage yet.</p>
      )}
      {showCategories && orderedCategories.length > 0 && (
        <details className="promptTokensDisclosure">
          <summary>
            <span>Prompt tokens</span>
            <ChevronRight size={13} aria-hidden="true" />
          </summary>
          <div className="promptTokenCategoryList">
            {orderedCategories.map((category) => (
              <PromptTokenCategory category={category} key={category.id} />
            ))}
          </div>
        </details>
      )}
    </section>
  );
}

function PromptTokenStack({
  categories,
  contextPercent,
  contextPercentAvailable
}: {
  categories: ContextUsageCategory[];
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
    return <div className="promptTokenStack is-empty" aria-label="No prompt token categories" />;
  }
  return (
    <div
      className="promptTokenStack"
      aria-label={`Prompt token categories use ${formatPercentPrecise(displayPercentTotal)} of the context window`}
    >
      {rawSegments.map(({ category, percent }) => {
        const title = `${contextCategoryLabel(category)}: ${formatTokenEstimate(category.tokens, category.estimated)} (${formatPercentPrecise(category.percent ?? percent)})`;
        return (
          <span
            aria-label={title}
            className="promptTokenSegment"
            key={category.id}
            style={{
              "--prompt-token-color": promptTokenCategoryColor(category.id),
              "--prompt-token-width": `${Math.max(0, percent * scale)}%`
            } as CSSProperties}
            title={title}
          />
        );
      })}
      {freePercent > 0 && (
        <span
          aria-hidden="true"
          className="promptTokenFreeSpace"
          style={{ "--prompt-token-width": `${freePercent}%` } as CSSProperties}
        />
      )}
    </div>
  );
}

export function SessionUsageGrid({
  compact = false,
  usage
}: {
  compact?: boolean;
  usage: SessionUsageSummaryView;
}) {
  const metrics = [
    { label: "Session tokens", value: formatCompactNumber(usage.reportedTotalTokens) },
    { label: "Cache read", value: formatPercent(usage.cacheReadPercent) },
    { label: "Cost", value: formatNanodollars(usage.estimatedCostNanodollars) },
    { label: "Reasoning", value: formatCompactNumber(usage.reasoningTokens) },
    { label: "Input", value: formatCompactNumber(usage.billableInputTokens) },
    { label: "Output", value: formatCompactNumber(usage.billableOutputTokens) },
    { label: "Cache write", value: formatCompactNumber(usage.cacheWriteTokens) }
  ];
  const visibleMetrics = compact ? metrics.slice(0, 4) : metrics;
  return (
    <div className={compact ? "composerContextUsageGrid" : "sessionUsageGrid"}>
      {visibleMetrics.map((metric) => (
        <div key={metric.label} title={metric.value}>
          <span>{metric.label}</span>
          <strong>{metric.value}</strong>
        </div>
      ))}
    </div>
  );
}

function PromptTokenCategory({ category }: { category: ContextUsageCategory }) {
  const rows = contextCategoryDetailRows(category);
  return (
    <div className="promptTokenCategory">
      <div className="promptTokenCategorySummary">
        <span>{contextCategoryLabel(category)}</span>
        <strong>{formatTokenEstimate(category.tokens, category.estimated)}</strong>
        <small>{formatPercentPrecise(category.percent)}</small>
      </div>
      {rows.length > 0 && (
        <dl className="promptTokenCategoryDetails">
          {rows.map((row) => (
            <div key={`${row.label}:${row.value}`}>
              <dt>{row.label}</dt>
              <dd>{row.value}</dd>
            </div>
          ))}
        </dl>
      )}
    </div>
  );
}

export function normalizedPercent(value: number | null | undefined): number {
  return typeof value === "number" && Number.isFinite(value)
    ? Math.max(0, Math.min(100, value))
    : 0;
}

function orderedContextCategories(categories: ContextUsageCategory[]): ContextUsageCategory[] {
  const order = new Map(CONTEXT_CATEGORY_ORDER.map((id, index) => [id, index]));
  return [...categories].sort((left, right) => {
    const leftOrder = order.get(left.id) ?? 99;
    const rightOrder = order.get(right.id) ?? 99;
    return leftOrder - rightOrder || left.label.localeCompare(right.label);
  });
}

function contextCategoryLabel(category: ContextUsageCategory): string {
  return category.label || category.id;
}

function promptTokenCategoryColor(categoryId: string): string {
  switch (categoryId) {
    case "base_policy":
      return "var(--pevo-context-base-policy)";
    case "developer_prompt":
      return "var(--pevo-context-developer-prompt)";
    case "project_context":
      return "var(--pevo-context-project-context)";
    case "history":
      return "var(--pevo-context-history)";
    case "turn_context":
      return "var(--pevo-context-turn-context)";
    case "current_prompt":
      return "var(--pevo-context-current-prompt)";
    case "system_tools":
      return "var(--pevo-context-system-tools)";
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

function contextCategoryDetailRows(category: ContextUsageCategory): Array<{ label: string; value: string }> {
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

function numericDetail(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? Math.max(0, value) : 0;
}
