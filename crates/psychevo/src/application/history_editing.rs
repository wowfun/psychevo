use std::path::PathBuf;

use futures::future::BoxFuture;
use tokio::sync::oneshot;

use super::{ImageInput, Thread};
use crate::store::ConversationDraftPart;
use crate::store::store_undo_state::{
    ConversationEditConflict, ConversationEditDraftFidelity, ConversationEditDraftRead,
    ConversationEditDraftReadOutcome, ConversationEditDraftUnavailable,
    ConversationEditRestoreOutcome, ConversationEditStageOutcome, ConversationEditUnavailable,
    HistoryEditingEligibility, HistoryEditingFacts, HistoryEditingRevertFacts,
    HistoryEditingUnavailable,
};
use crate::{Error, Result};

/// One ordered input part in a user-authored conversation draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadEditableDraftPart {
    Text { text: String },
    Image { input: ImageInput },
}

/// The user-authored input that can replace a conversation boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadEditableDraft {
    pub parts: Vec<ThreadEditableDraftPart>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadEditableDraftFidelity {
    Exact,
    BestEffort,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadEditableDraftRead {
    pub message_seq: i64,
    pub draft: ThreadEditableDraft,
    pub fidelity: ThreadEditableDraftFidelity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadEditableDraftUnavailable {
    ThreadNotFound,
    MessageNotFound,
    NotUserMessage,
    CorruptMessage,
    CorruptMetadata,
    CorruptEditableInput,
    EditableInputMismatch,
    NoEditableInput,
    EmptyReplacement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadEditableDraftReadOutcome {
    Available(ThreadEditableDraftRead),
    Unavailable(ThreadEditableDraftUnavailable),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadHistoryEditingEligibility {
    Eligible,
    Unavailable(ThreadHistoryEditingUnavailable),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadHistoryEditingUnavailable {
    ThreadNotFound,
    UnsupportedSource,
    ChildThread,
    AgentChildThread,
    SideConversation,
    RuntimeBindingMissing,
    RuntimeBindingUnresolved,
    RuntimeBindingNotNative,
    RuntimeBindingReadOnly,
    CorruptThreadMetadata,
    ThreadBusy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadHistoryEditingStaged {
    WorkspaceUndo {
        boundary_message_seq: i64,
        hidden_entry_count: usize,
    },
    ConversationEdit {
        boundary_message_seq: i64,
        hidden_entry_count: usize,
        draft: ThreadEditableDraft,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadHistoryEditingState {
    pub eligibility: ThreadHistoryEditingEligibility,
    pub staged: Option<ThreadHistoryEditingStaged>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadConversationEditUnavailable {
    HistoryEditing(ThreadHistoryEditingUnavailable),
    Draft(ThreadEditableDraftUnavailable),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadConversationEditConflict {
    WorkspaceUndoStaged,
    ConversationEditStaged,
    ConcurrentMetadataChange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadConversationEditStageOutcome {
    Staged,
    AlreadyStaged,
    Unchanged,
    Unavailable(ThreadConversationEditUnavailable),
    Conflict(ThreadConversationEditConflict),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadConversationEditRestoreOutcome {
    Restored(ThreadEditableDraft),
    ThreadNotFound,
    NotStaged,
    Conflict(ThreadConversationEditConflict),
}

impl Thread {
    pub async fn history_editing_state(&self) -> Result<ThreadHistoryEditingState> {
        let facts = self
            .client
            .inner
            .state
            .history_editing_facts(&self.id)
            .await?;
        let mut state = map_history_editing_facts(facts);
        if matches!(state.eligibility, ThreadHistoryEditingEligibility::Eligible)
            && self
                .client
                .inner
                .runtime
                .thread_history_editing_busy(&self.id)
        {
            state.eligibility = ThreadHistoryEditingEligibility::Unavailable(
                ThreadHistoryEditingUnavailable::ThreadBusy,
            );
        }
        Ok(state)
    }

    pub async fn editable_draft(&self, message_seq: i64) -> Result<ThreadEditableDraftReadOutcome> {
        self.client
            .inner
            .state
            .conversation_editable_draft(&self.id, message_seq)
            .await
            .map(map_editable_draft_read_outcome)
    }

    pub async fn stage_conversation_edit(
        &self,
        message_seq: i64,
        draft: ThreadEditableDraft,
    ) -> Result<ThreadConversationEditStageOutcome> {
        self.enqueue_idle_history_mutation(move |thread| async move {
            thread
                .client
                .inner
                .state
                .stage_conversation_edit_atomic(&thread.id, message_seq, draft.into_store_parts())
                .await
                .map(map_stage_outcome)
        })
        .await
    }

    pub async fn restore_conversation_edit(&self) -> Result<ThreadConversationEditRestoreOutcome> {
        self.enqueue_idle_history_mutation(move |thread| async move {
            thread
                .client
                .inner
                .state
                .restore_conversation_edit_atomic(&thread.id)
                .await
                .map(map_restore_outcome)
        })
        .await
    }

    pub(super) fn enqueue_idle_history_mutation<T, F, Fut>(
        &self,
        operation: F,
    ) -> BoxFuture<'static, Result<T>>
    where
        T: Send + 'static,
        F: FnOnce(Thread) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T>> + Send + 'static,
    {
        let thread = self.clone();
        Box::pin(async move {
            let runtime = thread.client.inner.runtime.clone();
            let admission = runtime.begin_admission().await?;
            let reservation = runtime.reserve_idle_mutation(&thread.id)?;
            let (result_tx, result_rx) = oneshot::channel();
            runtime.spawn(async move {
                drop(admission);
                let result = async {
                    let _reservation = reservation.acquire().await?;
                    operation(thread).await
                }
                .await;
                let _ = result_tx.send(result);
            });
            result_rx.await.map_err(|_| {
                Error::Message(
                    "accepted Thread history mutation ended without a result".to_string(),
                )
            })?
        })
    }
}

impl ThreadEditableDraft {
    fn into_store_parts(self) -> Vec<ConversationDraftPart> {
        self.parts
            .into_iter()
            .map(|part| match part {
                ThreadEditableDraftPart::Text { text } => ConversationDraftPart::Text { text },
                ThreadEditableDraftPart::Image {
                    input: ImageInput::LocalPath(path),
                } => ConversationDraftPart::LocalImage {
                    path: path.to_string_lossy().into_owned(),
                },
                ThreadEditableDraftPart::Image {
                    input: ImageInput::ImageUrl(url),
                } => ConversationDraftPart::ImageUrl { url },
            })
            .collect()
    }
}

fn map_history_editing_facts(facts: HistoryEditingFacts) -> ThreadHistoryEditingState {
    ThreadHistoryEditingState {
        eligibility: match facts.eligibility {
            HistoryEditingEligibility::Eligible => ThreadHistoryEditingEligibility::Eligible,
            HistoryEditingEligibility::Unavailable(reason) => {
                ThreadHistoryEditingEligibility::Unavailable(map_history_unavailable(reason))
            }
        },
        staged: facts.staged.map(|staged| match staged {
            HistoryEditingRevertFacts::WorkspaceUndo {
                boundary_seq,
                hidden_entry_count,
            } => ThreadHistoryEditingStaged::WorkspaceUndo {
                boundary_message_seq: boundary_seq,
                hidden_entry_count,
            },
            HistoryEditingRevertFacts::ConversationEdit {
                boundary_seq,
                hidden_entry_count,
                draft,
            } => ThreadHistoryEditingStaged::ConversationEdit {
                boundary_message_seq: boundary_seq,
                hidden_entry_count,
                draft: map_draft(draft),
            },
        }),
    }
}

fn map_editable_draft_read_outcome(
    outcome: ConversationEditDraftReadOutcome,
) -> ThreadEditableDraftReadOutcome {
    match outcome {
        ConversationEditDraftReadOutcome::Available(read) => {
            ThreadEditableDraftReadOutcome::Available(map_editable_draft_read(read))
        }
        ConversationEditDraftReadOutcome::Unavailable(reason) => {
            ThreadEditableDraftReadOutcome::Unavailable(map_draft_unavailable(reason))
        }
    }
}

fn map_editable_draft_read(read: ConversationEditDraftRead) -> ThreadEditableDraftRead {
    ThreadEditableDraftRead {
        message_seq: read.session_seq,
        draft: map_draft(read.draft),
        fidelity: match read.fidelity {
            ConversationEditDraftFidelity::Exact => ThreadEditableDraftFidelity::Exact,
            ConversationEditDraftFidelity::BestEffort => ThreadEditableDraftFidelity::BestEffort,
        },
    }
}

fn map_draft(parts: Vec<ConversationDraftPart>) -> ThreadEditableDraft {
    ThreadEditableDraft {
        parts: parts
            .into_iter()
            .map(|part| match part {
                ConversationDraftPart::Text { text } => ThreadEditableDraftPart::Text { text },
                ConversationDraftPart::LocalImage { path } => ThreadEditableDraftPart::Image {
                    input: ImageInput::LocalPath(PathBuf::from(path)),
                },
                ConversationDraftPart::ImageUrl { url } => ThreadEditableDraftPart::Image {
                    input: ImageInput::ImageUrl(url),
                },
            })
            .collect(),
    }
}

fn map_history_unavailable(reason: HistoryEditingUnavailable) -> ThreadHistoryEditingUnavailable {
    match reason {
        HistoryEditingUnavailable::SessionNotFound => {
            ThreadHistoryEditingUnavailable::ThreadNotFound
        }
        HistoryEditingUnavailable::UnsupportedSource => {
            ThreadHistoryEditingUnavailable::UnsupportedSource
        }
        HistoryEditingUnavailable::ChildThread => ThreadHistoryEditingUnavailable::ChildThread,
        HistoryEditingUnavailable::AgentChildThread => {
            ThreadHistoryEditingUnavailable::AgentChildThread
        }
        HistoryEditingUnavailable::SideConversation => {
            ThreadHistoryEditingUnavailable::SideConversation
        }
        HistoryEditingUnavailable::RuntimeBindingMissing => {
            ThreadHistoryEditingUnavailable::RuntimeBindingMissing
        }
        HistoryEditingUnavailable::RuntimeBindingUnresolved => {
            ThreadHistoryEditingUnavailable::RuntimeBindingUnresolved
        }
        HistoryEditingUnavailable::RuntimeBindingNotNative => {
            ThreadHistoryEditingUnavailable::RuntimeBindingNotNative
        }
        HistoryEditingUnavailable::RuntimeBindingReadOnly => {
            ThreadHistoryEditingUnavailable::RuntimeBindingReadOnly
        }
        HistoryEditingUnavailable::CorruptSessionMetadata => {
            ThreadHistoryEditingUnavailable::CorruptThreadMetadata
        }
    }
}

fn map_draft_unavailable(
    reason: ConversationEditDraftUnavailable,
) -> ThreadEditableDraftUnavailable {
    match reason {
        ConversationEditDraftUnavailable::SessionNotFound => {
            ThreadEditableDraftUnavailable::ThreadNotFound
        }
        ConversationEditDraftUnavailable::MessageNotFound => {
            ThreadEditableDraftUnavailable::MessageNotFound
        }
        ConversationEditDraftUnavailable::NotUserMessage => {
            ThreadEditableDraftUnavailable::NotUserMessage
        }
        ConversationEditDraftUnavailable::CorruptMessage => {
            ThreadEditableDraftUnavailable::CorruptMessage
        }
        ConversationEditDraftUnavailable::CorruptMetadata => {
            ThreadEditableDraftUnavailable::CorruptMetadata
        }
        ConversationEditDraftUnavailable::CorruptEditableInput => {
            ThreadEditableDraftUnavailable::CorruptEditableInput
        }
        ConversationEditDraftUnavailable::EditableInputMismatch => {
            ThreadEditableDraftUnavailable::EditableInputMismatch
        }
        ConversationEditDraftUnavailable::NoEditableInput => {
            ThreadEditableDraftUnavailable::NoEditableInput
        }
        ConversationEditDraftUnavailable::EmptyReplacement => {
            ThreadEditableDraftUnavailable::EmptyReplacement
        }
    }
}

fn map_edit_unavailable(reason: ConversationEditUnavailable) -> ThreadConversationEditUnavailable {
    match reason {
        ConversationEditUnavailable::HistoryEditing(reason) => {
            ThreadConversationEditUnavailable::HistoryEditing(map_history_unavailable(reason))
        }
        ConversationEditUnavailable::Draft(reason) => {
            ThreadConversationEditUnavailable::Draft(map_draft_unavailable(reason))
        }
    }
}

fn map_conflict(reason: ConversationEditConflict) -> ThreadConversationEditConflict {
    match reason {
        ConversationEditConflict::WorkspaceUndoStaged => {
            ThreadConversationEditConflict::WorkspaceUndoStaged
        }
        ConversationEditConflict::ConversationEditStaged => {
            ThreadConversationEditConflict::ConversationEditStaged
        }
        ConversationEditConflict::ConcurrentMetadataChange => {
            ThreadConversationEditConflict::ConcurrentMetadataChange
        }
    }
}

fn map_stage_outcome(outcome: ConversationEditStageOutcome) -> ThreadConversationEditStageOutcome {
    match outcome {
        ConversationEditStageOutcome::Staged => ThreadConversationEditStageOutcome::Staged,
        ConversationEditStageOutcome::AlreadyStaged => {
            ThreadConversationEditStageOutcome::AlreadyStaged
        }
        ConversationEditStageOutcome::Unchanged => ThreadConversationEditStageOutcome::Unchanged,
        ConversationEditStageOutcome::Unavailable(reason) => {
            ThreadConversationEditStageOutcome::Unavailable(map_edit_unavailable(reason))
        }
        ConversationEditStageOutcome::Conflict(reason) => {
            ThreadConversationEditStageOutcome::Conflict(map_conflict(reason))
        }
    }
}

fn map_restore_outcome(
    outcome: ConversationEditRestoreOutcome,
) -> ThreadConversationEditRestoreOutcome {
    match outcome {
        ConversationEditRestoreOutcome::Restored(draft) => {
            ThreadConversationEditRestoreOutcome::Restored(map_draft(draft))
        }
        ConversationEditRestoreOutcome::SessionNotFound => {
            ThreadConversationEditRestoreOutcome::ThreadNotFound
        }
        ConversationEditRestoreOutcome::NotStaged => {
            ThreadConversationEditRestoreOutcome::NotStaged
        }
        ConversationEditRestoreOutcome::Conflict(reason) => {
            ThreadConversationEditRestoreOutcome::Conflict(map_conflict(reason))
        }
    }
}

#[cfg(test)]
mod tests;
