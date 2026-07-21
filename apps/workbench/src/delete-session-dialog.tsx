import type { SessionSummary } from "@psychevo/protocol";
import { ConfirmDialog } from "@psychevo/components";

export function DeleteSessionDialog({
  disabled,
  onCancel,
  onConfirm,
  session
}: {
  disabled: boolean;
  onCancel(): void;
  onConfirm(): void;
  session: SessionSummary;
}) {
  const targetLabel = session.lifecycle?.targetLabel;
  const remote = Boolean(targetLabel && targetLabel !== "Psychevo (Native)");
  const title = session.displayTitle?.trim() || session.title?.trim() || session.id.slice(0, 8);
  return (
    <ConfirmDialog
      confirmLabel="Delete session"
      description={<>
        <p className="agentSessionDeleteName">{title}</p>
        <p>{remote
          ? `This permanently deletes the Psychevo Thread and its ${targetLabel} session.`
          : "This permanently deletes the Psychevo Thread from local history."}</p>
        {remote ? <p className="agentSessionDeleteWarning">Remote deletion must succeed before Psychevo removes local history.</p> : null}
      </>}
      disabled={disabled}
      onCancel={onCancel}
      onConfirm={onConfirm}
      open
      title="Delete session?"
      tone="danger"
    />
  );
}
