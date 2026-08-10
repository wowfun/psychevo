use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use psychevo_agent_core::now_ms;
use sqlx::{Acquire, QueryBuilder, Row, Sqlite, SqliteConnection, Transaction};
use uuid::Uuid;

use crate::error::{Error, Result};

use super::StateRuntime;

const MAX_WORKSPACE_ROOTS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceRecord {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) roots: Vec<String>,
    pub(crate) revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThreadWorkspaceRecord {
    pub(crate) workspace_id: String,
    pub(crate) workspace_revision: Option<i64>,
    pub(crate) cwd: String,
    pub(crate) roots: Vec<String>,
    pub(crate) root_source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GatewayNavigationRecord {
    pub(crate) revision: i64,
    pub(crate) pinned_thread_ids: Vec<String>,
    pub(crate) pinned_workspace_ids: Vec<String>,
}

impl StateRuntime {
    pub(crate) async fn workspace(&self, workspace_id: &str) -> Result<Option<WorkspaceRecord>> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            workspace_in_executor(&mut conn, workspace_id).await
        }
        .await;
        operation.finish(&result);
        result
    }

    pub(crate) async fn workspaces(&self) -> Result<Vec<WorkspaceRecord>> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            workspace_list_in_executor(&mut conn).await
        }
        .await;
        operation.finish(&result);
        result
    }

    pub(crate) async fn update_workspace(
        &self,
        workspace_id: &str,
        expected_revision: i64,
        name: &str,
        roots: &[PathBuf],
    ) -> Result<WorkspaceRecord> {
        let current = workspace_in_pool(self, workspace_id)
            .await?
            .ok_or_else(|| Error::Message(format!("workspace not found: {workspace_id}")))?;
        ensure_workspace_revision(&current, workspace_id, expected_revision)?;
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(Error::Message(
                "workspace name must not be empty".to_string(),
            ));
        }
        let roots = roots.to_vec();
        let roots = tokio::task::spawn_blocking(move || canonical_workspace_roots(&roots))
            .await
            .map_err(|error| {
                Error::Message(format!("workspace root validation failed: {error}"))
            })??;
        self.observe_sqlx(async {
            let mut tx = self.begin_sqlx_write().await?;
            let current = workspace_in_executor(&mut tx, workspace_id)
                .await?
                .ok_or_else(|| Error::Message(format!("workspace not found: {workspace_id}")))?;
            ensure_workspace_revision(&current, workspace_id, expected_revision)?;

            let mut ownership = QueryBuilder::<Sqlite>::new(
                "SELECT canonical_path, workspace_id FROM workspace_roots WHERE workspace_id <> ",
            );
            ownership
                .push_bind(workspace_id)
                .push(" AND canonical_path IN (");
            let mut separated = ownership.separated(", ");
            for root in &roots {
                separated.push_bind(root);
            }
            separated.push_unseparated(") LIMIT 1");
            if let Some(owner) = ownership.build().fetch_optional(&mut *tx).await? {
                let path: String = owner.try_get("canonical_path")?;
                let owner_id: String = owner.try_get("workspace_id")?;
                return Err(Error::structured(
                    format!("Directory is already part of workspace `{owner_id}`."),
                    serde_json::json!({
                        "kind": "workspace_root_owned",
                        "workspaceId": workspace_id,
                        "ownerWorkspaceId": owner_id,
                        "path": path,
                    }),
                ));
            }

            sqlx::query("DELETE FROM workspace_roots WHERE workspace_id = ?1")
                .bind(workspace_id)
                .execute(&mut *tx)
                .await?;
            insert_workspace_roots_in_tx(&mut tx, workspace_id, &roots).await?;
            sqlx::query(
                r#"
                UPDATE workspaces
                SET display_name = ?1, revision = revision + 1,
                    updated_at_ms = ?2
                WHERE id = ?3
                "#,
            )
            .bind(&name)
            .bind(now_ms())
            .bind(workspace_id)
            .execute(&mut *tx)
            .await?;
            let updated = workspace_in_executor(&mut tx, workspace_id)
                .await?
                .ok_or_else(|| Error::Message(format!("workspace not found: {workspace_id}")))?;
            tx.commit().await?;
            Ok(updated)
        })
        .await
    }

    pub(crate) async fn thread_workspace_context(
        &self,
        thread_id: &str,
    ) -> Result<Option<ThreadWorkspaceRecord>> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            let mut tx = conn.begin().await?;
            let binding = sqlx::query_as::<_, (String, String, String, i64)>(
                r#"
                SELECT b.workspace_id, s.cwd, b.root_source, w.revision
                FROM thread_workspace_bindings b
                JOIN sessions s ON s.id = b.thread_id
                JOIN workspaces w ON w.id = b.workspace_id
                WHERE b.thread_id = ?1
                "#,
            )
            .bind(thread_id)
            .fetch_optional(&mut *tx)
            .await?;
            let Some((workspace_id, cwd, root_source, workspace_revision)) = binding else {
                return Ok(None);
            };
            let roots = if root_source == "workspace" {
                let catalog_roots = sqlx::query_scalar::<_, String>(
                    "SELECT canonical_path FROM workspace_roots WHERE workspace_id = ?1 ORDER BY ordinal ASC",
                )
                .bind(&workspace_id)
                .fetch_all(&mut *tx)
                .await?;
                std::iter::once(cwd.clone())
                    .chain(catalog_roots.into_iter().filter(|root| root != &cwd))
                    .collect()
            } else {
                sqlx::query_scalar::<_, String>(
                    "SELECT canonical_path FROM thread_workspace_roots WHERE thread_id = ?1 ORDER BY ordinal ASC",
                )
                .bind(thread_id)
                .fetch_all(&mut *tx)
                .await?
            };
            tx.commit().await?;
            Ok(Some(ThreadWorkspaceRecord {
                workspace_id,
                workspace_revision: (root_source == "workspace").then_some(workspace_revision),
                cwd,
                roots,
                root_source,
            }))
        }
        .await;
        operation.finish(&result);
        result
    }

    pub(crate) async fn gateway_navigation(&self) -> Result<GatewayNavigationRecord> {
        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut conn = self.acquire_sqlx().await?;
            let mut tx = conn.begin().await?;
            let navigation = gateway_navigation_in_executor(&mut tx).await?;
            tx.commit().await?;
            Ok(navigation)
        }
        .await;
        operation.finish(&result);
        result
    }

    pub(crate) async fn set_gateway_thread_pinned(
        &self,
        thread_id: &str,
        pinned: bool,
    ) -> Result<GatewayNavigationRecord> {
        self.set_gateway_pin(GatewayPinKind::Thread, thread_id, pinned)
            .await
    }

    pub(crate) async fn set_gateway_workspace_pinned(
        &self,
        workspace_id: &str,
        pinned: bool,
    ) -> Result<GatewayNavigationRecord> {
        self.set_gateway_pin(GatewayPinKind::Workspace, workspace_id, pinned)
            .await
    }

    async fn set_gateway_pin(
        &self,
        kind: GatewayPinKind,
        id: &str,
        pinned: bool,
    ) -> Result<GatewayNavigationRecord> {
        let (exists, delete, insert): (&'static str, &'static str, &'static str) = match kind {
            GatewayPinKind::Thread => (
                "SELECT EXISTS(SELECT 1 FROM gateway_pinned_threads WHERE thread_id = ?1)",
                "DELETE FROM gateway_pinned_threads WHERE thread_id = ?1",
                "INSERT OR IGNORE INTO gateway_pinned_threads(thread_id, position) VALUES (?1, (SELECT COALESCE(MAX(position), -1) + 1 FROM gateway_pinned_threads))",
            ),
            GatewayPinKind::Workspace => (
                "SELECT EXISTS(SELECT 1 FROM gateway_pinned_workspaces WHERE workspace_id = ?1)",
                "DELETE FROM gateway_pinned_workspaces WHERE workspace_id = ?1",
                "INSERT OR IGNORE INTO gateway_pinned_workspaces(workspace_id, position) VALUES (?1, (SELECT COALESCE(MAX(position), -1) + 1 FROM gateway_pinned_workspaces))",
            ),
        };
        self.observe_sqlx(async {
            let mut conn = self.acquire_sqlx().await?;
            let is_pinned = sqlx::query_scalar::<_, i64>(exists)
                .bind(id)
                .fetch_one(&mut *conn)
                .await?
                != 0;
            if is_pinned == pinned {
                return Ok(());
            }
            drop(conn);
            let mut tx = self.begin_sqlx_write().await?;
            let is_pinned = sqlx::query_scalar::<_, i64>(exists)
                .bind(id)
                .fetch_one(&mut *tx)
                .await?
                != 0;
            if is_pinned == pinned {
                tx.commit().await?;
                return Ok(());
            }
            let changed = sqlx::query(if pinned { insert } else { delete })
                .bind(id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            if changed != 1 {
                return Err(Error::Message(format!(
                    "failed to {} navigation item `{id}`",
                    if pinned { "pin" } else { "unpin" }
                )));
            }
            sqlx::query(
                "UPDATE gateway_navigation_state SET revision = revision + 1 WHERE singleton = 1",
            )
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(())
        })
        .await?;
        self.gateway_navigation().await
    }
}

