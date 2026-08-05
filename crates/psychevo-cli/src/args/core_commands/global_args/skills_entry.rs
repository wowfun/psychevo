use clap::Parser;

use super::skills::SkillsCommand;

#[derive(Debug, Parser)]
pub(crate) struct SkillsArgs {
    #[command(subcommand)]
    pub(crate) command: Option<SkillsCommand>,
}
