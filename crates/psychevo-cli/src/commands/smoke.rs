use std::process::ExitCode;

use anyhow::Result;
use psychevo_ai::Outcome;
use psychevo_runtime::{SmokeOptions, StateRuntime, run_smoke};

use crate::args::SmokeArgs;

pub(crate) async fn run_smoke_command(args: SmokeArgs) -> Result<ExitCode> {
    let state = StateRuntime::open(&args.db)?;
    let result = run_smoke(SmokeOptions {
        state,
        workdir: args.workdir,
        session: args.session,
        prompt: args.prompt,
        max_context_messages: args.max_context_messages,
        control: args.control.into(),
        reset: args.reset,
    })
    .await?;

    println!("session_id: {}", result.session_id);
    println!("outcome: {}", result.outcome.as_str());
    println!("final_answer: {}", result.final_answer);
    println!("db: {}", result.db_path.display());
    println!("workdir: {}", result.workdir.display());

    let success = if let Some(expected) = result.expected_control_outcome {
        result.outcome == expected
    } else {
        result.outcome == Outcome::Normal && result.tool_failures == 0
    };
    Ok(if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}
