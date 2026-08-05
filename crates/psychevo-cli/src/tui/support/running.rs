use futures::FutureExt;
#[cfg(test)]
use psychevo::{Application, TurnRequest};
use psychevo::{
    ApprovalHandler, ImageInput, InteractionResponse, PromptAttachmentDisplay,
    PromptDisplayMetadata, ShellCommandControl, ShellCommandEvent, ShellCommandRequest,
    ShellCommandResult, TurnAdmissionCancellation, TurnEvent, TurnEventStream, TurnHandle,
    TurnResult,
    application::{
        ClarifyResult, PermissionApprovalDecision, PermissionApprovalRequest, QueuedSteerId,
        StoredEditableInputEnvelope, StoredEditableInputPart,
    },
};
#[cfg(test)]
use psychevo::{
    StartThreadRequest,
    application::{ClarifyAnswer, ClarifyResponse},
};
use psychevo_gateway_protocol::events_transcript::GatewayEvent;
use psychevo_gateway_protocol::source::GatewayThreadSelector;
#[cfg(test)]
use std::sync::Arc;
use std::{
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
pub(crate) struct RunningTurn {
    pub(crate) session_id: Option<String>,
    pub(crate) control: RunningTurnControl,
    pub(crate) selector: Option<GatewayThreadSelector>,
    pub(crate) turn_id: Option<String>,
    pub(crate) events: RunningTurnEvents,
    pub(crate) task: RunningTask,
}

pub(crate) struct StartingTurn {
    pub(crate) session_id: Option<String>,
    pub(crate) queue_owner_id: String,
    pub(crate) display_prompt: String,
    pub(crate) images: Vec<PendingImageAttachment>,
    pub(crate) cancellation: TurnAdmissionCancellation,
    pub(crate) task: JoinHandle<psychevo::Result<StartedTurn>>,
}

impl StartingTurn {
    pub(crate) fn into_cleanup(self) -> StartingTurnCleanup {
        self.cancellation.cancel();
        StartingTurnCleanup::spawn(self.task)
    }

    pub(crate) fn into_cleanup_with_input(
        self,
    ) -> (StartingTurnCleanup, String, Vec<PendingImageAttachment>) {
        let Self {
            display_prompt,
            images,
            cancellation,
            task,
            ..
        } = self;
        cancellation.cancel();
        (StartingTurnCleanup::spawn(task), display_prompt, images)
    }
}

pub(crate) struct StartingTurnCleanup {
    task: JoinHandle<()>,
}

impl StartingTurnCleanup {
    fn spawn(task: JoinHandle<psychevo::Result<StartedTurn>>) -> Self {
        Self {
            task: tokio::spawn(async move {
                if let Ok(Ok(StartedTurn {
                    handle,
                    approval_rx,
                })) = task.await
                {
                    drop(approval_rx);
                    handle.interrupt();
                    let _ = handle.wait().await;
                }
            }),
        }
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.task.is_finished()
    }

    pub(crate) async fn join(self) {
        let _ = self.task.await;
    }
}

pub(crate) struct StartedTurn {
    pub(crate) handle: TurnHandle,
    pub(crate) approval_rx: mpsc::UnboundedReceiver<TuiApprovalEvent>,
}

#[derive(Clone)]
pub(crate) enum RunningTurnControl {
    Agent(TurnHandle),
    Shell(ShellCommandControl),
}

impl RunningTurnControl {
    pub(crate) fn abort(&self) {
        match self {
            Self::Agent(handle) => handle.interrupt(),
            Self::Shell(control) => control.interrupt(),
        }
    }

    pub(crate) fn steer_user_message(
        &self,
        message: psychevo::application::Message,
    ) -> std::result::Result<QueuedSteerId, psychevo::ControlInputError> {
        match self {
            Self::Agent(handle) => handle.queue_steer_message(message),
            Self::Shell(_) => Err(psychevo::ControlInputError::Closed),
        }
    }

    pub(crate) fn update_pending_user_message(
        &self,
        id: QueuedSteerId,
        message: psychevo::application::Message,
    ) -> std::result::Result<(), psychevo::ControlInputError> {
        match self {
            Self::Agent(handle) => handle.update_queued_steer(id, message),
            Self::Shell(_) => Err(psychevo::ControlInputError::Closed),
        }
    }

    pub(crate) fn cancel_pending_user_message(&self, id: QueuedSteerId) -> bool {
        match self {
            Self::Agent(handle) => handle.cancel_queued_steer(id),
            Self::Shell(_) => false,
        }
    }

    pub(crate) async fn submit_clarify_result(&self, call_id: &str, result: ClarifyResult) -> bool {
        match self {
            Self::Agent(handle) => {
                let response = match result {
                    ClarifyResult::Answered(response) => InteractionResponse::Clarify(
                        response
                            .answers
                            .into_iter()
                            .map(|answer| answer.answers)
                            .collect(),
                    ),
                    ClarifyResult::Cancelled => InteractionResponse::Cancel,
                };
                handle
                    .respond(call_id, response)
                    .await
                    .is_ok_and(|receipt| receipt.accepted)
            }
            Self::Shell(_) => false,
        }
    }

    pub(crate) fn agent_handle(&self) -> Option<TurnHandle> {
        match self {
            Self::Agent(handle) => Some(handle.clone()),
            Self::Shell(_) => None,
        }
    }

    pub(crate) fn shell_control(&self) -> Option<ShellCommandControl> {
        match self {
            Self::Shell(control) => Some(control.clone()),
            Self::Agent(_) => None,
        }
    }
}

impl From<TurnHandle> for RunningTurnControl {
    fn from(handle: TurnHandle) -> Self {
        Self::Agent(handle)
    }
}

impl From<ShellCommandControl> for RunningTurnControl {
    fn from(control: ShellCommandControl) -> Self {
        Self::Shell(control)
    }
}

static NEXT_SHELL_PRESENTATION_ID: AtomicU64 = AtomicU64::new(1);

fn next_shell_presentation_id() -> u64 {
    NEXT_SHELL_PRESENTATION_ID.fetch_add(1, Ordering::Relaxed)
}

pub(crate) fn presented_shell_event_channel() -> (
    mpsc::UnboundedReceiver<PresentedShellEvent>,
    impl Fn(ShellCommandEvent) + Send + Sync + 'static,
) {
    let presentation_id = next_shell_presentation_id();
    let (sender, receiver) = mpsc::unbounded_channel();
    let emit = move |event| {
        let _ = sender.send(PresentedShellEvent {
            presentation_id,
            event,
        });
    };
    (receiver, emit)
}

#[derive(Debug, Clone)]
pub(crate) struct PresentedShellEvent {
    pub(crate) presentation_id: u64,
    pub(crate) event: ShellCommandEvent,
}

impl PresentedShellEvent {
    pub(crate) fn thread_id(&self) -> Option<&str> {
        match &self.event {
            ShellCommandEvent::Started { thread_id, .. }
            | ShellCommandEvent::Completed { thread_id, .. } => thread_id.as_deref(),
            ShellCommandEvent::Warning { .. } => None,
        }
    }
}

pub(crate) enum RunningTurnEvents {
    #[cfg(test)]
    Gateway(mpsc::UnboundedReceiver<GatewayEvent>),
    Turn(TurnEventStream),
    #[cfg(test)]
    TurnTest(mpsc::UnboundedReceiver<TurnEvent>),
    Shell(mpsc::UnboundedReceiver<PresentedShellEvent>),
}

pub(crate) enum TuiLiveEvent {
    Gateway(Box<GatewayEvent>),
    Turn(TurnEvent),
    Shell(PresentedShellEvent),
}

impl RunningTurnEvents {
    pub(crate) fn try_recv(&mut self) -> Option<TuiLiveEvent> {
        match self {
            #[cfg(test)]
            Self::Gateway(rx) => rx
                .try_recv()
                .ok()
                .map(|event| TuiLiveEvent::Gateway(Box::new(event))),
            Self::Turn(events) => events
                .next()
                .now_or_never()
                .flatten()
                .map(TuiLiveEvent::Turn),
            #[cfg(test)]
            Self::TurnTest(rx) => rx.try_recv().ok().map(TuiLiveEvent::Turn),
            Self::Shell(rx) => rx.try_recv().ok().map(TuiLiveEvent::Shell),
        }
    }
}

impl From<TurnEvent> for TuiLiveEvent {
    fn from(event: TurnEvent) -> Self {
        Self::Turn(event)
    }
}

impl From<GatewayEvent> for TuiLiveEvent {
    fn from(event: GatewayEvent) -> Self {
        Self::Gateway(Box::new(event))
    }
}

impl From<PresentedShellEvent> for TuiLiveEvent {
    fn from(event: PresentedShellEvent) -> Self {
        Self::Shell(event)
    }
}

pub(crate) struct TuiApprovalRequest {
    pub(crate) session_id: Option<String>,
    pub(crate) request: PermissionApprovalRequest,
    pub(crate) response: oneshot::Sender<PermissionApprovalDecision>,
}

pub(crate) enum TuiApprovalEvent {
    Request {
        sequence: u64,
        request: Box<TuiApprovalRequest>,
    },
    Cancel {
        sequence: u64,
        session_id: Option<String>,
        tool_call_id: String,
        reason: String,
    },
}

impl TuiApprovalEvent {
    pub(crate) fn sequence(&self) -> u64 {
        match self {
            Self::Request { sequence, .. } | Self::Cancel { sequence, .. } => *sequence,
        }
    }
}

static TUI_APPROVAL_EVENT_SEQUENCER: Mutex<u64> = Mutex::new(1);

fn send_tui_approval_event(
    sender: &mpsc::UnboundedSender<TuiApprovalEvent>,
    build: impl FnOnce(u64) -> TuiApprovalEvent,
) -> Result<(), mpsc::error::SendError<TuiApprovalEvent>> {
    send_tui_approval_event_with_gap(sender, build, || {})
}

fn send_tui_approval_event_with_gap(
    sender: &mpsc::UnboundedSender<TuiApprovalEvent>,
    build: impl FnOnce(u64) -> TuiApprovalEvent,
    before_send: impl FnOnce(),
) -> Result<(), mpsc::error::SendError<TuiApprovalEvent>> {
    let mut next = TUI_APPROVAL_EVENT_SEQUENCER
        .lock()
        .expect("TUI approval event sequencer poisoned");
    let sequence = *next;
    *next = next.saturating_add(1);
    before_send();
    sender.send(build(sequence))
}

#[derive(Clone)]
pub(crate) struct TuiApprovalHandler {
    pub(crate) session_id: Option<String>,
    pub(crate) sender: mpsc::UnboundedSender<TuiApprovalEvent>,
}

impl std::fmt::Debug for TuiApprovalHandler {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TuiApprovalHandler(..)")
    }
}

