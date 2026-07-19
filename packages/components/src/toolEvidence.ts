import type { TranscriptBlock } from "@psychevo/protocol";
import { diffDisplayPath, diffFilesStats, parseStrictGitPatchDiff, type ParsedDiffFile } from "./diff";
import { asRecord, compactText, stringValue } from "./shared";

type ToolDisplayCategory = "explore" | "run" | "update" | "status";
type ToolDisplayBodyPolicy = "summary" | "body";

type ToolDisplaySpec = {
  bodyKeys: string[];
  bodyPolicy: ToolDisplayBodyPolicy;
  category: ToolDisplayCategory;
  summaryKeys: string[];
  titleArgKeys: string[];
  titleResultKeys: string[];
};

export type ToolDetailSection =
  | {
      kind: "kv";
      rows: ToolDetailRow[];
      title: string;
      tone?: "default" | "error" | "muted";
    }
  | {
      files: ParsedDiffFile[];
      kind: "diff";
      title: string;
      tone?: "default" | "error" | "muted";
    }
  | {
      code?: boolean;
      kind: "text";
      text: string;
      title: string;
      tone?: "default" | "error" | "muted";
    };

export type ToolDetailRow = {
  label: string;
  value: string;
};

export type EvidenceDisplay = {
  category: ToolDisplayCategory;
  defaultOpen: boolean;
  sections: ToolDetailSection[];
  singleTitle: boolean;
  summary: string | null;
  title: string;
};

const BODY_KEYS = new Set(["body", "chars", "content", "diff", "input", "metadata", "output", "result"]);
const INTERNAL_KEYS = new Set([
  "arguments",
  "arguments_json",
  "call_index",
  "content_index",
  "display",
  "hidden",
  "metadata",
  "projection",
  "result",
  "source",
  "tool_call_id",
  "tool_name",
  "type"
]);

export function evidenceDisplay(block: TranscriptBlock, fallbackText: string): EvidenceDisplay {
  const metadata = asRecord(block.metadata);
  const title = block.title ?? block.kind;
  if (metadata.projection !== "tool") {
    const summary = block.preview ?? compactText(fallbackText, 180);
    const detail = [block.detail, block.body, block.result?.content, block.preview]
      .filter((value): value is string => Boolean(value?.trim()))
      .filter((value) => value.trim() !== summary.trim())[0] ?? null;
    const invocation = block.kind === "shell" ? execCommandInvocation(title, title, undefined, block.preview ?? "") : null;
    return {
      category: "run",
      defaultOpen: false,
      sections: detail ? [{ code: block.kind === "shell", kind: "text", text: detail, title: "Detail" }] : [],
      singleTitle: Boolean(invocation || !summary),
      summary: invocation ? null : summary,
      title: invocation ?? title
    };
  }

  const toolName = stringValue(metadata.tool_name) ?? title;
  const args = parseJsonLike(metadata.args ?? metadata.arguments);
  const result = toolResultValue(block, metadata);
  const spec = toolDisplaySpec(toolName, metadata);
  const explicitTitle = explicitToolTitle(toolName, title, metadata);
  const invocation = explicitTitle ? null : execCommandInvocation(toolName, title, args, block.preview ?? "");
  const inlineDiff = inlineDiffDisplay(spec, result, block);
  const displayTitle = inlineDiff?.title ?? explicitTitle ?? invocation ?? toolTitle(toolName, title, spec, args, result);
  const summary = inlineDiff || invocation || explicitTitle ? null : toolSummary(spec, result, args);
  const sections = toolSections(toolName, spec, args, result, metadata, block, inlineDiff);
  const singleTitle = !summary;

  return {
    category: spec.category,
    defaultOpen: Boolean(inlineDiff),
    sections,
    singleTitle,
    summary,
    title: displayTitle
  };
}

function explicitToolTitle(toolName: string, title: string, metadata: Record<string, unknown>): string | null {
  const display = stringValue(metadata.display)?.trim();
  if (display) {
    return display;
  }
  const trimmedTitle = title.trim();
  if (!trimmedTitle) {
    return null;
  }
  const trimmedTool = toolName.trim();
  if (
    trimmedTitle === trimmedTool ||
    trimmedTitle === "exec_command" ||
    trimmedTitle.startsWith("exec_command ")
  ) {
    return null;
  }
  return trimmedTitle;
}

