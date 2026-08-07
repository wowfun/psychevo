import { MarkdownText } from "@psychevo/components";
import type { RightWorkspacePinnedMessage } from "../types";

export function PinnedMessagePanel({ message }: { message: RightWorkspacePinnedMessage }) {
  const role = message.role === "user" ? "You" : "Assistant";
  const timestamp = new Date(message.createdAtMs);
  const validTimestamp = Number.isFinite(timestamp.getTime());
  return (
    <section className="pinnedMessagePanel" aria-label="Pinned message">
      <header>
        <div>
          <h2>{role}</h2>
          <p title={message.threadId}>{message.sourceTitle}</p>
        </div>
        <div className="pinnedMessageMeta">
          {message.status !== "completed" && (
            <span className={`is-${message.status}`}>{message.status === "failed" ? "Failed" : "Cancelled"}</span>
          )}
          {validTimestamp && (
            <time dateTime={timestamp.toISOString()}>
              {new Intl.DateTimeFormat(undefined, {
                dateStyle: "medium",
                timeStyle: "short"
              }).format(timestamp)}
            </time>
          )}
        </div>
      </header>
      <div className="pinnedMessageBody">
        <MarkdownText text={message.text} />
      </div>
    </section>
  );
}
