#[derive(Debug, Parser)]
pub(crate) struct SkillsArgs {
    #[command(subcommand)]
    pub(crate) command: Option<SkillsCommand>,
}