function toolDisplaySpec(toolName: string, metadata: Record<string, unknown>): ToolDisplaySpec {
  if (toolName === "read") {
    return {
      ...genericDisplaySpec(),
      bodyKeys: ["content"],
      bodyPolicy: "body",
      category: "explore",
      summaryKeys: [],
      titleArgKeys: ["path"],
      titleResultKeys: ["path"]
    };
  }
  const fromMetadata = displaySpecFromValue(metadata.display);
  if (fromMetadata) {
    return fromMetadata;
  }
  if (toolName === "exec_command" || toolName === "write_stdin") {
    return {
      ...genericDisplaySpec(),
      bodyKeys: ["output", "content", "error"],
      bodyPolicy: "body",
      category: "run",
      titleArgKeys: ["cmd", "command", "session_id", "path", "url", "query", "name"]
    };
  }
  if (toolName === "write" || toolName === "edit" || toolName === "apply_patch") {
    return {
      ...genericDisplaySpec(),
      bodyKeys: ["diff", "output", "error"],
      bodyPolicy: "body",
      category: "update"
    };
  }
  if (toolName === "clarify") {
    return {
      ...genericDisplaySpec(),
      category: "status"
    };
  }
  if (toolName === "spawn_agent") {
    return {
      ...genericDisplaySpec(),
      category: "status",
      summaryKeys: ["task_name", "taskName", "message", "task", "summary", "status"],
      titleArgKeys: ["agent_type", "agentType"],
      titleResultKeys: ["agent_name", "agentName", "agent_type", "agentType"]
    };
  }
  if (toolName === "web_fetch") {
    return {
      ...genericDisplaySpec(),
      bodyKeys: ["content"],
      bodyPolicy: "summary",
      category: "explore",
      summaryKeys: ["error", "status", "final_url", "content_type", "output_bytes", "original_bytes", "truncated"],
      titleArgKeys: ["url"],
      titleResultKeys: ["final_url", "url"]
    };
  }
  if (toolName === "web_search") {
    return {
      ...genericDisplaySpec(),
      bodyKeys: ["payload", "error"],
      bodyPolicy: "body",
      category: "explore",
      summaryKeys: ["provider", "truncated", "error"],
      titleArgKeys: ["query"],
      titleResultKeys: ["query", "provider"]
    };
  }
  if (toolName === "mcp" || toolName === "mcp_call" || toolName.startsWith("mcp__")) {
    return {
      ...genericDisplaySpec(),
      bodyKeys: ["content", "output", "error"],
      category: "run"
    };
  }
  return genericDisplaySpec();
}

function genericDisplaySpec(): ToolDisplaySpec {
  return {
    bodyKeys: ["content", "output", "diff"],
    bodyPolicy: "summary",
    category: "run",
    summaryKeys: [
      "error",
      "summary",
      "status",
      "path",
      "files_modified",
      "bytes_written",
      "exit_code",
      "truncated",
      "url",
      "final_url",
      "content_type",
      "output_bytes",
      "original_bytes"
    ],
    titleArgKeys: ["path", "file", "file_path", "cmd", "command", "url", "query", "pattern", "name", "session_id"],
    titleResultKeys: ["path", "url", "final_url", "name", "session_id"]
  };
}

function displaySpecFromValue(value: unknown): ToolDisplaySpec | null {
  const record = asRecord(value);
  const category = displayCategory(record.category);
  if (!category) {
    return null;
  }
  return {
    bodyKeys: stringArray(record.body_keys ?? record.bodyKeys),
    bodyPolicy: displayBodyPolicy(record.body_policy ?? record.bodyPolicy) ?? "summary",
    category,
    summaryKeys: stringArray(record.summary_keys ?? record.summaryKeys),
    titleArgKeys: stringArray(record.title_arg_keys ?? record.titleArgKeys),
    titleResultKeys: stringArray(record.title_result_keys ?? record.titleResultKeys)
  };
}

function displayCategory(value: unknown): ToolDisplayCategory | null {
  return value === "explore" || value === "run" || value === "update" || value === "status" ? value : null;
}

function displayBodyPolicy(value: unknown): ToolDisplayBodyPolicy | null {
  return value === "summary" || value === "body" ? value : null;
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string" && item.trim().length > 0) : [];
}

