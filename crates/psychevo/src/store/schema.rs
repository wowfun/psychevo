use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sqlx::Connection;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteConnection, SqlitePoolOptions, SqliteSynchronous,
};

use crate::error::{Error, Result};

use super::{
    MIN_SUPPORTED_SQLITE_SCHEMA_VERSION, SQLITE_SCHEMA_VERSION, StateRuntime, StateRuntimeInner,
};

static STATE_MIGRATOR: Migrator = sqlx::migrate!("./migrations");
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const WAL_BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(5);
const WAL_BOOTSTRAP_INITIAL_BACKOFF: Duration = Duration::from_millis(10);
const WAL_BOOTSTRAP_MAX_BACKOFF: Duration = Duration::from_millis(250);

fn state_pool_options(in_memory: bool) -> SqlitePoolOptions {
    let options = SqlitePoolOptions::new().max_connections(if in_memory { 1 } else { 5 });
    if in_memory {
        options.idle_timeout(None).max_lifetime(None)
    } else {
        options
    }
}

async fn bootstrap_persistent_wal(options: &SqliteConnectOptions) -> Result<()> {
    let mut connection = SqliteConnection::connect_with(options).await?;
    let started = Instant::now();
    let mut backoff = WAL_BOOTSTRAP_INITIAL_BACKOFF;

    loop {
        match sqlx::query_scalar::<_, String>("PRAGMA journal_mode = WAL")
            .fetch_one(&mut connection)
            .await
        {
            Ok(mode) if mode.eq_ignore_ascii_case("wal") => {
                connection.close().await?;
                return Ok(());
            }
            Ok(mode) if started.elapsed() >= WAL_BOOTSTRAP_TIMEOUT => {
                let _ = connection.close().await;
                return Err(Error::Message(format!(
                    "sqlite failed to enable WAL journal mode within five seconds; database remained in {mode} mode"
                )));
            }
            Ok(_) => {}
            Err(error)
                if super::store_sqlx_runtime::is_sqlx_busy_error(&error)
                    && started.elapsed() < WAL_BOOTSTRAP_TIMEOUT => {}
            Err(error) => {
                let _ = connection.close().await;
                return Err(error.into());
            }
        }

        let remaining = WAL_BOOTSTRAP_TIMEOUT.saturating_sub(started.elapsed());
        tokio::time::sleep(backoff.min(remaining)).await;
        backoff = backoff.saturating_mul(2).min(WAL_BOOTSTRAP_MAX_BACKOFF);
    }
}

