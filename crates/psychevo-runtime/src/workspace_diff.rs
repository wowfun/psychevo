use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const WORKSPACE_DIFF_MAX_BYTES: usize = 256 * 1024;
pub const WORKSPACE_DIFF_MAX_LINES: usize = 3000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceDiff {
    pub is_git_repo: bool,
    pub files: Vec<WorkspaceDiffFile>,
    pub unified_diff: String,
    pub truncation: WorkspaceDiffTruncation,
}

impl WorkspaceDiff {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.unified_diff.trim().is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceDiffTruncation {
    pub truncated: bool,
    pub max_bytes: usize,
    pub max_lines: usize,
    pub omitted_bytes: usize,
    pub omitted_lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceDiffFile {
    pub path: String,
    pub status: WorkspaceDiffFileStatus,
    pub old_text: Option<String>,
    pub new_text: Option<String>,
    pub binary: bool,
    pub unreadable: bool,
    pub placeholder: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceDiffFileStatus {
    Modified,
    Added,
    Deleted,
    Untracked,
    Binary,
    Unreadable,
}

pub fn collect_workspace_diff(workdir: &Path) -> Result<WorkspaceDiff> {
    collect_workspace_diff_with_caps(workdir, WORKSPACE_DIFF_MAX_BYTES, WORKSPACE_DIFF_MAX_LINES)
}

pub fn collect_workspace_diff_with_caps(
    workdir: &Path,
    max_bytes: usize,
    max_lines: usize,
) -> Result<WorkspaceDiff> {
    if !is_inside_git_work_tree(workdir)? {
        return Ok(WorkspaceDiff {
            is_git_repo: false,
            files: Vec::new(),
            unified_diff: String::new(),
            truncation: WorkspaceDiffTruncation {
                truncated: false,
                max_bytes,
                max_lines,
                omitted_bytes: 0,
                omitted_lines: 0,
            },
        });
    }

    let tracked_diff = git_stdout(workdir, ["diff", "--no-color"], true)?;
    let tracked_files = tracked_diff_files(workdir)?;
    let untracked_paths = untracked_files(workdir)?;
    let mut files = Vec::new();
    for tracked in tracked_files {
        files.push(build_tracked_file_entry(workdir, tracked));
    }

    let mut accumulator = DiffAccumulator::new(max_bytes, max_lines);
    accumulator.append(&tracked_diff);

    for path in untracked_paths {
        let entry = build_untracked_file_entry(workdir, &path);
        match git_stdout(
            workdir,
            [
                OsStr::new("diff"),
                OsStr::new("--no-color"),
                OsStr::new("--no-index"),
                OsStr::new("--"),
                OsStr::new("/dev/null"),
                OsStr::new(&path),
            ],
            true,
        ) {
            Ok(diff) if !diff.is_empty() => accumulator.append(&diff),
            Ok(_) => accumulator.append(&placeholder_diff(&path, &entry)),
            Err(_) => accumulator.append(&placeholder_diff(&path, &entry)),
        }
        files.push(entry);
    }

    let (unified_diff, truncation) = accumulator.finish();
    Ok(WorkspaceDiff {
        is_git_repo: true,
        files,
        unified_diff,
        truncation,
    })
}

fn is_inside_git_work_tree(workdir: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(workdir)
        .output()?;
    if !output.status.success() {
        return Ok(false);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim() == "true")
}

fn tracked_diff_files(workdir: &Path) -> Result<Vec<TrackedDiffFile>> {
    let output = git_stdout_bytes(workdir, ["diff", "--name-status", "-z"], false)?;
    let fields = nul_fields(&output);
    let mut files = Vec::new();
    let mut index = 0usize;
    while index < fields.len() {
        let status = fields[index].clone();
        index += 1;
        if status.is_empty() || index >= fields.len() {
            break;
        }
        let code = status.as_bytes()[0] as char;
        let path = if matches!(code, 'R' | 'C') {
            if index + 1 >= fields.len() {
                break;
            }
            index += 1;
            let new_path = fields[index].clone();
            index += 1;
            new_path
        } else {
            let path = fields[index].clone();
            index += 1;
            path
        };
        files.push(TrackedDiffFile {
            path,
            status: tracked_status(code),
        });
    }
    Ok(files)
}

fn untracked_files(workdir: &Path) -> Result<Vec<String>> {
    git_stdout_bytes(
        workdir,
        ["ls-files", "--others", "--exclude-standard", "-z"],
        false,
    )
    .map(|stdout| nul_fields(&stdout))
}

fn tracked_status(code: char) -> WorkspaceDiffFileStatus {
    match code {
        'A' => WorkspaceDiffFileStatus::Added,
        'D' => WorkspaceDiffFileStatus::Deleted,
        _ => WorkspaceDiffFileStatus::Modified,
    }
}

fn build_tracked_file_entry(workdir: &Path, tracked: TrackedDiffFile) -> WorkspaceDiffFile {
    let old_text = git_blob_text(workdir, &tracked.path);
    let new_text = match tracked.status {
        WorkspaceDiffFileStatus::Deleted => SafeText::Text(String::new()),
        _ => read_text_file(&workdir.join(&tracked.path)),
    };
    finalize_file_entry(tracked.path, tracked.status, old_text, new_text)
}

fn build_untracked_file_entry(workdir: &Path, path: &str) -> WorkspaceDiffFile {
    finalize_file_entry(
        path.to_string(),
        WorkspaceDiffFileStatus::Untracked,
        SafeText::Unreadable,
        read_text_file(&workdir.join(path)),
    )
}

fn finalize_file_entry(
    path: String,
    status: WorkspaceDiffFileStatus,
    old_text: SafeText,
    new_text: SafeText,
) -> WorkspaceDiffFile {
    let old_required = matches!(
        status,
        WorkspaceDiffFileStatus::Modified | WorkspaceDiffFileStatus::Deleted
    );
    let new_required = matches!(
        status,
        WorkspaceDiffFileStatus::Modified
            | WorkspaceDiffFileStatus::Added
            | WorkspaceDiffFileStatus::Untracked
    );
    let binary = (old_required && matches!(old_text, SafeText::Binary))
        || (new_required && matches!(new_text, SafeText::Binary));
    let unreadable = (old_required && matches!(old_text, SafeText::Unreadable))
        || (new_required && matches!(new_text, SafeText::Unreadable));
    let placeholder = if binary {
        Some(format!("binary file omitted: {path}"))
    } else if unreadable {
        Some(format!("unreadable file omitted: {path}"))
    } else {
        None
    };
    let old_text = match old_text {
        SafeText::Text(text) => Some(text),
        SafeText::Binary | SafeText::Unreadable => None,
    };
    let new_text = match new_text {
        SafeText::Text(text) => Some(text),
        SafeText::Binary | SafeText::Unreadable => None,
    };
    WorkspaceDiffFile {
        path,
        status: if unreadable {
            WorkspaceDiffFileStatus::Unreadable
        } else if binary {
            WorkspaceDiffFileStatus::Binary
        } else {
            status
        },
        old_text,
        new_text,
        binary,
        unreadable,
        placeholder,
    }
}

fn git_blob_text(workdir: &Path, path: &str) -> SafeText {
    let spec = format!("HEAD:{path}");
    git_stdout_bytes(workdir, [OsStr::new("show"), OsStr::new(&spec)], false)
        .map(bytes_to_text)
        .unwrap_or(SafeText::Unreadable)
}

fn read_text_file(path: &Path) -> SafeText {
    fs::read(path)
        .map(bytes_to_text)
        .unwrap_or(SafeText::Unreadable)
}

fn bytes_to_text(bytes: Vec<u8>) -> SafeText {
    if bytes.contains(&0) {
        return SafeText::Binary;
    }
    String::from_utf8(bytes)
        .map(SafeText::Text)
        .unwrap_or(SafeText::Binary)
}

fn placeholder_diff(path: &str, entry: &WorkspaceDiffFile) -> String {
    let message = entry
        .placeholder
        .clone()
        .unwrap_or_else(|| format!("diff unavailable for {path}"));
    format!("diff --git a/{path} b/{path}\n[{message}]\n")
}

fn git_stdout<I, S>(workdir: &Path, args: I, allow_exit_one: bool) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let bytes = git_stdout_bytes(workdir, args, allow_exit_one)?;
    String::from_utf8(bytes)
        .map_err(|err| Error::Message(format!("git output was not UTF-8: {err}")))
}

fn git_stdout_bytes<I, S>(workdir: &Path, args: I, allow_exit_one: bool) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git")
        .args(args)
        .current_dir(workdir)
        .output()?;
    if output.status.success() || (allow_exit_one && output.status.code() == Some(1)) {
        return Ok(output.stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(Error::Message(format!(
        "git command failed with status {}: {}",
        output.status,
        stderr.trim()
    )))
}

fn nul_fields(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| String::from_utf8_lossy(field).into_owned())
        .collect()
}

#[derive(Debug)]
struct TrackedDiffFile {
    path: String,
    status: WorkspaceDiffFileStatus,
}

struct DiffAccumulator {
    text: String,
    max_bytes: usize,
    max_lines: usize,
    bytes: usize,
    lines: usize,
    omitted_bytes: usize,
    omitted_lines: usize,
    truncated: bool,
}

impl DiffAccumulator {
    fn new(max_bytes: usize, max_lines: usize) -> Self {
        Self {
            text: String::new(),
            max_bytes,
            max_lines,
            bytes: 0,
            lines: 0,
            omitted_bytes: 0,
            omitted_lines: 0,
            truncated: false,
        }
    }