function toolTitle(toolName: string, blockTitle: string, spec: ToolDisplaySpec, args: unknown, result: unknown): string {
  const detail = titleDetailFromKeys(spec.titleArgKeys, args) ?? titleDetailFromKeys(spec.titleResultKeys, result);
  const name = displayToolName(toolName);
  if (detail) {
    return compactText(`${name} ${detail}`, 180);
  }
  const cleanBlockTitle = blockTitle.trim();
  if (cleanBlockTitle && cleanBlockTitle !== toolName && !cleanBlockTitle.startsWith("Tool:")) {
    return compactText(cleanBlockTitle, 180);
  }
  return name;
}

function displayToolName(toolName: string): string {
  const mcp = mcpNameParts(toolName);
  return mcp ? `${mcp.server}/${mcp.tool}` : toolName;
}

function mcpNameParts(toolName: string): { server: string; tool: string } | null {
  const rest = toolName.startsWith("mcp__") ? toolName.slice("mcp__".length) : "";
  const [server, tool] = rest.split("__");
  return server && tool ? { server, tool } : null;
}

function titleDetailFromKeys(keys: string[], source: unknown): string | null {
  const record = asRecord(source);
  for (const key of keys) {
    const value = record[key];
    if (value === null || value === undefined) {
      continue;
    }
    if (key === "cmd" || key === "command") {
      const command = stringValue(value);
      if (command) {
        return firstEffectiveCommand(command);
      }
      continue;
    }
    const detail = displayValueInline(value);
    if (detail) {
      return compactText(detail, 180);
    }
  }
  return null;
}

function toolSummary(spec: ToolDisplaySpec, result: unknown, args: unknown): string | null {
  const resultRecord = asRecord(result);
  const argRecord = asRecord(args);
  const parts: string[] = [];
  for (const key of spec.summaryKeys) {
    const value = resultRecord[key] ?? argRecord[key];
    const display = summaryValue(key, value);
    if (!display) {
      continue;
    }
    parts.push(display);
    if (parts.length >= 3) {
      break;
    }
  }
  return parts.length > 0 ? compactText(parts.join(" / "), 180) : null;
}

function summaryValue(key: string, value: unknown): string | null {
  if (value === null || value === undefined || value === false) {
    return null;
  }
  if (key === "error") {
    return compactText(String(value), 120);
  }
  if (key === "exit_code") {
    return `exit ${String(value)}`;
  }
  if (key === "bytes_written") {
    return `${formatCount(value)} bytes`;
  }
  if (key === "output_bytes") {
    return `${formatCount(value)} output bytes`;
  }
  if (key === "original_bytes") {
    return `${formatCount(value)} original bytes`;
  }
  if (key === "truncated") {
    return value === true ? "truncated" : null;
  }
  if (key === "files_modified") {
    return `${formatCount(value)} files`;
  }
  const text = displayValueInline(value);
  return text ? compactText(text, 90) : null;
}

function toolSections(
  toolName: string,
  spec: ToolDisplaySpec,
  args: unknown,
  result: unknown,
  metadata: Record<string, unknown>,
  block: TranscriptBlock,
  inlineDiff: InlineDiffDisplay | null
): ToolDetailSection[] {
  if (toolName === "exec_command") {
    return execCommandSections(args, result, metadata, block);
  }
  if (toolName === "write_stdin") {
    return writeStdinSections(args, result, metadata);
  }
  if (inlineDiff) {
    return [{ files: inlineDiff.files, kind: "diff", title: "Diff" }];
  }
  if (toolName === "read") {
    return readSections(result, block);
  }
  const sections: ToolDetailSection[] = [];
  const inputs = visibleRows(args, "input", EMPTY_KEYS);
  if (inputs.length > 0) {
    sections.push({ kind: "kv", rows: inputs, title: "Input" });
  }
  const resultRows = visibleRows(result, "result", EMPTY_KEYS);
  if (resultRows.length > 0) {
    sections.push({ kind: "kv", rows: resultRows, title: resultTitle(toolName) });
  }
  const bodySections = bodyTextSections(spec, result, toolName);
  sections.push(...bodySections);
  const outcome = stringValue(metadata.outcome);
  if (outcome && outcome !== "normal") {
    sections.push({ kind: "kv", rows: [{ label: "outcome", value: outcome }], title: "Status", tone: "error" });
  }
  if (block.result?.isError && !sections.some((section) => section.tone === "error")) {
    sections.push({ kind: "kv", rows: [{ label: "status", value: "error" }], title: "Status", tone: "error" });
  }
  return sections;
}

