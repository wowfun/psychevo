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
    Prompt {
        prompt: String,
        display_prompt: String,
        images: Vec<PendingImageAttachment>,
    },
    Shell(String),
}

fn queued_input_text(input: QueuedInput) -> String {
    match input {
        QueuedInput::Prompt { display_prompt, .. } => display_prompt,
        QueuedInput::Shell(command) => format!("!{command}"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingImageAttachment {
    placeholder: String,
    image: ImageInput,
}

fn image_placeholder(index: usize) -> String {
    format!("[Image #{index}]")
}

fn next_image_placeholder(attachments: &[PendingImageAttachment], text: &str) -> String {
    let mut index = attachments.len() + 1;
    loop {
        let placeholder = image_placeholder(index);
        if !text.contains(&placeholder)
            && attachments
                .iter()
                .all(|attachment| attachment.placeholder != placeholder)
        {
            return placeholder;
        }
        index += 1;
    }
}

fn prompt_without_image_placeholders(
    prompt: &str,
    attachments: &[PendingImageAttachment],
) -> String {
    let mut text = prompt.to_string();
    for attachment in attachments {
        text = text.replace(&attachment.placeholder, "");
    }
    normalize_prompt_text(&text)
}

fn normalize_prompt_text(text: &str) -> String {
    text.lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn attachment_metadata_text(attachments: &[PendingImageAttachment], workdir: &Path) -> Option<String> {
    if attachments.is_empty() {
        return None;
    }
    let mut lines = vec!["attachments".to_string()];
    for (index, attachment) in attachments.iter().enumerate() {
        lines.push(format!(
            "image {}: {}",
            index + 1,
            display_image_source(&attachment.image, workdir)
        ));
    }
    Some(lines.join("\n"))
}

fn display_image_source(image: &ImageInput, workdir: &Path) -> String {
    match image {
        ImageInput::LocalPath(path) => path
            .strip_prefix(workdir)
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| path.display().to_string()),
        ImageInput::ImageUrl(url) => url.clone(),
    }
}

fn prompt_display_metadata(
    content_text: String,
    attachments: &[PendingImageAttachment],
    workdir: &Path,
) -> Option<PromptDisplayMetadata> {
    (!attachments.is_empty()).then(|| PromptDisplayMetadata {
        content_text,
        attachments: attachments
            .iter()
            .map(|attachment| PromptAttachmentDisplay {
                kind: "image".to_string(),
                placeholder: attachment.placeholder.clone(),
                source: display_image_source(&attachment.image, workdir),
            })
            .collect(),
    })
}
