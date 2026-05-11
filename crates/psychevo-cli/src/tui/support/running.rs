struct RunningTurn {
    control: RunControlHandle,
    rx: mpsc::UnboundedReceiver<RunStreamEvent>,
    task: RunningTask,
}

enum RunningTask {
    Agent(JoinHandle<psychevo_runtime::Result<psychevo_runtime::RunResult>>),
    UserShell(JoinHandle<psychevo_runtime::Result<psychevo_runtime::UserShellResult>>),
}

enum RunningCompletion {
    Agent(Box<
        std::result::Result<
            psychevo_runtime::Result<psychevo_runtime::RunResult>,
            tokio::task::JoinError,
        >,
    >),
    UserShell(
        std::result::Result<
            psychevo_runtime::Result<psychevo_runtime::UserShellResult>,
            tokio::task::JoinError,
        >,
    ),
}

impl RunningTask {
    fn is_finished(&self) -> bool {
        match self {
            Self::Agent(task) => task.is_finished(),
            Self::UserShell(task) => task.is_finished(),
        }
    }

    #[cfg(test)]
    fn abort(&self) {
        match self {
            Self::Agent(task) => task.abort(),
            Self::UserShell(task) => task.abort(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum QueuedInput {
    Prompt(String),
    Shell(String),
}

fn queued_input_text(input: QueuedInput) -> String {
    match input {
        QueuedInput::Prompt(prompt) => prompt,
        QueuedInput::Shell(command) => format!("!{command}"),
    }
}
