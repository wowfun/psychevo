use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use flate2::read::GzDecoder;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy)]
pub(crate) struct PluginMaterializationLimits {
    pub(crate) subprocess_deadline: Duration,
    pub(crate) max_archive_bytes: u64,
    pub(crate) max_tree_bytes: u64,
    pub(crate) max_entries: usize,
    pub(crate) max_file_bytes: u64,
    pub(crate) max_relative_path_bytes: usize,
    pub(crate) max_components: usize,
}

impl Default for PluginMaterializationLimits {
    fn default() -> Self {
        Self {
            subprocess_deadline: Duration::from_secs(120),
            max_archive_bytes: 50 * 1024 * 1024,
            max_tree_bytes: 200 * 1024 * 1024,
            max_entries: 10_000,
            max_file_bytes: 50 * 1024 * 1024,
            max_relative_path_bytes: 1024,
            max_components: 64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoundedEntryKind {
    Directory,
    File,
}

#[derive(Debug, Clone)]
pub(crate) struct BoundedTreeEntry {
    pub(crate) relative: PathBuf,
    pub(crate) kind: BoundedEntryKind,
    pub(crate) len: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct BoundedTree {
    pub(crate) entries: Vec<BoundedTreeEntry>,
}

pub(crate) fn bounded_tree(root: &Path) -> Result<BoundedTree> {
    bounded_tree_with_limits(root, PluginMaterializationLimits::default())
}

pub(crate) fn bounded_tree_with_limits(
    root: &Path,
    limits: PluginMaterializationLimits,
) -> Result<BoundedTree> {
    let root_metadata = fs::symlink_metadata(root)?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(Error::Config(format!(
            "plugin package root must be a directory without symlinks: {}",
            root.display()
        )));
    }
    let mut pending = vec![root.to_path_buf()];
    let mut entries = Vec::new();
    let mut total_bytes = 0_u64;
    while let Some(directory) = pending.pop() {
        let mut children = fs::read_dir(&directory)?.collect::<io::Result<Vec<_>>>()?;
        children.sort_by_key(|entry| entry.file_name());
        for entry in children {
            let path = entry.path();
            let relative = path.strip_prefix(root).map_err(|error| {
                Error::Message(format!("failed to relativize plugin path: {error}"))
            })?;
            validate_relative_path(relative, limits)?;
            if entries.len() >= limits.max_entries {
                return Err(limit_error("entry count", limits.max_entries));
            }
            let metadata = fs::symlink_metadata(&path)?;
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                return Err(Error::Config(format!(
                    "plugin package contains unsupported symlink: {}",
                    path.display()
                )));
            }
            if file_type.is_dir() {
                entries.push(BoundedTreeEntry {
                    relative: relative.to_path_buf(),
                    kind: BoundedEntryKind::Directory,
                    len: 0,
                });
                pending.push(path);
                continue;
            }
            if !file_type.is_file() {
                return Err(Error::Config(format!(
                    "plugin package contains unsupported filesystem entry: {}",
                    path.display()
                )));
            }
            let len = metadata.len();
            if len > limits.max_file_bytes {
                return Err(limit_error("single file bytes", limits.max_file_bytes));
            }
            total_bytes = total_bytes
                .checked_add(len)
                .ok_or_else(|| limit_error("unpacked bytes", limits.max_tree_bytes))?;
            if total_bytes > limits.max_tree_bytes {
                return Err(limit_error("unpacked bytes", limits.max_tree_bytes));
            }
            entries.push(BoundedTreeEntry {
                relative: relative.to_path_buf(),
                kind: BoundedEntryKind::File,
                len,
            });
        }
    }
    entries.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(BoundedTree { entries })
}

pub(crate) fn copy_tree_bounded(source: &Path, destination: &Path) -> Result<()> {
    let limits = PluginMaterializationLimits::default();
    let tree = bounded_tree_with_limits(source, limits)?;
    fs::create_dir_all(destination)?;
    let mut copied_total = 0_u64;
    for entry in tree.entries {
        let source_path = source.join(&entry.relative);
        let destination_path = destination.join(&entry.relative);
        match entry.kind {
            BoundedEntryKind::Directory => fs::create_dir_all(&destination_path)?,
            BoundedEntryKind::File => {
                let metadata = fs::symlink_metadata(&source_path)?;
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err(Error::Config(format!(
                        "plugin package changed to an unsupported entry during copy: {}",
                        source_path.display()
                    )));
                }
                if let Some(parent) = destination_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut input = File::open(&source_path)?;
                let mut output = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&destination_path)?;
                let copied = io::copy(
                    &mut (&mut input).take(limits.max_file_bytes + 1),
                    &mut output,
                )?;
                if copied > limits.max_file_bytes {
                    return Err(limit_error("single file bytes", limits.max_file_bytes));
                }
                if copied != entry.len {
                    return Err(Error::Config(format!(
                        "plugin package file changed while copying: {}",
                        source_path.display()
                    )));
                }
                copied_total = copied_total
                    .checked_add(copied)
                    .ok_or_else(|| limit_error("installed bytes", limits.max_tree_bytes))?;
                if copied_total > limits.max_tree_bytes {
                    return Err(limit_error("installed bytes", limits.max_tree_bytes));
                }
                output.flush()?;
                drop(output);
                fs::set_permissions(&destination_path, metadata.permissions())?;
            }
        }
    }
    Ok(())
}