type InlineDiffDisplay = {
  files: ParsedDiffFile[];
  title: string;
};

const EMPTY_KEYS = new Set<string>();

function inlineDiffDisplay(spec: ToolDisplaySpec, result: unknown, block: TranscriptBlock): InlineDiffDisplay | null {
  if (spec.category !== "update" || block.status !== "completed" || block.result?.isError) {
    return null;
  }
  const resultRecord = asRecord(result);
  const diffText = stringValue(resultRecord.diff);
  if (!diffText) {
    return null;
  }
  const files = parseStrictGitPatchDiff(diffText);
  if (files.length === 0) {
    return null;
  }
  return {
    files,
    title: editedDiffTitle(files)
  };
}

function editedDiffTitle(files: ParsedDiffFile[]): string {
  const stats = diffFilesStats(files);
  const suffix = `(+${stats.additions} -${stats.deletions})`;
  const onlyFile = files[0] ?? null;
  if (files.length === 1 && onlyFile) {
    const path = diffDisplayPath(onlyFile);
    return compactText(`Edited ${path} ${suffix}`, 180);
  }
  return compactText(`Edited ${files.length} files ${suffix}`, 180);
}

function execCommandSections(
  args: unknown,
  result: unknown,
  metadata: Record<string, unknown>,
  block: TranscriptBlock
): ToolDetailSection[] {
  const sections: ToolDetailSection[] = [];
  const argsRecord = asRecord(args);
  const resultRecord = asRecord(result);
  const command = stringValue(argsRecord.cmd) ??
    stringValue(argsRecord.command) ??
    execCommandTitleSubject(block.title ?? "") ??
    (block.preview?.trim() || null);
  if (command) {
    sections.push({ code: true, kind: "text", text: command, title: "Command" });
  }
  const inputRows = visibleRowsFromKeys(argsRecord, [
    ["cwd", "cwd"],
    ["yield_time_ms", "yield"],
    ["max_output_tokens", "output limit"],
    ["sandbox_permissions", "sandbox"],
    ["shell", "shell"]
  ]);
  if (inputRows.length > 0) {
    sections.push({ kind: "kv", rows: inputRows, title: "Input" });
  }
  const output = textFromValue(resultRecord.output) ?? textFromValue(result);
  if (output) {
    sections.push({ code: true, kind: "text", text: output, title: "Output" });
  }
  const error = stringValue(resultRecord.error);
  if (error) {
    sections.push({ kind: "text", text: error, title: "Error", tone: "error" });
  }
  const statusRows = visibleRowsFromKeys(resultRecord, [
    ["exit_code", "exit"],
    ["duration_ms", "duration"],
    ["wall_time_seconds", "wall time"]
  ]);
  const outcome = stringValue(metadata.outcome);
  if (outcome && outcome !== "normal") {
    statusRows.push({ label: "outcome", value: outcome });
  }
  if (block.result?.isError && !statusRows.some((row) => row.label === "outcome")) {
    statusRows.push({ label: "status", value: "error" });
  }
  if (statusRows.length > 0) {
    sections.push({ kind: "kv", rows: statusRows, title: "Status", tone: error || block.result?.isError ? "error" : "default" });
  }
  return sections;
}

function writeStdinSections(args: unknown, result: unknown, metadata: Record<string, unknown>): ToolDetailSection[] {
  const sections: ToolDetailSection[] = [];
  const argsRecord = asRecord(args);
  const chars = stringValue(argsRecord.chars);
  if (chars) {
    sections.push({ code: true, kind: "text", text: chars, title: "Input" });
  }
  const resultRecord = asRecord(result);
  const output = textFromValue(resultRecord.output) ?? textFromValue(result);
  if (output) {
    sections.push({ code: true, kind: "text", text: output, title: "Output" });
  }
  const rows = visibleRowsFromKeys(resultRecord, [["exit_code", "exit"], ["duration_ms", "duration"]]);
  const outcome = stringValue(metadata.outcome);
  if (outcome && outcome !== "normal") {
    rows.push({ label: "outcome", value: outcome });
  }
  if (rows.length > 0) {
    sections.push({ kind: "kv", rows, title: "Status" });
  }
  return sections;
}

