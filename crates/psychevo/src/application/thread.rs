use super::*;

impl Client {
    pub async fn start_thread(&self, request: StartThreadRequest) -> Result<Thread> {
        let cwd = canonicalize_cwd(&request.cwd)?;
        let runtime = self.inner.runtime.clone();
        let admission = runtime.begin_admission().await?;
        let reservation = runtime.reserve_application_operation()?;
        let state = self.inner.state.clone();
        let (result_tx, result_rx) = oneshot::channel();
        runtime.spawn(async move {
            drop(admission);
            let result = async {
                let _reservation = reservation.acquire().await?;
                state
                    .create_session_with_metadata(
                        &cwd,
                        &request.source,
                        "pending",
                        "pending",
                        request.metadata,
                    )
                    .await
            }
            .await;
            let _ = result_tx.send(result);
        });
        let id = result_rx.await.map_err(|_| {
            Error::Message("accepted start_thread task ended without a result".to_string())
        })??;
        Ok(Thread {
            client: self.clone(),
            id,
        })
    }

    pub async fn resume_thread(&self, id: impl Into<String>) -> Result<Thread> {
        self.ensure_open()?;
        let id = id.into();
        self.inner
            .state
            .session_summary(&id)
            .await?
            .ok_or_else(|| Error::Message(format!("thread not found: {id}")))?;
        Ok(Thread {
            client: self.clone(),
            id,
        })
    }

    pub async fn list_threads(&self, mut query: ThreadListQuery) -> Result<ThreadListPage> {
        self.ensure_open()?;
        let cwd = query
            .cwd
            .as_deref()
            .map(canonicalize_cwd)
            .transpose()?
            .map(|cwd| cwd.to_string_lossy().into_owned());
        query.sources.sort();
        query.sources.dedup();
        let cursor = query
            .cursor
            .as_deref()
            .map(|cursor| {
                decode_thread_list_cursor(cursor, cwd.as_deref(), query.archived, &query.sources)
            })
            .transpose()?;
        let page = self
            .inner
            .state
            .list_session_summary_page(
                cwd.as_deref(),
                &query.sources,
                query.archived,
                cursor.as_ref(),
                query.limit.clamp(1, MAX_THREAD_LIST_LIMIT),
            )
            .await?;
        let threads = page
            .summaries
            .into_iter()
            .map(|summary| self.summary_from_summary(summary))
            .collect();
        let next_cursor = page
            .next_cursor
            .map(|cursor| encode_thread_list_cursor(cwd, query.archived, query.sources, cursor))
            .transpose()?;
        Ok(ThreadListPage {
            threads,
            next_cursor,
        })
    }

    fn summary_from_summary(&self, summary: SessionSummary) -> ThreadSummary {
        let (_, active_turn_id, _) = self.inner.runtime.thread_activity(&summary.id);
        ThreadSummary::from_summary(summary, active_turn_id)
    }

    async fn snapshot_from_summary(&self, summary: SessionSummary) -> Result<ThreadSnapshot> {
        let summary = self.summary_from_summary(summary);
        let pending_interactions = self
            .inner
            .state
            .framework_interactions_for_thread(&summary.id, true)
            .await?
            .into_iter()
            .map(PendingInteraction::from)
            .collect();
        let history = HistoryReader::new(self.inner.state.clone(), summary.id.clone())
            .latest(None)
            .await?;
        Ok(ThreadSnapshot::from_summary(
            summary,
            pending_interactions,
            history.items,
            history.next_before,
        ))
    }
}

impl StartThreadRequest {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            source: "sdk".to_string(),
            metadata: None,
        }
    }
}

impl Default for ThreadListQuery {
    fn default() -> Self {
        Self {
            cwd: None,
            archived: false,
            sources: Vec::new(),
            cursor: None,
            limit: DEFAULT_THREAD_LIST_LIMIT,
        }
    }
}

fn encode_thread_list_cursor(
    cwd: Option<String>,
    archived: bool,
    sources: Vec<String>,
    position: SessionListCursor,
) -> Result<String> {
    let cursor = ThreadListCursor {
        cwd,
        archived,
        sources,
        position,
    };
    Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&cursor)?))
}

fn decode_thread_list_cursor(
    encoded: &str,
    cwd: Option<&str>,
    archived: bool,
    sources: &[String],
) -> Result<SessionListCursor> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| Error::Message("invalid thread list cursor".to_string()))?;
    let cursor = serde_json::from_slice::<ThreadListCursor>(&bytes)
        .map_err(|_| Error::Message("invalid thread list cursor".to_string()))?;
    if cursor.cwd.as_deref() != cwd || cursor.archived != archived || cursor.sources != sources {
        return Err(Error::Message(
            "thread list cursor does not match the current filters".to_string(),
        ));
    }
    Ok(cursor.position)
}

