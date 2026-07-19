use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::{ErrorKind, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use psychevo_gateway_protocol as wire;
use psychevo_runtime::{
    Error, WorkspaceDiffFileStatus, WorkspaceMutation, canonicalize_cwd, collect_workspace_diff,
    normalized_native_path, resolve_workspace_root,
};
use serde_json::Value;

use super::{
    AuthContext, ResolvedScope, WebState, cwd_source, now_ms, update_browser_session_scope,
};

const MAX_WORKSPACE_FILE_DEPTH: usize = 8;
const MAX_WORKSPACE_FILE_ITEMS: usize = 1_500;
const MAX_WORKSPACE_TEXT_FILE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Default)]
pub(super) struct WorkspaceReviewState {
    inner: Arc<Mutex<WorkspaceReviewInner>>,
}

#[derive(Default)]
struct WorkspaceReviewInner {
    pending: HashMap<String, PendingReviewTurn>,
    groups: Vec<WorkspaceReviewGroup>,
}

#[derive(Clone)]
struct PendingReviewTurn {
    thread_id: Option<String>,
    cwd: PathBuf,
    baseline: WorkspaceBaseline,
    invalidations: Vec<WorkspaceReviewInvalidation>,
    created_at_ms: i64,
}

#[derive(Clone, Default)]
struct WorkspaceBaseline {
    files: HashMap<String, ReviewBaseline>,
}

#[derive(Clone)]
struct WorkspaceReviewGroup {
    turn_id: String,
    thread_id: Option<String>,
    cwd: PathBuf,
    created_at_ms: i64,
    completed_at_ms: i64,
    files: Vec<WorkspaceReviewFile>,
    invalidations: Vec<WorkspaceReviewInvalidation>,
}

#[derive(Clone)]
struct WorkspaceReviewInvalidation {
    source: String,
}

#[derive(Clone)]
struct WorkspaceReviewFile {
    path: String,
    status: wire::WorkspaceDiffFileStatusView,
    binary: bool,
    unreadable: bool,
    review_status: wire::WorkspaceChangeReviewStatusView,
    baseline: ReviewBaseline,
    post_revision: String,
    message: Option<String>,
}

#[derive(Clone)]
enum ReviewBaseline {
    Text { content: String },
    Absent,
    Unsupported { reason: String },
}

impl ReviewBaseline {
    fn can_reject(&self) -> bool {
        !matches!(self, Self::Unsupported { .. })
    }

    fn message(&self) -> Option<String> {
        match self {
            Self::Unsupported { reason } => Some(reason.clone()),
            _ => None,
        }
    }
}

impl WorkspaceReviewState {
    pub(super) fn begin_turn(&self, turn_id: &str, thread_id: Option<String>, cwd: &Path) {
        let mut inner = self.inner.lock().expect("workspace review state poisoned");
        inner
            .pending
            .entry(turn_id.to_string())
            .or_insert_with(|| PendingReviewTurn {
                thread_id,
                cwd: cwd.to_path_buf(),
                baseline: WorkspaceBaseline::default(),
                invalidations: Vec::new(),
                created_at_ms: now_ms(),
            });
    }

    pub(super) fn observe_event(&self, event: &wire::GatewayEvent, cwd: &Path) {
        match event {
            wire::GatewayEvent::TurnStarted {
                thread_id, turn_id, ..
            } => self.begin_turn(turn_id, thread_id.clone(), cwd),
            wire::GatewayEvent::TurnCompleted { turn_id, .. } => self.complete_turn(turn_id),
            _ => {}
        }
    }

