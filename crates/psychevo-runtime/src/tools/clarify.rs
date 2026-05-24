#[allow(unused_imports)]
pub(crate) use super::*;
use crate::types::{
    ClarifyControl, ClarifyQuestion, ClarifyQuestionOption, ClarifyRequestEvent,
    ClarifyResolvedEvent, ClarifyResolvedReason, ClarifyResult,
    RunStreamEvent as ClarifyRunStreamEvent, RunStreamSink as ClarifyRunStreamSink,
};

#[cfg(not(test))]
pub(crate) fn clarify_timeout() -> Duration {
    Duration::from_secs(600)
}

#[cfg(test)]
pub(crate) fn clarify_timeout() -> Duration {
    Duration::from_millis(500)
}

#[derive(Clone)]
pub(crate) struct ClarifyTool {
    pub(crate) control: Option<Arc<ClarifyControl>>,
    pub(crate) stream: Option<ClarifyRunStreamSink>,
}

impl ClarifyTool {
    pub(crate) fn new(
        control: Option<Arc<ClarifyControl>>,
        stream: Option<ClarifyRunStreamSink>,
    ) -> Self {
        Self { control, stream }
    }
}

impl ToolBinding for ClarifyTool {
    fn name(&self) -> &str {
        "clarify"
    }

    fn description(&self) -> &str {
        "Ask the user for clarification, feedback, or a meaningful decision before proceeding. Use this for one to three short questions with concrete options. If one option is recommended, put it first and include \"(Recommended)\" in that option label. Do not use this tool for dangerous command, file write, or permission approval; those use the permission approval flow."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "questions": {
                    "type": "array",
                    "description": "One to three user-facing questions to ask before continuing; each question must include two or three concrete options.",
                    "minItems": 1,
                    "maxItems": 3,
                    "items": {
                        "type": "object",
                        "description": "A single clarification question and its selectable options.",
                        "additionalProperties": false,
                        "properties": {
                            "question": {
                                "type": "string",
                                "description": "The question to ask the user."
                            },
                            "options": {
                                "type": "array",
                                "description": "Two or three concrete choices for this question. The client also offers an Other/freeform path.",
                                "minItems": 2,
                                "maxItems": 3,
                                "items": {
                                    "type": "object",
                                    "description": "A selectable answer option.",
                                    "additionalProperties": false,
                                    "properties": {
                                        "label": {
                                            "type": "string",
                                            "description": "Short option label shown to the user. Put the recommended choice first and include '(Recommended)' in its label when applicable."
                                        },
                                        "description": {
                                            "type": "string",
                                            "description": "One-sentence explanation of the impact or tradeoff for choosing this option."
                                        }
                                    },
                                    "required": ["label", "description"]
                                }
                            }
                        },
                        "required": ["question", "options"]
                    }
                }
            },
            "required": ["questions"]
        })
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        tool_call_id: String,
        args: Value,
        mut abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let control = self.control.clone();
        let stream = self.stream.clone();
        Box::pin(async move {
            let request = match parse_clarify_request(tool_call_id.clone(), args) {
                Ok(request) => request,
                Err(err) => return ToolOutput::error(err.to_string()),
            };
            let Some(control) = control else {
                return ToolOutput::error("clarify is not available in this execution context");
            };
            let Some(stream) = stream else {
                return ToolOutput::error("clarify is not available in this execution context");
            };

            let receiver = control.register(tool_call_id.clone());
            stream(ClarifyRunStreamEvent::ClarifyRequest(request));
            let timeout = time::sleep(clarify_timeout());
            tokio::pin!(timeout);

            tokio::select! {
                result = receiver => {
                    match result {
                        Ok(ClarifyResult::Answered(response)) => {
                            emit_clarify_resolved(&stream, &tool_call_id, ClarifyResolvedReason::Answered);
                            ToolOutput {
                                json: serde_json::to_value(response)
                                    .unwrap_or_else(|err| json!({"error": format!("failed to serialize clarify response: {err}")})),
                                model_content: None,
                                attachments: Vec::new(),
                                is_error: false,
                            }
                        }
                        Ok(ClarifyResult::Cancelled) => {
                            emit_clarify_resolved(&stream, &tool_call_id, ClarifyResolvedReason::Cancelled);
                            ToolOutput::error("clarify was cancelled by the user")
                        }
                        Err(_) => {
                            emit_clarify_resolved(&stream, &tool_call_id, ClarifyResolvedReason::TurnFinished);
                            ToolOutput::error("clarify response channel closed")
                        }
                    }
                }
                _ = &mut timeout => {
                    control.remove(&tool_call_id);
                    emit_clarify_resolved(&stream, &tool_call_id, ClarifyResolvedReason::TimedOut);
                    ToolOutput::error("timed out waiting for user input")
                }
                _ = abort.wait_for_abort() => {
                    control.remove(&tool_call_id);
                    emit_clarify_resolved(&stream, &tool_call_id, ClarifyResolvedReason::TurnFinished);
                    ToolOutput::error("clarify was interrupted because the turn ended")
                }
            }
        })
    }
}

