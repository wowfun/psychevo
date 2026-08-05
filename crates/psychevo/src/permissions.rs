mod runtime;
#[cfg(test)]
pub(crate) use runtime::{ApprovalLifecycleEvent, PermissionDecision};
pub(crate) use runtime::{PermissionRuntime, exec_prefix_matches};
#[path = "permissions/shell.rs"]
mod shell;
pub(crate) use shell::{shell_command_tokens, shell_has_untracked_background};
#[path = "permissions/rules.rs"]
mod rules;