    pub(super) fn observe_mutation(&self, turn_id: &str, cwd: &Path, mutation: WorkspaceMutation) {
        let mut inner = self.inner.lock().expect("workspace review state poisoned");
        let Some(pending) = inner
            .pending
            .get_mut(turn_id)
            .filter(|pending| pending.cwd == cwd)
        else {
            return;
        };
        match mutation {
            WorkspaceMutation::ExactUtf8 { path, before, .. } => {
                let path = normalize_workspace_path(&path);
                if path.is_empty() || pending.baseline.files.contains_key(&path) {
                    return;
                }
                let baseline = match before {
                    Some(content) if content.len() <= MAX_WORKSPACE_TEXT_FILE_BYTES => {
                        ReviewBaseline::Text { content }
                    }
                    Some(_) => ReviewBaseline::Unsupported {
                        reason: "Files larger than 1 MB cannot be restored from Review."
                            .to_string(),
                    },
                    None => ReviewBaseline::Absent,
                };
                pending.baseline.files.insert(path, baseline);
            }
            WorkspaceMutation::Opaque { source } => {
                pending
                    .invalidations
                    .push(WorkspaceReviewInvalidation { source });
            }
        }
    }

    pub(super) fn complete_turn(&self, turn_id: &str) {
        let pending = {
            let mut inner = self.inner.lock().expect("workspace review state poisoned");
            inner.pending.remove(turn_id)
        };
        let Some(pending) = pending else {
            return;
        };
        let files = build_observed_review_files(&pending.cwd, &pending.baseline);
        if files.is_empty() && pending.invalidations.is_empty() {
            return;
        }
        let mut inner = self.inner.lock().expect("workspace review state poisoned");
        inner.groups.retain(|group| group.turn_id != turn_id);
        inner.groups.insert(
            0,
            WorkspaceReviewGroup {
                turn_id: turn_id.to_string(),
                thread_id: pending.thread_id,
                cwd: pending.cwd,
                created_at_ms: pending.created_at_ms,
                completed_at_ms: now_ms(),
                files,
                invalidations: pending.invalidations,
            },
        );
        inner.groups.truncate(40);
    }

    pub(super) fn changes_for_scope(&self, scope: &ResolvedScope) -> wire::WorkspaceChangesResult {
        let inner = self.inner.lock().expect("workspace review state poisoned");
        wire::WorkspaceChangesResult {
            groups: inner
                .groups
                .iter()
                .filter(|group| group.cwd == scope.cwd)
                .map(review_group_to_wire)
                .collect(),
        }
    }

    pub(super) fn accept(
        &self,
        scope: &ResolvedScope,
        turn_id: &str,
        path: &str,
    ) -> psychevo_runtime::Result<wire::WorkspaceChangeMutationResult> {
        let path = normalize_workspace_path(path);
        let mut accepted = false;
        {
            let mut inner = self.inner.lock().expect("workspace review state poisoned");
            if let Some(file) = inner
                .groups
                .iter_mut()
                .find(|group| group.cwd == scope.cwd && group.turn_id == turn_id)
                .and_then(|group| group.files.iter_mut().find(|file| file.path == path))
            {
                file.review_status = wire::WorkspaceChangeReviewStatusView::Accepted;
                file.message = None;
                accepted = true;
            }
        }
        Ok(wire::WorkspaceChangeMutationResult {
            accepted,
            changes: self.changes_for_scope(scope),
        })
    }

    pub(super) fn reject(
        &self,
        scope: &ResolvedScope,
        turn_id: &str,
        path: &str,
    ) -> psychevo_runtime::Result<wire::WorkspaceChangeMutationResult> {
        let path = normalize_workspace_path(path);
        let file = {
            let inner = self.inner.lock().expect("workspace review state poisoned");
            inner
                .groups
                .iter()
                .find(|group| group.cwd == scope.cwd && group.turn_id == turn_id)
                .and_then(|group| group.files.iter().find(|file| file.path == path))
                .cloned()
        };
        let Some(file) = file else {
            return Ok(wire::WorkspaceChangeMutationResult {
                accepted: false,
                changes: self.changes_for_scope(scope),
            });
        };
        if !file.baseline.can_reject() {
            return Ok(wire::WorkspaceChangeMutationResult {
                accepted: false,
                changes: self.changes_for_scope(scope),
            });
        }
        let current_revision = workspace_path_revision(&scope.cwd, &path)?;
        if current_revision != file.post_revision {
            self.mark_conflict(scope, turn_id, &path, "File changed after this turn.");
            return Ok(wire::WorkspaceChangeMutationResult {
                accepted: false,
                changes: self.changes_for_scope(scope),
            });
        }
        restore_review_baseline(&scope.cwd, &path, &file.baseline)?;
        {
            let mut inner = self.inner.lock().expect("workspace review state poisoned");
            if let Some(file) = inner
                .groups
                .iter_mut()
                .find(|group| group.cwd == scope.cwd && group.turn_id == turn_id)
                .and_then(|group| group.files.iter_mut().find(|file| file.path == path))
            {
                file.review_status = wire::WorkspaceChangeReviewStatusView::Rejected;
                file.message = None;
            }
        }
        Ok(wire::WorkspaceChangeMutationResult {
            accepted: true,
            changes: self.changes_for_scope(scope),
        })
    }