pub(crate) fn extract_tar_gz_bounded(archive_path: &Path, destination: &Path) -> Result<()> {
    extract_tar_gz_with_limits(
        archive_path,
        destination,
        PluginMaterializationLimits::default(),
    )
}

fn extract_tar_gz_with_limits(
    archive_path: &Path,
    destination: &Path,
    limits: PluginMaterializationLimits,
) -> Result<()> {
    let archive_size = fs::metadata(archive_path)?.len();
    if archive_size > limits.max_archive_bytes {
        return Err(limit_error("archive bytes", limits.max_archive_bytes));
    }
    fs::create_dir_all(destination)?;
    let decoder = GzDecoder::new(File::open(archive_path)?);
    let mut archive = tar::Archive::new(decoder);
    let mut unpacked_bytes = 0_u64;
    for (entries_seen, entry) in archive
        .entries()
        .map_err(archive_error)?
        .enumerate()
    {
        let mut entry = entry.map_err(archive_error)?;
        if entries_seen >= limits.max_entries {
            return Err(limit_error("archive entry count", limits.max_entries));
        }
        let entry_type = entry.header().entry_type();
        if !entry_type.is_file() && !entry_type.is_dir() {
            return Err(Error::Config(format!(
                "plugin archive contains unsupported entry type {:?}",
                entry_type
            )));
        }
        let relative = entry.path().map_err(archive_error)?.into_owned();
        validate_relative_path(&relative, limits)?;
        let destination_path = destination.join(&relative);
        if entry_type.is_dir() {
            fs::create_dir_all(&destination_path)?;
            continue;
        }
        let declared_size = entry.header().size().map_err(archive_error)?;
        #[cfg(unix)]
        let archive_mode = entry.header().mode().map_err(archive_error)? & 0o777;
        if declared_size > limits.max_file_bytes {
            return Err(limit_error("single archive file bytes", limits.max_file_bytes));
        }
        unpacked_bytes = unpacked_bytes
            .checked_add(declared_size)
            .ok_or_else(|| limit_error("archive unpacked bytes", limits.max_tree_bytes))?;
        if unpacked_bytes > limits.max_tree_bytes {
            return Err(limit_error("archive unpacked bytes", limits.max_tree_bytes));
        }
        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&destination_path)?;
        let copied = io::copy(
            &mut entry.by_ref().take(limits.max_file_bytes + 1),
            &mut output,
        )?;
        if copied != declared_size {
            return Err(Error::Config(format!(
                "plugin archive entry size mismatch for {}",
                relative.display()
            )));
        }
        output.flush()?;
        drop(output);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&destination_path, fs::Permissions::from_mode(archive_mode))?;
        }
    }
    bounded_tree_with_limits(destination, limits)?;
    Ok(())
}

pub(crate) fn run_materialization_command(
    command: &mut Command,
    operation: &str,
    source_display: &str,
    capture_stdout: bool,
) -> Result<Output> {
    run_materialization_command_with_limits(
        command,
        operation,
        source_display,
        capture_stdout,
        PluginMaterializationLimits::default(),
    )
}