fn ensure_workspace_revision(
    current: &WorkspaceRecord,
    workspace_id: &str,
    expected_revision: i64,
) -> Result<()> {
    if current.revision == expected_revision {
        return Ok(());
    }
    Err(Error::structured(
        "Workspace changed while it was being edited.",
        serde_json::json!({
            "kind": "workspace_revision_conflict",
            "workspaceId": workspace_id,
            "expectedRevision": expected_revision,
            "actualRevision": current.revision,
            "workspace": {
                "id": current.id,
                "name": current.name,
                "roots": current.roots,
                "revision": current.revision,
            },
        }),
    ))
}

#[derive(Debug, Clone, Copy)]
enum GatewayPinKind {
    Thread,
    Workspace,
}

pub(crate) async fn bind_thread_workspace_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    cwd: &str,
    workspace_id: Option<&str>,
    captured_workspace_roots: Option<&[String]>,
    captured_workspace_revision: Option<i64>,
    now: i64,
) -> Result<ThreadWorkspaceRecord> {
    let (workspace_id, workspace_revision, roots, root_source) =
        if let Some(workspace_id) = workspace_id {
            let workspace = workspace_in_executor(tx, workspace_id)
                .await?
                .ok_or_else(|| Error::Message(format!("workspace not found: {workspace_id}")))?;
            if let Some(revision) = captured_workspace_revision
                && revision != workspace.revision
            {
                return Err(Error::Message(format!(
                    "workspace `{workspace_id}` changed during admission"
                )));
            }
            let roots = if let Some(captured_roots) = captured_workspace_roots {
                if captured_roots != workspace.roots {
                    return Err(Error::Message(format!(
                        "workspace `{workspace_id}` roots changed during admission"
                    )));
                }
                captured_roots.to_vec()
            } else {
                workspace.roots
            };
            if roots.first().map(String::as_str) != Some(cwd) {
                return Err(Error::Message(format!(
                    "workspace `{workspace_id}` primary directory does not match Thread cwd"
                )));
            }
            (workspace.id, Some(workspace.revision), roots, "workspace")
        } else {
            let workspace = ensure_direct_workspace_in_tx(tx, cwd, now).await?;
            (workspace.id, None, vec![cwd.to_string()], "direct")
        };
    sqlx::query(
        "INSERT INTO thread_workspace_bindings(thread_id, workspace_id, root_source, created_at_ms) VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(thread_id)
    .bind(&workspace_id)
    .bind(root_source)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    if root_source == "direct" {
        insert_thread_roots_in_tx(tx, thread_id, &roots).await?;
    }
    Ok(ThreadWorkspaceRecord {
        workspace_id,
        workspace_revision,
        cwd: cwd.to_string(),
        roots,
        root_source: root_source.to_string(),
    })
}

pub(crate) async fn copy_thread_workspace_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    source_thread_id: &str,
    destination_thread_id: &str,
    now: i64,
) -> Result<()> {
    let inserted = sqlx::query(
        r#"
        INSERT INTO thread_workspace_bindings(thread_id, workspace_id, root_source, created_at_ms)
        SELECT ?1, workspace_id, root_source, ?2
        FROM thread_workspace_bindings
        WHERE thread_id = ?3
        "#,
    )
    .bind(destination_thread_id)
    .bind(now)
    .bind(source_thread_id)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if inserted != 1 {
        return Err(Error::Message(format!(
            "Thread `{source_thread_id}` has no Workspace context"
        )));
    }
    sqlx::query(
        r#"
        INSERT INTO thread_workspace_roots(thread_id, ordinal, canonical_path)
        SELECT ?1, ordinal, canonical_path
        FROM thread_workspace_roots
        WHERE thread_id = ?2
        ORDER BY ordinal ASC
        "#,
    )
    .bind(destination_thread_id)
    .bind(source_thread_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn ensure_direct_workspace_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    cwd: &str,
    now: i64,
) -> Result<WorkspaceRecord> {
    let ancestors = Path::new(cwd)
        .ancestors()
        .map(|ancestor| ancestor.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let ancestors_json = serde_json::to_string(&ancestors)?;
    if let Some(workspace_id) = sqlx::query_scalar::<_, String>(
        r#"
        SELECT workspace_id
        FROM workspace_roots
        WHERE canonical_path IN (SELECT value FROM json_each(?1))
        ORDER BY length(canonical_path) DESC
        LIMIT 1
        "#,
    )
    .bind(ancestors_json)
    .fetch_optional(&mut **tx)
    .await?
    {
        return workspace_in_executor(tx, &workspace_id)
            .await?
            .ok_or_else(|| Error::Message(format!("workspace not found: {workspace_id}")));
    }

    let id = format!("w_{}", Uuid::now_v7().simple());
    let name = Path::new(cwd)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("workspace");
    sqlx::query(
        r#"
        INSERT INTO workspaces(id, display_name, revision, created_at_ms, updated_at_ms)
        VALUES (?1, ?2, 1, ?3, ?3)
        "#,
    )
    .bind(&id)
    .bind(name)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    insert_workspace_roots_in_tx(tx, &id, &[cwd.to_string()]).await?;
    Ok(WorkspaceRecord {
        id,
        name: name.to_string(),
        roots: vec![cwd.to_string()],
        revision: 1,
    })
}

fn canonical_workspace_roots(roots: &[PathBuf]) -> Result<Vec<String>> {
    if roots.is_empty() {
        return Err(Error::Message(
            "workspace must contain at least one directory".to_string(),
        ));
    }
    if roots.len() > MAX_WORKSPACE_ROOTS {
        return Err(Error::Message(format!(
            "workspace contains more than {MAX_WORKSPACE_ROOTS} directories"
        )));
    }
    let mut seen = BTreeSet::new();
    let mut canonical = Vec::with_capacity(roots.len());
    for root in roots {
        let root = crate::host_paths::normalized_native_path(&std::fs::canonicalize(root)?);
        if !root.is_dir() {
            return Err(Error::Message(format!(
                "workspace root is not a directory: {}",
                root.display()
            )));
        }
        let path = root.to_string_lossy().into_owned();
        if !seen.insert(path.clone()) {
            return Err(Error::Message(format!(
                "workspace root is duplicated: {path}"
            )));
        }
        canonical.push(path);
    }
    Ok(canonical)
}

async fn insert_workspace_roots_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: &str,
    roots: &[String],
) -> Result<()> {
    let mut insert = QueryBuilder::<Sqlite>::new(
        "INSERT INTO workspace_roots(workspace_id, ordinal, canonical_path) ",
    );
    insert.push_values(roots.iter().enumerate(), |mut row, (ordinal, root)| {
        row.push_bind(workspace_id)
            .push_bind(i64::try_from(ordinal).expect("Workspace root limit fits i64"))
            .push_bind(root);
    });
    insert.build().execute(&mut **tx).await?;
    Ok(())
}

async fn insert_thread_roots_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    roots: &[String],
) -> Result<()> {
    let mut insert = QueryBuilder::<Sqlite>::new(
        "INSERT INTO thread_workspace_roots(thread_id, ordinal, canonical_path) ",
    );
    insert.push_values(roots.iter().enumerate(), |mut row, (ordinal, root)| {
        row.push_bind(thread_id)
            .push_bind(i64::try_from(ordinal).expect("Thread root limit fits i64"))
            .push_bind(root);
    });
    insert.build().execute(&mut **tx).await?;
    Ok(())
}

