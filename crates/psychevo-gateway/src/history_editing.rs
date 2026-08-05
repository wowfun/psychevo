use std::path::PathBuf;

use psychevo::{
    ForkThreadRequest, ImageInput, ThreadConversationEditConflict,
    ThreadConversationEditRestoreOutcome, ThreadConversationEditStageOutcome,
    ThreadConversationEditUnavailable, ThreadEditableDraft, ThreadEditableDraftFidelity,
    ThreadEditableDraftPart, ThreadEditableDraftRead, ThreadEditableDraftReadOutcome,
    ThreadEditableDraftUnavailable, ThreadHistoryEditingEligibility, ThreadHistoryEditingStaged,
    ThreadHistoryEditingState, ThreadHistoryEditingUnavailable,
};
use psychevo_gateway_protocol as wire;

use crate::gateway::Gateway;

const RUNNING_HISTORY_REASON: &str =
    "Finish the running turn before editing or forking conversation history.";
const RUNNING_RESTORE_REASON: &str =
    "Finish the running turn before restoring conversation history.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryEditingSurface {
    Workbench,
    Tui,
}

#[derive(Clone, Debug)]
pub struct NativeHistoryActions {
    pub unavailable_reason: Option<String>,
    pub staged: Option<wire::events_transcript::ThreadHistoryEditingView>,
}

impl Gateway {
    pub async fn native_history_actions(
        &self,
        thread_id: &str,
        _surface: HistoryEditingSurface,
    ) -> psychevo::Result<NativeHistoryActions> {
        let state = self
            .framework_thread(thread_id)
            .await?
            .history_editing_state()
            .await?;
        Ok(self.project_native_history_actions(thread_id, state))
    }

    pub async fn history_editing_state(
        &self,
        thread_id: &str,
    ) -> psychevo::Result<Option<wire::events_transcript::ThreadHistoryEditingView>> {
        Ok(self
            .framework_thread(thread_id)
            .await?
            .history_editing_state()
            .await?
            .staged
            .map(history_staged_to_wire))
    }

    pub async fn read_native_editable_draft(
        &self,
        thread_id: &str,
        message_id: &str,
        _surface: HistoryEditingSurface,
    ) -> psychevo::Result<wire::thread_command_turn::ThreadHistoryDraftReadResult> {
        let thread = self.framework_thread(thread_id).await?;
        let state = thread.history_editing_state().await?;
        if let Some(reason) = self
            .project_native_history_actions(thread_id, state)
            .unavailable_reason
        {
            return Ok(unavailable_draft(thread_id, message_id, None, &reason));
        }
        let Some(message_seq) = parse_message_seq(message_id) else {
            return Ok(unavailable_draft(
                thread_id,
                message_id,
                None,
                "Only visible, finalized user messages with durable history can be edited.",
            ));
        };
        let outcome = thread.editable_draft(message_seq).await?;
        Ok(match outcome {
            ThreadEditableDraftReadOutcome::Available(read) => {
                editable_draft_read_to_wire(thread_id, message_id, read)
            }
            ThreadEditableDraftReadOutcome::Unavailable(reason) => unavailable_draft(
                thread_id,
                message_id,
                Some(message_seq),
                editable_draft_unavailable_reason(reason),
            ),
        })
    }

    pub async fn stage_native_conversation_edit(
        &self,
        thread_id: &str,
        message_id: &str,
        draft: &wire::thread_command_turn::ThreadEditableDraft,
        _surface: HistoryEditingSurface,
    ) -> psychevo::Result<bool> {
        let message_seq = parse_message_seq(message_id).ok_or_else(|| {
            psychevo::Error::Message(
                "Only visible, finalized user messages with durable history can be edited."
                    .to_string(),
            )
        })?;
        let thread = self.framework_thread(thread_id).await?;
        let _reservation = self.reserve_history_mutation(thread_id, RUNNING_HISTORY_REASON)?;
        match thread
            .stage_conversation_edit(message_seq, draft_from_wire(draft))
            .await
            .map_err(|error| remap_thread_busy(error, thread_id, RUNNING_HISTORY_REASON))?
        {
            ThreadConversationEditStageOutcome::Staged
            | ThreadConversationEditStageOutcome::AlreadyStaged => Ok(true),
            ThreadConversationEditStageOutcome::Unchanged => Ok(false),
            ThreadConversationEditStageOutcome::Unavailable(reason) => Err(
                psychevo::Error::Message(conversation_edit_unavailable_reason(reason).to_string()),
            ),
            ThreadConversationEditStageOutcome::Conflict(reason) => Err(psychevo::Error::Message(
                conversation_edit_conflict_reason(reason).to_string(),
            )),
        }
    }

