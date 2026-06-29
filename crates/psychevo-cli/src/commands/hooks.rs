use std::env;
use std::process::ExitCode;

use anyhow::Result;
use clap::CommandFactory;
use serde_json::Value;

use crate::args::{HookKeyArgs, HooksArgs, HooksCommand, HooksListArgs};
use crate::commands::common::base_run_options;
use crate::env::{ensure_home_initialized, inherited_env, resolve_psychevo_home};

pub(crate) fn run_hooks_command(args: HooksArgs) -> Result<ExitCode> {
    let Some(command) = args.command else {
        HooksArgs::command().print_help()?;
        println!();
        return Ok(ExitCode::SUCCESS);
    };

    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    ensure_home_initialized(&home)?;
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    let options = base_run_options(&env_map, &home, &cwd)?;

    match command {
        HooksCommand::List(args) => list_hooks(args, &options, &cwd)?,
        HooksCommand::Trust(args) => {
            let value = psychevo_runtime::hooks::trust_hook_in_profile(&options, &cwd, &args.key)?;
            print_hooks_value(&value, args.json)?;
        }
        HooksCommand::Enable(args) => set_hook_enabled(args, &options, true)?,
        HooksCommand::Disable(args) => set_hook_enabled(args, &options, false)?,
    }

    Ok(ExitCode::SUCCESS)
}

fn list_hooks(
    args: HooksListArgs,
    options: &psychevo_runtime::RunOptions,
    cwd: &std::path::Path,
) -> Result<()> {
    let value = psychevo_runtime::hooks::hook_metadata_value(options, cwd)?;
    print_hooks_value(&value, args.json)
}

fn set_hook_enabled(
    args: HookKeyArgs,
    options: &psychevo_runtime::RunOptions,
    enabled: bool,
) -> Result<()> {
    let value = psychevo_runtime::hooks::set_hook_enabled_in_profile(options, &args.key, enabled)?;
    print_hooks_value(&value, args.json)
}

fn print_hooks_value(value: &Value, json_output: bool) -> Result<()> {
    if json_output {
        println!("{}", serde_json::to_string_pretty(value)?);
        return Ok(());
    }
    if let Some(hooks) = value.get("hooks").and_then(Value::as_array) {
        if hooks.is_empty() {
            println!("No hooks found.");
            return Ok(());
        }
        for hook in hooks {
            let key = hook.get("key").and_then(Value::as_str).unwrap_or("hook");
            let event = hook.get("event").and_then(Value::as_str).unwrap_or("");
            let handler = hook
                .get("handler_type")
                .and_then(Value::as_str)
                .unwrap_or("");
            let source = hook
                .get("source_kind")
                .and_then(Value::as_str)
                .unwrap_or("");
            let trust = hook
                .get("trust_status")
                .and_then(Value::as_str)
                .unwrap_or("");
            let enabled = hook.get("enabled").and_then(Value::as_bool).unwrap_or(true);
            let skipped = hook
                .get("skipped_reason")
                .and_then(Value::as_str)
                .map(|reason| format!(" skipped:{reason}"))
                .unwrap_or_default();
            println!(
                "{key} {event} {handler} [{source}] {trust} {}{skipped}",
                if enabled { "enabled" } else { "disabled" }
            );
        }
        return Ok(());
    }
    if let Some(key) = value.get("hook").and_then(Value::as_str) {
        println!("{key}");
        return Ok(());
    }
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