function readSections(result: unknown, block: TranscriptBlock): ToolDetailSection[] {
  const resultRecord = asRecord(result);
  if (Object.prototype.hasOwnProperty.call(resultRecord, "content")) {
    const text = readContentText(resultRecord.content);
    if (text !== null) {
      return [{ code: true, kind: "text", text, title: "" }];
    }
  }
  const error = stringValue(resultRecord.error);
  if (error) {
    return [{ kind: "text", text: error, title: "Error", tone: "error" }];
  }
  if (block.result?.isError) {
    const fallback = textFromValue(result);
    return fallback
      ? [{ kind: "text", text: fallback, title: "Error", tone: "error" }]
      : [{ kind: "kv", rows: [{ label: "status", value: "error" }], title: "Status", tone: "error" }];
  }
  return [];
}

function readContentText(value: unknown): string | null {
  if (value === null || value === undefined) {
    return null;
  }
  if (typeof value === "string") {
    return value;
  }
  return textFromValue(value);
}

function bodyTextSections(spec: ToolDisplaySpec, result: unknown, toolName: string): ToolDetailSection[] {
  const resultRecord = asRecord(result);
  const sections: ToolDetailSection[] = [];
  for (const key of spec.bodyKeys) {
    const value = resultRecord[key];
    const text = textFromValue(value);
    if (!text) {
      continue;
    }
    sections.push({
      code: key === "diff" || key === "output" || toolName === "read",
      kind: "text",
      text,
      title: sectionTitleForKey(key)
    });
  }
  if (sections.length === 0 && isMcpTool(toolName)) {
    const text = mcpContentText(resultRecord.content);
    if (text) {
      sections.push({ kind: "text", text, title: "Content" });
    }
  }
  return sections;
}

function resultTitle(toolName: string): string {
  if (toolName === "read" || toolName === "web_fetch" || toolName === "web_search") {
    return "Result";
  }
  if (toolName === "write" || toolName === "edit" || toolName === "apply_patch") {
    return "Change";
  }
  return "Result";
}

function sectionTitleForKey(key: string): string {
  switch (key) {
    case "content":
      return "Content";
    case "diff":
      return "Diff";
    case "error":
      return "Error";
    case "output":
      return "Output";
    case "results":
    case "items":
      return "Results";
    default:
      return key;
  }
}

function visibleRows(value: unknown, source: "input" | "result", hiddenKeys = EMPTY_KEYS): ToolDetailRow[] {
  const record = asRecord(value);
  return Object.entries(record).flatMap(([key, field]) => {
    if (INTERNAL_KEYS.has(key) || BODY_KEYS.has(key) || hiddenKeys.has(key) || field === null || field === undefined) {
      return [];
    }
    if (source === "input" && (key === "session_id" || key === "yield_time_ms")) {
      return [];
    }
    const display = displayValueInline(field);
    return display ? [{ label: readableLabel(key), value: compactText(display, 220) }] : [];
  }).slice(0, 8);
}

function visibleRowsFromKeys(record: Record<string, unknown>, keys: Array<[string, string]>): ToolDetailRow[] {
  return keys.flatMap(([key, label]) => {
    const value = record[key];
    if (value === null || value === undefined) {
      return [];
    }
    const display = displayValueInline(value);
    return display ? [{ label, value: display }] : [];
  });
}

function readableLabel(key: string): string {
  switch (key) {
    case "bytes_written":
      return "bytes";
    case "content_type":
      return "type";
    case "exit_code":
      return "exit";
    case "file_path":
      return "path";
    case "final_url":
      return "final URL";
    case "files_modified":
      return "files";
    case "original_bytes":
      return "original bytes";
    case "output_bytes":
      return "output bytes";
    case "wall_time_seconds":
      return "wall time";
    default:
      return key.replace(/_/g, " ");
  }
}