async fn workspace_in_pool(
    state: &StateRuntime,
    workspace_id: &str,
) -> Result<Option<WorkspaceRecord>> {
    let mut conn = state.acquire_sqlx().await?;
    workspace_in_executor(&mut conn, workspace_id).await
}

pub(super) async fn workspace_in_executor(
    conn: &mut SqliteConnection,
    workspace_id: &str,
) -> Result<Option<WorkspaceRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT w.id, w.display_name, w.revision,
               r.ordinal, r.canonical_path
        FROM workspaces w
        JOIN workspace_roots r ON r.workspace_id = w.id
        WHERE w.id = ?1
        ORDER BY r.ordinal ASC
        "#,
    )
    .bind(workspace_id)
    .fetch_all(&mut *conn)
    .await?;
    let Some(first) = rows.first() else {
        return Ok(None);
    };
    Ok(Some(WorkspaceRecord {
        id: first.try_get("id")?,
        name: first.try_get("display_name")?,
        revision: first.try_get("revision")?,
        roots: rows
            .iter()
            .map(|row| row.try_get("canonical_path"))
            .collect::<std::result::Result<Vec<String>, sqlx::Error>>()?,
    }))
}

pub(super) async fn workspace_list_in_executor(
    conn: &mut SqliteConnection,
) -> Result<Vec<WorkspaceRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT w.id, w.display_name, w.revision,
               r.ordinal, r.canonical_path
        FROM workspaces w
        JOIN workspace_roots r ON r.workspace_id = w.id
        ORDER BY w.created_at_ms ASC, w.id ASC, r.ordinal ASC
        "#,
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut workspaces = Vec::<WorkspaceRecord>::new();
    for row in rows {
        let id: String = row.try_get("id")?;
        if workspaces.last().map(|workspace| workspace.id.as_str()) != Some(id.as_str()) {
            workspaces.push(WorkspaceRecord {
                id: id.clone(),
                name: row.try_get("display_name")?,
                roots: Vec::new(),
                revision: row.try_get("revision")?,
            });
        }
        workspaces
            .last_mut()
            .expect("workspace row initialized")
            .roots
            .push(row.try_get("canonical_path")?);
    }
    Ok(workspaces)
}

