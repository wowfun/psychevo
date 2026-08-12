use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

pub struct ChildSessionSnapshotInput<'a> {
    pub parent_session_id: &'a str,
    pub cwd: &'a Path,
    pub source: &'a str,
    pub model: &'a str,
    pub provider: &'a str,
    pub metadata: Option<Value>,
    pub inherited_message_metadata: Value,
    pub boundary_text: &'a str,
    pub runtime_binding: Option<ChildSessionRuntimeBindingSnapshotInput<'a>>,
}

pub(crate) struct WorkspaceSessionSnapshotInput<'a> {
    pub cwd: &'a Path,
    pub workspace_id: &'a str,
    pub workspace_roots: &'a [String],
    pub workspace_revision: i64,
    pub source: &'a str,
    pub model: &'a str,
    pub provider: &'a str,
    pub metadata: Option<Value>,
}

pub struct ChildSessionRuntimeBindingSnapshotInput<'a> {
    pub expected_binding_revision: i64,
    pub expected_control_revision: i64,
    pub effective_controls: &'a BTreeMap<String, Value>,
}
