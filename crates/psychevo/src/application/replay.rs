use serde::{Deserialize, Serialize};

use super::{DEFAULT_HISTORY_PAGE_SIZE, HistoryReader, MAX_HISTORY_PAGE_SIZE, ThreadItem};
use crate::Result;
use crate::state::store_messages::{StoredHistoryReplayDecodeField, StoredHistoryReplayItem};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistoryReplayItem {
    Available { item: Box<ThreadItem> },
    Unavailable { session_seq: i64 },
}

impl HistoryReplayItem {
    pub fn session_seq(&self) -> i64 {
        match self {
            Self::Available { item } => item.session_seq,
            Self::Unavailable { session_seq } => *session_seq,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryReplayWarningKind {
    InvalidMessage,
    InvalidUsage,
    InvalidMetadata,
    InvalidAccounting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryReplayWarning {
    pub session_seq: i64,
    pub kind: HistoryReplayWarningKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryReplayPage {
    pub thread_id: String,
    pub items: Vec<HistoryReplayItem>,
    pub next_after: Option<i64>,
    pub warnings: Vec<HistoryReplayWarning>,
}

impl HistoryReader {
    pub async fn replay_after(
        &self,
        after_session_seq: Option<i64>,
        limit: Option<usize>,
    ) -> Result<HistoryReplayPage> {
        let limit = limit
            .unwrap_or(DEFAULT_HISTORY_PAGE_SIZE)
            .clamp(1, MAX_HISTORY_PAGE_SIZE);
        let stored = self
            .state
            .load_history_replay_after(&self.thread_id, after_session_seq, limit)
            .await?;
        let mut items = Vec::with_capacity(stored.items.len());
        let mut warnings = Vec::new();
        for stored_item in stored.items {
            match stored_item {
                StoredHistoryReplayItem::Available(item) => {
                    items.push(HistoryReplayItem::Available {
                        item: Box::new(ThreadItem::from(*item)),
                    });
                }
                StoredHistoryReplayItem::Unavailable {
                    session_seq,
                    invalid_fields,
                } => {
                    items.push(HistoryReplayItem::Unavailable { session_seq });
                    warnings.extend(
                        invalid_fields
                            .into_iter()
                            .map(|field| HistoryReplayWarning {
                                session_seq,
                                kind: warning_kind(field),
                            }),
                    );
                }
            }
        }
        let next_after = stored
            .has_more
            .then(|| items.last().map(HistoryReplayItem::session_seq))
            .flatten();
        Ok(HistoryReplayPage {
            thread_id: self.thread_id.clone(),
            items,
            next_after,
            warnings,
        })
    }
}

fn warning_kind(field: StoredHistoryReplayDecodeField) -> HistoryReplayWarningKind {
    match field {
        StoredHistoryReplayDecodeField::Message => HistoryReplayWarningKind::InvalidMessage,
        StoredHistoryReplayDecodeField::Usage => HistoryReplayWarningKind::InvalidUsage,
        StoredHistoryReplayDecodeField::Metadata => HistoryReplayWarningKind::InvalidMetadata,
        StoredHistoryReplayDecodeField::Accounting => HistoryReplayWarningKind::InvalidAccounting,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{HistoryReader, HistoryReplayItem, HistoryReplayWarningKind};
    use crate::state::{SessionRevertState, StateRuntime};
    use psychevo_agent_core::{AssistantBlock, Message, user_text_message};
    use psychevo_ai::{AssistantSource, Outcome};

    #[tokio::test]
    async fn replay_round_trips_persisted_assistant_source() {
        let state = StateRuntime::open(Path::new(":memory:"))
            .await
            .expect("state");
        let thread_id = state.create_session(Path::new(".")).await.expect("thread");
        let message = Message::Assistant {
            content: vec![AssistantBlock::Source {
                source: AssistantSource::Provider {
                    kind: "future_source".to_string(),
                    data: serde_json::json!({ "id": "source-1" }),
                },
            }],
            timestamp_ms: 1,
            finish_reason: Some("stop".to_string()),
            outcome: Outcome::Normal,
            model: Some("model".to_string()),
            provider: Some("provider".to_string()),
        };
        state
            .append_message(&thread_id, &message)
            .await
            .expect("message");

        let page = HistoryReader::new(state, thread_id)
            .replay_after(None, None)
            .await
            .expect("replay");
        assert_eq!(page.warnings, Vec::new());
        assert_eq!(page.items.len(), 1);
        let HistoryReplayItem::Available { item } = &page.items[0] else {
            panic!("persisted assistant source must remain replayable");
        };
        assert_eq!(item.message, message);
    }

    #[tokio::test]
    async fn replay_is_keyset_paged_and_isolates_corrupt_rows_before_revert_boundary() {
        let state = StateRuntime::open(Path::new(":memory:"))
            .await
            .expect("state");
        let thread_id = state.create_session(Path::new(".")).await.expect("thread");
        for text in ["one", "two", "three", "four", "reverted"] {
            state
                .append_message(&thread_id, &user_text_message(text))
                .await
                .expect("message");
        }
        let mut connection = state.acquire_sqlx().await.expect("connection");
        sqlx::query(
            "UPDATE messages SET message_json = 'not-json' \
             WHERE session_id = ?1 AND session_seq = 2",
        )
        .bind(&thread_id)
        .execute(&mut *connection)
        .await
        .expect("corrupt message");
        sqlx::query(
            "UPDATE messages SET usage_json = 'not-json' \
             WHERE session_id = ?1 AND session_seq = 3",
        )
        .bind(&thread_id)
        .execute(&mut *connection)
        .await
        .expect("corrupt usage");
        drop(connection);
        state
            .set_session_revert_state(
                &thread_id,
                SessionRevertState::workspace_undo(5, "snapshot".to_string()),
            )
            .await
            .expect("revert boundary");

        let history = HistoryReader::new(state, thread_id.clone());
        let first = history
            .replay_after(None, Some(2))
            .await
            .expect("first page");
        assert_eq!(first.thread_id, thread_id);
        assert_eq!(
            first
                .items
                .iter()
                .map(HistoryReplayItem::session_seq)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(matches!(
            first.items[0],
            HistoryReplayItem::Available { .. }
        ));
        assert_eq!(
            first.items[1],
            HistoryReplayItem::Unavailable { session_seq: 2 }
        );
        assert_eq!(first.next_after, Some(2));
        assert_eq!(first.warnings.len(), 1);
        assert_eq!(
            first.warnings[0].kind,
            HistoryReplayWarningKind::InvalidMessage
        );

        let second = history
            .replay_after(first.next_after, Some(2))
            .await
            .expect("second page");
        assert_eq!(
            second
                .items
                .iter()
                .map(HistoryReplayItem::session_seq)
                .collect::<Vec<_>>(),
            vec![3, 4]
        );
        assert_eq!(
            second.items[0],
            HistoryReplayItem::Unavailable { session_seq: 3 }
        );
        assert!(matches!(
            second.items[1],
            HistoryReplayItem::Available { .. }
        ));
        assert_eq!(second.next_after, None);
        assert_eq!(second.warnings.len(), 1);
        assert_eq!(
            second.warnings[0].kind,
            HistoryReplayWarningKind::InvalidUsage
        );
    }
}