    fn mark_conflict(&self, scope: &ResolvedScope, turn_id: &str, path: &str, message: &str) {
        let mut inner = self.inner.lock().expect("workspace review state poisoned");
        if let Some(file) = inner
            .groups
            .iter_mut()
            .find(|group| group.cwd == scope.cwd && group.turn_id == turn_id)
            .and_then(|group| group.files.iter_mut().find(|file| file.path == path))
        {
            file.review_status = wire::WorkspaceChangeReviewStatusView::Conflict;
            file.message = Some(message.to_string());
        }
    }
}

pub(super) fn workspace_files_value(scope: &ResolvedScope) -> psychevo_runtime::Result<Value> {
    let mut entries = Vec::new();
    let mut truncated = false;
    collect_workspace_file_entries(&scope.cwd, &scope.cwd, 0, &mut entries, &mut truncated);
    Ok(serde_json::to_value(wire::WorkspaceFilesResult {
        root: scope.cwd.display().to_string(),
        entries,
        truncated,
    })?)
}

pub(super) fn workspace_folder_list_value(
    _state: &WebState,
    scope: &ResolvedScope,
    requested_path: Option<&str>,
) -> psychevo_runtime::Result<Value> {
    let requested = requested_path
        .map(PathBuf::from)
        .unwrap_or_else(|| scope.cwd.clone());
    let current = canonical_existing_directory(&requested)?;
    let root = current
        .ancestors()
        .last()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| current.clone());
    let mut folders = std::fs::read_dir(&current)?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                return None;
            }
            let path = canonicalize_cwd(&entry.path()).ok()?;
            Some(wire::WorkspaceFolderEntry {
                name,
                path: path.display().to_string(),
            })
        })
        .collect::<Vec<_>>();
    folders.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.name.cmp(&right.name))
    });
    let parent = (current != root)
        .then(|| current.parent())
        .flatten()
        .map(|parent| parent.display().to_string());
    Ok(serde_json::to_value(wire::WorkspaceFolderListResult {
        root: root.display().to_string(),
        current: current.display().to_string(),
        parent,
        folders,
    })?)
}

