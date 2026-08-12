use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationTaskInput {
    pub id: Option<String>,
    pub cwd: String,
    pub kind: AutomationTaskKind,
    pub target_thread_id: Option<String>,
    pub title: String,
    pub prompt: String,
    pub schedule: Value,
    pub enabled: bool,
    pub execution: Value,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub source_key: Option<String>,
    pub next_run_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationTaskRecord {
    pub id: String,
    pub cwd: String,
    pub kind: AutomationTaskKind,
    pub target_thread_id: Option<String>,
    pub title: String,
    pub prompt: String,
    pub schedule: Value,
    pub enabled: bool,
    pub execution: Value,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub source_key: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub last_run_at_ms: Option<i64>,
    pub next_run_at_ms: Option<i64>,
    pub last_status: Option<AutomationRunStatus>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationRunRecord {
    pub id: String,
    pub automation_id: String,
    pub trigger: String,
    pub status: AutomationRunStatus,
    pub started_at_ms: i64,
    pub completed_at_ms: Option<i64>,
    pub thread_id: Option<String>,
    pub source_key: Option<String>,
    pub error: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationRunRecoveryCandidate {
    pub task: AutomationTaskRecord,
    pub run: AutomationRunRecord,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationRunFinishInput<'a> {
    pub run_id: &'a str,
    pub status: AutomationRunTerminalStatus,
    pub thread_id: Option<&'a str>,
    pub source_key: Option<&'a str>,
    pub error: Option<&'a str>,
    pub metadata: Option<Value>,
    pub next_run_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationTaskKind {
    Project,
    ThreadHeartbeat,
}

impl AutomationTaskKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::ThreadHeartbeat => "thread_heartbeat",
        }
    }

    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "project" => Some(Self::Project),
            "thread_heartbeat" => Some(Self::ThreadHeartbeat),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationRunStatus {
    Running,
    Completed,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationRunTerminalStatus {
    Completed,
    Failed,
    Interrupted,
}

impl AutomationRunTerminalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

impl AutomationRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "running" => Some(Self::Running),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "interrupted" => Some(Self::Interrupted),
            _ => None,
        }
    }
}