    pub async fn restore_native_conversation_edit(
        &self,
        thread_id: &str,
        _surface: HistoryEditingSurface,
    ) -> psychevo::Result<wire::thread_command_turn::ThreadEditableDraft> {
        let thread = self.framework_thread(thread_id).await?;
        let _reservation = self.reserve_history_mutation(thread_id, RUNNING_RESTORE_REASON)?;
        match thread
            .restore_conversation_edit()
            .await
            .map_err(|error| remap_thread_busy(error, thread_id, RUNNING_RESTORE_REASON))?
        {
            ThreadConversationEditRestoreOutcome::Restored(draft) => Ok(draft_to_wire(draft)),
            ThreadConversationEditRestoreOutcome::ThreadNotFound => Err(psychevo::Error::Message(
                "The durable Thread is unavailable.".to_string(),
            )),
            ThreadConversationEditRestoreOutcome::NotStaged => Err(psychevo::Error::Message(
                "No conversation edit is staged.".to_string(),
            )),
            ThreadConversationEditRestoreOutcome::Conflict(reason) => Err(
                psychevo::Error::Message(restore_conflict_reason(reason).to_string()),
            ),
        }
    }

    pub async fn fork_native_history(
        &self,
        thread_id: &str,
        before_session_seq: Option<i64>,
        _surface: HistoryEditingSurface,
    ) -> psychevo::Result<String> {
        let thread = self.framework_thread(thread_id).await?;
        let state = thread.history_editing_state().await?;
        let staged = state.staged.map(history_staged_to_wire);
        if let Some(reason) = history_eligibility_reason(state.eligibility)
            .or_else(|| staged.as_ref().map(staged_reason))
        {
            return Err(psychevo::Error::Message(reason));
        }
        let _reservation = self.reserve_history_mutation(thread_id, RUNNING_HISTORY_REASON)?;
        Ok(thread
            .fork(ForkThreadRequest { before_session_seq })
            .await
            .map_err(|error| remap_thread_busy(error, thread_id, RUNNING_HISTORY_REASON))?
            .id()
            .to_string())
    }

    fn project_native_history_actions(
        &self,
        thread_id: &str,
        state: ThreadHistoryEditingState,
    ) -> NativeHistoryActions {
        let staged = state.staged.map(history_staged_to_wire);
        let unavailable_reason = history_eligibility_reason(state.eligibility)
            .or_else(|| self.local_history_editing_unavailable_reason(thread_id))
            .or_else(|| staged.as_ref().map(staged_reason));
        NativeHistoryActions {
            unavailable_reason,
            staged,
        }
    }
}

fn parse_message_seq(message_id: &str) -> Option<i64> {
    message_id
        .strip_prefix("message:")?
        .parse::<i64>()
        .ok()
        .filter(|seq| *seq > 0)
}

fn history_staged_to_wire(
    staged: ThreadHistoryEditingStaged,
) -> wire::events_transcript::ThreadHistoryEditingView {
    match staged {
        ThreadHistoryEditingStaged::WorkspaceUndo {
            boundary_message_seq,
            hidden_entry_count,
        } => wire::events_transcript::ThreadHistoryEditingView {
            kind: wire::events_transcript::ThreadHistoryEditingKind::WorkspaceUndo,
            boundary_message_id: Some(format!("message:{boundary_message_seq}")),
            hidden_entry_count,
            replacement_draft: None,
            available_actions: vec![
                wire::events_transcript::ThreadHistoryRecoveryActionKind::RedoWorkspace,
            ],
        },
        ThreadHistoryEditingStaged::ConversationEdit {
            boundary_message_seq,
            hidden_entry_count,
            draft,
        } => wire::events_transcript::ThreadHistoryEditingView {
            kind: wire::events_transcript::ThreadHistoryEditingKind::ConversationEdit,
            boundary_message_id: Some(format!("message:{boundary_message_seq}")),
            hidden_entry_count,
            replacement_draft: Some(draft_to_wire(draft)),
            available_actions: vec![
                wire::events_transcript::ThreadHistoryRecoveryActionKind::RestoreHistory,
            ],
        },
    }
}