pub(super) async fn workspace_list_for_cwd_in_executor(
    conn: &mut SqliteConnection,
    cwd: &str,
) -> Result<Vec<WorkspaceRecord>> {
    let ancestors = Path::new(cwd)
        .ancestors()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let ancestors_json = serde_json::to_string(&ancestors)?;
    let rows = sqlx::query(
        r#"
        WITH eligible(workspace_id) AS (
            SELECT DISTINCT workspace_id
            FROM workspace_roots
            WHERE canonical_path IN (SELECT value FROM json_each(?1))
            UNION
            SELECT DISTINCT b.workspace_id
            FROM sessions s
            JOIN thread_workspace_bindings b ON b.thread_id = s.id
            WHERE s.cwd = ?2
        )
        SELECT w.id, w.display_name, w.revision,
               r.ordinal, r.canonical_path
        FROM eligible e
        JOIN workspaces w ON w.id = e.workspace_id
        JOIN workspace_roots r ON r.workspace_id = w.id
        ORDER BY w.created_at_ms ASC, w.id ASC, r.ordinal ASC
        "#,
    )
    .bind(ancestors_json)
    .bind(cwd)
    .fetch_all(&mut *conn)
    .await?;
    let mut workspaces = Vec::<WorkspaceRecord>::new();
    for row in rows {
        let id: String = row.try_get("id")?;
        if workspaces.last().map(|workspace| workspace.id.as_str()) != Some(id.as_str()) {
            workspaces.push(WorkspaceRecord {
                id: id.clone(),
                name: row.try_get("display_name")?,
                roots: Vec::new(),
                revision: row.try_get("revision")?,
            });
        }
        workspaces
            .last_mut()
            .expect("workspace row initialized")
            .roots
            .push(row.try_get("canonical_path")?);
    }
    Ok(workspaces)
}