fn run_materialization_command_with_limits(
    command: &mut Command,
    operation: &str,
    source_display: &str,
    capture_stdout: bool,
    limits: PluginMaterializationLimits,
) -> Result<Output> {
    const MAX_CAPTURE_BYTES: usize = 1024 * 1024;
    let started = Instant::now();
    command
        .stdout(if capture_stdout {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stderr(Stdio::null());
    configure_materialization_process_group(command);
    let mut child = command.spawn().map_err(|error| {
        Error::Config(format!(
            "{operation} failed to start for {}: {error}",
            redact_source(source_display)
        ))
    })?;
    let mut process_tree = MaterializationProcessTree::attach(&child).map_err(|error| {
        let _ = child.kill();
        let _ = child.wait();
        Error::Config(format!(
            "{operation} failed to contain process tree for {}: {error}",
            redact_source(source_display)
        ))
    })?;
    let output_reader = child.stdout.take().map(|mut stdout| {
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let mut captured = Vec::new();
            let mut truncated = false;
            let mut buffer = [0_u8; 8192];
            let result: io::Result<(Vec<u8>, bool)> = loop {
                let read = stdout.read(&mut buffer)?;
                if read == 0 {
                    break Ok((captured, truncated));
                }
                let remaining = MAX_CAPTURE_BYTES.saturating_sub(captured.len());
                let retained = remaining.min(read);
                captured.extend_from_slice(&buffer[..retained]);
                truncated |= retained < read;
            };
            let _ = sender.send(result);
            Ok::<(), io::Error>(())
        });
        receiver
    });
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= limits.subprocess_deadline {
            process_tree.terminate();
            let _ = child.kill();
            let _ = child.wait();
            return Err(materialization_deadline_error(
                operation,
                source_display,
                limits,
            ));
        }
        thread::sleep(Duration::from_millis(20));
    };
    let (stdout, truncated) = match output_reader {
        Some(receiver) => {
            let remaining = limits
                .subprocess_deadline
                .saturating_sub(started.elapsed());
            match receiver.recv_timeout(remaining) {
                Ok(result) => result?,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    process_tree.terminate();
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(materialization_deadline_error(
                        operation,
                        source_display,
                        limits,
                    ));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(Error::Message(format!(
                        "{operation} output reader stopped unexpectedly"
                    )));
                }
            }
        }
        None => (Vec::new(), false),
    };
    process_tree.terminate();
    if truncated {
        return Err(Error::Config(format!(
            "{operation} output exceeded {MAX_CAPTURE_BYTES} bytes for {}",
            redact_source(source_display)
        )));
    }
    if !status.success() {
        return Err(Error::Config(format!(
            "{operation} failed for {}",
            redact_source(source_display)
        )));
    }
    Ok(Output {
        status,
        stdout,
        stderr: Vec::new(),
    })
}

fn materialization_deadline_error(
    operation: &str,
    source_display: &str,
    limits: PluginMaterializationLimits,
) -> Error {
    Error::Config(format!(
        "{operation} exceeded {} seconds for {}",
        limits.subprocess_deadline.as_secs_f64(),
        redact_source(source_display)
    ))
}

#[cfg(unix)]
fn configure_materialization_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_materialization_process_group(_command: &mut Command) {}

struct MaterializationProcessTree {
    #[cfg(unix)]
    process_group: libc::pid_t,
    #[cfg(windows)]
    job: std::os::windows::io::OwnedHandle,
}

impl MaterializationProcessTree {
    fn attach(child: &std::process::Child) -> io::Result<Self> {
        #[cfg(unix)]
        {
            Ok(Self {
                process_group: child.id() as libc::pid_t,
            })
        }
        #[cfg(windows)]
        {
            create_materialization_job(child)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = child;
            Ok(Self {})
        }
    }

    fn terminate(&mut self) {
        #[cfg(unix)]
        unsafe {
            let _ = libc::killpg(self.process_group, libc::SIGKILL);
        }
        #[cfg(windows)]
        unsafe {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::System::JobObjects::TerminateJobObject;
            let _ = TerminateJobObject(self.job.as_raw_handle() as _, 1);
        }
    }
}