pub(crate) fn emit_clarify_resolved(
    stream: &ClarifyRunStreamSink,
    call_id: &str,
    reason: ClarifyResolvedReason,
) {
    stream(ClarifyRunStreamEvent::ClarifyResolved(
        ClarifyResolvedEvent {
            call_id: call_id.to_string(),
            reason,
        },
    ));
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClarifyArgs {
    pub(crate) questions: Vec<RawClarifyQuestion>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawClarifyQuestion {
    pub(crate) question: String,
    pub(crate) options: Vec<RawClarifyQuestionOption>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawClarifyQuestionOption {
    pub(crate) label: String,
    pub(crate) description: String,
}

pub(crate) fn parse_clarify_request(call_id: String, args: Value) -> Result<ClarifyRequestEvent> {
    let args: ClarifyArgs = serde_json::from_value(args)
        .map_err(|err| Error::Message(format!("invalid clarify arguments: {err}")))?;
    validate_question_count(args.questions.len())?;
    let mut questions = Vec::with_capacity(args.questions.len());
    for raw in args.questions {
        let question = raw.question.trim().to_string();
        if question.is_empty() {
            return Err(Error::Message(
                "clarify question text is required".to_string(),
            ));
        }
        validate_option_count(raw.options.len())?;
        let mut options = Vec::with_capacity(raw.options.len());
        for raw_option in raw.options {
            let label = raw_option.label.trim().to_string();
            let description = raw_option.description.trim().to_string();
            if label.is_empty() || description.is_empty() {
                return Err(Error::Message(
                    "clarify options require non-empty label and description".to_string(),
                ));
            }
            options.push(ClarifyQuestionOption { label, description });
        }
        questions.push(ClarifyQuestion { question, options });
    }
    Ok(ClarifyRequestEvent { call_id, questions })
}

pub(crate) fn validate_question_count(count: usize) -> Result<()> {
    if (1..=3).contains(&count) {
        Ok(())
    } else {
        Err(Error::Message(
            "clarify requires one to three questions".to_string(),
        ))
    }
}

pub(crate) fn validate_option_count(count: usize) -> Result<()> {
    if (2..=3).contains(&count) {
        Ok(())
    } else {
        Err(Error::Message(
            "clarify requires two to three options for every question".to_string(),
        ))
    }
}

#[cfg(test)]
pub(crate) fn clarify_tool_impl(
    args: Value,
    control: Option<Arc<ClarifyControl>>,
    stream: Option<ClarifyRunStreamSink>,
) -> BoxFuture<'static, ToolOutput> {
    ClarifyTool::new(control, stream).execute(
        "call_clarify".to_string(),
        args,
        never_abort_signal(),
    )
}

#[cfg(test)]
pub(crate) fn never_abort_signal() -> AbortSignal {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    AbortSignal::new(rx)
}