pub(super) fn workspace_git_branches_value(
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<Value> {
    Ok(serde_json::to_value(workspace_git_branches(scope)?)?)
}

pub(super) fn workspace_git_checkout_value(
    scope: &ResolvedScope,
    params: wire::WorkspaceGitCheckoutParams,
) -> psychevo_runtime::Result<Value> {
    let branch = params.branch.trim();
    if branch.is_empty() {
        return Err(Error::Message(
            "Git branch name must not be empty".to_string(),
        ));
    }
    let checked = run_git(&scope.cwd, &["check-ref-format", "--branch", branch])?;
    if checked.trim() != branch {
        return Err(Error::Message("Git branch name is not valid".to_string()));
    }
    let before = workspace_git_branches(scope)?;
    let exists = before.branches.iter().any(|candidate| candidate == branch);
    if params.create && exists {
        return Err(Error::Message(format!(
            "Git branch already exists: {branch}"
        )));
    }
    if !params.create && !exists {
        return Err(Error::Message(format!(
            "Git branch does not exist: {branch}"
        )));
    }
    if params.create {
        run_git(&scope.cwd, &["switch", "-c", branch])?;
    } else if before.current.as_deref() != Some(branch) {
        run_git(&scope.cwd, &["switch", "--", branch])?;
    }
    Ok(serde_json::to_value(workspace_git_branches(scope)?)?)
}

fn workspace_git_branches(
    scope: &ResolvedScope,
) -> psychevo_runtime::Result<wire::WorkspaceGitBranchesResult> {
    let branches = run_git(
        &scope.cwd,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )?
    .lines()
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .map(ToOwned::to_owned)
    .collect::<Vec<_>>();
    let current_output = std::process::Command::new("git")
        .arg("-C")
        .arg(&scope.cwd)
        .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
        .output()?;
    let current = current_output.status.success().then(|| {
        String::from_utf8_lossy(&current_output.stdout)
            .trim()
            .to_string()
    });
    Ok(wire::WorkspaceGitBranchesResult { current, branches })
}

fn run_git(cwd: &Path, args: &[&str]) -> psychevo_runtime::Result<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr.trim();
        let bounded = message.chars().take(480).collect::<String>();
        return Err(Error::Message(if bounded.is_empty() {
            "Git command failed".to_string()
        } else {
            bounded
        }));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(super) fn workspace_create_value(
    state: &WebState,
    auth: &AuthContext,
    params: wire::WorkspaceCreateParams,
) -> psychevo_runtime::Result<Value> {
    let dir_name = workspace_dir_name(&params.name)?;
    let parent = if let Some(parent) = params.parent.as_deref() {
        canonical_existing_directory(Path::new(parent))?
    } else {
        let options = state.run_options(state.inner.cwd.clone(), None);
        canonicalize_cwd(&resolve_workspace_root(&options, &state.inner.cwd)?)?
    };
    let cwd = canonicalize_cwd(&parent.join(&dir_name))?;
    let scope = ResolvedScope {
        source: cwd_source(&cwd),
        cwd,
    };
    update_browser_session_scope(state, auth, &scope);
    Ok(serde_json::to_value(wire::WorkspaceCreateResult {
        cwd: scope.cwd.display().to_string(),
        scope: scope.to_wire_scope(),
    })?)
}

fn canonical_existing_directory(path: &Path) -> psychevo_runtime::Result<PathBuf> {
    let canonical = normalized_native_path(&std::fs::canonicalize(path)?);
    if !canonical.is_dir() {
        return Err(Error::Message(format!(
            "workspace parent is not a directory: {}",
            canonical.display()
        )));
    }
    Ok(canonical)
}

pub(super) fn workspace_dir_name(input: &str) -> psychevo_runtime::Result<String> {
    let name = input.trim();
    if name.is_empty() {
        return Err(Error::Message(
            "workspace name must not be empty".to_string(),
        ));
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err(Error::Message(
            "workspace name must be a single directory name".to_string(),
        ));
    }
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(name.to_string()),
        _ => Err(Error::Message(
            "workspace name must be a single directory name".to_string(),
        )),
    }
}