    fn append(&mut self, chunk: &str) {
        for piece in chunk.split_inclusive('\n') {
            self.append_piece(piece);
        }
    }

    fn append_piece(&mut self, piece: &str) {
        if piece.is_empty() {
            return;
        }
        let piece_lines = 1;
        if self.truncated
            || self.bytes + piece.len() > self.max_bytes
            || self.lines + piece_lines > self.max_lines
        {
            self.truncated = true;
            self.omitted_bytes += piece.len();
            self.omitted_lines += piece_lines;
            return;
        }
        self.text.push_str(piece);
        self.bytes += piece.len();
        self.lines += piece_lines;
    }

    fn finish(mut self) -> (String, WorkspaceDiffTruncation) {
        if self.truncated {
            let notice = format!(
                "[diff truncated: omitted at least {} bytes across {} lines]\n",
                self.omitted_bytes, self.omitted_lines
            );
            self.make_room_for_notice(&notice);
            if !self.text.ends_with('\n') && self.bytes + 1 < self.max_bytes {
                self.text.push('\n');
                self.bytes += 1;
                self.lines += 1;
            }
            self.text.push_str(&notice);
        }
        (
            self.text,
            WorkspaceDiffTruncation {
                truncated: self.truncated,
                max_bytes: self.max_bytes,
                max_lines: self.max_lines,
                omitted_bytes: self.omitted_bytes,
                omitted_lines: self.omitted_lines,
            },
        )
    }