#[cfg(windows)]
fn create_materialization_job(
    child: &std::process::Child,
) -> io::Result<MaterializationProcessTree> {
    use std::ffi::c_void;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    let raw_job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if raw_job.is_null() {
        return Err(io::Error::last_os_error());
    }
    let job = unsafe { OwnedHandle::from_raw_handle(raw_job as _) };
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            job.as_raw_handle() as _,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast::<c_void>(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if unsafe {
        AssignProcessToJobObject(
            job.as_raw_handle() as _,
            child.as_raw_handle() as _,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(MaterializationProcessTree { job })
}

pub(crate) fn redact_source(source: &str) -> String {
    let end = source
        .char_indices()
        .filter(|(_, ch)| matches!(ch, '?' | '#'))
        .map(|(index, _)| index)
        .min()
        .unwrap_or(source.len());
    let without_query = &source[..end];
    let Some(scheme_end) = without_query.find("://") else {
        return without_query.to_string();
    };
    let authority_start = scheme_end + 3;
    let authority_end = without_query[authority_start..]
        .find('/')
        .map(|index| authority_start + index)
        .unwrap_or(without_query.len());
    let authority = &without_query[authority_start..authority_end];
    let Some(userinfo_end) = authority.rfind('@') else {
        return without_query.to_string();
    };
    format!(
        "{}{}{}",
        &without_query[..authority_start],
        &authority[userinfo_end + 1..],
        &without_query[authority_end..]
    )
}

fn validate_relative_path(
    relative: &Path,
    limits: PluginMaterializationLimits,
) -> Result<()> {
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err(Error::Config(format!(
            "plugin package contains invalid relative path: {}",
            relative.display()
        )));
    }
    let mut components = 0_usize;
    for component in relative.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(Error::Config(format!(
                "plugin package path escapes its root: {}",
                relative.display()
            )));
        }
        components += 1;
    }
    if components > limits.max_components {
        return Err(limit_error(
            "relative path component depth",
            limits.max_components,
        ));
    }
    if relative.as_os_str().as_encoded_bytes().len() > limits.max_relative_path_bytes {
        return Err(limit_error(
            "relative path bytes",
            limits.max_relative_path_bytes,
        ));
    }
    Ok(())
}

fn archive_error(error: impl std::fmt::Display) -> Error {
    Error::Config(format!("invalid plugin archive: {error}"))
}

