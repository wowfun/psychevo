use serde::Serialize;

use crate::live::LiveEnvMode;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ProfileKind {
    Ci,
    Cd,
}

impl std::fmt::Display for ProfileKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ci => f.write_str("ci"),
            Self::Cd => f.write_str("cd"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WorkflowProfile {
    pub(crate) id: &'static str,
    pub(crate) description: &'static str,
    pub(crate) kind: ProfileKind,
    pub(crate) live: bool,
    pub(crate) artifact_only: bool,
    pub(crate) steps: &'static [WorkflowStep],
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WorkflowStep {
    pub(crate) id: &'static str,
    pub(crate) description: &'static str,
    pub(crate) action: WorkflowStepAction,
    pub(crate) live: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum WorkflowStepAction {
    Command(&'static [&'static str]),
    SingleProviderLive,
    TuiVhsDemo,
    WorkbenchVisual,
}

impl WorkflowStepAction {
    pub(crate) fn command_for_plan(self) -> &'static [&'static str] {
        match self {
            Self::Command(command) => command,
            Self::SingleProviderLive => &["xtask-internal", "single-provider-live"],
            Self::TuiVhsDemo => &["xtask-internal", "tui-vhs-demo"],
            Self::WorkbenchVisual => &["xtask-internal", "workbench-visual"],
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ProfileSummary {
    pub(crate) id: &'static str,
    pub(crate) description: &'static str,
    pub(crate) kind: ProfileKind,
    pub(crate) live: bool,
    pub(crate) artifact_only: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProfileListOutput<'a> {
    pub(crate) profiles: &'a [ProfileSummary],
}

#[derive(Debug, Serialize)]
pub(crate) struct PlanOutput {
    pub(crate) profile: ProfileSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) environment: Option<CiEnvironmentOutput>,
    pub(crate) artifact_root: Option<String>,
    pub(crate) steps: Vec<StepPlanOutput>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StepPlanOutput {
    pub(crate) id: &'static str,
    pub(crate) description: &'static str,
    pub(crate) command: Vec<String>,
    pub(crate) live: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct RunOutput {
    pub(crate) profile: ProfileSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) environment: Option<CiEnvironmentOutput>,
    pub(crate) artifact_root: String,
    pub(crate) steps: Vec<StepRunOutput>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StepRunOutput {
    pub(crate) id: &'static str,
    pub(crate) description: &'static str,
    pub(crate) command: Vec<String>,
    pub(crate) live: bool,
    pub(crate) status: StepStatus,
    pub(crate) exit_code: Option<i32>,
    pub(crate) log_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum StepStatus {
    Passed,
    Failed,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct CiEnvironmentOutput {
    pub(crate) mode: LiveEnvMode,
}

pub(crate) fn profile_summary(profile: &WorkflowProfile) -> ProfileSummary {
    ProfileSummary {
        id: profile.id,
        description: profile.description,
        kind: profile.kind,
        live: profile.live,
        artifact_only: profile.artifact_only,
    }
}
