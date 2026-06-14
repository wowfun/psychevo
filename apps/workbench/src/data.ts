import type {
  BackendConfigTarget,
  CommandAlternateAction,
  CommandFeedback,
  CommandTrigger,
  WorkbenchAgent,
  WorkbenchBackend,
  WorkbenchBackendDoctor,
  WorkbenchCommand,
  WorkbenchDiagnostic
} from "./types";

export function parseAgentList(value: unknown): WorkbenchAgent[] {
  const agents = asRecord(value).agents;
  return Array.isArray(agents)
    ? agents.map((agent) => {
        const item = asRecord(agent);
        return {
          name: stringField(item.name),
          description: stringField(item.description),
          source: stringField(item.source),
          generated: item.generated === true,
          path: optionalStringField(item.path),
          entrypoints: stringArray(item.entrypoints),
          backend: asOptionalRecord(item.backend) as { ref?: string } | null
        };
      }).filter((agent) => agent.name)
    : [];
}

export function parseBackendList(value: unknown): WorkbenchBackend[] {
  const backends = asRecord(value).backends;
  return Array.isArray(backends)
    ? backends.map((backend) => {
        const item = asRecord(backend);
        return {
          id: stringField(item.id),
          kind: stringField(item.kind),
          enabled: item.enabled !== false,
          label: stringField(item.label),
          description: optionalStringField(item.description),
          command: optionalStringField(item.command),
          args: stringArray(item.args),
          cwd: optionalStringField(item.cwd) ?? "invocation",
          entrypoints: stringArray(item.entrypoints),
          clientCapabilities: stringArray(item.clientCapabilities),
          mcpServers: stringArray(item.mcpServers),
          envKeys: stringArray(item.envKeys),
          sourceTargets: parseBackendTargets(item.sourceTargets),
          diagnostics: parseDiagnostics(item.diagnostics)
        };
      }).filter((backend) => backend.id)
    : [];
}

export function parseBackendDoctor(value: unknown): WorkbenchBackendDoctor {
  const item = asRecord(value);
  const checks = Array.isArray(item.checks) ? item.checks : [];
  return {
    id: stringField(item.id),
    ok: item.ok === true,
    checks: checks.map((check) => {
      const record = asRecord(check);
      return {
        name: stringField(record.name),
        ok: record.ok === true,
        message: stringField(record.message),
        path: optionalStringField(record.path)
      };
    }).filter((check) => check.name)
  };
}

function parseBackendTargets(value: unknown): BackendConfigTarget[] {
  return stringArray(value)
    .filter((item): item is BackendConfigTarget => item === "project" || item === "profile");
}

function parseDiagnostics(value: unknown): WorkbenchDiagnostic[] {
  return Array.isArray(value)
    ? value.map((diagnostic) => {
        const item = asRecord(diagnostic);
        return {
          kind: stringField(item.kind),
          message: stringField(item.message)
        };
      }).filter((diagnostic) => diagnostic.message)
    : [];
}

export function parseCommandList(value: unknown): WorkbenchCommand[] {
  const commands = asRecord(value).commands;
  return Array.isArray(commands)
    ? commands.map((command) => {
        const item = asRecord(command);
        return {
          name: stringField(item.name),
          slash: stringField(item.slash),
          usage: stringField(item.usage),
          summary: stringField(item.summary),
          aliases: stringArray(item.aliases),
          argumentKind: stringField(item.argumentKind),
          source: stringField(item.source),
          presentationKind: optionalStringField(item.presentationKind) ?? "control",
          destination: optionalStringField(item.destination),
          feedbackAnchor: optionalStringField(item.feedbackAnchor),
          alternateAction: parseCommandAlternateAction(item.alternateAction)
        };
      }).filter((command) => command.name)
    : [];
}

function parseCommandAlternateAction(value: unknown): CommandAlternateAction | null {
  const action = asOptionalRecord(value);
  if (!action) {
    return null;
  }
  const type = stringField(action.type);
  const target = stringField(action.target);
  const label = stringField(action.label);
  return type && target && label ? { type, target, label } : null;
}

export function commandFeedbackFromResult(
  command: string,
  record: Record<string, unknown>,
  trigger: CommandTrigger,
  options: { downloadAvailable?: boolean } = {}
): CommandFeedback {
  const action = asRecord(record.action);
  const message = optionalStringField(record.message) ?? commandActionFeedbackMessage(action, options);
  if (!message) {
    return null;
  }
  const downloadFailed = action.type === "downloadSession" && options.downloadAvailable === false;
  return {
    accepted: record.accepted === true && !downloadFailed,
    command: optionalStringField(record.command) ?? command,
    message,
    feedbackAnchor: resolveCommandFeedbackAnchor(optionalStringField(record.feedbackAnchor), trigger),
    alternateAction: parseCommandAlternateAction(record.alternateAction)
  };
}