impl fmt::Debug for Thread {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Thread")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl Thread {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub async fn snapshot(&self) -> Result<ThreadSnapshot> {
        let summary = self
            .client
            .inner
            .state
            .session_summary(&self.id)
            .await?
            .ok_or_else(|| Error::Message(format!("thread not found: {}", self.id)))?;
        self.client.snapshot_from_summary(summary).await
    }

    pub async fn archive(&self) -> Result<()> {
        self.enqueue_mutation(|thread| async move {
            thread
                .client
                .inner
                .state
                .archive_session(&thread.id)
                .await?;
            thread.client.inner.runtime.remove_mcp_runtime(&thread.id);
            Ok(())
        })
        .await
    }

    pub async fn delete(&self) -> Result<()> {
        self.enqueue_mutation(|thread| async move {
            thread.client.inner.state.delete_session(&thread.id).await?;
            thread.client.inner.runtime.remove_mcp_runtime(&thread.id);
            Ok(())
        })
        .await
    }

    pub async fn compact(&self, request: CompactThreadRequest) -> Result<CompactionResult> {
        self.enqueue_compact(request).await
    }

    fn enqueue_compact(
        &self,
        request: CompactThreadRequest,
    ) -> BoxFuture<'static, Result<CompactionResult>> {
        self.enqueue_mutation(move |thread| async move { thread.compact_reserved(request).await })
    }

    async fn compact_reserved(&self, request: CompactThreadRequest) -> Result<CompactionResult> {
        let snapshot = self.snapshot().await?;
        let inherited_env = self.client.application_environment(request.inherited_env);
        crate::compaction::compact_session(CompactSessionOptions {
            state: self.client.inner.state.clone(),
            cwd: PathBuf::from(snapshot.cwd.clone()),
            session: self.id.clone(),
            config_path: request
                .config_path
                .or_else(|| self.client.inner.config_path.clone()),
            model: request.model,
            reasoning_effort: request.reasoning_effort,
            inherited_env: Some(inherited_env),
            reason: request.reason,
            instructions: request.instructions,
            force: request.force,
        })
        .await
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __enqueue_compact(
        &self,
        request: CompactThreadRequest,
    ) -> BoxFuture<'static, Result<CompactionResult>> {
        self.enqueue_compact(request)
    }

    pub async fn fork(&self, request: ForkThreadRequest) -> Result<Thread> {
        self.enqueue_mutation(move |thread| async move {
            let id = thread
                .client
                .inner
                .state
                .fork_native_session_history(NativeSessionForkInput {
                    source_session_id: &thread.id,
                    before_session_seq: request.before_session_seq,
                })
                .await?;
            Ok(Thread {
                client: thread.client,
                id,
            })
        })
        .await
    }

    fn enqueue_mutation<T, F, Fut>(&self, operation: F) -> BoxFuture<'static, Result<T>>
    where
        T: Send + 'static,
        F: FnOnce(Thread) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T>> + Send + 'static,
    {
        let thread = self.clone();
        Box::pin(async move {
            let runtime = thread.client.inner.runtime.clone();
            let admission = runtime.begin_admission().await?;
            let reservation = runtime.reserve_mutation(&thread.id)?;
            let (result_tx, result_rx) = oneshot::channel();
            runtime.spawn(async move {
                drop(admission);
                let result = async {
                    let _reservation = reservation.acquire().await?;
                    operation(thread).await
                }
                .await;
                let _ = result_tx.send(result);
            });
            result_rx.await.map_err(|_| {
                Error::Message("accepted Thread mutation ended without a result".to_string())
            })?
        })
    }

    pub async fn respond(
        &self,
        interaction_id: &str,
        response: InteractionResponse,
    ) -> Result<InteractionResponseReceipt> {
        match self
            .client
            .inner
            .runtime
            .thread_turn_handles(&self.id)
            .into_iter()
            .next()
        {
            Some(turn) => turn.respond(interaction_id, response).await,
            None => Ok(InteractionResponseReceipt { accepted: false }),
        }
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __activity(&self) -> (bool, Option<String>, usize) {
        self.client.inner.runtime.thread_activity(&self.id)
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __interrupt_all(&self) -> (bool, usize) {
        let handles = self.client.inner.runtime.thread_turn_handles(&self.id);
        let mut interrupted = false;
        let mut cleared = 0;
        for (index, handle) in handles.into_iter().enumerate() {
            handle.interrupt();
            if index == 0 {
                interrupted = true;
            } else {
                cleared += 1;
            }
        }
        (interrupted, cleared)
    }

    #[doc(hidden)]
    #[cfg(feature = "product")]
    pub fn __steer(
        &self,
        expected_turn_id: &str,
        input: impl Into<String>,
    ) -> std::result::Result<bool, psychevo_agent_core::ControlInputError> {
        let (_, active_turn_id, _) = self.client.inner.runtime.thread_activity(&self.id);
        if active_turn_id.as_deref() != Some(expected_turn_id) {
            return Ok(false);
        }
        match self.client.inner.runtime.turn_handle(expected_turn_id) {
            Some(turn) => turn.steer(input).map(|()| true),
            None => Ok(false),
        }
    }

    pub async fn pending_interactions(&self) -> Result<Vec<PendingInteraction>> {
        Ok(self
            .client
            .inner
            .state
            .framework_interactions_for_thread(&self.id, true)
            .await?
            .into_iter()
            .map(PendingInteraction::from)
            .collect())
    }

    pub fn history(&self) -> HistoryReader {
        HistoryReader::new(self.client.inner.state.clone(), self.id.clone())
    }

    #[cfg(test)]
    pub(super) fn has_activity(&self) -> bool {
        self.client.inner.runtime.thread_activity(&self.id).0
    }
}

impl ThreadSummary {
    fn from_summary(summary: SessionSummary, active_turn_id: Option<String>) -> Self {
        Self {
            id: summary.id,
            source: summary.source,
            cwd: summary.cwd,
            title: summary.title,
            started_at_ms: summary.started_at_ms,
            updated_at_ms: summary.updated_at_ms,
            archived: summary.archived_at_ms.is_some(),
            message_count: summary.message_count,
            tool_call_count: summary.tool_call_count,
            active_turn_id,
        }
    }
}

impl ThreadExecutionContext {
    pub(super) fn from_summary(summary: SessionSummary) -> Self {
        Self {
            id: summary.id,
            cwd: summary.cwd,
            source: summary.source,
        }
    }
}

impl HistoryReader {
    pub(super) fn new(state: StateRuntime, thread_id: String) -> Self {
        Self { state, thread_id }
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub async fn latest(&self, limit: Option<usize>) -> Result<HistoryPage> {
        self.before(None, limit).await
    }

    pub async fn before(
        &self,
        before_session_seq: Option<i64>,
        limit: Option<usize>,
    ) -> Result<HistoryPage> {
        let limit = limit
            .unwrap_or(DEFAULT_HISTORY_PAGE_SIZE)
            .clamp(1, MAX_HISTORY_PAGE_SIZE);
        let mut items = self
            .state
            .load_tui_message_summaries_before(
                &self.thread_id,
                before_session_seq,
                limit.saturating_add(1),
            )
            .await?
            .into_iter()
            .map(ThreadItem::from)
            .collect::<Vec<_>>();
        let has_more = items.len() > limit;
        if has_more {
            items.remove(0);
        }
        let next_before = has_more
            .then(|| items.first().map(|item| item.session_seq))
            .flatten();
        Ok(HistoryPage {
            thread_id: self.thread_id.clone(),
            items,
            next_before,
        })
    }
}

impl fmt::Debug for HistoryReader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HistoryReader")
            .field("thread_id", &self.thread_id)
            .finish_non_exhaustive()
    }
}