fn limit_error(label: &str, limit: impl std::fmt::Display) -> Error {
    Error::Config(format!("plugin materialization exceeds {label} limit {limit}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};
    use tempfile::tempdir;

    fn tiny_limits() -> PluginMaterializationLimits {
        PluginMaterializationLimits {
            subprocess_deadline: Duration::from_millis(30),
            max_archive_bytes: 4096,
            max_tree_bytes: 12,
            max_entries: 3,
            max_file_bytes: 8,
            max_relative_path_bytes: 24,
            max_components: 3,
        }
    }

    fn write_archive(
        path: &Path,
        entries: impl FnOnce(&mut tar::Builder<GzEncoder<File>>),
    ) {
        let encoder = GzEncoder::new(File::create(path).expect("archive"), Compression::fast());
        let mut builder = tar::Builder::new(encoder);
        entries(&mut builder);
        let encoder = builder.into_inner().expect("tar finish");
        encoder.finish().expect("gzip finish");
    }

    #[test]
    fn bounded_walker_rejects_file_count_size_depth_and_path_limits() {
        let temp = tempdir().expect("temp");
        let root = temp.path().join("tree");
        fs::create_dir_all(root.join("a/b/c")).expect("dirs");
        fs::write(root.join("large"), b"123456789").expect("large");
        let error = bounded_tree_with_limits(&root, tiny_limits()).expect_err("large rejected");
        assert!(error.to_string().contains("single file bytes"));

        fs::remove_file(root.join("large")).expect("remove");
        fs::write(root.join("a/b/c/file"), b"x").expect("deep");
        let error = bounded_tree_with_limits(&root, tiny_limits()).expect_err("depth rejected");
        assert!(error.to_string().contains("component depth"));

        assert!(validate_relative_path(Path::new("../escape"), tiny_limits()).is_err());
        assert!(
            validate_relative_path(Path::new("component-name-that-is-too-long"), tiny_limits())
                .is_err()
        );

        let count_root = temp.path().join("count");
        fs::create_dir_all(&count_root).expect("count root");
        for index in 0..4 {
            fs::write(count_root.join(format!("{index}")), b"").expect("count file");
        }
        let error =
            bounded_tree_with_limits(&count_root, tiny_limits()).expect_err("count rejected");
        assert!(error.to_string().contains("entry count"));

        let total_root = temp.path().join("total");
        fs::create_dir_all(&total_root).expect("total root");
        fs::write(total_root.join("one"), b"1234567").expect("one");
        fs::write(total_root.join("two"), b"1234567").expect("two");
        let error =
            bounded_tree_with_limits(&total_root, tiny_limits()).expect_err("total rejected");
        assert!(error.to_string().contains("unpacked bytes"));
    }

    #[test]
    fn streaming_extractor_rejects_archive_bomb_and_links_before_writing() {
        let temp = tempdir().expect("temp");
        let bomb = temp.path().join("bomb.tgz");
        write_archive(&bomb, |builder| {
            let mut header = tar::Header::new_gnu();
            header.set_size(9);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, "package/bomb", &b"123456789"[..])
                .expect("bomb entry");
        });
        let error = extract_tar_gz_with_limits(&bomb, &temp.path().join("bomb-out"), tiny_limits())
            .expect_err("bomb rejected");
        assert!(error.to_string().contains("single archive file bytes"));

        let linked = temp.path().join("linked.tgz");
        write_archive(&linked, |builder| {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            header.set_path("package/link").expect("path");
            header.set_link_name("../outside").expect("link");
            header.set_cksum();
            builder.append(&header, io::empty()).expect("link entry");
        });
        let output = temp.path().join("linked-out");
        let error = extract_tar_gz_with_limits(&linked, &output, tiny_limits())
            .expect_err("link rejected");
        assert!(error.to_string().contains("unsupported entry type"));
        assert!(!output.join("package/link").exists());
    }

    #[cfg(unix)]
    #[test]
    fn streaming_extractor_preserves_executable_file_mode() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().expect("temp");
        let archive = temp.path().join("executable.tgz");
        write_archive(&archive, |builder| {
            let mut header = tar::Header::new_gnu();
            header.set_size(2);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, "package/run", &b"ok"[..])
                .expect("executable entry");
        });
        let output = temp.path().join("executable-out");
        extract_tar_gz_with_limits(&archive, &output, tiny_limits()).expect("extract");
        assert_eq!(
            fs::metadata(output.join("package/run"))
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }

    #[test]
    fn source_redaction_removes_url_credentials_query_and_fragment() {
        assert_eq!(
            redact_source("https://user:secret@example.test/repo.git?token=secret#main"),
            "https://example.test/repo.git"
        );
        assert_eq!(redact_source("git@example.test:repo.git"), "git@example.test:repo.git");
    }

    #[cfg(unix)]
    #[test]
    fn subprocess_deadline_kills_a_stubborn_materializer() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 5"]);
        let error = run_materialization_command_with_limits(
            &mut command,
            "test command",
            "https://user:secret@example.test/repo?token=secret",
            false,
            tiny_limits(),
        )
        .expect_err("deadline");
        let message = error.to_string();
        assert!(message.contains("exceeded"), "{message}");
        assert!(!message.contains("secret"), "{message}");
    }

    #[cfg(unix)]
    #[test]
    fn subprocess_deadline_covers_descendants_that_inherit_stdout() {
        let mut command = Command::new("sh");
        command.args(["-c", "(sleep 1) & printf ready"]);
        let started = Instant::now();
        let error = run_materialization_command_with_limits(
            &mut command,
            "test command",
            "local fixture",
            true,
            tiny_limits(),
        )
        .expect_err("inherited stdout must not outlive the materialization deadline");

        assert!(error.to_string().contains("exceeded"), "{error}");
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "reader waited {:?} for a descendant-owned pipe",
            started.elapsed()
        );
    }
}
