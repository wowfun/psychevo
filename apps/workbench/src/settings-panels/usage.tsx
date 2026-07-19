import { BarChart3, RotateCcw } from "lucide-react";
import type { WorkbenchUsageStats } from "../types";

export function UsageSettingsPanel({
  error,
  loading,
  stats,
  onRefresh
}: {
  error: string | null;
  loading: boolean;
  stats: WorkbenchUsageStats | null;
  onRefresh(): void;
}) {
  const windows = stats?.windows ?? [];
  const primaryWindows = ["all", "30d", "7d"]
    .map((id) => windows.find((window) => window.id === id))
    .filter((window): window is WorkbenchUsageStats["windows"][number] => Boolean(window));
  return (
    <section className="usageSettingsPanel" aria-label="Usage">
      <div className="usageSettingsToolbar">
        <span>{stats ? `Updated ${formatShortDateTime(stats.generatedAtMs)}` : loading ? "Loading" : "No data"}</span>
        <button aria-label="Refresh usage" disabled={loading} onClick={onRefresh} title="Refresh usage" type="button">
          <RotateCcw size={13} />
          <span>Refresh</span>
        </button>
      </div>
      {error && <div className="usageSettingsError" role="alert">{error}</div>}
      {primaryWindows.length > 0 ? (
        <div className="usageWindowGrid">
          {primaryWindows.map((window) => <UsageWindowCard key={window.id} window={window} />)}
        </div>
      ) : (
        <div className="usageSettingsEmpty">{loading ? "Loading usage" : "No usage recorded"}</div>
      )}
      {stats && <UsageActivityHeatmap activity={stats.activity} />}
    </section>
  );
}

function UsageWindowCard({ window }: { window: WorkbenchUsageStats["windows"][number] }) {
  const inputTokens = window.billableInputTokens + window.cacheReadTokens + window.cacheWriteTokens;
  return (
    <section className="usageWindowCard" aria-label={window.label}>
      <header>
        <span>{window.label}</span>
        <strong>{formatUsageTotal(window.effectiveTotalTokens, window.totalStatus)}</strong>
      </header>
      <div className="usageWindowMetrics">
        <div>
          <span>Cost</span>
          <strong>{formatUsageCost(window)}</strong>
        </div>
        <div>
          <span>Cache read</span>
          <strong>{formatPercent(window.cacheReadPercent)}</strong>
        </div>
        <div>
          <span>Sessions</span>
          <strong>{formatCompactNumber(window.sessionCount)}</strong>
        </div>
      </div>
      <dl className="usageBreakdown">
        <div><dt>Input</dt><dd>{formatCompactNumber(inputTokens)}</dd></div>
        <div><dt>Output</dt><dd>{formatCompactNumber(window.billableOutputTokens)}</dd></div>
        <div><dt>Reasoning</dt><dd>{formatCompactNumber(window.reasoningTokens)}</dd></div>
        <div><dt>Cache write</dt><dd>{formatCompactNumber(window.cacheWriteTokens)}</dd></div>
        {window.unknownPricingCount > 0 && (
          <div><dt>Unknown pricing</dt><dd>{formatCompactNumber(window.unknownPricingCount)}</dd></div>
        )}
      </dl>
    </section>
  );
}

