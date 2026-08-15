use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::{Stream, StreamExt};
use psychevo_agent_core::Message;
use serde::Serialize;
use tokio::io::{AsyncWrite, AsyncWriteExt, BufWriter};
use tokio::sync::mpsc;

use super::inputs::{
    ExportMessageRecord, SessionExportArtifact, SessionExportFormat, SessionExportInclude,
    SessionExportIncludeSet, SessionExportOptions, SessionExportWriteResult,
};
use super::runtime::{
    ExportSections, assemble_export_sections, export_evidence_item, export_mailbox_event_value,
};
use crate::error::{Error, Result};
use crate::session_export::markdown_helpers::push_line;
use crate::session_export::markdown_helpers::sanitize_reasoning_for_export;
use crate::session_export::reconstruction_markdown::{
    export_header, render_markdown, render_markdown_evidence_item, render_markdown_header,
    render_markdown_mailbox_event, render_markdown_message, sanitize_message_without_reasoning,
};
use crate::state::StateRuntime;
use crate::types::{SessionExportMessageSummary, SessionSummary};

const EXPORT_MESSAGE_PAGE_SIZE: usize = 256;
const MAX_IN_MEMORY_EXPORT_BYTES: usize = 32 * 1024 * 1024;

pub struct SessionExportByteStream {
    receiver: mpsc::Receiver<Result<Vec<u8>>>,
}

impl Stream for SessionExportByteStream {
    type Item = Result<Vec<u8>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}

pub async fn render_session_export(
    store: &StateRuntime,
    session_id: &str,
    options: SessionExportOptions,
) -> Result<SessionExportArtifact> {
    let format = options.format;
    let stream = stream_session_export(store, session_id, options).await?;
    let bytes = collect_byte_stream_with_limit(stream, MAX_IN_MEMORY_EXPORT_BYTES).await?;
    let content = String::from_utf8(bytes).map_err(|error| {
        Error::Message(format!("session export emitted invalid UTF-8: {error}"))
    })?;
    Ok(SessionExportArtifact {
        content,
        format,
        session_id: session_id.to_string(),
    })
}

async fn collect_byte_stream_with_limit<S>(stream: S, limit: usize) -> Result<Vec<u8>>
where
    S: Stream<Item = Result<Vec<u8>>>,
{
    futures::pin_mut!(stream);
    let mut collected = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let next_len = collected.len().saturating_add(chunk.len());
        if next_len > limit {
            return Err(export_too_large_for_memory(next_len as u64, limit));
        }
        collected.extend_from_slice(&chunk);
    }
    Ok(collected)
}

fn export_too_large_for_memory(bytes: u64, limit: usize) -> Error {
    Error::structured(
        format!(
            "session export exceeds the {limit} byte in-memory limit; write it to a file or stream it"
        ),
        serde_json::json!({
            "code": "export_too_large_for_memory",
            "bytes": bytes,
            "limit": limit,
        }),
    )
}

pub async fn write_session_export(
    store: &StateRuntime,
    session_id: &str,
    output_path: &Path,
    options: SessionExportOptions,
) -> Result<SessionExportWriteResult> {
    let prepared = prepare_export(store, session_id, options).await?;
    let format = prepared.options.format;
    let exported_session_id = prepared.summary.id.clone();
    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent).await?;
    }
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = output_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("session-export");
    let temporary_path = parent.join(format!(
        ".{file_name}.{}.tmp",
        uuid::Uuid::now_v7().simple()
    ));
    let mut temporary = TemporaryExportPath::new(temporary_path.clone());
    let file = match tokio::fs::File::create(&temporary_path).await {
        Ok(file) => file,
        Err(error) => {
            temporary.remove().await;
            return Err(error.into());
        }
    };
    let mut writer = BufWriter::new(file);
    let write_result = {
        let mut sink = ExportSink::Writer(&mut writer);
        write_prepared_export(store, prepared, &mut sink).await
    };
    let bytes = match write_result {
        Ok(bytes) => bytes,
        Err(error) => {
            drop(writer);
            temporary.remove().await;
            return Err(error);
        }
    };
    if let Err(error) = writer.flush().await {
        drop(writer);
        temporary.remove().await;
        return Err(error.into());
    }
    drop(writer);
    if let Err(error) = replace_export_file(&temporary_path, output_path).await {
        temporary.remove().await;
        return Err(error);
    }
    temporary.disarm();
    Ok(SessionExportWriteResult {
        path: output_path.to_path_buf(),
        bytes,
        format,
        session_id: exported_session_id,
    })
}