impl ApprovalHandler for TuiApprovalHandler {
    fn request_permission(
        &self,
        request: PermissionApprovalRequest,
    ) -> futures::future::BoxFuture<'static, PermissionApprovalDecision> {
        let session_id = self.session_id.clone();
        let sender = self.sender.clone();
        Box::pin(async move {
            let tool_call_id = request.tool_call_id.clone();
            let (response, receiver) = oneshot::channel();
            if send_tui_approval_event(&sender, |sequence| TuiApprovalEvent::Request {
                sequence,
                request: Box::new(TuiApprovalRequest {
                    session_id: session_id.clone(),
                    request,
                    response,
                }),
            })
            .is_err()
            {
                return PermissionApprovalDecision::deny();
            }
            match receiver.await {
                Ok(decision) => decision,
                Err(_) => {
                    let _ = send_tui_approval_event(&sender, |sequence| TuiApprovalEvent::Cancel {
                        sequence,
                        session_id,
                        tool_call_id,
                        reason: "response_closed".to_string(),
                    });
                    PermissionApprovalDecision::deny()
                }
            }
        })
    }

    fn cancel_permission(&self, tool_call_id: &str) -> futures::future::BoxFuture<'static, ()> {
        self.cancel_permission_with_reason(tool_call_id, "cancelled")
    }

    fn cancel_permission_with_reason(
        &self,
        tool_call_id: &str,
        reason: &str,
    ) -> futures::future::BoxFuture<'static, ()> {
        let session_id = self.session_id.clone();
        let tool_call_id = tool_call_id.to_string();
        let reason = reason.to_string();
        let sender = self.sender.clone();
        Box::pin(async move {
            let _ = send_tui_approval_event(&sender, |sequence| TuiApprovalEvent::Cancel {
                sequence,
                session_id,
                tool_call_id,
                reason,
            });
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ForeignGatewayActivity {
    pub(crate) active_turn_id: Option<String>,
    pub(crate) started: Instant,
}

pub(crate) struct AuxiliaryShellTask {
    pub(crate) session_id: Option<String>,
    pub(crate) control: ShellCommandControl,
    pub(crate) rx: mpsc::UnboundedReceiver<PresentedShellEvent>,
    pub(crate) task: JoinHandle<psychevo::Result<ShellCommandResult>>,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingAuxiliaryShellCommand {
    pub(crate) owner_session_id: Option<String>,
    pub(crate) owner_turn_id: Option<String>,
    pub(crate) request: ShellCommandRequest,
}

impl PendingAuxiliaryShellCommand {
    pub(crate) fn matches_owner(&self, session_id: Option<&str>, turn_id: Option<&str>) -> bool {
        match self.owner_turn_id.as_deref() {
            Some(owner_turn_id) => turn_id == Some(owner_turn_id),
            None => self.owner_session_id.as_deref() == session_id,
        }
    }
}

pub(crate) struct AuxiliaryAgentTask {
    pub(crate) session_id: Option<String>,
    pub(crate) turn_id: Option<String>,
    pub(crate) child_session_id: Option<String>,
    pub(crate) visible_live: bool,
    pub(crate) pending_unowned_live_events: Vec<TurnEvent>,
    pub(crate) approval_rx: Option<mpsc::UnboundedReceiver<TuiApprovalEvent>>,
    pub(crate) control: RunningTurnControl,
    pub(crate) events: RunningTurnEvents,
    pub(crate) task: JoinHandle<psychevo::Result<TurnResult>>,
}

pub(crate) enum RunningTask {
    Agent(JoinHandle<psychevo::Result<TurnResult>>),
    UserShell(JoinHandle<psychevo::Result<ShellCommandResult>>),
}

pub(crate) enum RunningCompletion {
    Agent(Box<std::result::Result<psychevo::Result<TurnResult>, tokio::task::JoinError>>),
    UserShell(std::result::Result<psychevo::Result<ShellCommandResult>, tokio::task::JoinError>),
}

#[cfg(test)]
mod semantic_control_tests {
    use super::*;
    use std::sync::mpsc as std_mpsc;
    use tokio::sync::Notify;

    #[test]
    fn approval_sequence_allocation_and_cross_receiver_enqueue_are_atomic() {
        let (first_tx, mut first_rx) = mpsc::unbounded_channel();
        let (second_tx, mut second_rx) = mpsc::unbounded_channel();
        let (allocated_tx, allocated_rx) = std_mpsc::channel();
        let (release_tx, release_rx) = std_mpsc::channel();

        let first = std::thread::spawn(move || {
            send_tui_approval_event_with_gap(
                &first_tx,
                |sequence| TuiApprovalEvent::Cancel {
                    sequence,
                    session_id: Some("first".to_string()),
                    tool_call_id: "first".to_string(),
                    reason: "test".to_string(),
                },
                || {
                    allocated_tx.send(()).expect("allocated");
                    release_rx.recv().expect("release");
                },
            )
            .expect("first enqueue");
        });
        allocated_rx.recv().expect("first allocation");
        let second = std::thread::spawn(move || {
            send_tui_approval_event(&second_tx, |sequence| TuiApprovalEvent::Cancel {
                sequence,
                session_id: Some("second".to_string()),
                tool_call_id: "second".to_string(),
                reason: "test".to_string(),
            })
            .expect("second enqueue");
        });

        assert!(
            second_rx.try_recv().is_err(),
            "later event overtook the gap"
        );
        release_tx.send(()).expect("release first");
        first.join().expect("first thread");
        second.join().expect("second thread");
        let first_event = first_rx.try_recv().expect("first event");
        let second_event = second_rx.try_recv().expect("second event");
        assert!(first_event.sequence() < second_event.sequence());
    }

    #[derive(Debug)]
    struct InterruptibleAdapter {
        started: Arc<Notify>,
    }

    #[derive(Debug)]
    struct PreparedInterruptibleTurn {
        started: Arc<Notify>,
    }

    #[derive(Debug)]
    struct ClarifyAdapter {
        started: Arc<Notify>,
        outcome: Arc<Mutex<Option<psychevo::application::ClarifyInteractionOutcome>>>,
    }

    #[derive(Debug)]
    struct PreparedClarifyTurn {
        started: Arc<Notify>,
        outcome: Arc<Mutex<Option<psychevo::application::ClarifyInteractionOutcome>>>,
    }

    impl psychevo::AgentSessionAdapter for InterruptibleAdapter {
        fn prepare_turn(
            self: Arc<Self>,
            _request: psychevo::AgentTurnPreparation,
        ) -> futures::future::BoxFuture<
            'static,
            psychevo::Result<Box<dyn psychevo::PreparedAgentTurn>>,
        > {
            Box::pin(async move {
                Ok(Box::new(PreparedInterruptibleTurn {
                    started: self.started.clone(),
                }) as Box<dyn psychevo::PreparedAgentTurn>)
            })
        }
    }

    impl psychevo::PreparedAgentTurn for PreparedInterruptibleTurn {
        fn invoke(
            self: Box<Self>,
            invocation: psychevo::AgentTurnInvocation,
        ) -> futures::future::BoxFuture<'static, psychevo::Result<psychevo::TurnResult>> {
            Box::pin(async move {
                invocation.events.emit(TurnEvent::MessageDelta {
                    text: "semantic live event".to_string(),
                });
                self.started.notify_one();
                invocation.control.wait_for_interrupt().await;
                drop(invocation);
                Err(psychevo::Error::Message(
                    "semantic interrupt observed".to_string(),
                ))
            })
        }
    }

    impl psychevo::AgentSessionAdapter for ClarifyAdapter {
        fn prepare_turn(
            self: Arc<Self>,
            _request: psychevo::AgentTurnPreparation,
        ) -> futures::future::BoxFuture<
            'static,
            psychevo::Result<Box<dyn psychevo::PreparedAgentTurn>>,
        > {
            Box::pin(async move {
                Ok(Box::new(PreparedClarifyTurn {
                    started: self.started.clone(),
                    outcome: self.outcome.clone(),
                }) as Box<dyn psychevo::PreparedAgentTurn>)
            })
        }
    }

    impl psychevo::PreparedAgentTurn for PreparedClarifyTurn {
        fn invoke(
            self: Box<Self>,
            invocation: psychevo::AgentTurnInvocation,
        ) -> futures::future::BoxFuture<'static, psychevo::Result<psychevo::TurnResult>> {
            Box::pin(async move {
                let thread_id = invocation.receipt.thread_id.clone();
                self.started.notify_one();
                let outcome = invocation
                    .control
                    .request_clarification(psychevo::application::ClarifyRequestEvent {
                        call_id: "semantic-clarify-1".to_string(),
                        questions: vec![psychevo::application::ClarifyQuestion {
                            header: "Target".to_string(),
                            question: "Which workspace?".to_string(),
                            options: Vec::new(),
                            multiple: false,
                            custom: true,
                            secret: false,
                        }],
                    })
                    .await;
                drop(invocation);
                *self.outcome.lock().expect("clarify outcome poisoned") = Some(outcome);
                Ok(psychevo::TurnResult {
                    thread_id,
                    outcome: psychevo::TurnOutcome::Completed,
                    final_answer: "clarified".to_string(),
                    provider: "fake".to_string(),
                    model: "fake".to_string(),
                    reasoning_effort: None,
                    tool_failures: 0,
                    context_limit: None,
                    context_snapshot: None,
                    warnings: Vec::new(),
                    terminal_reason: None,
                    terminal_error: None,
                    selected_agent: None,
                    selected_skills: Vec::new(),
                })
            })
        }
    }

    #[tokio::test]
    async fn foreground_agent_control_uses_the_application_turn_handle() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(InterruptibleAdapter {
                started: started.clone(),
            }))
            .build()
            .await
            .expect("Application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("Thread");
        let handle = thread
            .start_turn(TurnRequest::new("semantic control"))
            .await
            .expect("accepted Turn");
        started.notified().await;
        let mut events = RunningTurnEvents::Turn(handle.events());
        assert!((0..16).any(|_| {
            matches!(
                events.try_recv(),
                Some(TuiLiveEvent::Turn(TurnEvent::MessageDelta { text }))
                    if text == "semantic live event"
            )
        }));
        let control = RunningTurnControl::Agent(handle.clone());

        assert!(control.agent_handle().is_some());
        let steer = control
            .steer_user_message(psychevo::application::user_text_message("first"))
            .expect("queue steer");
        control
            .update_pending_user_message(steer, psychevo::application::user_text_message("updated"))
            .expect("update steer");
        assert!(control.cancel_pending_user_message(steer));
        control.abort();
        assert_eq!(
            handle
                .wait()
                .await
                .expect_err("fixture stops after interrupt")
                .to_string(),
            "semantic interrupt observed"
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn foreground_agent_clarify_uses_the_application_interaction_broker() {
        let temp = tempfile::tempdir().expect("tempdir");
        let started = Arc::new(Notify::new());
        let outcome = Arc::new(Mutex::new(None));
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .agent_session_adapter(Arc::new(ClarifyAdapter {
                started: started.clone(),
                outcome: outcome.clone(),
            }))
            .build()
            .await
            .expect("Application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(temp.path()))
            .await
            .expect("Thread");
        let handle = thread
            .start_turn(TurnRequest::new("semantic clarify"))
            .await
            .expect("accepted Turn");
        started.notified().await;
        let control = RunningTurnControl::Agent(handle.clone());
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if thread
                    .pending_interactions()
                    .await
                    .expect("pending interactions")
                    .iter()
                    .any(|interaction| interaction.interaction_id == "semantic-clarify-1")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("clarify interaction persisted");

        assert!(
            control
                .submit_clarify_result(
                    "semantic-clarify-1",
                    ClarifyResult::Answered(ClarifyResponse {
                        answers: vec![ClarifyAnswer {
                            answers: vec!["/workspace/a".to_string()],
                        }],
                    }),
                )
                .await
        );
        handle.wait().await.expect("clarified Turn");
        assert_eq!(
            *outcome.lock().expect("clarify outcome poisoned"),
            Some(psychevo::application::ClarifyInteractionOutcome::Answered(
                ClarifyResponse {
                    answers: vec![ClarifyAnswer {
                        answers: vec!["/workspace/a".to_string()],
                    }],
                }
            ))
        );
        application.shutdown().await.expect("shutdown");
    }
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

#[derive(Debug, Clone)]
pub(crate) enum QueuedInput {
    Prompt {
        session_id: Option<String>,
        prompt: String,
        display_prompt: String,
        images: Vec<PendingImageAttachment>,
        mission: Option<Box<psychevo::application::AgentMissionRegistration>>,
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
    pub(crate) id: QueuedSteerId,
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

pub(crate) fn rebind_queued_input_session(input: &mut QueuedInput, from: &str, to: &str) {
    let session_id = match input {
        QueuedInput::Prompt { session_id, .. }
        | QueuedInput::Shell { session_id, .. }
        | QueuedInput::Compact { session_id, .. } => session_id,
    };
    if session_id.as_deref() == Some(from) {
        *session_id = Some(to.to_string());
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
    cwd: &Path,
) -> Option<String> {
    if attachments.is_empty() {
        return None;
    }
    let mut lines = vec!["attachments".to_string()];
    for (index, attachment) in attachments.iter().enumerate() {
        lines.push(format!(
            "image {}: {}",
            index + 1,
            display_image_source(&attachment.image, cwd)
        ));
    }
    Some(lines.join("\n"))
}

pub(crate) fn display_image_source(image: &ImageInput, cwd: &Path) -> String {
    match image {
        ImageInput::LocalPath(path) => path
            .strip_prefix(cwd)
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| path.display().to_string()),
        ImageInput::ImageUrl(url) => url.clone(),
    }
}

pub(crate) fn prompt_display_metadata(
    content_text: &str,
    attachments: &[PendingImageAttachment],
    cwd: &Path,
) -> Option<PromptDisplayMetadata> {
    let editable_input = tui_editable_input_envelope(content_text, attachments);
    Some(PromptDisplayMetadata {
        content_text: content_text.to_string(),
        attachments: attachments
            .iter()
            .map(|attachment| PromptAttachmentDisplay {
                kind: "image".to_string(),
                placeholder: attachment.placeholder.clone(),
                source: display_image_source(&attachment.image, cwd),
            })
            .collect(),
        editable_input: Some(editable_input),
    })
}

fn tui_editable_input_envelope(
    content_text: &str,
    attachments: &[PendingImageAttachment],
) -> StoredEditableInputEnvelope {
    let mut parts = Vec::new();
    let mut cursor = 0usize;
    for (image_block_index, attachment) in attachments.iter().enumerate() {
        let Some(relative) = content_text[cursor..].find(&attachment.placeholder) else {
            continue;
        };
        let placeholder_start = cursor + relative;
        if placeholder_start > cursor {
            parts.push(StoredEditableInputPart::Text {
                text: content_text[cursor..placeholder_start].to_string(),
            });
        }
        parts.push(StoredEditableInputPart::Image { image_block_index });
        cursor = placeholder_start + attachment.placeholder.len();
    }
    if cursor < content_text.len() {
        parts.push(StoredEditableInputPart::Text {
            text: content_text[cursor..].to_string(),
        });
    } else if parts.is_empty() {
        parts.push(StoredEditableInputPart::Text {
            text: content_text.to_string(),
        });
    }
    StoredEditableInputEnvelope { version: 1, parts }
}
