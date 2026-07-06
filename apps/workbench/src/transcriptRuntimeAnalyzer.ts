export type TranscriptRuntimeRowSample = {
  blockId: string | null;
  entryId: string | null;
  header: string;
  kind: string;
  running: boolean;
  source: string | null;
  status: string | null;
  text: string;
  turnId: string | null;
};

export type TranscriptRuntimeAnalysis = {
  errors: string[];
  warnings: string[];
};

export type TranscriptRuntimeAnalysisOptions = {
  activeTurnRunning?: boolean;
};

export function analyzeTranscriptRuntimeRows(
  rows: TranscriptRuntimeRowSample[],
  options: TranscriptRuntimeAnalysisOptions = {}
): TranscriptRuntimeAnalysis {
  const errors: string[] = [];
  const warnings: string[] = [];
  const blockIds = new Set<string>();
  const runningToolBlocks = new Set<string>();
  for (const row of rows) {
    if (row.blockId) {
      const key = `${row.turnId ?? ""}:${row.blockId}`;
      if (blockIds.has(key)) {
        errors.push(`duplicateLiveToolIdentity: duplicate block identity ${key}`);
      }
      blockIds.add(key);
    }
    if (row.kind === "tool" && row.running) {
      const identity = `${row.turnId ?? ""}:${row.blockId ?? row.header}`;
      if (runningToolBlocks.has(identity)) {
        errors.push(`duplicateLiveToolIdentity: duplicate running tool ${identity}`);
      }
      runningToolBlocks.add(identity);
    }
    if (row.kind === "tool" && row.running && row.source === "runtime.message" && !options.activeTurnRunning) {
      errors.push(`activeRowAfterTerminal: committed tool row is still active ${row.blockId ?? row.header}`);
    }
    if (row.kind === "tool" && row.status === "pending" && row.source === "runtime.message") {
      warnings.push(`staleOverlayDropped: committed snapshot exposed pending tool ${row.blockId ?? row.header}`);
    }
    if (barePendingExecCommandRow(row)) {
      errors.push(`barePendingToolInvocation: pending exec_command row lacks invocation identity ${row.blockId ?? row.header}`);
    }
  }
  return { errors, warnings };
}

function barePendingExecCommandRow(row: TranscriptRuntimeRowSample): boolean {
  if (row.kind !== "tool" || normalizeSampleText(row.status) !== "pending") {
    return false;
  }
  const header = normalizeSampleText(row.header);
  const text = normalizeSampleText(row.text);
  return header === "exec_command" ||
    header === "exec_command pending" ||
    text === "exec_command" ||
    text === "exec_command pending";
}

function normalizeSampleText(value: string | null): string {
  return (value ?? "").replace(/\s+/g, " ").trim().toLowerCase();
}
