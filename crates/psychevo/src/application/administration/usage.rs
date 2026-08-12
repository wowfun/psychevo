use serde::{Deserialize, Serialize};

pub type ThreadUndoResult = crate::types::SessionUndoResult;
pub type ThreadRedoResult = crate::types::SessionRedoResult;
pub type ThreadUsageSummary = crate::types::SessionUsageSummary;
pub type UsageOverview = crate::types::UsageReadResult;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsageObservation {
    pub used_tokens: Option<u64>,
    pub context_limit: Option<u64>,
    pub estimated_cost_nanodollars: Option<i64>,
}
