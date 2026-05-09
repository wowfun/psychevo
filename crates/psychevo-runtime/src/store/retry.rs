impl SqliteStore {
    fn write_retry<T>(&self, mut f: impl FnMut(&Connection) -> rusqlite::Result<T>) -> Result<T> {
        let mut last = None;
        for attempt in 0..8 {
            let conn = self.conn.lock().expect("sqlite lock poisoned");
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
                    if attempt % 4 == 0
                        && let Ok(conn) = self.conn.lock()
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
