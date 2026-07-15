import { Trash2 } from "lucide-react";
import type { SessionSummary } from "@psychevo/protocol";
import { ActionButton, CreatePanel } from "@psychevo/components";

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
    <div className="modalBackdrop" role="presentation" onMouseDown={(event) => {
      if (event.target === event.currentTarget && !disabled) onCancel();
    }}>
      <CreatePanel
        className="agentSessionDeleteDialog"
        description={remote
          ? `This permanently deletes the Psychevo Thread and its ${targetLabel} session.`
          : "This permanently deletes the Psychevo Thread from local history."}
        icon={<Trash2 size={18} />}
        layout="dialog"
        onClose={disabled ? undefined : onCancel}
        title="Delete session?"
        footer={
          <>
            <ActionButton disabled={disabled} onClick={onCancel} variant="ghost">Cancel</ActionButton>
            <ActionButton disabled={disabled} onClick={onConfirm} variant="danger">Delete session</ActionButton>
          </>
        }
      >
        <p className="agentSessionDeleteName">{title}</p>
        {remote ? <p className="agentSessionDeleteWarning">Remote deletion must succeed before Psychevo removes local history.</p> : null}
      </CreatePanel>
    </div>
  );
}
