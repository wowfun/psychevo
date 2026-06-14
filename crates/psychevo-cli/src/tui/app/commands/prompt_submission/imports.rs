#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) enum SubmittedSlashInput {
    Command(SlashCommand),
    PassThroughPrompt(String),
    NotSlash,
}
