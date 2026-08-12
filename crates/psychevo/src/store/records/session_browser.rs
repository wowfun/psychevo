use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::store_workspaces::WorkspaceRecord;
use crate::types::SessionSummary;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SessionListProjection {
    pub(crate) summary: SessionSummary,
    pub(crate) first_user_text: Option<String>,
    pub(crate) metadata: Option<Value>,
    pub(crate) runtime_backend_kind: Option<String>,
    pub(crate) runtime_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SessionListCursor {
    pub(crate) updated_at_ms: i64,
    pub(crate) id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SessionSummaryPage {
    pub(crate) summaries: Vec<SessionSummary>,
    pub(crate) next_cursor: Option<SessionListCursor>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SessionListProjectionPage {
    pub(crate) sessions: Vec<SessionListProjection>,
    pub(crate) next_cursor: Option<SessionListCursor>,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionBrowserRequest<'a> {
    pub(crate) cwd: Option<&'a str>,
    pub(crate) archived: bool,
    pub(crate) cursor_workspace_id: Option<&'a str>,
    pub(crate) cursor_offset: usize,
    pub(crate) limit: usize,
    pub(crate) recent_since_ms: i64,
    pub(crate) include_session_ids: &'a [String],
    pub(crate) active_session_ids: &'a [String],
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SessionBrowserWorkspaceProjection {
    pub(crate) workspace: WorkspaceRecord,
    pub(crate) sessions: Vec<SessionListProjection>,
    pub(crate) hidden_count: usize,
    pub(crate) next_offset: Option<usize>,
}
