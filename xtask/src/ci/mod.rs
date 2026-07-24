pub(crate) mod artifacts;
mod desktop_manifest_parity;
mod desktop_visual;
mod model;
pub(crate) mod process;
mod profiles;
pub(crate) mod retention;
mod runner;
mod surface_profile;
mod tui_capture;
mod workbench_visual;

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Subcommand;
use serde::Serialize;

use self::model::{PlanOutput, ProfileListOutput, RunOutput};
use crate::live::LiveEnvMode;

#[derive(Debug, Subcommand)]
pub(crate) enum CiCommand {
    List {
        #[arg(long)]
        json: bool,
    },
    Plan {
        #[arg(long)]
        profile: String,
        #[arg(long, value_enum)]
        live_env: Option<LiveEnvMode>,
        #[arg(long)]
        json: bool,
    },
    Run {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        json: bool,
        #[arg(long, help = "Explicitly allow live provider or live service steps")]
        live: bool,
        #[arg(long, value_enum)]
        live_env: Option<LiveEnvMode>,
        #[arg(long)]
        artifact_root: Option<PathBuf>,
    },
}

pub(crate) fn run(command: CiCommand, root: &Path) -> Result<()> {
    match command {
        CiCommand::List { json } => {
            let profiles = profiles::profile_summaries();
            if json {
                print_json(&ProfileListOutput {
                    profiles: &profiles,
                })
            } else {
                for profile in profiles {
                    println!("{}\t{}\t{}", profile.id, profile.kind, profile.description);
                }
                Ok(())
            }
        }
        CiCommand::Plan {
            profile,
            live_env,
            json,
        } => {
            let plan = profiles::plan_profile(&profile, live_env)?;
            if json {
                print_json(&plan)
            } else {
                print_plan(&plan);
                Ok(())
            }
        }
        CiCommand::Run {
            profile,
            json,
            live,
            live_env,
            artifact_root,
        } => {
            let run = runner::execute_profile(root, &profile, live, live_env, artifact_root)?;
            if json {
                print_json(&run)
            } else {
                print_run_summary(&run);
                Ok(())
            }
        }
    }
}

fn print_plan(plan: &PlanOutput) {
    println!(
        "{}\t{}\t{}",
        plan.profile.id, plan.profile.kind, plan.profile.description
    );
    for step in &plan.steps {
        println!("  {}\t{}", step.id, shell_words(&step.command));
    }
}

fn print_run_summary(run: &RunOutput) {
    println!("artifacts: {}", run.artifact_root);
    for step in &run.steps {
        println!("{}\t{:?}\t{}", step.id, step.status, step.log_path);
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn shell_words(command: &[String]) -> String {
    command.join(" ")
}