pub async fn stream_session_export(
    store: &StateRuntime,
    session_id: &str,
    options: SessionExportOptions,
) -> Result<SessionExportByteStream> {
    let prepared = prepare_export(store, session_id, options).await?;
    let store = store.clone();
    let (sender, receiver) = mpsc::channel(8);
    tokio::spawn(async move {
        let error_sender = sender.clone();
        let mut sink = ExportSink::Channel(sender);
        if let Err(error) = write_prepared_export(&store, prepared, &mut sink).await {
            let _ = error_sender.send(Err(error)).await;
        }
    });
    Ok(SessionExportByteStream { receiver })
}

struct PreparedExport {
    summary: SessionSummary,
    sections: ExportSections,
    options: SessionExportOptions,
    before_session_seq: i64,
    evidence_through_id: i64,
    mailbox_through_id: i64,
}

async fn prepare_export(
    store: &StateRuntime,
    session_id: &str,
    options: SessionExportOptions,
) -> Result<PreparedExport> {
    let summary = store
        .session_summary(session_id)
        .await?
        .ok_or_else(|| Error::Message(format!("session not found: {session_id}")))?;
    let before_session_seq = store.export_message_before_session_seq(session_id).await?;
    let evidence_through_id = if options
        .include
        .contains(SessionExportInclude::ProviderInputEvidence)
    {
        store
            .latest_export_context_evidence_id(session_id, before_session_seq)
            .await?
    } else {
        0
    };
    let mailbox_through_id = if options.include.contains(SessionExportInclude::Messages) {
        store.latest_agent_mailbox_event_id(session_id).await?
    } else {
        0
    };
    let sections =
        assemble_export_sections(store, session_id, &summary, &options, before_session_seq).await?;
    Ok(PreparedExport {
        summary,
        sections,
        options,
        before_session_seq,
        evidence_through_id,
        mailbox_through_id,
    })
}

enum ExportSink<'a> {
    Writer(&'a mut (dyn AsyncWrite + Unpin + Send)),
    Channel(mpsc::Sender<Result<Vec<u8>>>),
}

impl ExportSink<'_> {
    async fn write(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        match self {
            Self::Writer(writer) => writer.write_all(bytes).await.map_err(Into::into),
            Self::Channel(sender) => sender
                .send(Ok(bytes.to_vec()))
                .await
                .map_err(|_| Error::Message("session export consumer closed".to_string())),
        }
    }
}

struct TemporaryExportPath {
    path: PathBuf,
    armed: bool,
}

impl TemporaryExportPath {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }

    async fn remove(&mut self) {
        if self.armed {
            let _ = tokio::fs::remove_file(&self.path).await;
            self.armed = false;
        }
    }
}

impl Drop for TemporaryExportPath {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let path = self.path.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = tokio::fs::remove_file(path).await;
            });
        }
    }
}

#[cfg(not(windows))]
async fn replace_export_file(source: &Path, destination: &Path) -> Result<()> {
    tokio::fs::rename(source, destination)
        .await
        .map_err(Into::into)
}

#[cfg(windows)]
async fn replace_export_file(source: &Path, destination: &Path) -> Result<()> {
    let source = source.to_path_buf();
    let destination = destination.to_path_buf();
    tokio::task::spawn_blocking(move || crate::host_process::replace_file(&source, &destination))
        .await
        .map_err(|error| Error::Message(format!("export replace task failed: {error}")))??;
    Ok(())
}

async fn write_prepared_export(
    store: &StateRuntime,
    prepared: PreparedExport,
    sink: &mut ExportSink<'_>,
) -> Result<usize> {
    match prepared.options.format {
        SessionExportFormat::Markdown => write_markdown_export(store, prepared, sink).await,
        SessionExportFormat::Json => write_json_export(store, prepared, sink).await,
    }
}

async fn sink_write_counted(
    sink: &mut ExportSink<'_>,
    bytes: &[u8],
    total: &mut usize,
) -> Result<()> {
    sink.write(bytes).await?;
    *total = total.saturating_add(bytes.len());
    Ok(())
}