impl ThreadSnapshot {
    fn from_summary(
        summary: ThreadSummary,
        pending_interactions: Vec<PendingInteraction>,
        items: Vec<ThreadItem>,
        history_cursor: Option<i64>,
    ) -> Self {
        Self {
            summary,
            pending_interactions,
            items,
            history_cursor,
        }
    }
}

impl std::ops::Deref for ThreadSnapshot {
    type Target = ThreadSummary;

    fn deref(&self) -> &Self::Target {
        &self.summary
    }
}

impl From<crate::types::TuiMessageSummary> for ThreadItem {
    fn from(summary: crate::types::TuiMessageSummary) -> Self {
        Self {
            session_seq: summary.session_seq,
            message: summary.message,
            usage: summary.usage,
            metadata: summary.metadata,
            accounting: summary.accounting,
        }
    }
}

impl From<crate::state::FrameworkInteractionRecord> for PendingInteraction {
    fn from(record: crate::state::FrameworkInteractionRecord) -> Self {
        Self {
            interaction_id: record.interaction_id,
            thread_id: record.thread_id,
            turn_id: record.turn_id,
            kind: record.kind,
            status: record.status,
            payload: record.payload,
            resolution: record.resolution,
            requested_at_ms: record.requested_at_ms,
            resolved_at_ms: record.resolved_at_ms,
        }
    }
}