async fn gateway_navigation_in_executor(
    conn: &mut SqliteConnection,
) -> Result<GatewayNavigationRecord> {
    let revision = sqlx::query_scalar::<_, i64>(
        "SELECT revision FROM gateway_navigation_state WHERE singleton = 1",
    )
    .fetch_one(&mut *conn)
    .await?;
    let pinned_thread_ids = sqlx::query_scalar::<_, String>(
        "SELECT thread_id FROM gateway_pinned_threads ORDER BY position DESC",
    )
    .fetch_all(&mut *conn)
    .await?;
    let pinned_workspace_ids = sqlx::query_scalar::<_, String>(
        "SELECT workspace_id FROM gateway_pinned_workspaces ORDER BY position DESC",
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(GatewayNavigationRecord {
        revision,
        pinned_thread_ids,
        pinned_workspace_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn direct_threads_share_a_stable_workspace_but_keep_single_runtime_root() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let state = StateRuntime::open(":memory:").await.expect("state");

        let first = state.create_session(&cwd).await.expect("first");
        let second = state.create_session(&cwd).await.expect("second");
        let first_context = state
            .thread_workspace_context(&first)
            .await
            .expect("context")
            .expect("first context");
        let second_context = state
            .thread_workspace_context(&second)
            .await
            .expect("context")
            .expect("second context");

        assert_eq!(first_context.workspace_id, second_context.workspace_id);
        assert_eq!(first_context.roots, vec![cwd.to_string_lossy()]);
    }

    #[tokio::test]
    async fn workspace_update_is_atomic_and_does_not_rewrite_existing_thread_roots() {
        let temp = tempfile::tempdir().expect("temp");
        let primary = temp.path().join("primary");
        let secondary = temp.path().join("secondary");
        std::fs::create_dir_all(&primary).expect("primary");
        std::fs::create_dir_all(&secondary).expect("secondary");
        let state = StateRuntime::open(":memory:").await.expect("state");

        let old_thread = state.create_session(&primary).await.expect("old thread");
        let old_context = state
            .thread_workspace_context(&old_thread)
            .await
            .expect("context")
            .expect("old context");
        let current = state
            .workspace(&old_context.workspace_id)
            .await
            .expect("workspace")
            .expect("current workspace");
        let updated = state
            .update_workspace(
                &current.id,
                current.revision,
                "Renamed workspace",
                &[secondary.clone(), primary.clone()],
            )
            .await
            .expect("update workspace");

        assert_eq!(updated.name, "Renamed workspace");
        assert_eq!(
            updated.roots,
            vec![
                secondary.to_string_lossy().into_owned(),
                primary.to_string_lossy().into_owned()
            ]
        );
        assert_eq!(
            state
                .thread_workspace_context(&old_thread)
                .await
                .expect("context")
                .expect("old context")
                .roots,
            vec![primary.to_string_lossy().into_owned()]
        );

        let new_thread = state
            .create_session_in_workspace_with_metadata(
                &secondary,
                &updated.id,
                "test",
                "model",
                "provider",
                None,
            )
            .await
            .expect("new thread");
        let mut conn = state.acquire_sqlx().await.expect("connection");
        let persisted_root_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM thread_workspace_roots WHERE thread_id = ?1",
        )
        .bind(&new_thread)
        .fetch_one(&mut *conn)
        .await
        .expect("thread root count");
        assert_eq!(
            persisted_root_count, 0,
            "explicit Workspace Threads resolve live catalog roots and must not duplicate them"
        );
        drop(conn);
        assert_eq!(
            state
                .thread_workspace_context(&new_thread)
                .await
                .expect("context")
                .expect("new context")
                .roots,
            updated.roots
        );

        let latest = state
            .update_workspace(
                &updated.id,
                updated.revision,
                "Renamed again",
                std::slice::from_ref(&secondary),
            )
            .await
            .expect("remove non-cwd root");
        assert_eq!(latest.roots, vec![secondary.to_string_lossy().into_owned()]);
        assert_eq!(
            state
                .thread_workspace_context(&new_thread)
                .await
                .expect("context")
                .expect("rebound context")
                .roots,
            latest.roots
        );
    }

    #[tokio::test]
    async fn workspace_update_rejects_roots_owned_by_another_implicit_workspace() {
        let temp = tempfile::tempdir().expect("temp");
        let first_root = temp.path().join("first");
        let second_root = temp.path().join("second");
        std::fs::create_dir_all(&first_root).expect("first root");
        std::fs::create_dir_all(&second_root).expect("second root");
        let state = StateRuntime::open(":memory:").await.expect("state");

        let first_thread = state
            .create_session(&first_root)
            .await
            .expect("first thread");
        let second_thread = state
            .create_session(&second_root)
            .await
            .expect("second thread");
        let first_workspace = state
            .thread_workspace_context(&first_thread)
            .await
            .expect("first context")
            .expect("first workspace");
        let second_workspace = state
            .thread_workspace_context(&second_thread)
            .await
            .expect("second context")
            .expect("second workspace");
        let current = state
            .workspace(&first_workspace.workspace_id)
            .await
            .expect("workspace")
            .expect("current workspace");

        let error = state
            .update_workspace(
                &current.id,
                current.revision,
                "First",
                &[first_root, second_root],
            )
            .await
            .expect_err("an owned root must conflict even when its owner is implicit");

        assert_eq!(
            error
                .structured_data()
                .and_then(|details| details["kind"].as_str()),
            Some("workspace_root_owned")
        );
        assert!(
            state
                .workspace(&second_workspace.workspace_id)
                .await
                .expect("second workspace lookup")
                .is_some()
        );
        assert_eq!(
            state
                .thread_workspace_context(&second_thread)
                .await
                .expect("second context after conflict")
                .expect("second workspace after conflict")
                .workspace_id,
            second_workspace.workspace_id
        );
    }

    #[tokio::test]
    async fn thread_and_workspace_pins_are_independent_and_most_recent_first() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let state = StateRuntime::open(":memory:").await.expect("state");
        let thread_a = state.create_session(&cwd).await.expect("thread a");
        let thread_b = state.create_session(&cwd).await.expect("thread b");
        let workspace_id = state
            .thread_workspace_context(&thread_a)
            .await
            .expect("context")
            .expect("workspace context")
            .workspace_id;

        state
            .set_gateway_thread_pinned(&thread_a, true)
            .await
            .expect("pin thread a");
        state
            .set_gateway_workspace_pinned(&workspace_id, true)
            .await
            .expect("pin workspace");
        let navigation = state
            .set_gateway_thread_pinned(&thread_b, true)
            .await
            .expect("pin thread b");

        assert_eq!(
            navigation.pinned_thread_ids,
            [thread_b.clone(), thread_a.clone()]
        );
        assert_eq!(
            navigation.pinned_workspace_ids,
            std::slice::from_ref(&workspace_id)
        );

        let navigation = state
            .set_gateway_thread_pinned(&thread_b, false)
            .await
            .expect("unpin thread b");
        assert_eq!(navigation.pinned_thread_ids, [thread_a]);
        assert_eq!(navigation.pinned_workspace_ids, [workspace_id]);
    }

    #[tokio::test]
    async fn repeating_an_existing_pin_is_idempotent() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let state = StateRuntime::open(":memory:").await.expect("state");
        let thread_a = state.create_session(&cwd).await.expect("thread a");
        let thread_b = state.create_session(&cwd).await.expect("thread b");

        state
            .set_gateway_thread_pinned(&thread_a, true)
            .await
            .expect("pin thread a");
        let before = state
            .set_gateway_thread_pinned(&thread_b, true)
            .await
            .expect("pin thread b");
        let after = state
            .set_gateway_thread_pinned(&thread_a, true)
            .await
            .expect("repeat pin thread a");

        assert_eq!(after, before);
    }

    #[tokio::test]
    async fn adding_a_pin_preserves_existing_pin_rows() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let state = StateRuntime::open(":memory:").await.expect("state");
        let first = state.create_session(&cwd).await.expect("first");
        let second = state.create_session(&cwd).await.expect("second");
        let third = state.create_session(&cwd).await.expect("third");
        state
            .set_gateway_thread_pinned(&first, true)
            .await
            .expect("pin first");
        state
            .set_gateway_thread_pinned(&second, true)
            .await
            .expect("pin second");
        let mut conn = state.acquire_sqlx().await.expect("connection");
        let before = sqlx::query_as::<_, (String, i64)>(
            "SELECT thread_id, position FROM gateway_pinned_threads ORDER BY thread_id",
        )
        .fetch_all(&mut *conn)
        .await
        .expect("positions before");
        drop(conn);

        state
            .set_gateway_thread_pinned(&third, true)
            .await
            .expect("pin third");
        let mut conn = state.acquire_sqlx().await.expect("connection");
        let after = sqlx::query_as::<_, (String, i64)>(
            "SELECT thread_id, position FROM gateway_pinned_threads WHERE thread_id != ?1 ORDER BY thread_id",
        )
        .bind(&third)
        .fetch_all(&mut *conn)
        .await
        .expect("positions after");

        assert_eq!(after, before);
    }

    #[tokio::test]
    async fn deleting_a_pinned_thread_advances_navigation_revision() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let state = StateRuntime::open(":memory:").await.expect("state");
        let thread_id = state.create_session(&cwd).await.expect("thread");
        let pinned = state
            .set_gateway_thread_pinned(&thread_id, true)
            .await
            .expect("pin thread");

        state
            .delete_session(&thread_id)
            .await
            .expect("delete thread");
        let after = state.gateway_navigation().await.expect("navigation");

        assert!(after.pinned_thread_ids.is_empty());
        assert_eq!(after.revision, pinned.revision + 1);
    }

    #[tokio::test]
    async fn cwd_filter_finds_a_workspace_by_containment_and_retained_thread_cwd() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("repo");
        let nested = root.join("sub");
        let replacement = temp.path().join("replacement");
        for path in [&nested, &replacement] {
            std::fs::create_dir_all(path).expect("root");
        }
        let state = StateRuntime::open(":memory:").await.expect("state");
        let initial = state.create_session(&root).await.expect("initial thread");
        let workspace_id = state
            .thread_workspace_context(&initial)
            .await
            .expect("context")
            .expect("workspace context")
            .workspace_id;
        let include_ids = Vec::new();
        let active_ids = Vec::new();
        let nested_string = nested.to_string_lossy().into_owned();

        let contained = state
            .browse_human_sessions(crate::store::SessionBrowserRequest {
                cwd: Some(&nested_string),
                archived: false,
                cursor_workspace_id: None,
                cursor_offset: 0,
                limit: 20,
                recent_since_ms: 0,
                include_session_ids: &include_ids,
                active_session_ids: &active_ids,
            })
            .await
            .expect("contained browse");
        assert!(
            contained
                .iter()
                .any(|workspace| workspace.workspace.id == workspace_id)
        );

        let retained_state = StateRuntime::open(":memory:")
            .await
            .expect("retained state");
        let nested_thread = retained_state
            .create_session(&nested)
            .await
            .expect("nested thread");
        let retained_workspace_id = retained_state
            .thread_workspace_context(&nested_thread)
            .await
            .expect("retained context")
            .expect("retained workspace context")
            .workspace_id;
        let current = retained_state
            .workspace(&retained_workspace_id)
            .await
            .expect("workspace")
            .expect("current workspace");
        retained_state
            .update_workspace(
                &retained_workspace_id,
                current.revision,
                &current.name,
                std::slice::from_ref(&replacement),
            )
            .await
            .expect("replace root");
        let retained = retained_state
            .browse_human_sessions(crate::store::SessionBrowserRequest {
                cwd: Some(&nested_string),
                archived: false,
                cursor_workspace_id: None,
                cursor_offset: 0,
                limit: 20,
                recent_since_ms: 0,
                include_session_ids: &include_ids,
                active_session_ids: &active_ids,
            })
            .await
            .expect("retained browse");
        let retained_ids = retained
            .iter()
            .find(|workspace| workspace.workspace.id == retained_workspace_id)
            .expect("retained workspace")
            .sessions
            .iter()
            .map(|session| session.summary.id.as_str())
            .collect::<BTreeSet<_>>();
        assert!(retained_ids.contains(nested_thread.as_str()));
    }

    #[tokio::test]
    async fn workspace_cursor_ignores_primary_cwd_and_returns_only_its_workspace() {
        let temp = tempfile::tempdir().expect("temp");
        let primary = temp.path().join("primary");
        let secondary = temp.path().join("secondary");
        let unrelated = temp.path().join("unrelated");
        for root in [&primary, &secondary, &unrelated] {
            std::fs::create_dir_all(root).expect("root");
        }
        let state = StateRuntime::open(":memory:").await.expect("state");
        let initial_thread = state
            .create_session(&primary)
            .await
            .expect("initial thread");
        let workspace_id = state
            .thread_workspace_context(&initial_thread)
            .await
            .expect("initial context")
            .expect("initial workspace")
            .workspace_id;
        let current = state
            .workspace(&workspace_id)
            .await
            .expect("workspace")
            .expect("current workspace");
        let workspace = state
            .update_workspace(
                &workspace_id,
                current.revision,
                "Multi-root",
                &[secondary.clone(), primary.clone()],
            )
            .await
            .expect("reorder roots");
        let secondary_thread = state
            .create_session_in_workspace_with_metadata(
                &secondary,
                &workspace.id,
                "test",
                "model",
                "provider",
                None,
            )
            .await
            .expect("secondary cwd thread");
        let unrelated_thread = state
            .create_session(&unrelated)
            .await
            .expect("unrelated thread");
        let unrelated_workspace_id = state
            .thread_workspace_context(&unrelated_thread)
            .await
            .expect("unrelated context")
            .expect("unrelated Workspace")
            .workspace_id;
        let include_ids = Vec::new();
        let active_ids = Vec::new();

        let initial_page = state
            .browse_human_sessions(crate::store::SessionBrowserRequest {
                cwd: Some(secondary.to_string_lossy().as_ref()),
                archived: false,
                cursor_workspace_id: None,
                cursor_offset: 0,
                limit: 20,
                recent_since_ms: 0,
                include_session_ids: &include_ids,
                active_session_ids: &active_ids,
            })
            .await
            .expect("initial workspace page");
        let initial_ids = initial_page
            .iter()
            .find(|projection| projection.workspace.id == workspace.id)
            .expect("workspace initial projection")
            .sessions
            .iter()
            .map(|session| session.summary.id.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            initial_page.len(),
            1,
            "cwd browse must omit unrelated groups"
        );
        assert_eq!(
            initial_ids,
            BTreeSet::from([initial_thread.as_str(), secondary_thread.as_str()]),
            "initial and cursor pages must rank the same Workspace membership"
        );

        let page = state
            .browse_human_sessions(crate::store::SessionBrowserRequest {
                cwd: Some(secondary.to_string_lossy().as_ref()),
                archived: false,
                cursor_workspace_id: Some(&workspace.id),
                cursor_offset: 0,
                limit: 20,
                recent_since_ms: 0,
                include_session_ids: &include_ids,
                active_session_ids: &active_ids,
            })
            .await
            .expect("workspace cursor page");

        assert_eq!(
            page.len(),
            1,
            "cursor pages must not synthesize other groups"
        );
        assert_eq!(page[0].workspace.id, workspace.id);
        let ids = page[0]
            .sessions
            .iter()
            .map(|session| session.summary.id.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            ids,
            BTreeSet::from([initial_thread.as_str(), secondary_thread.as_str()]),
            "cursor pages must traverse all Threads bound to the Workspace"
        );

        let unrelated_page = state
            .browse_human_sessions(crate::store::SessionBrowserRequest {
                cwd: Some(secondary.to_string_lossy().as_ref()),
                archived: false,
                cursor_workspace_id: Some(&unrelated_workspace_id),
                cursor_offset: 0,
                limit: 20,
                recent_since_ms: 0,
                include_session_ids: &include_ids,
                active_session_ids: &active_ids,
            })
            .await
            .expect("cursor identity must override the initial-page cwd filter");
        assert_eq!(unrelated_page.len(), 1);
        assert_eq!(unrelated_page[0].workspace.id, unrelated_workspace_id);
    }

    #[tokio::test]
    async fn initial_browser_keeps_pagination_for_a_workspace_with_only_old_threads() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("old-only");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let state = StateRuntime::open(":memory:").await.expect("state");
        let thread_id = state.create_session(&cwd).await.expect("thread");
        let workspace_id = state
            .thread_workspace_context(&thread_id)
            .await
            .expect("context")
            .expect("workspace context")
            .workspace_id;
        let include_ids = Vec::new();
        let active_ids = Vec::new();

        let page = state
            .browse_human_sessions(crate::store::SessionBrowserRequest {
                cwd: None,
                archived: false,
                cursor_workspace_id: None,
                cursor_offset: 0,
                limit: 20,
                recent_since_ms: i64::MAX,
                include_session_ids: &include_ids,
                active_session_ids: &active_ids,
            })
            .await
            .expect("initial browser page");
        let workspace = page
            .iter()
            .find(|workspace| workspace.workspace.id == workspace_id)
            .expect("workspace projection");

        assert!(workspace.sessions.is_empty());
        assert_eq!(workspace.hidden_count, 1);
        assert_eq!(workspace.next_offset, Some(0));
    }

    #[tokio::test]
    async fn stale_workspace_revision_wins_before_draft_filesystem_validation() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("workspace");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let state = StateRuntime::open(":memory:").await.expect("state");
        let thread_id = state.create_session(&cwd).await.expect("thread");
        let workspace_id = state
            .thread_workspace_context(&thread_id)
            .await
            .expect("context")
            .expect("workspace context")
            .workspace_id;
        let current = state
            .workspace(&workspace_id)
            .await
            .expect("workspace")
            .expect("workspace record");
        state
            .update_workspace(
                &workspace_id,
                current.revision,
                "Current",
                std::slice::from_ref(&cwd),
            )
            .await
            .expect("advance revision");

        let error = state
            .update_workspace(
                &workspace_id,
                current.revision,
                "Stale",
                &[temp.path().join("missing")],
            )
            .await
            .expect_err("stale revision");

        assert_eq!(
            error
                .structured_data()
                .and_then(|data| data["kind"].as_str()),
            Some("workspace_revision_conflict")
        );
    }

    #[tokio::test]
    async fn thread_admission_transaction_rejects_a_stale_workspace_snapshot() {
        let temp = tempfile::tempdir().expect("temp");
        let primary = temp.path().join("primary");
        let secondary = temp.path().join("secondary");
        for path in [&primary, &secondary] {
            std::fs::create_dir_all(path).expect("root");
        }
        let state = StateRuntime::open(":memory:").await.expect("state");
        let seed = state.create_session(&primary).await.expect("seed");
        let workspace_id = state
            .thread_workspace_context(&seed)
            .await
            .expect("context")
            .expect("Workspace context")
            .workspace_id;
        let captured = state
            .workspace(&workspace_id)
            .await
            .expect("Workspace")
            .expect("Workspace record");
        state
            .update_workspace(
                &workspace_id,
                captured.revision,
                &captured.name,
                &[primary.clone(), secondary],
            )
            .await
            .expect("advance Workspace revision");

        let error = state
            .create_session_in_workspace_snapshot_with_metadata(
                crate::store::WorkspaceSessionSnapshotInput {
                    cwd: &primary,
                    workspace_id: &workspace_id,
                    workspace_roots: &captured.roots,
                    workspace_revision: captured.revision,
                    source: "test",
                    model: "model",
                    provider: "provider",
                    metadata: None,
                },
            )
            .await
            .expect_err("stale admission snapshot");

        assert!(error.to_string().contains("changed during admission"));
    }

    #[tokio::test]
    async fn retained_cwd_workspace_lookup_uses_a_general_sessions_cwd_index() {
        let state = StateRuntime::open(":memory:").await.expect("state");
        let mut connection = state.acquire_sqlx().await.expect("connection");
        let plan = sqlx::query("EXPLAIN QUERY PLAN SELECT id FROM sessions WHERE cwd = ?1")
            .bind("/workspace")
            .fetch_all(&mut *connection)
            .await
            .expect("query plan")
            .into_iter()
            .map(|row| row.get::<String, _>(3))
            .collect::<Vec<_>>();

        assert!(
            plan.iter()
                .any(|detail| detail.contains("idx_sessions_cwd")),
            "cwd lookup must not scan all sessions: {plan:?}"
        );
    }
}
