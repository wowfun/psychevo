use psychevo_agent_core::{Message, UserContentBlock, now_ms};
use psychevo_gateway_protocol as wire;
use psychevo_runtime::{
    ConversationDraftPart, StateRuntime, StoredEditableInputEnvelope, StoredEditableInputPart,
};

use crate::Gateway;

pub fn native_history_action_unavailable_reason(
    state: &StateRuntime,
    thread_id: &str,
    surface_kind: &str,
) -> psychevo_runtime::Result<Option<String>> {
    if !matches!(surface_kind, "web" | "tui") {
        return Ok(Some(
            "History editing is available only in Workbench and TUI.".to_string(),
        ));
    }
    let Some(summary) = state.store().session_summary(thread_id)? else {
        return Ok(Some("The durable Thread is unavailable.".to_string()));
    };
    if summary.parent_session_id.is_some() || state.store().find_agent_edge(thread_id)?.is_some() {
        return Ok(Some(
            "Subagent and side Threads cannot edit or fork conversation history.".to_string(),
        ));
    }
    if !matches!(summary.source.as_str(), "web" | "tui") {
        return Ok(Some(
            "Dedicated channel and automation Threads cannot edit or fork conversation history."
                .to_string(),
        ));
    }
    let native_binding = state
        .store()
        .gateway_runtime_binding(thread_id)?
        .is_some_and(|binding| {
            binding.status == psychevo_runtime::GatewayRuntimeBindingStatus::Resolved
                && binding.backend_kind.as_deref() == Some("native")
        });
    if !native_binding {
        return Ok(Some(
            "History editing requires a resolved Native Thread binding.".to_string(),
        ));
    }
    let side = state
        .store()
        .session_metadata(thread_id)?
        .as_ref()
        .and_then(|metadata| metadata.get(psychevo_runtime::SIDE_CONVERSATION_METADATA_KEY))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if side {
        return Ok(Some(
            "Side Threads cannot edit or fork conversation history.".to_string(),
        ));
    }
    if state
        .store()
        .active_gateway_activity_for_thread(thread_id)?
        .is_some_and(|activity| activity.lease_expires_at_ms >= now_ms())
    {
        return Ok(Some(
            "Finish the running turn before editing or forking conversation history.".to_string(),
        ));
    }
    Ok(None)
}

pub fn read_native_editable_draft(
    state: &StateRuntime,
    gateway: &Gateway,
    thread_id: &str,
    message_id: &str,
    surface_kind: &str,
) -> psychevo_runtime::Result<wire::ThreadHistoryDraftReadResult> {
    if let Some(reason) = native_history_action_unavailable_reason(state, thread_id, surface_kind)?
    {
        return Ok(unavailable_draft(thread_id, message_id, None, &reason));
    }
    let entry = gateway
        .thread_transcript(thread_id)?
        .into_iter()
        .find(|entry| entry.id == message_id)
        .filter(|entry| {
            entry.role == wire::TranscriptEntryRole::User
                && entry.status == wire::TranscriptBlockStatus::Completed
                && entry.message_seq.is_some()
        });
    let Some(entry) = entry else {
        return Ok(unavailable_draft(
            thread_id,
            message_id,
            None,
            "Only visible, finalized user messages with durable history can be edited.",
        ));
    };
    let message_seq = entry.message_seq.expect("filtered durable entry");
    let Some(summary) = state
        .store()
        .load_export_message_summaries(thread_id)?
        .into_iter()
        .find(|summary| summary.session_seq == message_seq)
    else {
        return Ok(unavailable_draft(
            thread_id,
            message_id,
            Some(message_seq),
            "The durable user message is no longer available in this Thread.",
        ));
    };
    let Message::User { content, .. } = summary.message else {
        return Ok(unavailable_draft(
            thread_id,
            message_id,
            Some(message_seq),
            "The durable history entry no longer resolves to a user message.",
        ));
    };
    let envelope = summary
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(psychevo_runtime::EDITABLE_INPUT_METADATA_KEY))
        .cloned()
        .map(serde_json::from_value::<StoredEditableInputEnvelope>)
        .transpose()?;
    let (parts, fidelity, warning, unavailable_reason) = match envelope {
        Some(envelope) if envelope.version == 1 => match parts_from_envelope(&envelope, &content) {
            Some(parts) if !parts.is_empty() => (
                parts,
                wire::ThreadEditableDraftFidelity::Exact,
                None,
                None,
            ),
            Some(_) => (
                Vec::new(),
                wire::ThreadEditableDraftFidelity::Exact,
                None,
                Some("This message has no editable text or image input.".to_string()),
            ),
            None => (
                Vec::new(),
                wire::ThreadEditableDraftFidelity::Exact,
                None,
                Some(
                    "The editable input envelope no longer matches the durable message."
                        .to_string(),
                ),
            ),
        },
        _ => (
            parts_from_legacy_message(&content),
            wire::ThreadEditableDraftFidelity::BestEffort,
            Some(
                "This older message was reconstructed from durable history; hidden context or synthetic input may not be recoverable."
                    .to_string(),
            ),
            None,
        ),
    };
    Ok(wire::ThreadHistoryDraftReadResult {
        thread_id: thread_id.to_string(),
        message_id: message_id.to_string(),
        message_seq: Some(message_seq),
        parts,
        fidelity,
        warning,
        unavailable_reason,
    })
}

