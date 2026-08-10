use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::Client;
use crate::store::WorkspaceRecord;
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub roots: Vec<String>,
    pub revision: i64,
    #[serde(skip)]
    capture: WorkspaceCapture,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceCapture {
    id: String,
    name: String,
    roots: Vec<String>,
    revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceUpdate {
    pub workspace_id: String,
    pub expected_revision: i64,
    pub name: String,
    pub roots: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadWorkspaceContext {
    pub workspace_id: String,
    pub workspace_revision: Option<i64>,
    pub cwd: String,
    pub roots: Vec<String>,
    pub root_source: ThreadWorkspaceRootSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadWorkspaceRootSource {
    Direct,
    Workspace,
}

impl Client {
    pub async fn workspace(&self, workspace_id: &str) -> Result<Option<Workspace>> {
        self.ensure_open()?;
        self.inner
            .state
            .workspace(workspace_id)
            .await
            .map(|workspace| workspace.map(Workspace::from))
    }

    pub async fn workspaces(&self) -> Result<Vec<Workspace>> {
        self.ensure_open()?;
        Ok(self
            .inner
            .state
            .workspaces()
            .await?
            .into_iter()
            .map(Workspace::from)
            .collect())
    }

    pub async fn update_workspace(&self, update: WorkspaceUpdate) -> Result<Workspace> {
        self.ensure_open()?;
        self.inner
            .state
            .update_workspace(
                &update.workspace_id,
                update.expected_revision,
                &update.name,
                &update.roots,
            )
            .await
            .map(Workspace::from)
    }

    pub async fn thread_workspace_context(
        &self,
        thread_id: &str,
    ) -> Result<ThreadWorkspaceContext> {
        self.ensure_open()?;
        self.inner
            .state
            .thread_workspace_context(thread_id)
            .await?
            .map(|context| ThreadWorkspaceContext {
                workspace_id: context.workspace_id,
                workspace_revision: context.workspace_revision,
                cwd: context.cwd,
                roots: context.roots,
                root_source: match context.root_source.as_str() {
                    "workspace" => ThreadWorkspaceRootSource::Workspace,
                    _ => ThreadWorkspaceRootSource::Direct,
                },
            })
            .ok_or_else(|| Error::Message(format!("Thread `{thread_id}` has no Workspace context")))
    }
}

impl From<WorkspaceRecord> for Workspace {
    fn from(workspace: WorkspaceRecord) -> Self {
        let capture = WorkspaceCapture {
            id: workspace.id.clone(),
            name: workspace.name.clone(),
            roots: workspace.roots.clone(),
            revision: workspace.revision,
        };
        Self {
            id: workspace.id,
            name: workspace.name,
            roots: workspace.roots,
            revision: workspace.revision,
            capture,
        }
    }
}

impl Workspace {
    pub(super) fn validate_capture(&self) -> Result<()> {
        if self.id != self.capture.id
            || self.name != self.capture.name
            || self.roots != self.capture.roots
            || self.revision != self.capture.revision
        {
            return Err(Error::Message(
                "captured Workspace was modified outside the Framework".to_string(),
            ));
        }
        Ok(())
    }
}
