use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use rusqlite::Connection;

use crate::error::Result;

use super::StateRuntime;
use super::store_schema_helpers::is_busy;

impl StateRuntime {
    pub(crate) fn write_retry<T>(
        &self,
        mut f: impl FnMut(&Connection) -> rusqlite::Result<T>,
    ) -> Result<T> {
        let mut last = None;
        for attempt in 0..8 {
            let conn = self.inner.conn.lock().expect("sqlite lock poisoned");
            let tx_result = (|| {
                conn.execute_batch("BEGIN IMMEDIATE")?;
                match f(&conn) {
                    Ok(value) => {
                        conn.execute_batch("COMMIT")?;
                        Ok(value)
                    }
                    Err(err) => {
                        let _ = conn.execute_batch("ROLLBACK");
                        Err(err)
                    }
                }
            })();
            drop(conn);
            match tx_result {
                Ok(value) => {
                    let successful_writes =
                        self.inner.successful_writes.fetch_add(1, Ordering::Relaxed) + 1;
                    if should_checkpoint(successful_writes)
                        && let Ok(conn) = self.inner.conn.lock()
                    {
                        let _ = conn.pragma_update(None, "wal_checkpoint", "PASSIVE");
                    }
                    return Ok(value);
                }
                Err(err) if is_busy(&err) && attempt < 7 => {
                    last = Some(err);
                    thread::sleep(Duration::from_millis(20 + (attempt as u64 * 17)));
                }
                Err(err) => return Err(err.into()),
            }
        }
        Err(last
            .unwrap_or(rusqlite::Error::ExecuteReturnedResults)
            .into())
    }
}

pub(crate) const WAL_CHECKPOINT_EVERY_WRITES: usize = 50;

pub(crate) fn should_checkpoint(successful_writes: usize) -> bool {
    successful_writes != 0 && successful_writes.is_multiple_of(WAL_CHECKPOINT_EVERY_WRITES)
}

#[cfg(test)]
mod tests {
    use super::should_checkpoint;

    #[test]
    fn checkpoint_cadence_is_every_50_successful_writes() {
        assert!(!should_checkpoint(0));
        assert!(!should_checkpoint(1));
        assert!(!should_checkpoint(49));
        assert!(should_checkpoint(50));
        assert!(!should_checkpoint(51));
        assert!(should_checkpoint(100));
    }
}