pub fn stage_native_conversation_edit(
    state: &StateRuntime,
    gateway: &Gateway,
    thread_id: &str,
    message_id: &str,
    draft: &wire::ThreadEditableDraft,
    surface_kind: &str,
) -> psychevo_runtime::Result<bool> {
    if let Some(reason) = native_history_action_unavailable_reason(state, thread_id, surface_kind)?
    {
        return Err(psychevo_runtime::Error::Message(reason));
    }
    let requested_parts = draft_parts_from_wire(&draft.parts);
    if let Some(existing) = state.store().session_revert_state(thread_id)? {
        return match existing.kind {
            psychevo_runtime::SessionRevertKind::ConversationEdit {
                boundary_message_id,
                draft: existing_parts,
            } if boundary_message_id == message_id && existing_parts == requested_parts => Ok(true),
            psychevo_runtime::SessionRevertKind::WorkspaceUndo { .. } => {
                Err(psychevo_runtime::Error::Message(
                    "Redo workspace files before editing conversation history.".to_string(),
                ))
            }
            psychevo_runtime::SessionRevertKind::ConversationEdit { .. } => {
                Err(psychevo_runtime::Error::Message(
                    "Restore or run the staged conversation edit before starting another edit."
                        .to_string(),
                ))
            }
        };
    }
    let current = read_native_editable_draft(state, gateway, thread_id, message_id, surface_kind)?;
    if let Some(reason) = current.unavailable_reason {
        return Err(psychevo_runtime::Error::Message(reason));
    }
    let message_seq = current.message_seq.ok_or_else(|| {
        psychevo_runtime::Error::Message(
            "The selected message does not have a durable sequence.".to_string(),
        )
    })?;
    if current.parts == draft.parts {
        return Ok(false);
    }
    state.store().set_session_revert_state(
        thread_id,
        psychevo_runtime::SessionRevertState::conversation_edit(
            message_seq,
            message_id.to_string(),
            requested_parts,
        ),
    )?;
    Ok(true)
}

pub fn restore_native_conversation_edit(
    state: &StateRuntime,
    thread_id: &str,
) -> psychevo_runtime::Result<wire::ThreadEditableDraft> {
    let revert = state
        .store()
        .session_revert_state(thread_id)?
        .ok_or_else(|| {
            psychevo_runtime::Error::Message("No conversation edit is staged.".to_string())
        })?;
    let psychevo_runtime::SessionRevertKind::ConversationEdit { draft, .. } = revert.kind else {
        return Err(psychevo_runtime::Error::Message(
            "The staged state belongs to workspace undo; use /redo instead.".to_string(),
        ));
    };
    state.store().clear_session_revert_state(thread_id)?;
    Ok(wire::ThreadEditableDraft {
        parts: draft_parts_to_wire(draft),
    })
}