async fn write_markdown_export(
    store: &StateRuntime,
    mut prepared: PreparedExport,
    sink: &mut ExportSink<'_>,
) -> Result<usize> {
    let mut total = 0;
    let mut header = String::new();
    render_markdown_header(
        &mut header,
        &prepared.summary,
        prepared.sections.prompt_prefix.as_ref(),
        &prepared.options,
    );
    sink_write_counted(sink, header.as_bytes(), &mut total).await?;

    if prepared
        .options
        .include
        .contains(SessionExportInclude::Messages)
    {
        if total > 0 {
            sink_write_counted(sink, b"\n", &mut total).await?;
        }
        sink_write_counted(sink, b"## Transcript\n", &mut total).await?;
        let mut after = 0;
        loop {
            let page = store
                .load_export_message_summaries_page(
                    &prepared.summary.id,
                    after,
                    prepared.before_session_seq,
                    EXPORT_MESSAGE_PAGE_SIZE,
                )
                .await?;
            let page_len = page.len();
            for record in page {
                after = record.session_seq;
                let record = sanitized_record(
                    record,
                    prepared
                        .options
                        .include
                        .contains(SessionExportInclude::Reasoning),
                );
                write_markdown_record(sink, &record, &mut total).await?;
            }
            if page_len < EXPORT_MESSAGE_PAGE_SIZE {
                break;
            }
        }
    }

    write_markdown_mailbox_events(store, &prepared, sink, &mut total).await?;
    write_markdown_provider_evidence(store, &prepared, sink, &mut total).await?;

    prepared.sections.prompt_prefix = None;
    let tail_options = SessionExportOptions {
        format: prepared.options.format,
        include: SessionExportIncludeSet::from_values(prepared.options.include.values().filter(
            |include| {
                !matches!(
                    include,
                    SessionExportInclude::Header
                        | SessionExportInclude::Messages
                        | SessionExportInclude::Reasoning
                )
            },
        )),
        artifact_kind: prepared.options.artifact_kind,
    };
    let tail = render_markdown(&prepared.summary, &prepared.sections, &tail_options);
    if !tail.is_empty() {
        if total > 0 {
            sink_write_counted(sink, b"\n", &mut total).await?;
        }
        sink_write_counted(sink, tail.as_bytes(), &mut total).await?;
    }
    Ok(total)
}

async fn write_markdown_record(
    sink: &mut ExportSink<'_>,
    record: &ExportMessageRecord,
    total: &mut usize,
) -> Result<()> {
    let mut rendered = String::from("\n");
    render_markdown_message(&mut rendered, record);
    sink_write_counted(sink, rendered.as_bytes(), total).await
}

async fn write_markdown_mailbox_events(
    store: &StateRuntime,
    prepared: &PreparedExport,
    sink: &mut ExportSink<'_>,
    total: &mut usize,
) -> Result<()> {
    if prepared.mailbox_through_id == 0
        || !prepared
            .options
            .include
            .contains(SessionExportInclude::Messages)
    {
        return Ok(());
    }
    let mut after = None;
    let mut wrote_heading = false;
    loop {
        let page = store
            .load_agent_mailbox_events_page(
                &prepared.summary.id,
                after,
                prepared.mailbox_through_id,
                EXPORT_MESSAGE_PAGE_SIZE,
            )
            .await?;
        let page_len = page.len();
        for record in page {
            after = Some((record.created_at_ms, record.id));
            if !wrote_heading {
                if *total > 0 {
                    sink_write_counted(sink, b"\n", total).await?;
                }
                sink_write_counted(sink, b"## Mailbox Events\n", total).await?;
                wrote_heading = true;
            }
            let mut rendered = String::new();
            render_markdown_mailbox_event(&mut rendered, &export_mailbox_event_value(record));
            sink_write_counted(sink, rendered.as_bytes(), total).await?;
        }
        if page_len < EXPORT_MESSAGE_PAGE_SIZE {
            break;
        }
    }
    Ok(())
}

