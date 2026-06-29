use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::{ErrorKind, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use psychevo_gateway_protocol as wire;
use psychevo_runtime::{
    Error, WorkspaceDiffFile, WorkspaceDiffFileStatus, canonicalize_cwd, collect_workspace_diff,
    resolve_workspace_root,
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
        let baseline = capture_workspace_baseline(cwd);
        let mut inner = self.inner.lock().expect("workspace review state poisoned");
        inner
            .pending
            .entry(turn_id.to_string())
            .or_insert_with(|| PendingReviewTurn {
                thread_id,
                cwd: cwd.to_path_buf(),
                baseline,
                created_at_ms: now_ms(),
            });
    }

    pub(super) fn complete_turn(&self, turn_id: &str) {
        let pending = {
            let mut inner = self.inner.lock().expect("workspace review state poisoned");
            inner.pending.remove(turn_id)
        };
        let Some(pending) = pending else {
            return;
        };
        let files = build_review_files(&pending.cwd, &pending.baseline).unwrap_or_default();
        if files.is_empty() {
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

pub(super) fn workspace_create_value(
    state: &WebState,
    auth: &AuthContext,
    params: wire::WorkspaceCreateParams,
) -> psychevo_runtime::Result<Value> {
    let dir_name = workspace_dir_name(&params.name)?;
    let options = state.run_options(state.inner.cwd.clone(), None);
    let root = canonicalize_cwd(&resolve_workspace_root(&options, &state.inner.cwd)?)?;
    let cwd = canonicalize_cwd(&root.join(&dir_name))?;
    if !cwd.starts_with(&root) {
        return Err(Error::Message(
            "workspace path is outside the configured workspace root".to_string(),
        ));
    }
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
    let canonical = candidate.canonicalize()?;
    if !canonical.starts_with(root) {
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
    if candidate.exists() {
        return resolve_workspace_relative_path(root, &normalized);
    }
    let parent = candidate
        .parent()
        .ok_or_else(|| Error::Message("workspace file parent is unavailable".to_string()))?;
    let canonical_parent = parent.canonicalize()?;
    if !canonical_parent.starts_with(root) {
        return Err(Error::Message(
            "workspace path is outside the workspace".to_string(),
        ));
    }
    Ok(candidate)
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

fn capture_workspace_baseline(cwd: &Path) -> WorkspaceBaseline {
    let mut baseline = WorkspaceBaseline::default();
    if let Ok(diff) = collect_workspace_diff(cwd) {
        for file in diff.files {
            baseline
                .files
                .insert(file.path.clone(), baseline_from_pre_turn_file(&file));
        }
    }
    baseline
}

fn baseline_from_pre_turn_file(file: &WorkspaceDiffFile) -> ReviewBaseline {
    if file.binary {
        return ReviewBaseline::Unsupported {
            reason: "Binary baseline cannot be restored.".to_string(),
        };
    }
    if file.unreadable {
        return ReviewBaseline::Unsupported {
            reason: "Unreadable baseline cannot be restored.".to_string(),
        };
    }
    if matches!(file.status, WorkspaceDiffFileStatus::Deleted) {
        return ReviewBaseline::Absent;
    }
    file.new_text
        .as_ref()
        .map(|content| ReviewBaseline::Text {
            content: content.clone(),
        })
        .unwrap_or_else(|| ReviewBaseline::Unsupported {
            reason: "Baseline content is unavailable.".to_string(),
        })
}

fn build_review_files(
    cwd: &Path,
    baseline: &WorkspaceBaseline,
) -> psychevo_runtime::Result<Vec<WorkspaceReviewFile>> {
    let diff = collect_workspace_diff(cwd)?;
    let mut post_by_path = HashMap::new();
    let mut candidates = HashSet::new();
    for file in diff.files {
        candidates.insert(file.path.clone());
        post_by_path.insert(file.path.clone(), file);
    }
    for path in baseline.files.keys() {
        candidates.insert(path.clone());
    }
    let mut files = Vec::new();
    for path in candidates {
        let pre = baseline.files.get(&path);
        if let Some(pre) = pre
            && baseline_matches_current(cwd, &path, pre)?
        {
            continue;
        }
        let post = post_by_path.get(&path);
        let review_baseline = pre
            .cloned()
            .or_else(|| post.and_then(baseline_from_post_diff))
            .unwrap_or_else(|| ReviewBaseline::Unsupported {
                reason: "Turn-start baseline is unavailable.".to_string(),
            });
        if post.is_none() && matches!(review_baseline, ReviewBaseline::Unsupported { .. }) {
            continue;
        }
        let post_revision = workspace_path_revision(cwd, &path)?;
        let status = post
            .map(|file| workspace_diff_status(file.status))
            .unwrap_or(wire::WorkspaceDiffFileStatusView::Modified);
        let binary = post.is_some_and(|file| file.binary);
        let unreadable = post.is_some_and(|file| file.unreadable);
        let message = review_baseline.message();
        files.push(WorkspaceReviewFile {
            path,
            status,
            binary,
            unreadable,
            review_status: wire::WorkspaceChangeReviewStatusView::Pending,
            baseline: review_baseline,
            post_revision,
            message,
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn baseline_from_post_diff(file: &WorkspaceDiffFile) -> Option<ReviewBaseline> {
    if file.binary {
        return Some(ReviewBaseline::Unsupported {
            reason: "Binary baseline cannot be restored.".to_string(),
        });
    }
    if file.unreadable {
        return Some(ReviewBaseline::Unsupported {
            reason: "Unreadable baseline cannot be restored.".to_string(),
        });
    }
    match file.status {
        WorkspaceDiffFileStatus::Added | WorkspaceDiffFileStatus::Untracked => {
            Some(ReviewBaseline::Absent)
        }
        WorkspaceDiffFileStatus::Deleted | WorkspaceDiffFileStatus::Modified => file
            .old_text
            .as_ref()
            .map(|content| ReviewBaseline::Text {
                content: content.clone(),
            })
            .or_else(|| {
                Some(ReviewBaseline::Unsupported {
                    reason: "Baseline content is unavailable.".to_string(),
                })
            }),
        WorkspaceDiffFileStatus::Binary | WorkspaceDiffFileStatus::Unreadable => {
            Some(ReviewBaseline::Unsupported {
                reason: "Baseline content is unavailable.".to_string(),
            })
        }
    }
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
