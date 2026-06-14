import { TerminalSquare, X } from "lucide-react";
import {
  commandDestinationLabel,
  commandPresentationGroups,
  commandPresentationLabel
} from "./data";
import type { CommandAlternateAction, CommandFeedback, WorkbenchCommand } from "./types";

export function CommandOverlayView({
  commands,
  feedback,
  onAlternateAction,
  onClose,
  onExecute
}: {
  commands: WorkbenchCommand[];
  feedback: CommandFeedback;
  onAlternateAction(action: CommandAlternateAction): void;
  onClose(): void;
  onExecute: (slash: string) => void;
}) {
  return (
    <section className="commandOverlay" aria-label="Commands overlay">
      <header>
        <div className="centerPageTitle">
          <TerminalSquare size={18} />
          <div>
            <h2>Commands</h2>
            <p>Slash command catalog</p>
          </div>
        </div>
        <button
          aria-label="Close Commands"
          className="centerPageBack"
          data-tooltip="Back to transcript"
          onClick={onClose}
          title="Back to transcript"
          type="button"
        >
          <X size={15} />
        </button>
      </header>
      <div className="commandOverlayBody">
        <CommandsPanel
          commands={commands}
          feedback={feedback}
          onAlternateAction={onAlternateAction}
          onExecute={onExecute}
        />
      </div>
    </section>
  );
}

function CommandsPanel({
  commands,
  feedback,
  onAlternateAction,
  onExecute
}: {
  commands: WorkbenchCommand[];
  feedback: CommandFeedback;
  onAlternateAction(action: CommandAlternateAction): void;
  onExecute: (slash: string) => void;
}) {
  const groups = commandPresentationGroups(commands);
  return (
    <section className="agentSurfacePanel commandSurfacePanel" aria-label="Commands">
      <header>
        <span><TerminalSquare size={15} /> Commands</span>
        <b>{commands.length}</b>
      </header>
      {feedback && (
        <CommandFeedbackView feedback={feedback} onAlternateAction={onAlternateAction} />
      )}
      <div className="commandSurfaceList">
        {groups.map((group) => (
          <div className="commandSurfaceGroup" key={group.kind}>
            <h3>{commandPresentationLabel(group.kind)}</h3>
            {group.commands.map((command) => {
              const details = [
                commandDestinationLabel(command.destination),
                command.aliases.length > 0 ? command.aliases.map((alias) => `/${alias}`).join(" ") : null
              ].filter(Boolean).join(" · ");
              return (
                <button
                  className="commandSurfaceRow"
                  key={`${command.source}:${command.name}`}
                  onClick={() => onExecute(command.slash)}
                  title={command.usage || command.summary}
                  type="button"
                >
                  <code>{command.slash}</code>
                  <span>{command.summary}</span>
                  {details && <small>{details}</small>}
                </button>
              );
            })}
          </div>
        ))}
        {commands.length === 0 && <p>No commands available.</p>}
      </div>
    </section>
  );
}

export function CommandFeedbackView({
  className = "",
  feedback,
  onAlternateAction
}: {
  className?: string;
  feedback: NonNullable<CommandFeedback>;
  onAlternateAction(action: CommandAlternateAction): void;
}) {
  const alternateAction = feedback.alternateAction;
  return (
    <div className={`commandFeedback ${feedback.accepted ? "is-ok" : "is-error"} ${className}`.trim()}>
      <div>
        <strong>{feedback.command}</strong>
        <span>{feedback.message}</span>
      </div>
      {alternateAction && (
        <button
          className="commandFeedbackAction"
          onClick={() => onAlternateAction(alternateAction)}
          type="button"
        >
          {alternateAction.label}
        </button>
      )}
    </div>
  );
}
