use crate::error::{Error, Result};
use crate::snapshot::SnapshotStore;
use crate::store::SessionRevertState;
use crate::types::{SessionRedoResult, SessionUndoOptions, SessionUndoResult};

pub fn undo_session(options: SessionUndoOptions) -> Result<SessionUndoResult> {
    let store = options.state.store().clone();
    let target = store
        .latest_undo_target(&options.session_id)?
        .ok_or_else(|| Error::Message("nothing to undo".to_string()))?;
    let snapshot = target
        .snapshot
        .clone()
        .ok_or_else(|| Error::Message("undo snapshot is unavailable".to_string()))?;
    let snapshots = SnapshotStore::new(options.snapshot_root, options.workdir);
    let original_snapshot = match store.session_revert_state(&options.session_id)? {
        Some(revert) => revert.original_snapshot,
        None => snapshots
            .track()?
            .ok_or_else(|| Error::Message("Git snapshot is unavailable".to_string()))?,
    };
    snapshots.restore(&snapshot)?;
    let reverted_messages = store.messages_from_count(&options.session_id, target.seq)?;
    store.set_session_revert_state(
        &options.session_id,
        SessionRevertState {
            start_seq: target.seq,
            original_snapshot,
        },
    )?;
    Ok(SessionUndoResult {
        session_id: options.session_id,
        prompt: target.prompt,
        reverted_messages,
    })
}

pub fn redo_session(options: SessionUndoOptions) -> Result<SessionRedoResult> {
    let store = options.state.store().clone();
    let revert = store
        .session_revert_state(&options.session_id)?
        .ok_or_else(|| Error::Message("nothing to redo".to_string()))?;
    let snapshots = SnapshotStore::new(options.snapshot_root, options.workdir);
    if let Some(target) = store.next_redo_target(&options.session_id)? {
        let snapshot = target
            .snapshot
            .clone()
            .ok_or_else(|| Error::Message("redo snapshot is unavailable".to_string()))?;
        snapshots.restore(&snapshot)?;
        let before = store.messages_from_count(&options.session_id, revert.start_seq)?;
        let after = store.messages_from_count(&options.session_id, target.seq)?;
        store.set_session_revert_state(
            &options.session_id,
            SessionRevertState {
                start_seq: target.seq,
                original_snapshot: revert.original_snapshot,
            },
        )?;
        return Ok(SessionRedoResult {
            session_id: options.session_id,
            restored_messages: before.saturating_sub(after),
            complete: false,
        });
    }

    snapshots.restore(&revert.original_snapshot)?;
    let restored_messages = store.messages_from_count(&options.session_id, revert.start_seq)?;
    store.clear_session_revert_state(&options.session_id)?;
    Ok(SessionRedoResult {
        session_id: options.session_id,
        restored_messages,
        complete: true,
    })
}