function UsageActivityHeatmap({ activity }: { activity: WorkbenchUsageStats["activity"] }) {
  const days = activity.days;
  const positiveTokenScale = heatmapPositiveTokenScale(days);
  const startPadding = days[0] ? new Date(`${days[0].date}T00:00:00`).getDay() : 0;
  const cells: Array<null | WorkbenchUsageStats["activity"]["days"][number]> = [
    ...Array.from({ length: startPadding }, () => null),
    ...days
  ];
  const weekCount = Math.max(1, Math.ceil(cells.length / 7));
  const monthLabels = heatmapMonthLabels(cells, weekCount);
  return (
    <section className="usageHeatmapPanel" aria-label="Token activity">
      <header>
        <span><BarChart3 size={14} /> Token activity</span>
        <small>{activity.startDate} to {activity.endDate}</small>
      </header>
      <div className="usageHeatmapScroller">
        <div
          className="usageHeatmapMonths"
          style={{ gridTemplateColumns: `repeat(${weekCount}, 11px)` }}
        >
          {monthLabels.map((label) => (
            <span key={`${label.month}-${label.week}`} style={{ gridColumn: `${label.week + 1} / span ${label.span}` }}>
              {label.month}
            </span>
          ))}
        </div>
        <div className="usageHeatmapBody">
          <div className="usageHeatmapWeekdays" aria-hidden>
            <span>Sun</span>
            <span>Mon</span>
            <span>Tue</span>
            <span>Wed</span>
            <span>Thu</span>
            <span>Fri</span>
            <span>Sat</span>
          </div>
          <div
            className="usageHeatmapGrid"
            style={{ gridTemplateColumns: `repeat(${weekCount}, 11px)` }}
          >
            {cells.map((day, index) => {
              const level = day ? heatmapLevel(day.effectiveTotalTokens, positiveTokenScale) : 0;
              return (
                <span
                  aria-label={day ? `${day.date}: ${formatUsageTotalWithUnit(day.effectiveTotalTokens, day.totalStatus)}` : undefined}
                  className={day ? "usageHeatmapCell" : "usageHeatmapCell is-empty"}
                  data-level={level}
                  key={day?.date ?? `pad-${index}`}
                  title={day ? `${day.date}: ${formatUsageTotalWithUnit(day.effectiveTotalTokens, day.totalStatus)}` : undefined}
                />
              );
            })}
          </div>
        </div>
      </div>
    </section>
  );
}

function heatmapPositiveTokenScale(days: WorkbenchUsageStats["activity"]["days"]): number[] {
  return [...new Set(days
    .map((day) => day.effectiveTotalTokens)
    .filter((tokens) => tokens > 0))]
    .sort((left, right) => left - right);
}

function heatmapLevel(tokens: number, positiveTokenScale: number[]): number {
  if (tokens <= 0) {
    return 0;
  }
  if (positiveTokenScale.length <= 1) {
    return 4;
  }
  const index = positiveTokenScale.findIndex((value) => tokens <= value);
  const boundedIndex = index >= 0 ? index : positiveTokenScale.length - 1;
  const ratio = boundedIndex / (positiveTokenScale.length - 1);
  return Math.max(1, Math.min(4, Math.round(ratio * 3) + 1));
}

function heatmapMonthLabels(
  cells: Array<null | WorkbenchUsageStats["activity"]["days"][number]>,
  weekCount: number
): Array<{ month: string; span: number; week: number }> {
  const labels: Array<{ month: string; span: number; week: number }> = [];
  let lastMonth = "";
  for (let index = 0; index < cells.length; index += 1) {
    const day = cells[index];
    if (!day) {
      continue;
    }
    const date = new Date(`${day.date}T00:00:00`);
    const month = date.toLocaleString(undefined, { month: "short" });
    const week = Math.floor(index / 7);
    if (month !== lastMonth) {
      labels.push({ month, span: 1, week });
      lastMonth = month;
    }
  }
  return labels;
}

function formatUsageCost(window: WorkbenchUsageStats["windows"][number]): string {
  if (window.costStatus === "unknown" && window.estimatedCostNanodollars === 0) {
    return "Unknown";
  }
  const value = formatNanodollars(window.estimatedCostNanodollars);
  return window.unknownPricingCount > 0 ? `${value} + unknown` : value;
}

function formatCompactNumber(value: number): string {
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 1, notation: "compact" }).format(value);
}

function formatUsageTotal(value: number, status: string): string {
  if (status === "unavailable") {
    return "Unavailable";
  }
  return `${status === "partial" ? "≥" : ""}${formatCompactNumber(value)}`;
}

function formatUsageTotalWithUnit(value: number, status: string): string {
  const total = formatUsageTotal(value, status);
  return status === "unavailable" ? total : `${total} tokens`;
}

function formatPercent(value: number | null): string {
  return value === null ? "-" : `${Math.round(value)}%`;
}

function formatNanodollars(value: number): string {
  return `$${(value / 1_000_000_000).toFixed(6)}`;
}

function formatShortDateTime(value: number): string {
  return new Date(value).toLocaleString(undefined, {
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    month: "short"
  });
}