export function commandFeedbackAutoDismissable(feedback: NonNullable<CommandFeedback>): boolean {
  return feedback.accepted && !feedback.alternateAction;
}

function resolveCommandFeedbackAnchor(anchor: string | null, trigger: CommandTrigger): string {
  if (
    (trigger === "commandsPanel" || trigger === "commandOverlay")
    && (!anchor || anchor === "trigger" || anchor === "commandsPanel")
  ) {
    return "commandsPanel";
  }
  if (!anchor || anchor === "trigger") {
    return trigger;
  }
  if (trigger === "composer" && anchor === "commandsPanel") {
    return "composer";
  }
  return anchor;
}

function commandActionFeedbackMessage(
  action: Record<string, unknown>,
  options: { downloadAvailable?: boolean } = {}
): string | null {
  if (action.type === "downloadSession") {
    if (options.downloadAvailable === false) {
      return stringField(action.kind) === "share"
        ? "Share is not available for this session."
        : "Export is not available for this session.";
    }
    return stringField(action.kind) === "share" ? "Share artifact opened." : "Export download opened.";
  }
  if (action.type === "showPanel") {
    return `Opened ${hostPanelLabel(stringField(action.panel))}.`;
  }
  return null;
}

function hostPanelLabel(panel: string): string {
  switch (panel) {
    case "history":
    case "sessions":
      return "History";
    case "agents":
      return "Agents";
    case "commands":
    case "help":
      return "Commands";
    case "preview":
      return "Preview";
    case "files":
      return "Files";
    case "debug":
      return "Debug";
    case "status":
    default:
      return "Status";
  }
}

const COMMAND_PRESENTATION_ORDER = ["navigate", "inspect", "control", "submit", "export", "extension"];

export function commandPresentationGroups(commands: WorkbenchCommand[]): Array<{ kind: string; commands: WorkbenchCommand[] }> {
  const order = new Map(COMMAND_PRESENTATION_ORDER.map((kind, index) => [kind, index]));
  const grouped = new Map<string, WorkbenchCommand[]>();
  for (const command of commands) {
    const kind = command.presentationKind || "control";
    grouped.set(kind, [...(grouped.get(kind) ?? []), command]);
  }
  return [...grouped.entries()]
    .sort(([left], [right]) => (order.get(left) ?? 99) - (order.get(right) ?? 99) || left.localeCompare(right))
    .map(([kind, commands]) => ({ kind, commands }));
}

export function commandPresentationLabel(kind: string): string {
  switch (kind) {
    case "navigate":
      return "Navigate";
    case "inspect":
      return "Inspect";
    case "control":
      return "Control";
    case "submit":
      return "Submit";
    case "export":
      return "Export";
    case "extension":
      return "Extensions";
    default:
      return kind ? `${kind.slice(0, 1).toUpperCase()}${kind.slice(1)}` : "Commands";
  }
}

export function commandDestinationLabel(destination: string | null): string | null {
  switch (destination) {
    case "commands":
      return "Commands";
    case "history":
      return "History";
    case "agents":
      return "Agents";
    case "status":
      return "Status";
    case "preview":
      return "Preview";
    case "composer":
      return "Composer";
    case "download":
      return "Download";
    default:
      return null;
  }
}

export function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

export function asOptionalRecord(value: unknown): Record<string, unknown> | null {
  const record = asRecord(value);
  return Object.keys(record).length > 0 ? record : null;
}

export function stringField(value: unknown): string {
  return typeof value === "string" ? value : "";
}

export function optionalStringField(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

export function traceEventSeq(value: unknown): number | null {
  const seq = asRecord(value).seq;
  return typeof seq === "number" && Number.isFinite(seq) ? seq : null;
}

export function traceEventLabel(value: unknown): string {
  const record = asRecord(value);
  const seq = traceEventSeq(value);
  const kind = optionalStringField(record.kind) ?? optionalStringField(record.type) ?? "event";
  return seq === null ? kind : `#${seq} ${kind}`;
}

export function traceEventTime(value: unknown): string {
  const timestamp = asRecord(value).timestamp_ms;
  if (typeof timestamp !== "number" || !Number.isFinite(timestamp)) {
    return "";
  }
  return new Date(timestamp).toLocaleTimeString();
}

export function stringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
}

export function prettyJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}