fn collect_workspace_file_entries(
    root: &Path,
    dir: &Path,
    depth: usize,
    entries: &mut Vec<wire::WorkspaceFileEntry>,
    truncated: &mut bool,
) {
    if depth > MAX_WORKSPACE_FILE_DEPTH || entries.len() >= MAX_WORKSPACE_FILE_ITEMS {
        *truncated = true;
        return;
    }
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    let mut children = read_dir.flatten().collect::<Vec<_>>();
    children.sort_by_key(|entry| {
        let dir_rank = if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            0
        } else {
            1
        };
        (
            dir_rank,
            entry.file_name().to_string_lossy().to_ascii_lowercase(),
        )
    });
    for entry in children {
        if entries.len() >= MAX_WORKSPACE_FILE_ITEMS {
            *truncated = true;
            return;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if should_skip_workspace_path(&name) {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        let is_dir = file_type.is_dir();
        if !is_dir && !file_type.is_file() {
            continue;
        }
        entries.push(wire::WorkspaceFileEntry {
            path: relative,
            name,
            kind: if is_dir {
                wire::WorkspaceFileKind::Directory
            } else {
                wire::WorkspaceFileKind::File
            },
            depth,
        });
        if is_dir {
            collect_workspace_file_entries(root, &path, depth + 1, entries, truncated);
        }
    }
}

pub(super) fn workspace_file_read_value(
    scope: &ResolvedScope,
    path: &str,
) -> psychevo_runtime::Result<Value> {
    let resolved = resolve_workspace_relative_path(&scope.cwd, path)?;
    let display_path =
        path_from_root(&scope.cwd, &resolved).unwrap_or_else(|| normalize_workspace_path(path));
    let snapshot = match read_workspace_text_snapshot(&resolved) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            return Ok(serde_json::to_value(wire::WorkspaceFileReadResult {
                path: display_path,
                content: None,
                truncated: false,
                binary: false,
                editable: false,
                editable_reason: Some(err.to_string()),
                size_bytes: 0,
                revision: "unreadable".to_string(),
                line_ending: None,
                unreadable: Some(err.to_string()),
            })?);
        }
    };
    let editable_reason = workspace_editable_reason(&snapshot);
    Ok(serde_json::to_value(wire::WorkspaceFileReadResult {
        path: display_path,
        content: snapshot.content,
        truncated: snapshot.truncated,
        binary: snapshot.binary,
        editable: editable_reason.is_none(),
        editable_reason,
        size_bytes: snapshot.size_bytes,
        revision: snapshot.revision,
        line_ending: snapshot.line_ending,
        unreadable: None,
    })?)
}

pub(super) fn workspace_file_write_value(
    scope: &ResolvedScope,
    params: wire::WorkspaceFileWriteParams,
) -> psychevo_runtime::Result<Value> {
    if params.content.len() > MAX_WORKSPACE_TEXT_FILE_BYTES {
        return Err(Error::Message(
            "workspace file is larger than 1 MB".to_string(),
        ));
    }
    if params.content.as_bytes().contains(&0) {
        return Err(Error::Message(
            "workspace file content must be text".to_string(),
        ));
    }
    let resolved = resolve_workspace_write_path(&scope.cwd, &params.path)?;
    let path = path_from_root(&scope.cwd, &resolved)
        .unwrap_or_else(|| normalize_workspace_path(&params.path));
    let current_revision = workspace_path_revision(&scope.cwd, &path)?;
    if !params.force
        && let Some(expected) = params.expected_revision.as_deref()
        && expected != current_revision
    {
        return Err(Error::Message("workspace file changed on disk".to_string()));
    }
    std::fs::write(&resolved, params.content.as_bytes())?;
    let revision = workspace_path_revision(&scope.cwd, &path)?;
    Ok(serde_json::to_value(wire::WorkspaceFileWriteResult {
        path,
        revision,
        size_bytes: params.content.len(),
        line_ending: detect_line_ending(&params.content),
    })?)
}

fn resolve_workspace_relative_path(root: &Path, path: &str) -> psychevo_runtime::Result<PathBuf> {
    let raw = Path::new(path);
    if raw.is_absolute() || path.contains('\0') {
        return Err(Error::Message(
            "workspace path must be relative".to_string(),
        ));
    }
    let normalized = normalize_workspace_path(path);
    if normalized.is_empty() || normalized.starts_with("../") || normalized == ".." {
        return Err(Error::Message(
            "workspace path must be relative".to_string(),
        ));
    }
    let candidate = root.join(&normalized);
    let canonical_root = canonical_workspace_path_identity(root)?;
    let canonical = canonical_workspace_path_identity(&candidate)?;
    if !canonical.starts_with(&canonical_root) {
        return Err(Error::Message(
            "workspace path is outside the workspace".to_string(),
        ));
    }
    Ok(canonical)
}

