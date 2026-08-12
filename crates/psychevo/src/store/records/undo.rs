use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConversationDraftPart {
    Text { text: String },
    LocalImage { path: String },
    ImageUrl { url: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionRevertKind {
    WorkspaceUndo {
        original_snapshot: String,
    },
    ConversationEdit {
        boundary_message_id: String,
        draft: Vec<ConversationDraftPart>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRevertState {
    pub start_seq: i64,
    pub kind: SessionRevertKind,
}

#[derive(Debug, Clone, Copy)]
pub struct NativeSessionForkInput<'a> {
    pub source_session_id: &'a str,
    pub before_session_seq: Option<i64>,
}

impl SessionRevertState {
    pub fn workspace_undo(start_seq: i64, original_snapshot: String) -> Self {
        Self {
            start_seq,
            kind: SessionRevertKind::WorkspaceUndo { original_snapshot },
        }
    }

    pub fn conversation_edit(
        start_seq: i64,
        boundary_message_id: String,
        draft: Vec<ConversationDraftPart>,
    ) -> Self {
        Self {
            start_seq,
            kind: SessionRevertKind::ConversationEdit {
                boundary_message_id,
                draft,
            },
        }
    }

    pub fn original_snapshot(&self) -> Option<&str> {
        match &self.kind {
            SessionRevertKind::WorkspaceUndo { original_snapshot } => Some(original_snapshot),
            SessionRevertKind::ConversationEdit { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoTarget {
    pub seq: i64,
    pub prompt: String,
    pub snapshot: Option<String>,
}
