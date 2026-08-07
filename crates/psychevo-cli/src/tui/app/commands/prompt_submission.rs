use crate::tui::SlashCommand;

#[path = "prompt_submission/command_submission.rs"]
mod command_submission;
#[path = "prompt_submission/pending_edits.rs"]
mod pending_edits;
#[path = "prompt_submission/prompt_queue.rs"]
mod prompt_queue;

pub(crate) enum SubmittedSlashInput {
    Command(SlashCommand),
    ExtensionCommand { command: String, args: Vec<String> },
    PassThroughPrompt(String),
    NotSlash,
}