fn resolve_workspace_write_path(root: &Path, path: &str) -> psychevo_runtime::Result<PathBuf> {
    let raw = Path::new(path);
    if raw.is_absolute() || path.contains('\0') {
        return Err(Error::Message(
            "workspace path must be relative".to_string(),
        ));
    }
    let normalized = normalize_workspace_path(path);
    if normalized.is_empty() || normalized.starts_with("../") || normalized == ".." {
        return Err(Error::Message(
            "workspace path must be relative".to_string(),
        ));
    }
    let candidate = root.join(&normalized);
    match candidate.symlink_metadata() {
        Ok(_) => return resolve_workspace_relative_path(root, &normalized),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    let parent = candidate
        .parent()
        .ok_or_else(|| Error::Message("workspace file parent is unavailable".to_string()))?;
    let canonical_root = canonical_workspace_path_identity(root)?;
    let canonical_parent = canonical_workspace_path_identity(parent)?;
    if !canonical_parent.starts_with(&canonical_root) {
        return Err(Error::Message(
            "workspace path is outside the workspace".to_string(),
        ));
    }
    Ok(candidate)
}

fn canonical_workspace_path_identity(path: &Path) -> psychevo_runtime::Result<PathBuf> {
    Ok(normalized_workspace_path_identity(&path.canonicalize()?))
}

pub(super) fn normalized_workspace_path_identity(path: &Path) -> PathBuf {
    normalized_native_path(path)
}

fn normalize_workspace_path(path: &str) -> String {
    path.trim()
        .trim_start_matches('/')
        .replace('\\', "/")
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/")
}

fn path_from_root(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root)
        .ok()
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
}

struct WorkspaceTextSnapshot {
    content: Option<String>,
    truncated: bool,
    binary: bool,
    size_bytes: usize,
    revision: String,
    line_ending: Option<String>,
}

