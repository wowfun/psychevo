#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) struct RunningTurn {
    pub(crate) session_id: Option<String>,
    pub(crate) control: RunControlHandle,
    pub(crate) selector: Option<GatewayThreadSelector>,
    pub(crate) turn_id: Option<String>,
    pub(crate) events: RunningTurnEvents,
    pub(crate) task: RunningTask,
}

pub(crate) enum RunningTurnEvents {
    Gateway(mpsc::UnboundedReceiver<GatewayEvent>),
    Runtime(mpsc::UnboundedReceiver<RunStreamEvent>),
}

pub(crate) enum TuiLiveEvent {
    Gateway(Box<GatewayEvent>),
    Runtime(RunStreamEvent),
}

impl RunningTurnEvents {
    pub(crate) fn try_recv(
        &mut self,
    ) -> std::result::Result<TuiLiveEvent, mpsc::error::TryRecvError> {
        match self {
            Self::Gateway(rx) => rx
                .try_recv()
                .map(|event| TuiLiveEvent::Gateway(Box::new(event))),
            Self::Runtime(rx) => rx.try_recv().map(TuiLiveEvent::Runtime),
        }
    }
}

impl From<RunStreamEvent> for TuiLiveEvent {
    fn from(event: RunStreamEvent) -> Self {
        Self::Runtime(event)
    }
}

impl From<GatewayEvent> for TuiLiveEvent {
    fn from(event: GatewayEvent) -> Self {
        Self::Gateway(Box::new(event))
    }
}

pub(crate) struct TuiApprovalRequest {
    pub(crate) session_id: Option<String>,
    pub(crate) request: PermissionApprovalRequest,
    pub(crate) response: oneshot::Sender<PermissionApprovalDecision>,
}

pub(crate) struct AuxiliaryShellTask {
    pub(crate) session_id: Option<String>,
    pub(crate) control: RunControlHandle,
    pub(crate) rx: mpsc::UnboundedReceiver<RunStreamEvent>,
    pub(crate) task: JoinHandle<psychevo_runtime::Result<psychevo_runtime::UserShellResult>>,
}

pub(crate) struct AuxiliaryAgentTask {
    pub(crate) session_id: Option<String>,
    pub(crate) child_session_id: Option<String>,
    pub(crate) visible_live: bool,
    pub(crate) pending_unowned_live_events: Vec<RunStreamEvent>,
    pub(crate) control: RunControlHandle,
    pub(crate) events: RunningTurnEvents,
    pub(crate) task: JoinHandle<psychevo_runtime::Result<psychevo_runtime::RunResult>>,
}

pub(crate) enum RunningTask {
    Agent(JoinHandle<psychevo_runtime::Result<psychevo_runtime::RunResult>>),
    UserShell(JoinHandle<psychevo_runtime::Result<psychevo_runtime::UserShellResult>>),
}

pub(crate) enum RunningCompletion {
    Agent(
        Box<
            std::result::Result<
                psychevo_runtime::Result<psychevo_runtime::RunResult>,
                tokio::task::JoinError,
            >,
        >,
    ),
    UserShell(
        std::result::Result<
            psychevo_runtime::Result<psychevo_runtime::UserShellResult>,
            tokio::task::JoinError,
        >,
    ),
}

impl RunningTask {
    pub(crate) fn is_finished(&self) -> bool {
        match self {
            Self::Agent(task) => task.is_finished(),
            Self::UserShell(task) => task.is_finished(),
        }
    }

    #[cfg(test)]
    pub(crate) fn abort(&self) {
        match self {
            Self::Agent(task) => task.abort(),
            Self::UserShell(task) => task.abort(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum QueuedInput {
    Prompt {
        session_id: Option<String>,
        prompt: String,
        display_prompt: String,
        images: Vec<PendingImageAttachment>,
        sequence: u64,
    },
    Shell {
        session_id: Option<String>,
        command: String,
        sequence: u64,
    },
    Compact {
        session_id: Option<String>,
        instructions: Option<String>,
        command_echo: String,
        sequence: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingSteerInput {
    pub(crate) id: PendingInputId,
    pub(crate) session_id: Option<String>,
    pub(crate) prompt: String,
    pub(crate) display_prompt: String,
    pub(crate) images: Vec<PendingImageAttachment>,
    pub(crate) sequence: u64,
}

pub(crate) fn queued_input_session_id(input: &QueuedInput) -> Option<&str> {
    match input {
        QueuedInput::Prompt { session_id, .. } | QueuedInput::Shell { session_id, .. } => {
            session_id.as_deref()
        }
        QueuedInput::Compact { session_id, .. } => session_id.as_deref(),
    }
}

pub(crate) fn queued_input_sequence(input: &QueuedInput) -> u64 {
    match input {
        QueuedInput::Prompt { sequence, .. }
        | QueuedInput::Shell { sequence, .. }
        | QueuedInput::Compact { sequence, .. } => *sequence,
    }
}

pub(crate) fn queued_input_text(input: QueuedInput) -> String {
    match input {
        QueuedInput::Prompt { display_prompt, .. } => display_prompt,
        QueuedInput::Shell { command, .. } => format!("!{command}"),
        QueuedInput::Compact { command_echo, .. } => command_echo,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingImageAttachment {
    pub(crate) placeholder: String,
    pub(crate) image: ImageInput,
}

pub(crate) fn image_placeholder(index: usize) -> String {
    format!("[Image #{index}]")
}

pub(crate) fn next_image_placeholder(attachments: &[PendingImageAttachment], text: &str) -> String {
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

pub(crate) fn prompt_without_image_placeholders(
    prompt: &str,
    attachments: &[PendingImageAttachment],
) -> String {
    let mut text = prompt.to_string();
    for attachment in attachments {
        text = text.replace(&attachment.placeholder, "");
    }
    normalize_prompt_text(&text)
}

pub(crate) fn normalize_prompt_text(text: &str) -> String {
    text.lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn attachment_metadata_text(
    attachments: &[PendingImageAttachment],
    workdir: &Path,
) -> Option<String> {
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

pub(crate) fn display_image_source(image: &ImageInput, workdir: &Path) -> String {
    match image {
        ImageInput::LocalPath(path) => path
            .strip_prefix(workdir)
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| path.display().to_string()),
        ImageInput::ImageUrl(url) => url.clone(),
    }
}

pub(crate) fn prompt_display_metadata(
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