fn history_eligibility_reason(eligibility: ThreadHistoryEditingEligibility) -> Option<String> {
    match eligibility {
        ThreadHistoryEditingEligibility::Eligible => None,
        ThreadHistoryEditingEligibility::Unavailable(reason) => {
            Some(history_unavailable_reason(reason).to_string())
        }
    }
}

fn history_unavailable_reason(reason: ThreadHistoryEditingUnavailable) -> &'static str {
    match reason {
        ThreadHistoryEditingUnavailable::ThreadNotFound
        | ThreadHistoryEditingUnavailable::CorruptThreadMetadata => {
            "The durable Thread is unavailable."
        }
        ThreadHistoryEditingUnavailable::UnsupportedSource => {
            "Dedicated channel and automation Threads cannot edit or fork conversation history."
        }
        ThreadHistoryEditingUnavailable::ChildThread
        | ThreadHistoryEditingUnavailable::AgentChildThread => {
            "Subagent and side Threads cannot edit or fork conversation history."
        }
        ThreadHistoryEditingUnavailable::SideConversation => {
            "Side Threads cannot edit or fork conversation history."
        }
        ThreadHistoryEditingUnavailable::RuntimeBindingMissing
        | ThreadHistoryEditingUnavailable::RuntimeBindingUnresolved
        | ThreadHistoryEditingUnavailable::RuntimeBindingNotNative
        | ThreadHistoryEditingUnavailable::RuntimeBindingReadOnly => {
            "History editing requires a resolved Native Thread binding."
        }
        ThreadHistoryEditingUnavailable::ThreadBusy => RUNNING_HISTORY_REASON,
    }
}

fn editable_draft_read_to_wire(
    thread_id: &str,
    message_id: &str,
    read: ThreadEditableDraftRead,
) -> wire::thread_command_turn::ThreadHistoryDraftReadResult {
    let (fidelity, warning) = match read.fidelity {
        ThreadEditableDraftFidelity::Exact => (wire::thread_command_turn::ThreadEditableDraftFidelity::Exact, None),
        ThreadEditableDraftFidelity::BestEffort => (
            wire::thread_command_turn::ThreadEditableDraftFidelity::BestEffort,
            Some(
                "This older message was reconstructed from durable history; hidden context or synthetic input may not be recoverable."
                    .to_string(),
            ),
        ),
    };
    wire::thread_command_turn::ThreadHistoryDraftReadResult {
        thread_id: thread_id.to_string(),
        message_id: message_id.to_string(),
        message_seq: Some(read.message_seq),
        parts: draft_parts_to_wire(read.draft.parts),
        fidelity,
        warning,
        unavailable_reason: None,
    }
}

fn editable_draft_unavailable_reason(reason: ThreadEditableDraftUnavailable) -> &'static str {
    match reason {
        ThreadEditableDraftUnavailable::ThreadNotFound => "The durable Thread is unavailable.",
        ThreadEditableDraftUnavailable::MessageNotFound
        | ThreadEditableDraftUnavailable::CorruptMessage
        | ThreadEditableDraftUnavailable::CorruptMetadata => {
            "The durable user message is no longer available in this Thread."
        }
        ThreadEditableDraftUnavailable::NotUserMessage => {
            "The durable history entry no longer resolves to a user message."
        }
        ThreadEditableDraftUnavailable::CorruptEditableInput
        | ThreadEditableDraftUnavailable::EditableInputMismatch => {
            "The editable input envelope no longer matches the durable message."
        }
        ThreadEditableDraftUnavailable::NoEditableInput
        | ThreadEditableDraftUnavailable::EmptyReplacement => {
            "This message has no editable text or image input."
        }
    }
}

fn conversation_edit_unavailable_reason(reason: ThreadConversationEditUnavailable) -> &'static str {
    match reason {
        ThreadConversationEditUnavailable::HistoryEditing(reason) => {
            history_unavailable_reason(reason)
        }
        ThreadConversationEditUnavailable::Draft(reason) => {
            editable_draft_unavailable_reason(reason)
        }
    }
}

fn conversation_edit_conflict_reason(reason: ThreadConversationEditConflict) -> &'static str {
    match reason {
        ThreadConversationEditConflict::WorkspaceUndoStaged => {
            "Redo workspace files before editing conversation history."
        }
        ThreadConversationEditConflict::ConversationEditStaged => {
            "Restore or run the staged conversation edit before starting another edit."
        }
        ThreadConversationEditConflict::ConcurrentMetadataChange => {
            "Restore or redo the staged history state before editing conversation history."
        }
    }
}