impl StateRuntime {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let in_memory = path == Path::new(":memory:");
        if !in_memory
            && let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let sqlite_options = if in_memory {
            SqliteConnectOptions::new().in_memory(true)
        } else {
            let options = SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true)
                .busy_timeout(SQLITE_BUSY_TIMEOUT);
            bootstrap_persistent_wal(&options).await?;
            options
        }
        .foreign_keys(true)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(SQLITE_BUSY_TIMEOUT);
        let pool = state_pool_options(in_memory)
            .connect_with(sqlite_options)
            .await?;
        let mut migration = pool.begin_with("BEGIN IMMEDIATE").await?;
        let user_version = sqlx::query_scalar::<_, i64>("PRAGMA user_version")
            .fetch_one(&mut *migration)
            .await?;
        let has_schema = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('sessions', 'messages')",
        )
        .fetch_one(&mut *migration)
        .await?
            > 0;
        if user_version != 0
            && !(MIN_SUPPORTED_SQLITE_SCHEMA_VERSION..=SQLITE_SCHEMA_VERSION)
                .contains(&user_version)
        {
            return Err(Error::Config(format!(
                "state database schema version {user_version} is not supported; run `pevo init --reset-state` or set PSYCHEVO_DB to a new state database"
            )));
        }
        if user_version == 0 && has_schema {
            return Err(Error::Config(
                "state database has an unknown schema version; run `pevo init --reset-state` or set PSYCHEVO_DB to a new state database".to_string(),
            ));
        }
        STATE_MIGRATOR
            .run_direct(None, &mut *migration, false)
            .await?;
        migration.commit().await?;
        Ok(Self {
            inner: Arc::new(StateRuntimeInner {
                db_path: path.to_path_buf(),
                pool,
                in_flight_operations: AtomicU64::new(0),
                completed_operations: AtomicU64::new(0),
                failed_operations: AtomicU64::new(0),
                busy_operations: AtomicU64::new(0),
                acquire_latency_micros: AtomicU64::new(0),
                execute_latency_micros: AtomicU64::new(0),
                filesystem_grants: Mutex::new(Default::default()),
                #[cfg(test)]
                fail_next_framework_terminal: AtomicU64::new(0),
                #[cfg(test)]
                gateway_turn_acceptance_barrier: Mutex::new(None),
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::Barrier;
    use tokio::task::JoinSet;

    use super::*;

    #[test]
    fn in_memory_pool_keeps_its_only_connection_for_the_runtime_lifetime() {
        let options = state_pool_options(true);

        assert_eq!(options.get_max_connections(), 1);
        assert_eq!(options.get_idle_timeout(), None);
        assert_eq!(options.get_max_lifetime(), None);
    }

    #[tokio::test]
    async fn concurrent_first_open_records_complete_v29_migration_history() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = Arc::new(temp.path().join("state.db"));
        let barrier = Arc::new(Barrier::new(8));
        let mut opens = JoinSet::new();
        for _ in 0..8 {
            let db_path = db_path.clone();
            let barrier = barrier.clone();
            opens.spawn(async move {
                barrier.wait().await;
                StateRuntime::open(db_path.as_ref()).await
            });
        }

        let mut runtimes = Vec::new();
        while let Some(result) = opens.join_next().await {
            runtimes.push(result.expect("open task").expect("concurrent first open"));
        }

        let migration_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
                .fetch_one(&runtimes[0].inner.pool)
                .await
                .expect("migration history");
        let user_version = sqlx::query_scalar::<_, i64>("PRAGMA user_version")
            .fetch_one(&runtimes[0].inner.pool)
            .await
            .expect("schema version");
        let journal_mode = sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
            .fetch_one(&runtimes[0].inner.pool)
            .await
            .expect("journal mode");
        assert_eq!(migration_count, 2);
        assert_eq!(user_version, SQLITE_SCHEMA_VERSION);
        assert_eq!(journal_mode, "wal");
        assert!(
            runtimes
                .iter()
                .all(|runtime| runtime.diagnostics().pool_size <= 5)
        );

        for runtime in runtimes {
            runtime.close().await;
        }
    }

    #[tokio::test]
    async fn existing_v29_schema_registers_migrations_without_rewriting_data() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("state.db");
        let runtime = StateRuntime::open(&db_path).await.expect("initial open");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let session_id = runtime
            .create_session_with_metadata(&cwd, "test", "model", "provider", None)
            .await
            .expect("session");
        sqlx::query("DROP TABLE _sqlx_migrations")
            .execute(&runtime.inner.pool)
            .await
            .expect("remove migration registration");
        runtime.close().await;

        let reopened = StateRuntime::open(&db_path)
            .await
            .expect("register existing v29 schema");
        assert!(
            reopened
                .session_summary(&session_id)
                .await
                .expect("session lookup")
                .is_some()
        );
        let migration_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&reopened.inner.pool)
            .await
            .expect("migration history");
        assert_eq!(migration_count, 2);
        reopened.close().await;
    }

    #[tokio::test]
    async fn operations_publish_diagnostics_and_keep_sqlite_auto_checkpoint_enabled() {
        let temp = tempfile::tempdir().expect("tempdir");
        let runtime = StateRuntime::open(temp.path().join("state.db"))
            .await
            .expect("open state runtime");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");

        let auto_checkpoint_pages =
            sqlx::query_scalar::<_, i64>("PRAGMA wal_autocheckpoint")
                .fetch_one(&runtime.inner.pool)
                .await
                .expect("WAL auto-checkpoint setting");
        assert_eq!(auto_checkpoint_pages, 1000);

        for _ in 0..50 {
            runtime.create_session(&cwd).await.expect("create session");
        }

        let diagnostics = runtime.diagnostics();
        assert_eq!(diagnostics.in_flight_operations, 0);
        assert_eq!(diagnostics.failed_operations, 0);
        assert_eq!(diagnostics.completed_operations, 50);
        assert!(diagnostics.pool_size <= 5);
        runtime.close().await;
    }
}