pub fn fork_native_history(
    state: &StateRuntime,
    thread_id: &str,
    before_session_seq: Option<i64>,
    surface_kind: &str,
) -> psychevo_runtime::Result<String> {
    if let Some(reason) = native_history_action_unavailable_reason(state, thread_id, surface_kind)?
    {
        return Err(psychevo_runtime::Error::Message(reason));
    }
    if state.store().session_revert_state(thread_id)?.is_some() {
        return Err(psychevo_runtime::Error::Message(
            "Run, restore, or redo the staged history state before forking.".to_string(),
        ));
    }
    state
        .store()
        .fork_native_session_history(psychevo_runtime::NativeSessionForkInput {
            source_session_id: thread_id,
            before_session_seq,
        })
}

fn unavailable_draft(
    thread_id: &str,
    message_id: &str,
    message_seq: Option<i64>,
    reason: &str,
) -> wire::ThreadHistoryDraftReadResult {
    wire::ThreadHistoryDraftReadResult {
        thread_id: thread_id.to_string(),
        message_id: message_id.to_string(),
        message_seq,
        parts: Vec::new(),
        fidelity: wire::ThreadEditableDraftFidelity::BestEffort,
        warning: None,
        unavailable_reason: Some(reason.to_string()),
    }
}

fn parts_from_envelope(
    envelope: &StoredEditableInputEnvelope,
    content: &[UserContentBlock],
) -> Option<Vec<wire::ThreadEditableInputPart>> {
    let images = content
        .iter()
        .filter(|block| !matches!(block, UserContentBlock::Text(_)))
        .collect::<Vec<_>>();
    envelope
        .parts
        .iter()
        .map(|part| match part {
            StoredEditableInputPart::Text { text } => {
                Some(wire::ThreadEditableInputPart::Text { text: text.clone() })
            }
            StoredEditableInputPart::Image { image_block_index } => images
                .get(*image_block_index)
                .and_then(|block| image_part(block)),
        })
        .collect()
}

fn parts_from_legacy_message(content: &[UserContentBlock]) -> Vec<wire::ThreadEditableInputPart> {
    content
        .iter()
        .filter_map(|block| match block {
            UserContentBlock::Text(block) => Some(wire::ThreadEditableInputPart::Text {
                text: block.text.clone(),
            }),
            block => image_part(block),
        })
        .collect()
}

fn image_part(block: &UserContentBlock) -> Option<wire::ThreadEditableInputPart> {
    match block {
        UserContentBlock::LocalImage(block) => Some(wire::ThreadEditableInputPart::Image {
            input: wire::GatewayImageInput::LocalPath {
                path: block.path.display().to_string(),
            },
        }),
        UserContentBlock::ImageUrl(block) => Some(wire::ThreadEditableInputPart::Image {
            input: wire::GatewayImageInput::Url {
                url: block.url.clone(),
            },
        }),
        UserContentBlock::Text(_) => None,
    }
}

fn draft_parts_from_wire(parts: &[wire::ThreadEditableInputPart]) -> Vec<ConversationDraftPart> {
    parts
        .iter()
        .map(|part| match part {
            wire::ThreadEditableInputPart::Text { text } => {
                ConversationDraftPart::Text { text: text.clone() }
            }
            wire::ThreadEditableInputPart::Image {
                input: wire::GatewayImageInput::LocalPath { path },
            } => ConversationDraftPart::LocalImage { path: path.clone() },
            wire::ThreadEditableInputPart::Image {
                input: wire::GatewayImageInput::Url { url },
            } => ConversationDraftPart::ImageUrl { url: url.clone() },
        })
        .collect()
}

fn draft_parts_to_wire(parts: Vec<ConversationDraftPart>) -> Vec<wire::ThreadEditableInputPart> {
    parts
        .into_iter()
        .map(|part| match part {
            ConversationDraftPart::Text { text } => wire::ThreadEditableInputPart::Text { text },
            ConversationDraftPart::LocalImage { path } => wire::ThreadEditableInputPart::Image {
                input: wire::GatewayImageInput::LocalPath { path },
            },
            ConversationDraftPart::ImageUrl { url } => wire::ThreadEditableInputPart::Image {
                input: wire::GatewayImageInput::Url { url },
            },
        })
        .collect()
}