fn read_workspace_text_snapshot(path: &Path) -> psychevo_runtime::Result<WorkspaceTextSnapshot> {
    let metadata = std::fs::metadata(path)?;
    let size_bytes = metadata.len() as usize;
    let mut file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take((MAX_WORKSPACE_TEXT_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    let truncated = bytes.len() > MAX_WORKSPACE_TEXT_FILE_BYTES;
    if truncated {
        bytes.truncate(MAX_WORKSPACE_TEXT_FILE_BYTES);
    }
    let binary = bytes.contains(&0) || std::str::from_utf8(&bytes).is_err();
    let content = if binary {
        None
    } else {
        Some(String::from_utf8_lossy(&bytes).into_owned())
    };
    let line_ending = content.as_deref().and_then(detect_line_ending);
    Ok(WorkspaceTextSnapshot {
        content,
        truncated,
        binary,
        size_bytes,
        revision: revision_for_bytes(&bytes, Some(size_bytes)),
        line_ending,
    })
}

fn workspace_editable_reason(snapshot: &WorkspaceTextSnapshot) -> Option<String> {
    if snapshot.binary {
        Some("Binary files cannot be edited in Workbench.".to_string())
    } else if snapshot.truncated || snapshot.size_bytes > MAX_WORKSPACE_TEXT_FILE_BYTES {
        Some("Files larger than 1 MB cannot be edited in Workbench.".to_string())
    } else {
        None
    }
}

fn workspace_path_revision(root: &Path, path: &str) -> psychevo_runtime::Result<String> {
    let resolved = resolve_workspace_write_path(root, path)?;
    if !resolved.exists() {
        return Ok("missing".to_string());
    }
    let bytes = std::fs::read(&resolved)?;
    Ok(revision_for_bytes(&bytes, Some(bytes.len())))
}

fn revision_for_bytes(bytes: &[u8], full_size: Option<usize>) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    full_size.unwrap_or(bytes.len()).hash(&mut hasher);
    format!(
        "r{:016x}:{}",
        hasher.finish(),
        full_size.unwrap_or(bytes.len())
    )
}

fn detect_line_ending(content: &str) -> Option<String> {
    if content.contains("\r\n") {
        Some("crlf".to_string())
    } else if content.contains('\n') {
        Some("lf".to_string())
    } else {
        None
    }
}

pub(super) fn workspace_diff_value(
    scope: &ResolvedScope,
    path: Option<&str>,
) -> psychevo_runtime::Result<Value> {
    Ok(serde_json::to_value(workspace_diff_result(scope, path)?)?)
}

pub(super) fn workspace_diff_result(
    scope: &ResolvedScope,
    path: Option<&str>,
) -> psychevo_runtime::Result<wire::WorkspaceDiffResult> {
    let diff = collect_workspace_diff(&scope.cwd)?;
    let selected = path
        .map(|path| {
            let raw = Path::new(path);
            if raw.is_absolute() || path.contains('\0') {
                return Err(Error::Message(
                    "workspace diff path must be relative".to_string(),
                ));
            }
            Ok(normalize_workspace_path(path))
        })
        .transpose()?
        .filter(|path| !path.is_empty());
    let files = diff
        .files
        .iter()
        .filter(|file| {
            selected
                .as_deref()
                .is_none_or(|selected| file.path == selected)
        })
        .map(|file| wire::WorkspaceDiffFileView {
            path: file.path.clone(),
            status: workspace_diff_status(file.status),
            binary: file.binary,
            unreadable: file.unreadable,
            placeholder: file.placeholder.clone(),
        })
        .collect::<Vec<_>>();
    let unified_diff = if let Some(selected) = selected.as_deref() {
        extract_unified_diff_for_path(&diff.unified_diff, selected).unwrap_or_else(|| {
            diff.files
                .iter()
                .find(|file| file.path == selected)
                .and_then(|file| file.placeholder.clone())
                .unwrap_or_default()
        })
    } else {
        diff.unified_diff
    };
    Ok(wire::WorkspaceDiffResult {
        is_git_repo: diff.is_git_repo,
        files,
        unified_diff,
        truncation: wire::WorkspaceDiffTruncationView {
            truncated: diff.truncation.truncated,
            max_bytes: diff.truncation.max_bytes,
            max_lines: diff.truncation.max_lines,
            omitted_bytes: diff.truncation.omitted_bytes,
            omitted_lines: diff.truncation.omitted_lines,
        },
        selected_path: selected,
    })
}

fn workspace_diff_status(status: WorkspaceDiffFileStatus) -> wire::WorkspaceDiffFileStatusView {
    match status {
        WorkspaceDiffFileStatus::Modified => wire::WorkspaceDiffFileStatusView::Modified,
        WorkspaceDiffFileStatus::Added => wire::WorkspaceDiffFileStatusView::Added,
        WorkspaceDiffFileStatus::Deleted => wire::WorkspaceDiffFileStatusView::Deleted,
        WorkspaceDiffFileStatus::Untracked => wire::WorkspaceDiffFileStatusView::Untracked,
        WorkspaceDiffFileStatus::Binary => wire::WorkspaceDiffFileStatusView::Binary,
        WorkspaceDiffFileStatus::Unreadable => wire::WorkspaceDiffFileStatusView::Unreadable,
    }
}

fn build_observed_review_files(
    cwd: &Path,
    baseline: &WorkspaceBaseline,
) -> Vec<WorkspaceReviewFile> {
    let mut files = Vec::new();
    for (path, pre) in &baseline.files {
        if baseline_matches_current(cwd, path, pre).unwrap_or(false) {
            continue;
        }
        let resolved = match resolve_workspace_write_path(cwd, path) {
            Ok(resolved) => resolved,
            Err(_) => continue,
        };
        let (status, binary, unreadable, post_revision, post_message) = if !resolved.exists() {
            (
                wire::WorkspaceDiffFileStatusView::Deleted,
                false,
                false,
                "missing".to_string(),
                None,
            )
        } else {
            match read_workspace_text_snapshot(&resolved) {
                Ok(snapshot) => (
                    if snapshot.binary {
                        wire::WorkspaceDiffFileStatusView::Binary
                    } else if matches!(pre, ReviewBaseline::Absent) {
                        wire::WorkspaceDiffFileStatusView::Added
                    } else {
                        wire::WorkspaceDiffFileStatusView::Modified
                    },
                    snapshot.binary,
                    false,
                    snapshot.revision,
                    snapshot.truncated.then(|| {
                        "File is larger than 1 MB; Review recorded only the exact observed path."
                            .to_string()
                    }),
                ),
                Err(error) => (
                    wire::WorkspaceDiffFileStatusView::Unreadable,
                    false,
                    true,
                    "unreadable".to_string(),
                    Some(error.to_string()),
                ),
            }
        };
        files.push(WorkspaceReviewFile {
            path: path.clone(),
            status,
            binary,
            unreadable,
            review_status: wire::WorkspaceChangeReviewStatusView::Pending,
            baseline: pre.clone(),
            post_revision,
            message: post_message.or_else(|| pre.message()),
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    files
}

fn baseline_matches_current(
    cwd: &Path,
    path: &str,
    baseline: &ReviewBaseline,
) -> psychevo_runtime::Result<bool> {
    let resolved = resolve_workspace_write_path(cwd, path)?;
    match baseline {
        ReviewBaseline::Text { content } => {
            if !resolved.exists() {
                return Ok(false);
            }
            let bytes = std::fs::read(&resolved)?;
            Ok(bytes == content.as_bytes())
        }
        ReviewBaseline::Absent => Ok(!resolved.exists()),
        ReviewBaseline::Unsupported { .. } => Ok(false),
    }
}

fn restore_review_baseline(
    cwd: &Path,
    path: &str,
    baseline: &ReviewBaseline,
) -> psychevo_runtime::Result<()> {
    let resolved = resolve_workspace_write_path(cwd, path)?;
    match baseline {
        ReviewBaseline::Text { content } => {
            std::fs::write(resolved, content.as_bytes())?;
            Ok(())
        }
        ReviewBaseline::Absent => match std::fs::remove_file(&resolved) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        },
        ReviewBaseline::Unsupported { reason } => Err(Error::Message(reason.clone())),
    }
}

fn review_group_to_wire(group: &WorkspaceReviewGroup) -> wire::WorkspaceChangeGroupView {
    wire::WorkspaceChangeGroupView {
        turn_id: group.turn_id.clone(),
        thread_id: group.thread_id.clone(),
        created_at_ms: group.created_at_ms,
        completed_at_ms: group.completed_at_ms,
        files: group.files.iter().map(review_file_to_wire).collect(),
        invalidations: group
            .invalidations
            .iter()
            .map(|invalidation| wire::WorkspaceChangeInvalidationView {
                source: invalidation.source.clone(),
                message: format!(
                    "Workspace may have changed via {}; inspect the diff. Exact Reject is unavailable.",
                    invalidation.source
                ),
            })
            .collect(),
    }
}

fn review_file_to_wire(file: &WorkspaceReviewFile) -> wire::WorkspaceChangeFileView {
    wire::WorkspaceChangeFileView {
        path: file.path.clone(),
        status: file.status,
        binary: file.binary,
        unreadable: file.unreadable,
        review_status: file.review_status,
        can_reject: file.baseline.can_reject(),
        message: file.message.clone().or_else(|| file.baseline.message()),
    }
}

fn extract_unified_diff_for_path(diff: &str, path: &str) -> Option<String> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    for line in diff.split_inclusive('\n') {
        if line.starts_with("diff --git ") && !current.is_empty() {
            blocks.push(std::mem::take(&mut current));
        }
        current.push_str(line);
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks.into_iter().find(|block| {
        let header = block.lines().next().unwrap_or_default();
        diff_header_matches_path(header, path)
            || block.lines().take(6).any(|line| {
                line.strip_prefix("+++ b/")
                    .is_some_and(|candidate| candidate == path)
                    || line
                        .strip_prefix("--- a/")
                        .is_some_and(|candidate| candidate == path)
            })
    })
}

fn diff_header_matches_path(header: &str, path: &str) -> bool {
    header.contains(&format!(" a/{path} "))
        || header.ends_with(&format!(" a/{path}"))
        || header.contains(&format!(" b/{path} "))
        || header.ends_with(&format!(" b/{path}"))
}

fn should_skip_workspace_path(name: &str) -> bool {
    matches!(name, ".git" | ".local" | "target" | "node_modules")
}
