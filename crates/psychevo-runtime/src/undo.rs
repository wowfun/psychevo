use crate::error::{Error, Result};
use crate::snapshot::SnapshotStore;
use crate::store::{SessionRevertKind, SessionRevertState};
use crate::types::{SessionRedoResult, SessionUndoOptions, SessionUndoResult};

pub async fn undo_session(options: SessionUndoOptions) -> Result<SessionUndoResult> {
    let store = options.state.clone();
    let target = store
        .latest_undo_target(&options.session_id)
        .await?
        .ok_or_else(|| Error::Message("nothing to undo".to_string()))?;
    let snapshot = target
        .snapshot
        .clone()
        .ok_or_else(|| Error::Message("undo snapshot is unavailable".to_string()))?;
    let snapshots = SnapshotStore::new(options.snapshot_root, options.cwd);
    let original_snapshot = match store.session_revert_state(&options.session_id).await? {
        Some(SessionRevertState {
            kind: SessionRevertKind::WorkspaceUndo { original_snapshot },
            ..
        }) => original_snapshot,
        Some(SessionRevertState {
            kind: SessionRevertKind::ConversationEdit { .. },
            ..
        }) => {
            return Err(Error::Message(
                "restore or run the staged conversation edit before using /undo".to_string(),
            ));
        }
        None => snapshots
            .track()?
            .ok_or_else(|| Error::Message("Git snapshot is unavailable".to_string()))?,
    };
    snapshots.restore(&snapshot)?;
    let reverted_messages = store
        .messages_from_count(&options.session_id, target.seq)
        .await?;
    store
        .set_session_revert_state(
            &options.session_id,
            SessionRevertState::workspace_undo(target.seq, original_snapshot),
        )
        .await?;
    Ok(SessionUndoResult {
        session_id: options.session_id,
        prompt: target.prompt,
        reverted_messages,
    })
}

pub async fn redo_session(options: SessionUndoOptions) -> Result<SessionRedoResult> {
    let store = options.state.clone();
    let revert = store
        .session_revert_state(&options.session_id)
        .await?
        .ok_or_else(|| Error::Message("nothing to redo".to_string()))?;
    let original_snapshot = revert.original_snapshot().ok_or_else(|| {
        Error::Message("restore or run the staged conversation edit before using /redo".to_string())
    })?;
    let snapshots = SnapshotStore::new(options.snapshot_root, options.cwd);
    if let Some(target) = store.next_redo_target(&options.session_id).await? {
        let snapshot = target
            .snapshot
            .clone()
            .ok_or_else(|| Error::Message("redo snapshot is unavailable".to_string()))?;
        snapshots.restore(&snapshot)?;
        let before = store
            .messages_from_count(&options.session_id, revert.start_seq)
            .await?;
        let after = store
            .messages_from_count(&options.session_id, target.seq)
            .await?;
        store
            .set_session_revert_state(
                &options.session_id,
                SessionRevertState::workspace_undo(target.seq, original_snapshot.to_string()),
            )
            .await?;
        return Ok(SessionRedoResult {
            session_id: options.session_id,
            restored_messages: before.saturating_sub(after),
            complete: false,
        });
    }

    snapshots.restore(original_snapshot)?;
    let restored_messages = store
        .messages_from_count(&options.session_id, revert.start_seq)
        .await?;
    store
        .clear_session_revert_state(&options.session_id)
        .await?;
    Ok(SessionRedoResult {
        session_id: options.session_id,
        restored_messages,
        complete: true,
    })
}