    fn make_room_for_notice(&mut self, notice: &str) {
        while self.bytes + notice.len() > self.max_bytes && !self.text.is_empty() {
            let ch = self.text.pop().expect("non-empty");
            self.bytes -= ch.len_utf8();
            if ch == '\n' {
                self.lines = self.lines.saturating_sub(1);
            }
        }
        while self.lines + 1 > self.max_lines && !self.text.is_empty() {
            let Some(pos) = self.text[..self.text.len().saturating_sub(1)].rfind('\n') else {
                self.bytes = 0;
                self.lines = 0;
                self.text.clear();
                break;
            };
            self.text.truncate(pos + 1);
            self.bytes = self.text.len();
            self.lines = self.text.lines().count();
        }
        self.bytes += notice.len();
        self.lines += 1;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SafeText {
    Text(String),
    Binary,
    Unreadable,
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn non_git_workspace_reports_status_without_diff() {
        let temp = tempdir().expect("temp");
        fs::write(temp.path().join("file.txt"), "hello\n").expect("write");

        let diff = collect_workspace_diff(temp.path()).expect("diff");

        assert!(!diff.is_git_repo);
        assert!(diff.is_empty());
    }

    #[test]
    fn empty_git_workspace_reports_no_changes() {
        let temp = git_repo();
        fs::write(temp.path().join("file.txt"), "hello\n").expect("write");
        git(temp.path(), ["add", "."]);
        git(temp.path(), ["commit", "-m", "initial"]);

        let diff = collect_workspace_diff(temp.path()).expect("diff");

        assert!(diff.is_git_repo);
        assert!(diff.is_empty());
    }

    #[test]
    fn tracked_unstaged_diff_is_collected() {
        let temp = git_repo();
        fs::write(temp.path().join("file.txt"), "hello\n").expect("write");
        git(temp.path(), ["add", "."]);
        git(temp.path(), ["commit", "-m", "initial"]);
        fs::write(temp.path().join("file.txt"), "hello\nworld\n").expect("write");

        let diff = collect_workspace_diff(temp.path()).expect("diff");

        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].status, WorkspaceDiffFileStatus::Modified);
        assert!(diff.unified_diff.contains("+world"));
        assert_eq!(diff.files[0].old_text.as_deref(), Some("hello\n"));
        assert_eq!(diff.files[0].new_text.as_deref(), Some("hello\nworld\n"));
    }

