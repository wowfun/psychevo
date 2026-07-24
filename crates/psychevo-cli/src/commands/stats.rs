use std::env;
use std::process::ExitCode;

use anyhow::Result;
use psychevo_runtime::state::StateRuntime;
use psychevo_runtime::{stats::usage_stats, types::StatsOptions};
use serde_json::Value;

use crate::args::StatsArgs;
use crate::env::{ensure_home_initialized, inherited_env, resolve_psychevo_home, resolve_state_db};

pub(crate) fn run_stats_command(args: StatsArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let cwd = args.dir.clone().unwrap_or_else(|| cwd.clone());
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    ensure_home_initialized(&home)?;
    let db_path = resolve_state_db(&env_map, &home, &cwd)?;
    let report = usage_stats(StatsOptions {
        state: StateRuntime::open(&db_path)?,
        cwd,
        all: args.all,
        days: args.days,
        limit: args.limit,
    })?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human_report(&report);
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn print_human_report(report: &Value) {
    let scope = report.get("scope").unwrap_or(&Value::Null);
    let totals = report.get("totals").unwrap_or(&Value::Null);
    let scope_label = if scope.get("all").and_then(Value::as_bool) == Some(true) {
        "all sessions".to_string()
    } else {
        scope
            .get("cwd")
            .and_then(Value::as_str)
            .map(|cwd| format!("cwd {cwd}"))
            .unwrap_or_else(|| "current cwd".to_string())
    };
    println!("Stats for {scope_label}");
    println!(
        "Sessions: {}  Messages: {}",
        int(totals, "sessions"),
        int(totals, "messages")
    );
    println!(
        "Tokens: {} total  {} input  {} output  {} reasoning  {} cache read  {} cache write",
        int(totals, "reported_total_tokens"),
        int(totals, "billable_input_tokens"),
        int(totals, "billable_output_tokens"),
        int(totals, "reasoning_tokens"),
        int(totals, "cache_read_tokens"),
        int(totals, "cache_write_tokens")
    );
    println!(
        "Estimated cost: {}",
        format_nanodollars(int(totals, "estimated_cost_nanodollars"))
    );
    let estimated = int(totals, "estimated_priced_messages");
    let free = int(totals, "free_priced_messages");
    let included = int(totals, "included_priced_messages");
    if estimated + free + included > 0 {
        println!(
            "Priced assistant messages: {estimated} estimated  {free} free  {included} included"
        );
    }
    if int(totals, "unknown_priced_messages") > 0 {
        println!(
            "Unknown-priced assistant messages: {}",
            int(totals, "unknown_priced_messages")
        );
    }
    print_table(
        "Provider / model",
        report.get("provider_models"),
        |row| {
            format!(
                "{} / {}",
                row.get("provider").and_then(Value::as_str).unwrap_or("-"),
                row.get("model").and_then(Value::as_str).unwrap_or("-")
            )
        },
        |row| {
            format!(
                "{} tokens, {}",
                int(row, "reported_total_tokens"),
                format_nanodollars(int(row, "estimated_cost_nanodollars"))
            )
        },
    );
    print_table(
        "Top tools",
        report.get("top_tools"),
        |row| {
            row.get("tool")
                .and_then(Value::as_str)
                .unwrap_or("-")
                .to_string()
        },
        |row| format!("{} calls", int(row, "calls")),
    );
}

pub(crate) fn print_table(
    title: &str,
    rows: Option<&Value>,
    left: impl Fn(&Value) -> String,
    right: impl Fn(&Value) -> String,
) {
    let rows = rows.and_then(Value::as_array).cloned().unwrap_or_default();
    if rows.is_empty() {
        return;
    }
    println!();
    println!("{title}:");
    for row in rows {
        println!("  {:<36} {}", left(&row), right(&row));
    }
}

pub(crate) fn int(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}

pub(crate) fn format_nanodollars(value: i64) -> String {
    if value == 0 {
        "$0.000000".to_string()
    } else {
        format!("${:.6}", value as f64 / 1_000_000_000.0)
    }
}
