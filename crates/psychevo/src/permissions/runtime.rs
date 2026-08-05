mod actions;
mod approval;
mod core;
mod exec_matching;
mod grants_lifecycle;
mod policy;
mod profiles;
mod protected_paths;
mod state;
mod tool;

#[cfg(test)]
mod sandbox_approval_tests;

pub(crate) use exec_matching::exec_prefix_matches;
pub(crate) use state::PermissionRuntime;
#[cfg(test)]
pub(crate) use state::{ApprovalLifecycleEvent, PermissionDecisionView as PermissionDecision};
