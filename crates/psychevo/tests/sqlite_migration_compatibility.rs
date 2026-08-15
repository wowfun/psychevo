use std::collections::BTreeMap;
use std::path::Path;

use psychevo::application::Application;
use rusqlite::params;
use sha2::{Digest as _, Sha384};

const WORKSPACES_MIGRATION_VERSION: i64 = 20260808000000;
const WORKSPACES_MIGRATION: &str = include_str!("../migrations/20260808000000_v33_workspaces.sql");

async fn open_application(home: &Path, database: &Path) -> psychevo::Result<Application> {
    Application::builder()
        .home(home)
        .database_path(database)
        .inherited_environment(BTreeMap::new())
        .build()
        .await
}

fn record_workspace_migration_checksum(database: &Path, checksum: Vec<u8>) {
    let connection = rusqlite::Connection::open(database).expect("database");
    let changed = connection
        .execute(
            "UPDATE _sqlx_migrations SET checksum = ?1 WHERE version = ?2",
            params![checksum, WORKSPACES_MIGRATION_VERSION],
        )
        .expect("record workspace migration checksum");
    assert_eq!(changed, 1, "workspace migration must be applied");
}

#[tokio::test]
async fn opens_database_with_crlf_checksum_for_unchanged_migration() {
    let temp = tempfile::tempdir().expect("temp");
    let home = temp.path().join("home");
    let database = temp.path().join("state.db");
    std::fs::create_dir(&home).expect("home");

    let application = open_application(&home, &database)
        .await
        .expect("initial application");
    application.shutdown().await.expect("initial shutdown");

    assert!(
        !WORKSPACES_MIGRATION.contains('\r'),
        "repository migration must use LF line endings"
    );
    let crlf_checksum = Sha384::digest(WORKSPACES_MIGRATION.replace('\n', "\r\n")).to_vec();
    record_workspace_migration_checksum(&database, crlf_checksum);

    let reopened = open_application(&home, &database)
        .await
        .expect("CRLF-only checksum difference must be compatible");
    reopened.shutdown().await.expect("reopened shutdown");
}

#[tokio::test]
async fn rejects_checksum_for_modified_migration_content() {
    let temp = tempfile::tempdir().expect("temp");
    let home = temp.path().join("home");
    let database = temp.path().join("state.db");
    std::fs::create_dir(&home).expect("home");

    let application = open_application(&home, &database)
        .await
        .expect("initial application");
    application.shutdown().await.expect("initial shutdown");
    record_workspace_migration_checksum(&database, vec![0xa5; 48]);

    let error = open_application(&home, &database)
        .await
        .expect_err("content checksum mismatch must fail closed");

    assert!(matches!(
        error,
        psychevo::Error::SqlxMigration(sqlx::migrate::MigrateError::VersionMismatch(
            WORKSPACES_MIGRATION_VERSION
        ))
    ));
}