fn restore_conflict_reason(reason: ThreadConversationEditConflict) -> &'static str {
    match reason {
        ThreadConversationEditConflict::WorkspaceUndoStaged => {
            "The staged state belongs to workspace undo; use /redo instead."
        }
        ThreadConversationEditConflict::ConversationEditStaged => {
            "Restore or run the staged conversation edit before starting another edit."
        }
        ThreadConversationEditConflict::ConcurrentMetadataChange => {
            "The staged history state changed while it was being restored."
        }
    }
}

fn remap_thread_busy(error: psychevo::Error, thread_id: &str, message: &str) -> psychevo::Error {
    let Some(data) = error.structured_data() else {
        return error;
    };
    if data.get("kind").and_then(serde_json::Value::as_str) != Some("thread_busy") {
        return error;
    }
    let blocking_operation = data
        .get("blockingOperation")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("turn");
    psychevo::Error::structured(
        message,
        serde_json::json!({
            "kind": "thread_busy",
            "threadId": thread_id,
            "blockingOperation": blocking_operation,
            "retryable": true,
        }),
    )
}

fn staged_reason(staged: &wire::events_transcript::ThreadHistoryEditingView) -> String {
    match staged.kind {
        wire::events_transcript::ThreadHistoryEditingKind::WorkspaceUndo => {
            "Redo workspace files before editing or forking conversation history.".to_string()
        }
        wire::events_transcript::ThreadHistoryEditingKind::ConversationEdit => {
            "Restore or run the staged conversation edit before another edit or fork.".to_string()
        }
    }
}

fn unavailable_draft(
    thread_id: &str,
    message_id: &str,
    message_seq: Option<i64>,
    reason: &str,
) -> wire::thread_command_turn::ThreadHistoryDraftReadResult {
    wire::thread_command_turn::ThreadHistoryDraftReadResult {
        thread_id: thread_id.to_string(),
        message_id: message_id.to_string(),
        message_seq,
        parts: Vec::new(),
        fidelity: wire::thread_command_turn::ThreadEditableDraftFidelity::BestEffort,
        warning: None,
        unavailable_reason: Some(reason.to_string()),
    }
}

fn draft_from_wire(draft: &wire::thread_command_turn::ThreadEditableDraft) -> ThreadEditableDraft {
    ThreadEditableDraft {
        parts: draft
            .parts
            .iter()
            .map(|part| match part {
                wire::thread_command_turn::ThreadEditableInputPart::Text { text } => {
                    ThreadEditableDraftPart::Text { text: text.clone() }
                }
                wire::thread_command_turn::ThreadEditableInputPart::Image {
                    input: wire::source::GatewayImageInput::LocalPath { path },
                } => ThreadEditableDraftPart::Image {
                    input: ImageInput::LocalPath(PathBuf::from(path)),
                },
                wire::thread_command_turn::ThreadEditableInputPart::Image {
                    input: wire::source::GatewayImageInput::Url { url },
                } => ThreadEditableDraftPart::Image {
                    input: ImageInput::ImageUrl(url.clone()),
                },
            })
            .collect(),
    }
}

fn draft_to_wire(draft: ThreadEditableDraft) -> wire::thread_command_turn::ThreadEditableDraft {
    wire::thread_command_turn::ThreadEditableDraft {
        parts: draft_parts_to_wire(draft.parts),
    }
}

fn draft_parts_to_wire(
    parts: Vec<ThreadEditableDraftPart>,
) -> Vec<wire::thread_command_turn::ThreadEditableInputPart> {
    parts
        .into_iter()
        .map(|part| match part {
            ThreadEditableDraftPart::Text { text } => {
                wire::thread_command_turn::ThreadEditableInputPart::Text { text }
            }
            ThreadEditableDraftPart::Image {
                input: ImageInput::LocalPath(path),
            } => wire::thread_command_turn::ThreadEditableInputPart::Image {
                input: wire::source::GatewayImageInput::LocalPath {
                    path: path.display().to_string(),
                },
            },
            ThreadEditableDraftPart::Image {
                input: ImageInput::ImageUrl(url),
            } => wire::thread_command_turn::ThreadEditableInputPart::Image {
                input: wire::source::GatewayImageInput::Url { url },
            },
        })
        .collect()
}