    #[test]
    fn untracked_diff_uses_no_index_exit_one_as_success() {
        let temp = git_repo();
        fs::write(temp.path().join("new.txt"), "new\n").expect("write");

        let diff = collect_workspace_diff(temp.path()).expect("diff");

        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].status, WorkspaceDiffFileStatus::Untracked);
        assert!(diff.unified_diff.contains("new.txt"));
        assert!(diff.unified_diff.contains("+new"));
    }

    #[test]
    fn binary_file_uses_placeholder_metadata() {
        let temp = git_repo();
        fs::write(temp.path().join("data.bin"), [0, 159, 146, 150]).expect("write");

        let diff = collect_workspace_diff(temp.path()).expect("diff");

        assert_eq!(diff.files.len(), 1);
        assert!(diff.files[0].new_text.is_none());
        assert!(!diff.unified_diff.contains('\0'));
    }

    #[test]
    fn truncates_by_line_limit() {
        let temp = git_repo();
        let content = (0..20)
            .map(|index| format!("line {index}\n"))
            .collect::<String>();
        fs::write(temp.path().join("big.txt"), content).expect("write");

        let diff = collect_workspace_diff_with_caps(temp.path(), WORKSPACE_DIFF_MAX_BYTES, 6)
            .expect("diff");

        assert!(diff.truncation.truncated);
        assert!(diff.unified_diff.contains("diff truncated"));
        assert!(diff.unified_diff.lines().count() <= 6);
    }

    #[test]
    fn truncates_by_byte_limit() {
        let temp = git_repo();
        fs::write(temp.path().join("big.txt"), "x".repeat(2048)).expect("write");

        let diff = collect_workspace_diff_with_caps(temp.path(), 256, WORKSPACE_DIFF_MAX_LINES)
            .expect("diff");

        assert!(diff.truncation.truncated);
        assert!(diff.unified_diff.len() <= 256);
        assert!(diff.unified_diff.contains("diff truncated"));
    }

    fn git_repo() -> tempfile::TempDir {
        let temp = tempdir().expect("temp");
        git(temp.path(), ["init"]);
        git(temp.path(), ["config", "user.email", "test@example.com"]);
        git(temp.path(), ["config", "user.name", "Test User"]);
        temp
    }

    fn git<I, S>(workdir: &Path, args: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = Command::new("git")
            .args(args)
            .current_dir(workdir)
            .output()
            .expect("git");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn unreadable_untracked_file_records_placeholder_when_possible() {
        let temp = git_repo();
        let path = temp.path().join("secret.txt");
        fs::write(&path, "secret\n").expect("write");
        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(&path).expect("metadata").permissions();
            permissions.set_mode(0o000);
            fs::set_permissions(&path, permissions).expect("chmod");
        }

        let diff = collect_workspace_diff(temp.path()).expect("diff");

        assert_eq!(diff.files.len(), 1);
        #[cfg(unix)]
        assert!(
            diff.files[0].new_text.is_none()
                || diff.files[0].new_text.as_deref() == Some("secret\n")
        );

        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(&path).expect("metadata").permissions();
            permissions.set_mode(0o644);
            fs::set_permissions(&path, permissions).expect("chmod restore");
        }
    }
}