function displayValueInline(value: unknown): string | null {
  if (value === null || value === undefined) {
    return null;
  }
  if (typeof value === "string") {
    return value.trim() || null;
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  if (Array.isArray(value)) {
    const parts = value.map(displayValueInline).filter((item): item is string => Boolean(item));
    return parts.length > 0 ? parts.join(", ") : `${value.length} items`;
  }
  const record = asRecord(value);
  const rows = Object.entries(record)
    .filter(([key, field]) => !INTERNAL_KEYS.has(key) && !BODY_KEYS.has(key) && field !== null && field !== undefined)
    .slice(0, 4)
    .map(([key, field]) => {
      const display = displayValueInline(field);
      return display ? `${readableLabel(key)} ${display}` : null;
    })
    .filter((item): item is string => Boolean(item));
  return rows.length > 0 ? rows.join(", ") : null;
}

function textFromValue(value: unknown): string | null {
  if (value === null || value === undefined) {
    return null;
  }
  if (typeof value === "string") {
    return value.trim() || null;
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  if (Array.isArray(value)) {
    const lines = value.map((item) => textFromValue(item) ?? displayValueInline(item)).filter((item): item is string => Boolean(item));
    return lines.length > 0 ? lines.join("\n") : null;
  }
  const record = asRecord(value);
  const rows = Object.entries(record)
    .filter(([key, field]) => !INTERNAL_KEYS.has(key) && field !== null && field !== undefined)
    .map(([key, field]) => {
      const display = displayValueInline(field) ?? textFromValue(field);
      return display ? `${readableLabel(key)}: ${display}` : null;
    })
    .filter((item): item is string => Boolean(item));
  return rows.length > 0 ? rows.join("\n") : null;
}

function mcpContentText(value: unknown): string | null {
  if (!Array.isArray(value)) {
    return null;
  }
  const lines = value.flatMap((item) => {
    const record = asRecord(item);
    const text = stringValue(record.text) ?? stringValue(asRecord(record.raw).text);
    if (text) {
      return [text];
    }
    const resource = asRecord(record.resource);
    const uri = stringValue(resource.uri);
    if (uri) {
      return [`resource: ${uri}`];
    }
    return [];
  });
  return lines.length > 0 ? lines.join("\n") : null;
}

function isMcpTool(toolName: string): boolean {
  return toolName === "mcp" || toolName === "mcp_call" || toolName.startsWith("mcp__");
}

function parseJsonLike(value: unknown): unknown {
  if (typeof value !== "string") {
    return value;
  }
  const trimmed = value.trim();
  if (!trimmed || (!trimmed.startsWith("{") && !trimmed.startsWith("["))) {
    return value;
  }
  try {
    return JSON.parse(trimmed) as unknown;
  } catch {
    return value;
  }
}

function toolResultValue(block: TranscriptBlock, metadata: Record<string, unknown>): unknown {
  const resultContent = block.result?.content;
  if (resultContent !== undefined && resultContent !== null) {
    return parseJsonLike(resultContent);
  }
  return parseJsonLike(metadata.result);
}

function execCommandInvocation(toolName: string, title: string, args: unknown, fallbackCommand: string): string | null {
  const trimmedTitle = title.trim();
  const trimmedTool = toolName.trim();
  const isExecCommand = trimmedTool === "exec_command" ||
    trimmedTitle === "exec_command" ||
    trimmedTitle.startsWith("exec_command ");
  if (!isExecCommand) {
    return null;
  }
  const record = asRecord(args);
  const command = stringValue(record.cmd) ??
    stringValue(record.command) ??
    execCommandTitleSubject(trimmedTitle) ??
    (fallbackCommand.trim() || null);
  if (!command) {
    return "exec_command";
  }
  return compactText(`exec_command ${firstEffectiveCommand(command)}`, 180);
}

function execCommandTitleSubject(title: string): string | null {
  const subject = title.trim().replace(/^exec_command\s+/, "").trim();
  return subject && subject !== title.trim() ? subject : null;
}

function firstEffectiveCommand(command: string): string {
  return command
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find((line) => line && !line.startsWith("#")) ?? command.trim();
}

function formatCount(value: unknown): string {
  const number = typeof value === "number" ? value : typeof value === "string" ? Number(value) : NaN;
  return Number.isFinite(number) ? new Intl.NumberFormat("en-US").format(number) : String(value);
}
