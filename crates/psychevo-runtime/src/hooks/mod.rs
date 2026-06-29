mod command;
mod config;
mod declarations;
mod identity;
mod output;
mod runtime;
mod state;
mod types;
mod worker;

use std::path::Path;

use serde_json::{Value, json};

use crate::config::deep_merge;

pub use config::{
    hook_metadata_value, hook_runtime_config_from_options,
    hook_runtime_config_with_plugin_sources_from_options, set_hook_enabled_in_profile,
    trust_hook_in_profile,
};
pub use runtime::HookRuntime;
pub use types::*;

pub(crate) const DEFAULT_COMMAND_TIMEOUT_SECS: u64 = 600;
pub(crate) const OUTPUT_LIMIT: usize = 4096;

pub fn agent_hook_source(agent_name: &str, hooks: Option<&Value>) -> Option<HookSourceDescriptor> {
    hooks.map(|hooks| {
        HookSourceDescriptor::new(
            format!("agent:{agent_name}"),
            "agent",
            Some(agent_name.to_string()),
            None,
            hooks.clone(),
        )
    })
}

pub fn run_hook_commands(
    hooks: Option<&Value>,
    event: &str,
    cwd: &Path,
    payload: &Value,
) -> Option<String> {
    let source = HookSourceDescriptor::new(
        "agent:legacy",
        "agent",
        None,
        None,
        hooks.cloned().unwrap_or(Value::Null),
    );
    run_hook_sources(&[source], event, cwd, payload).blocked_reason
}

pub fn run_hook_sources(
    sources: &[HookSourceDescriptor],
    event: &str,
    cwd: &Path,
    payload: &Value,
) -> HookResponse {
    let runtime = HookRuntime::new(
        cwd.to_path_buf(),
        HookRuntimeConfig {
            sources: sources.to_vec(),
            state: HookStateStore::default(),
            bypass_trust: false,
        },
    );
    runtime.run_event(event, payload)
}

pub fn hook_payload(event: &str, source: Option<&HookSourceDescriptor>, payload: Value) -> Value {
    json!({
        "event": event,
        "source": source.map(|source| json!({
            "id": source.source_id,
            "kind": source.source_kind,
            "display_name": source.display_name,
            "path": source.path,
        })),
        "payload": payload,
    })
}

pub fn merge_hook_json_values(base: &mut Value, overlay: Value) {
    deep_merge(base, overlay);
}

#[cfg(test)]
mod tests;