async fn write_markdown_provider_evidence(
    store: &StateRuntime,
    prepared: &PreparedExport,
    sink: &mut ExportSink<'_>,
    total: &mut usize,
) -> Result<()> {
    if prepared.evidence_through_id == 0
        || !prepared
            .options
            .include
            .contains(SessionExportInclude::ProviderInputEvidence)
    {
        return Ok(());
    }
    let mut after = None;
    let mut current_prompt = None;
    let mut wrote_heading = false;
    loop {
        let page = store
            .load_export_context_evidence_page(
                &prepared.summary.id,
                after,
                prepared.before_session_seq,
                prepared.evidence_through_id,
                EXPORT_MESSAGE_PAGE_SIZE,
            )
            .await?;
        let page_len = page.len();
        for record in page {
            after = Some((record.prompt_session_seq, record.context_seq, record.id));
            if !wrote_heading {
                if *total > 0 {
                    sink_write_counted(sink, b"\n", total).await?;
                }
                sink_write_counted(sink, b"## Provider Input Evidence\n", total).await?;
                wrote_heading = true;
            }
            let mut rendered = String::new();
            if current_prompt != Some(record.prompt_session_seq) {
                current_prompt = Some(record.prompt_session_seq);
                push_line(&mut rendered, "");
                push_line(
                    &mut rendered,
                    &format!("### Prompt message #{}", record.prompt_session_seq),
                );
            }
            render_markdown_evidence_item(&mut rendered, &export_evidence_item(record));
            sink_write_counted(sink, rendered.as_bytes(), total).await?;
        }
        if page_len < EXPORT_MESSAGE_PAGE_SIZE {
            break;
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct ExportMessageRef<'a> {
    session_seq: i64,
    message: &'a Message,
}

async fn write_json_export(
    store: &StateRuntime,
    mut prepared: PreparedExport,
    sink: &mut ExportSink<'_>,
) -> Result<usize> {
    let mut total = 0;
    sink_write_counted(sink, b"{", &mut total).await?;
    let header = export_header(
        &prepared.summary,
        prepared.sections.prompt_prefix.take(),
        &prepared.options,
    );
    let last_provider_request = prepared.sections.last_request.take();
    let last_provider_response = prepared.sections.last_response.take();
    let mut first_field = true;
    if let Some(header) = header {
        write_json_value_field(sink, "header", &header, &mut first_field, &mut total).await?;
    }
    if prepared
        .options
        .include
        .contains(SessionExportInclude::Messages)
    {
        write_json_messages_field(store, &prepared, sink, &mut first_field, &mut total).await?;
    }
    write_json_mailbox_events(store, &prepared, sink, &mut first_field, &mut total).await?;
    write_json_provider_evidence(store, &prepared, sink, &mut first_field, &mut total).await?;
    if let Some(request) = last_provider_request {
        write_json_value_field(
            sink,
            "last_provider_request",
            &request,
            &mut first_field,
            &mut total,
        )
        .await?;
    }
    if let Some(response) = last_provider_response {
        write_json_value_field(
            sink,
            "last_provider_response",
            &response,
            &mut first_field,
            &mut total,
        )
        .await?;
    }
    if !first_field {
        sink_write_counted(sink, b"\n", &mut total).await?;
    }
    sink_write_counted(sink, b"}", &mut total).await?;
    Ok(total)
}

async fn write_json_messages_field(
    store: &StateRuntime,
    prepared: &PreparedExport,
    sink: &mut ExportSink<'_>,
    first_field: &mut bool,
    total: &mut usize,
) -> Result<()> {
    write_json_field_prefix(sink, "messages", first_field, total).await?;
    let include_reasoning = prepared
        .options
        .include
        .contains(SessionExportInclude::Reasoning);
    let mut after = 0;
    let mut wrote_any = false;
    loop {
        let page = store
            .load_export_message_summaries_page(
                &prepared.summary.id,
                after,
                prepared.before_session_seq,
                EXPORT_MESSAGE_PAGE_SIZE,
            )
            .await?;
        let page_len = page.len();
        for record in page {
            after = record.session_seq;
            let record = sanitized_record(record, include_reasoning);
            if !wrote_any {
                sink_write_counted(sink, b"[", total).await?;
            }
            write_json_array_item(
                sink,
                &ExportMessageRef {
                    session_seq: record.session_seq,
                    message: &record.message,
                },
                wrote_any,
                total,
            )
            .await?;
            wrote_any = true;
        }
        if page_len < EXPORT_MESSAGE_PAGE_SIZE {
            break;
        }
    }
    if wrote_any {
        sink_write_counted(sink, b"\n  ]", total).await
    } else {
        sink_write_counted(sink, b"[]", total).await
    }
}

async fn write_json_mailbox_events(
    store: &StateRuntime,
    prepared: &PreparedExport,
    sink: &mut ExportSink<'_>,
    first_field: &mut bool,
    total: &mut usize,
) -> Result<()> {
    if prepared.mailbox_through_id == 0
        || !prepared
            .options
            .include
            .contains(SessionExportInclude::Messages)
    {
        return Ok(());
    }
    write_json_field_prefix(sink, "mailbox_events", first_field, total).await?;
    sink_write_counted(sink, b"[", total).await?;
    let mut after = None;
    let mut wrote_any = false;
    loop {
        let page = store
            .load_agent_mailbox_events_page(
                &prepared.summary.id,
                after,
                prepared.mailbox_through_id,
                EXPORT_MESSAGE_PAGE_SIZE,
            )
            .await?;
        let page_len = page.len();
        for record in page {
            after = Some((record.created_at_ms, record.id));
            write_json_array_item(sink, &export_mailbox_event_value(record), wrote_any, total)
                .await?;
            wrote_any = true;
        }
        if page_len < EXPORT_MESSAGE_PAGE_SIZE {
            break;
        }
    }
    sink_write_counted(sink, b"\n  ]", total).await
}

async fn write_json_provider_evidence(
    store: &StateRuntime,
    prepared: &PreparedExport,
    sink: &mut ExportSink<'_>,
    first_field: &mut bool,
    total: &mut usize,
) -> Result<()> {
    if prepared.evidence_through_id == 0
        || !prepared
            .options
            .include
            .contains(SessionExportInclude::ProviderInputEvidence)
    {
        return Ok(());
    }
    write_json_field_prefix(sink, "provider_input_evidence", first_field, total).await?;
    sink_write_counted(sink, b"[", total).await?;
    let mut after = None;
    let mut current_prompt = None;
    let mut wrote_prompt = false;
    let mut wrote_item = false;
    loop {
        let page = store
            .load_export_context_evidence_page(
                &prepared.summary.id,
                after,
                prepared.before_session_seq,
                prepared.evidence_through_id,
                EXPORT_MESSAGE_PAGE_SIZE,
            )
            .await?;
        let page_len = page.len();
        for record in page {
            after = Some((record.prompt_session_seq, record.context_seq, record.id));
            if current_prompt != Some(record.prompt_session_seq) {
                if wrote_prompt {
                    sink_write_counted(sink, b"\n      ]\n    }", total).await?;
                    sink_write_counted(sink, b",", total).await?;
                }
                current_prompt = Some(record.prompt_session_seq);
                wrote_prompt = true;
                wrote_item = false;
                let prefix = format!(
                    "\n    {{\n      \"prompt_session_seq\": {},\n      \"items\": [",
                    record.prompt_session_seq
                );
                sink_write_counted(sink, prefix.as_bytes(), total).await?;
            }
            if wrote_item {
                sink_write_counted(sink, b",", total).await?;
            }
            let serialized = serde_json::to_string_pretty(&export_evidence_item(record))?;
            let indented = indent_json(&serialized, 8);
            sink_write_counted(sink, b"\n", total).await?;
            sink_write_counted(sink, indented.as_bytes(), total).await?;
            wrote_item = true;
        }
        if page_len < EXPORT_MESSAGE_PAGE_SIZE {
            break;
        }
    }
    if wrote_prompt {
        sink_write_counted(sink, b"\n      ]\n    }", total).await?;
    }
    sink_write_counted(sink, b"\n  ]", total).await
}

async fn write_json_array_item<T: Serialize>(
    sink: &mut ExportSink<'_>,
    value: &T,
    comma: bool,
    total: &mut usize,
) -> Result<()> {
    if comma {
        sink_write_counted(sink, b",", total).await?;
    }
    let serialized = serde_json::to_string_pretty(value)?;
    let indented = indent_json(&serialized, 4);
    sink_write_counted(sink, b"\n", total).await?;
    sink_write_counted(sink, indented.as_bytes(), total).await
}

async fn write_json_value_field<T: Serialize>(
    sink: &mut ExportSink<'_>,
    name: &str,
    value: &T,
    first_field: &mut bool,
    total: &mut usize,
) -> Result<()> {
    write_json_field_prefix(sink, name, first_field, total).await?;
    let serialized = serde_json::to_string_pretty(value)?;
    let indented = indent_json_continuation(&serialized, 2);
    sink_write_counted(sink, indented.as_bytes(), total).await
}

async fn write_json_field_prefix(
    sink: &mut ExportSink<'_>,
    name: &str,
    first_field: &mut bool,
    total: &mut usize,
) -> Result<()> {
    if !*first_field {
        sink_write_counted(sink, b",", total).await?;
    }
    *first_field = false;
    let prefix = format!("\n  {}: ", serde_json::to_string(name)?);
    sink_write_counted(sink, prefix.as_bytes(), total).await
}

fn indent_json(value: &str, spaces: usize) -> String {
    let indent = " ".repeat(spaces);
    format!("{indent}{}", value.replace('\n', &format!("\n{indent}")))
}

fn indent_json_continuation(value: &str, spaces: usize) -> String {
    let indent = " ".repeat(spaces);
    value.replace('\n', &format!("\n{indent}"))
}

fn sanitized_record(
    record: SessionExportMessageSummary,
    include_reasoning: bool,
) -> ExportMessageRecord {
    ExportMessageRecord {
        session_seq: record.session_seq,
        message: if include_reasoning {
            sanitize_reasoning_for_export(&record.message)
        } else {
            sanitize_message_without_reasoning(&record.message)
        },
        usage: record.usage,
        metadata: record.metadata,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use psychevo_agent_core::{AssistantBlock, user_text_message};
    use psychevo_ai::Outcome;
    use tempfile::TempDir;

    use crate::session_export::{SessionArtifactKind, render_session_export};
    use crate::store::{AgentMailboxEventInput, ContextEvidenceInput};

    fn assistant_text_message(text: &str) -> Message {
        Message::Assistant {
            content: vec![AssistantBlock::Text {
                text: text.to_string(),
            }],
            timestamp_ms: 1,
            finish_reason: Some("stop".to_string()),
            outcome: Outcome::Normal,
            model: None,
            provider: None,
        }
    }

    #[tokio::test]
    async fn file_and_byte_stream_match_in_memory_render_across_message_pages() {
        let temp = TempDir::new().expect("temp");
        let store = StateRuntime::open(&temp.path().join("state.db"))
            .await
            .expect("store");
        let session_id = store
            .create_session_with_metadata(temp.path(), "run", "model", "provider", None)
            .await
            .expect("session");
        for index in 0..(EXPORT_MESSAGE_PAGE_SIZE + 7) {
            store
                .append_message(
                    &session_id,
                    &user_text_message(format!("message {index}\nsecond line")),
                )
                .await
                .expect("message");
        }
        store
            .append_agent_mailbox_event(AgentMailboxEventInput {
                parent_session_id: session_id.clone(),
                child_session_id: None,
                agent_id: "worker-id".to_string(),
                task_name: Some("worker".to_string()),
                agent_name: "worker".to_string(),
                content_text: "mailbox payload".to_string(),
                payload: serde_json::json!({"content": "mailbox payload"}),
                metadata: None,
            })
            .await
            .expect("mailbox");

        for format in [SessionExportFormat::Markdown, SessionExportFormat::Json] {
            let options = SessionExportOptions {
                format,
                include: SessionExportIncludeSet::from_values([
                    SessionExportInclude::Header,
                    SessionExportInclude::Messages,
                    SessionExportInclude::LastProviderResponse,
                ]),
                artifact_kind: SessionArtifactKind::Export,
            };
            let rendered = render_session_export(&store, &session_id, options.clone())
                .await
                .expect("render");
            let output = temp.path().join(format!("output.{}", format.extension()));
            let written = write_session_export(&store, &session_id, &output, options.clone())
                .await
                .expect("write");
            let file = tokio::fs::read(&output).await.expect("file");
            assert_eq!(file, rendered.content.as_bytes());
            assert_eq!(written.bytes, file.len());

            let mut stream = stream_session_export(&store, &session_id, options)
                .await
                .expect("stream");
            let mut streamed = Vec::new();
            while let Some(chunk) = stream.next().await {
                streamed.extend(chunk.expect("chunk"));
            }
            assert_eq!(streamed, file);
        }
    }

    #[tokio::test]
    async fn cancelled_export_temp_path_is_removed_asynchronously() {
        let temp = TempDir::new().expect("temp");
        let path = temp.path().join(".cancelled.tmp");
        tokio::fs::write(&path, b"partial")
            .await
            .expect("partial file");
        drop(TemporaryExportPath::new(path.clone()));
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                match tokio::fs::try_exists(&path).await {
                    Ok(false) => break,
                    Ok(true) => tokio::task::yield_now().await,
                    #[cfg(windows)]
                    Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                        tokio::task::yield_now().await;
                    }
                    Err(error) => panic!("temp existence: {error}"),
                }
            }
        })
        .await
        .expect("cancellation cleanup");
    }

    #[tokio::test]
    async fn stream_uses_an_acceptance_high_watermark() {
        let temp = TempDir::new().expect("temp");
        let store = StateRuntime::open(&temp.path().join("state.db"))
            .await
            .expect("store");
        let session_id = store
            .create_session_with_metadata(temp.path(), "run", "model", "provider", None)
            .await
            .expect("session");
        store
            .append_message(&session_id, &user_text_message("accepted"))
            .await
            .expect("accepted message");
        let options = SessionExportOptions {
            format: SessionExportFormat::Json,
            include: SessionExportIncludeSet::from_values([SessionExportInclude::Messages]),
            artifact_kind: SessionArtifactKind::Export,
        };
        let mut stream = stream_session_export(&store, &session_id, options)
            .await
            .expect("stream");
        store
            .append_message(&session_id, &user_text_message("too late"))
            .await
            .expect("late message");
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            bytes.extend(chunk.expect("chunk"));
        }
        let text = String::from_utf8(bytes).expect("utf8");
        assert!(text.contains("accepted"));
        assert!(!text.contains("too late"));
    }

    #[tokio::test]
    async fn in_memory_collector_stops_at_the_first_chunk_over_its_bound() {
        let chunks = futures::stream::unfold(0, |index| async move {
            match index {
                0 => Some((Ok(vec![0; 9]), 1)),
                _ => panic!("collector polled after the limit was exceeded"),
            }
        });

        let error = collect_byte_stream_with_limit(chunks, 8)
            .await
            .expect_err("oversized stream");
        assert_eq!(
            error
                .structured_data()
                .and_then(|details| details["code"].as_str()),
            Some("export_too_large_for_memory")
        );
    }

    #[tokio::test]
    async fn provider_evidence_streams_across_keyset_pages_with_byte_exact_output() {
        let temp = TempDir::new().expect("temp");
        let store = StateRuntime::open(&temp.path().join("state.db"))
            .await
            .expect("store");
        let session_id = store
            .create_session_with_metadata(temp.path(), "run", "model", "provider", None)
            .await
            .expect("session");
        let evidence = (0..(EXPORT_MESSAGE_PAGE_SIZE + 7))
            .map(|index| ContextEvidenceInput {
                role: "user".to_string(),
                source_kind: "test".to_string(),
                source_name: Some(format!("source-{index}")),
                source_path: None,
                provider_group: None,
                provider_block_index: Some(index as i64),
                context_kind: Some("selected".to_string()),
                content_text: format!("evidence {index}"),
                metadata: Some(serde_json::json!({"index": index})),
            })
            .collect::<Vec<_>>();
        store
            .append_message_with_undo_snapshot_metadata_and_context_evidence(
                &session_id,
                &user_text_message("prompt"),
                None,
                None,
                &evidence,
            )
            .await
            .expect("prompt evidence");

        for format in [SessionExportFormat::Markdown, SessionExportFormat::Json] {
            let options = SessionExportOptions {
                format,
                include: SessionExportIncludeSet::from_values([
                    SessionExportInclude::ProviderInputEvidence,
                ]),
                artifact_kind: SessionArtifactKind::Export,
            };
            let rendered = render_session_export(&store, &session_id, options.clone())
                .await
                .expect("render");
            let mut stream = stream_session_export(&store, &session_id, options)
                .await
                .expect("stream");
            let mut streamed = Vec::new();
            while let Some(chunk) = stream.next().await {
                streamed.extend(chunk.expect("chunk"));
            }
            assert_eq!(streamed, rendered.content.as_bytes());
        }
    }

    #[tokio::test]
    async fn last_provider_request_loader_keeps_a_bounded_trailing_region() {
        let temp = TempDir::new().expect("temp");
        let store = StateRuntime::open(&temp.path().join("state.db"))
            .await
            .expect("store");
        let session_id = store
            .create_session_with_metadata(temp.path(), "run", "model", "provider", None)
            .await
            .expect("session");
        for index in 0..20 {
            store
                .append_message(
                    &session_id,
                    &user_text_message(format!("{index}:{}", "x".repeat(80))),
                )
                .await
                .expect("message");
        }

        let (messages, truncated) =
            super::super::runtime::load_bounded_last_provider_request_messages_with_limit(
                &store,
                &session_id,
                store
                    .export_message_before_session_seq(&session_id)
                    .await
                    .expect("message boundary"),
                600,
            )
            .await
            .expect("bounded messages");

        assert!(truncated);
        assert!(!messages.is_empty());
        assert_eq!(messages.last().map(|message| message.session_seq), Some(20));
        assert!(
            messages
                .first()
                .is_some_and(|message| message.session_seq > 1)
        );
    }

    #[tokio::test]
    async fn targeted_sections_share_the_accepted_transcript_watermark() {
        let temp = TempDir::new().expect("temp");
        let store = StateRuntime::open(&temp.path().join("state.db"))
            .await
            .expect("store");
        let session_id = store
            .create_session_with_metadata(temp.path(), "run", "model", "provider", None)
            .await
            .expect("session");
        store
            .append_message(&session_id, &user_text_message("accepted prompt"))
            .await
            .expect("accepted prompt");
        store
            .append_message(&session_id, &assistant_text_message("accepted response"))
            .await
            .expect("accepted response");
        let accepted_before = store
            .export_message_before_session_seq(&session_id)
            .await
            .expect("accepted watermark");
        store
            .append_message(&session_id, &user_text_message("late prompt"))
            .await
            .expect("late prompt");
        store
            .append_message(&session_id, &assistant_text_message("late response"))
            .await
            .expect("late response");
        let summary = store
            .session_summary(&session_id)
            .await
            .expect("summary query")
            .expect("summary");
        let options = SessionExportOptions {
            format: SessionExportFormat::Json,
            include: SessionExportIncludeSet::from_values([
                SessionExportInclude::LastProviderResponse,
            ]),
            artifact_kind: SessionArtifactKind::Export,
        };

        let sections = super::super::runtime::assemble_export_sections(
            &store,
            &session_id,
            &summary,
            &options,
            accepted_before,
        )
        .await
        .expect("targeted sections");

        let response = sections.last_response.expect("accepted response");
        assert_eq!(response.assistant_session_seq, 2);
        assert_eq!(
            response.message,
            assistant_text_message("accepted response")
        );
    }

    #[tokio::test]
    async fn lpr_byte_bound_ignores_a_newer_incomplete_record() {
        let temp = TempDir::new().expect("temp");
        let store = StateRuntime::open(&temp.path().join("state.db"))
            .await
            .expect("store");
        let session_id = store
            .create_session_with_metadata(temp.path(), "run", "model", "provider", None)
            .await
            .expect("session");
        store
            .append_message(&session_id, &user_text_message("completed prompt"))
            .await
            .expect("completed prompt");
        store
            .append_message(&session_id, &assistant_text_message("completed response"))
            .await
            .expect("completed response");
        store
            .append_message(&session_id, &user_text_message("x".repeat(4_096)))
            .await
            .expect("incomplete trailing prompt");

        let assistant = store
            .latest_export_assistant_message_summary(
                &session_id,
                store
                    .export_message_before_session_seq(&session_id)
                    .await
                    .expect("message boundary"),
            )
            .await
            .expect("assistant query")
            .expect("completed assistant");
        let (messages, truncated) =
            super::super::runtime::load_bounded_last_provider_request_messages_with_limit(
                &store,
                &session_id,
                assistant.session_seq.saturating_add(1),
                1_024,
            )
            .await
            .expect("bounded completed generation");

        assert!(!truncated);
        assert_eq!(
            messages
                .iter()
                .map(|message| message.session_seq)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[tokio::test]
    #[ignore = "explicit 100k-message export acceptance"]
    async fn exports_one_hundred_thousand_messages_in_bounded_pages() {
        let temp = TempDir::new().expect("temp");
        let store = StateRuntime::open(&temp.path().join("state.db"))
            .await
            .expect("store");
        let session_id = store
            .create_session_with_metadata(temp.path(), "run", "model", "provider", None)
            .await
            .expect("session");
        let message_json =
            serde_json::to_string(&user_text_message("bounded export row")).expect("message json");
        let mut connection = store.acquire_sqlx().await.expect("connection");
        sqlx::query(
            r#"
            WITH RECURSIVE sequence(value) AS (
                SELECT 1
                UNION ALL
                SELECT value + 1 FROM sequence WHERE value < 100000
            )
            INSERT INTO messages (
                session_id, session_seq, role, timestamp_ms, message_json, content_text
            )
            SELECT ?1, value, 'user', value, ?2, 'bounded export row'
            FROM sequence
            "#,
        )
        .bind(&session_id)
        .bind(message_json)
        .execute(&mut *connection)
        .await
        .expect("100k rows");
        drop(connection);

        let before = store.diagnostics();
        let output = temp.path().join("large.md");
        let result = write_session_export(
            &store,
            &session_id,
            &output,
            SessionExportOptions {
                format: SessionExportFormat::Markdown,
                include: SessionExportIncludeSet::from_values([SessionExportInclude::Messages]),
                artifact_kind: SessionArtifactKind::Export,
            },
        )
        .await
        .expect("large export");
        let after = store.diagnostics();
        assert!(result.bytes > 1_000_000, "wrote {} bytes", result.bytes);
        assert!(
            after
                .completed_operations
                .saturating_sub(before.completed_operations)
                < 410,
            "100k rows should require only fixed-size keyset pages"
        );
        let content = tokio::fs::read_to_string(output)
            .await
            .expect("large output");
        assert!(content.contains("### 1. User"));
        assert!(content.contains("### 100000. User"));
    }
}
