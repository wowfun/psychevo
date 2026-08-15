use std::io::{ErrorKind, Read, Seek};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::io::Write;

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub(crate) struct CapturedDirectoryIdentity {
    path: PathBuf,
    object: FilesystemObjectIdentity,
    #[cfg(unix)]
    handle: std::sync::Arc<std::fs::File>,
    #[cfg(windows)]
    _handle: std::sync::Arc<std::fs::File>,
}

impl PartialEq for CapturedDirectoryIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.object == other.object
    }
}

impl Eq for CapturedDirectoryIdentity {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRootCapture {
    identities: Vec<CapturedDirectoryIdentity>,
}

#[derive(Debug)]
pub(crate) struct CapturedFileTarget {
    target: PathBuf,
    #[cfg(unix)]
    parent: CapturedDirectoryIdentity,
    existing: Option<(FilesystemObjectIdentity, std::fs::File)>,
    #[cfg(unix)]
    writable: bool,
}

impl WorkspaceRootCapture {
    pub fn capture(roots: &[PathBuf]) -> Result<Self> {
        if roots.is_empty() {
            return Err(Error::Message(
                "a Turn must capture at least one Workspace root".to_string(),
            ));
        }
        let mut identities = Vec::with_capacity(roots.len());
        for root in roots {
            let identity = CapturedDirectoryIdentity::capture(root)?;
            if identities
                .iter()
                .all(|current: &CapturedDirectoryIdentity| current.path() != identity.path())
            {
                identities.push(identity);
            }
        }
        Ok(Self { identities })
    }

    pub async fn capture_async(roots: Vec<PathBuf>) -> Result<Self> {
        tokio::task::spawn_blocking(move || Self::capture(&roots))
            .await
            .map_err(|error| {
                Error::Message(format!("Workspace root identity worker failed: {error}"))
            })?
    }

    pub async fn validate_async(&self) -> Result<()> {
        let capture = self.clone();
        tokio::task::spawn_blocking(move || capture.validate())
            .await
            .map_err(|error| {
                Error::Message(format!("Workspace root identity worker failed: {error}"))
            })?
    }

    pub(crate) fn identities(&self) -> &[CapturedDirectoryIdentity] {
        &self.identities
    }

    pub(crate) fn from_identities(identities: Vec<CapturedDirectoryIdentity>) -> Self {
        Self { identities }
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.identities
            .iter()
            .map(|identity| identity.path().to_path_buf())
            .collect()
    }

    pub fn validate(&self) -> Result<()> {
        self.identities
            .iter()
            .try_for_each(CapturedDirectoryIdentity::validate)
    }

    pub(crate) fn open_directory(&self, target: &Path) -> Result<std::fs::File> {
        let identity = self.identity_for_target(target)?;
        identity.open_descendant_directory(target)
    }

    fn identity_for_target(&self, target: &Path) -> Result<&CapturedDirectoryIdentity> {
        self.identities
            .iter()
            .filter(|identity| is_within(identity.path(), target))
            .max_by_key(|identity| identity.path().components().count())
            .ok_or_else(|| {
                Error::Message(format!(
                    "filesystem target is outside the captured Workspace: {}",
                    target.display()
                ))
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FilesystemObjectIdentity {
    #[cfg(unix)]
    Unix { device: u64, inode: u64 },
    #[cfg(windows)]
    Windows { volume: u64, file_id: [u8; 16] },
}

impl CapturedDirectoryIdentity {
    pub(crate) fn capture(path: &Path) -> Result<Self> {
        let path = crate::host_paths::normalized_native_path(
            &std::fs::canonicalize(path).map_err(|error| {
                workspace_root_identity_error("Workspace root could not be resolved", error)
            })?,
        );
        #[cfg(any(unix, windows))]
        let handle = open_directory_identity_handle(&path).map_err(|error| {
            workspace_root_identity_error("Workspace root could not be opened", error)
        })?;
        #[cfg(any(unix, windows))]
        let metadata = handle.metadata().map_err(|error| {
            workspace_root_identity_error("Workspace root metadata is unavailable", error)
        })?;
        #[cfg(not(any(unix, windows)))]
        let metadata = std::fs::metadata(&path).map_err(|error| {
            workspace_root_identity_error("Workspace root metadata is unavailable", error)
        })?;
        if !metadata.is_dir() {
            return Err(Error::Message(format!(
                "path_identity_changed: captured Workspace root is not a directory: {}",
                path.display()
            )));
        }
        #[cfg(any(unix, windows))]
        let object = directory_object_identity(&handle).map_err(|error| {
            workspace_root_identity_error("Workspace root identity is unavailable", error)
        })?;
        #[cfg(not(any(unix, windows)))]
        let object = filesystem_object_identity(&metadata).map_err(|error| {
            workspace_root_identity_error("Workspace root identity is unavailable", error)
        })?;
        #[cfg(any(unix, windows))]
        {
            let current_path = crate::host_paths::normalized_native_path(
                &std::fs::canonicalize(&path).map_err(|error| {
                    workspace_root_identity_error(
                        "Workspace root changed while its identity was captured",
                        error,
                    )
                })?,
            );
            let current_handle =
                open_directory_identity_handle(&current_path).map_err(|error| {
                    workspace_root_identity_error(
                        "Workspace root changed while its identity was captured",
                        error,
                    )
                })?;
            let current = current_handle.metadata().map_err(|error| {
                workspace_root_identity_error(
                    "Workspace root changed while its identity was captured",
                    error,
                )
            })?;
            if current_path != path
                || !current.is_dir()
                || directory_object_identity(&current_handle).map_err(|error| {
                    workspace_root_identity_error(
                        "Workspace root changed while its identity was captured",
                        error,
                    )
                })? != object
            {
                return Err(Error::Message(
                    "path_identity_changed: Workspace root changed while its identity was captured"
                        .to_string(),
                ));
            }
            Ok(Self {
                path,
                object,
                #[cfg(unix)]
                handle: std::sync::Arc::new(handle),
                #[cfg(windows)]
                _handle: std::sync::Arc::new(handle),
            })
        }
        #[cfg(not(any(unix, windows)))]
        Ok(Self { path, object })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn validate(&self) -> Result<()> {
        let current_path = std::fs::canonicalize(&self.path).map_err(|error| {
            workspace_root_identity_error("captured Workspace root is unavailable", error)
        })?;
        let current_path = crate::host_paths::normalized_native_path(&current_path);
        #[cfg(any(unix, windows))]
        {
            let current_handle =
                open_directory_identity_handle(&current_path).map_err(|error| {
                    workspace_root_identity_error("captured Workspace root is unavailable", error)
                })?;
            let metadata = current_handle.metadata().map_err(|error| {
                workspace_root_identity_error("captured Workspace root is unavailable", error)
            })?;
            if current_path != self.path
                || !metadata.is_dir()
                || directory_object_identity(&current_handle).map_err(|error| {
                    workspace_root_identity_error(
                        "captured Workspace root identity is unavailable",
                        error,
                    )
                })? != self.object
            {
                return Err(Error::Message(
                    "path_identity_changed: captured Workspace root object changed after Turn admission"
                        .to_string(),
                ));
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let metadata = std::fs::metadata(&current_path).map_err(|error| {
                workspace_root_identity_error("captured Workspace root is unavailable", error)
            })?;
            if current_path != self.path
                || !metadata.is_dir()
                || filesystem_object_identity(&metadata).map_err(|error| {
                    workspace_root_identity_error(
                        "captured Workspace root identity is unavailable",
                        error,
                    )
                })? != self.object
            {
                return Err(Error::Message(
                    "path_identity_changed: captured Workspace root object changed after Turn admission"
                        .to_string(),
                ));
            }
        }
        Ok(())
    }

    #[cfg(unix)]
    pub(crate) fn open_verified(&self) -> std::io::Result<std::fs::File> {
        self.validate()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        self.handle.try_clone()
    }

    #[cfg(unix)]
    fn open_descendant(&self, target: &Path, writable: bool) -> Result<std::fs::File> {
        let flags = if writable {
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL
        } else {
            libc::O_RDONLY
        };
        self.open_descendant_with_flags(target, flags, 0o666)
    }

    #[cfg(unix)]
    fn open_descendant_directory(&self, target: &Path) -> Result<std::fs::File> {
        if target == self.path {
            return self.open_verified().map_err(Into::into);
        }
        self.open_descendant_with_flags(target, libc::O_RDONLY | libc::O_DIRECTORY, 0)
    }

    #[cfg(not(unix))]
    fn open_descendant_directory(&self, _target: &Path) -> Result<std::fs::File> {
        Err(Error::Message(
            "identity-bound terminal directories are unsupported on this platform".to_string(),
        ))
    }

    #[cfg(unix)]
    fn open_descendant_with_flags(
        &self,
        target: &Path,
        final_flags: libc::c_int,
        mode: libc::mode_t,
    ) -> Result<std::fs::File> {
        use std::ffi::CString;
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::ffi::OsStrExt as _;

        let relative = target.strip_prefix(&self.path).map_err(|_| {
            Error::Message(format!(
                "filesystem target is outside the captured Workspace root: {}",
                target.display()
            ))
        })?;
        if relative.as_os_str().is_empty() {
            return Err(Error::Message(
                "filesystem callback target must be below a Workspace root".to_string(),
            ));
        }
        let mut directory = self.open_verified()?;
        let mut components = relative.components().peekable();
        while let Some(component) = components.next() {
            let std::path::Component::Normal(name) = component else {
                return Err(Error::Message(
                    "filesystem callback target contains an invalid component".to_string(),
                ));
            };
            let name = CString::new(name.as_bytes()).map_err(|_| {
                Error::Message("filesystem callback target contains NUL".to_string())
            })?;
            let is_final = components.peek().is_none();
            let flags = if is_final {
                final_flags | libc::O_NOFOLLOW | libc::O_CLOEXEC
            } else {
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC
            };
            let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags, mode) };
            if fd < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let opened = unsafe { std::fs::File::from_raw_fd(fd) };
            if is_final {
                return Ok(opened);
            }
            directory = opened;
        }
        Err(Error::Message(
            "filesystem callback target is unavailable".to_string(),
        ))
    }
}

fn workspace_root_identity_error(context: &str, error: impl std::fmt::Display) -> Error {
    Error::Message(format!("path_identity_changed: {context}: {error}"))
}

impl CapturedFileTarget {
    #[cfg(unix)]
    pub(crate) fn capture(target: &Path, writable: bool) -> Result<Self> {
        use std::os::unix::fs::OpenOptionsExt as _;

        if !target.is_absolute() {
            return Err(Error::Message(
                "filesystem callback target must be absolute".to_string(),
            ));
        }
        let requested_parent = target.parent().ok_or_else(|| {
            Error::Message("filesystem callback target has no parent".to_string())
        })?;
        let mut existing_parent = requested_parent;
        while !existing_parent.exists() {
            existing_parent = existing_parent.parent().ok_or_else(|| {
                Error::Message("filesystem callback target has no existing ancestor".to_string())
            })?;
        }
        let parent = CapturedDirectoryIdentity::capture(existing_parent)?;
        let existing = match std::fs::symlink_metadata(target) {
            Ok(metadata) => {
                if !metadata.is_file() {
                    return Err(Error::Message(
                        "filesystem callback target must be a regular file".to_string(),
                    ));
                }
                let mut options = std::fs::OpenOptions::new();
                options
                    .read(true)
                    .write(writable)
                    .custom_flags(libc::O_NOFOLLOW);
                let file = options.open(target)?;
                let object = filesystem_object_identity(&file.metadata()?)?;
                Some((object, file))
            }
            Err(error) if error.kind() == ErrorKind::NotFound && writable => None,
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            target: target.to_path_buf(),
            parent,
            existing,
            writable,
        })
    }

    pub(crate) fn target(&self) -> &Path {
        &self.target
    }

    #[cfg(unix)]
    pub(crate) fn revalidate(&self) -> Result<()> {
        self.parent.validate()?;
        match &self.existing {
            Some((captured_object, _)) => {
                let metadata = std::fs::symlink_metadata(&self.target).map_err(|error| {
                    Error::Message(format!(
                        "path_identity_changed: approved filesystem target is unavailable: {error}"
                    ))
                })?;
                if !metadata.is_file() || filesystem_object_identity(&metadata)? != *captured_object
                {
                    return Err(Error::Message(
                        "path_identity_changed: approved filesystem target object changed"
                            .to_string(),
                    ));
                }
            }
            None if std::fs::symlink_metadata(&self.target).is_ok() => {
                return Err(Error::Message(
                    "path_identity_changed: approved filesystem target appeared before write"
                        .to_string(),
                ));
            }
            None => {}
        }
        Ok(())
    }

    #[cfg(not(unix))]
    pub(crate) fn revalidate(&self) -> Result<()> {
        Err(Error::Message(
            "identity-bound filesystem callbacks are unsupported on this platform".to_string(),
        ))
    }

    pub(crate) fn read_bytes(&mut self) -> Result<Vec<u8>> {
        self.revalidate()?;
        let Some((_, file)) = self.existing.as_mut() else {
            return Err(Error::Message(format!(
                "{} no longer exists; no changes were applied",
                self.target.display()
            )));
        };
        file.seek(std::io::SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        self.revalidate()?;
        Ok(bytes)
    }

    pub(crate) fn metadata(&self) -> Result<std::fs::Metadata> {
        self.revalidate()?;
        self.existing
            .as_ref()
            .map(|(_, file)| file.metadata())
            .transpose()?
            .ok_or_else(|| {
                Error::Message(format!(
                    "{} no longer exists; no changes were applied",
                    self.target.display()
                ))
            })
    }

    #[cfg(unix)]
    pub(crate) fn replace_bytes(&mut self, content: &[u8]) -> Result<()> {
        let Some((captured_object, existing_file)) = self.existing.as_ref() else {
            return Err(Error::Message(format!(
                "{} no longer exists; no changes were applied",
                self.target.display()
            )));
        };
        let captured_object = captured_object.clone();
        let captured_permissions = existing_file.metadata()?.permissions();
        self.revalidate()?;
        let (parent, target_name) = self.parent.open_descendant_parent(&self.target, false)?;
        let mut temporary = TemporaryFileAt::create(&parent)?;
        temporary.file_mut().set_permissions(captured_permissions)?;
        temporary.file_mut().write_all(content)?;
        temporary.file_mut().flush()?;
        self.revalidate()?;
        temporary.exchange_with(&target_name)?;
        let displaced = open_file_at(&parent, temporary.name(), libc::O_RDONLY)?;
        if filesystem_object_identity(&displaced.metadata()?)? != captured_object {
            return Err(Error::Message(
                "path_identity_changed: approved filesystem target object changed before atomic replacement"
                    .to_string(),
            ));
        }
        let replacement = temporary.commit_exchange()?;
        let replacement_object = filesystem_object_identity(&replacement.metadata()?)?;
        self.existing = Some((replacement_object, replacement));
        self.revalidate()
    }

    #[cfg(not(unix))]
    pub(crate) fn replace_bytes(&mut self, _content: &[u8]) -> Result<()> {
        Err(Error::Message(
            "identity-bound atomic filesystem replacement is unsupported on this platform"
                .to_string(),
        ))
    }

    #[cfg(unix)]
    pub(crate) fn create_bytes(&mut self, content: &[u8]) -> Result<()> {
        self.revalidate()?;
        if self.existing.is_some() {
            return Err(Error::Message(format!(
                "{} already exists; no changes were applied",
                self.target.display()
            )));
        }
        let (parent, target_name) = self.parent.open_descendant_parent(&self.target, true)?;
        let mut temporary = TemporaryFileAt::create(&parent)?;
        temporary.file_mut().write_all(content)?;
        temporary.file_mut().flush()?;
        link_at(&parent, temporary.name(), &target_name)?;
        let file = temporary.commit_link()?;
        let object = filesystem_object_identity(&file.metadata()?)?;
        self.existing = Some((object, file));
        self.revalidate()
    }

    #[cfg(not(unix))]
    pub(crate) fn create_bytes(&mut self, _content: &[u8]) -> Result<()> {
        Err(Error::Message(
            "identity-bound filesystem mutations are unsupported on this platform".to_string(),
        ))
    }

    #[cfg(unix)]
    pub(crate) fn delete(&mut self) -> Result<()> {
        self.revalidate()?;
        let Some((captured_object, _)) = self.existing.as_ref() else {
            return Err(Error::Message(format!(
                "{} no longer exists; no changes were applied",
                self.target.display()
            )));
        };
        let captured_object = captured_object.clone();
        let (parent, target_name) = self.parent.open_descendant_parent(&self.target, false)?;
        let mut temporary = TemporaryFileAt::create(&parent)?;
        self.revalidate()?;
        temporary.exchange_with(&target_name)?;
        let displaced = open_file_at(&parent, temporary.name(), libc::O_RDONLY)?;
        if filesystem_object_identity(&displaced.metadata()?)? != captured_object {
            return Err(Error::Message(
                "path_identity_changed: approved filesystem target object changed before deletion"
                    .to_string(),
            ));
        }
        temporary.commit_delete()?;
        self.existing = None;
        Ok(())
    }

    #[cfg(not(unix))]
    pub(crate) fn delete(&mut self) -> Result<()> {
        Err(Error::Message(
            "identity-bound filesystem mutations are unsupported on this platform".to_string(),
        ))
    }

    #[cfg(not(unix))]
    pub(crate) fn capture(_target: &Path, _writable: bool) -> Result<Self> {
        Err(Error::Message(
            "identity-bound filesystem callbacks are unsupported on this platform".to_string(),
        ))
    }

    #[cfg(unix)]
    pub(crate) fn open_after_authorization(self) -> Result<std::fs::File> {
        self.parent.validate()?;
        match self.existing {
            Some((captured_object, file)) => {
                let metadata = std::fs::metadata(&self.target).map_err(|error| {
                    Error::Message(format!(
                        "path_identity_changed: approved filesystem target is unavailable: {error}"
                    ))
                })?;
                if filesystem_object_identity(&metadata)? != captured_object {
                    return Err(Error::Message(
                        "path_identity_changed: approved filesystem target object changed"
                            .to_string(),
                    ));
                }
                if self.writable {
                    file.set_len(0)?;
                }
                Ok(file)
            }
            None => {
                if std::fs::symlink_metadata(&self.target).is_ok() {
                    return Err(Error::Message(
                        "path_identity_changed: approved filesystem target appeared before write"
                            .to_string(),
                    ));
                }
                self.parent.open_descendant(&self.target, true)
            }
        }
    }

    #[cfg(not(unix))]
    pub(crate) fn open_after_authorization(self) -> Result<std::fs::File> {
        Err(Error::Message(
            "identity-bound filesystem callbacks are unsupported on this platform".to_string(),
        ))
    }
}

pub const IDENTITY_BOUND_FILE_MUTATIONS_SUPPORTED: bool = cfg!(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos"
));

#[cfg(unix)]
impl CapturedDirectoryIdentity {
    fn open_descendant_parent(
        &self,
        target: &Path,
        create_directories: bool,
    ) -> Result<(std::fs::File, std::ffi::CString)> {
        use std::ffi::CString;
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::ffi::OsStrExt as _;

        let relative = target.strip_prefix(&self.path).map_err(|_| {
            Error::Message(format!(
                "filesystem target is outside the captured directory: {}",
                target.display()
            ))
        })?;
        let mut directory = self.open_verified()?;
        let mut components = relative.components().peekable();
        while let Some(component) = components.next() {
            let std::path::Component::Normal(name) = component else {
                return Err(Error::Message(
                    "filesystem mutation target contains an invalid component".to_string(),
                ));
            };
            let name = CString::new(name.as_bytes()).map_err(|_| {
                Error::Message("filesystem mutation target contains NUL".to_string())
            })?;
            if components.peek().is_none() {
                return Ok((directory, name));
            }
            let mut fd = unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    0,
                )
            };
            if fd < 0
                && create_directories
                && std::io::Error::last_os_error().kind() == ErrorKind::NotFound
            {
                let mkdir = unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o777) };
                if mkdir != 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
                fd = unsafe {
                    libc::openat(
                        directory.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                        0,
                    )
                };
            }
            if fd < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            directory = unsafe { std::fs::File::from_raw_fd(fd) };
        }
        Err(Error::Message(
            "filesystem mutation target is unavailable".to_string(),
        ))
    }
}

#[cfg(unix)]
#[derive(Debug)]
enum TemporaryFileState {
    Temporary,
    Exchanged { target_name: std::ffi::CString },
    Disarmed,
}

#[cfg(unix)]
#[derive(Debug)]
struct TemporaryFileAt<'a> {
    parent: &'a std::fs::File,
    name: std::ffi::CString,
    file: Option<std::fs::File>,
    state: TemporaryFileState,
}

#[cfg(unix)]
impl<'a> TemporaryFileAt<'a> {
    fn create(parent: &'a std::fs::File) -> Result<Self> {
        use std::os::fd::{AsRawFd, FromRawFd};

        for _ in 0..16 {
            let name = std::ffi::CString::new(format!(".psychevo-write-{}", uuid::Uuid::now_v7()))
                .expect("UUID contains no NUL");
            let fd = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDWR
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o666,
                )
            };
            if fd >= 0 {
                return Ok(Self {
                    parent,
                    name,
                    file: Some(unsafe { std::fs::File::from_raw_fd(fd) }),
                    state: TemporaryFileState::Temporary,
                });
            }
            if std::io::Error::last_os_error().kind() != ErrorKind::AlreadyExists {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        Err(Error::Message(
            "could not reserve an identity-bound temporary file".to_string(),
        ))
    }

    fn name(&self) -> &std::ffi::CStr {
        &self.name
    }

    fn file_mut(&mut self) -> &mut std::fs::File {
        self.file.as_mut().expect("temporary file is armed")
    }

    fn exchange_with(&mut self, target_name: &std::ffi::CStr) -> Result<()> {
        exchange_directory_entries(self.parent, &self.name, target_name)?;
        self.state = TemporaryFileState::Exchanged {
            target_name: target_name.to_owned(),
        };
        Ok(())
    }

    fn commit_exchange(mut self) -> Result<std::fs::File> {
        unlink_at(self.parent, &self.name)?;
        self.state = TemporaryFileState::Disarmed;
        Ok(self.file.take().expect("temporary file is armed"))
    }

    fn commit_link(mut self) -> Result<std::fs::File> {
        unlink_at(self.parent, &self.name)?;
        self.state = TemporaryFileState::Disarmed;
        Ok(self.file.take().expect("temporary file is armed"))
    }

    fn commit_delete(mut self) -> Result<()> {
        let TemporaryFileState::Exchanged { target_name } = &self.state else {
            return Err(Error::Message(
                "identity-bound deletion was not exchanged".to_string(),
            ));
        };
        unlink_at(self.parent, target_name)?;
        unlink_at(self.parent, &self.name)?;
        self.state = TemporaryFileState::Disarmed;
        Ok(())
    }
}

#[cfg(unix)]
impl Drop for TemporaryFileAt<'_> {
    fn drop(&mut self) {
        match &self.state {
            TemporaryFileState::Temporary => {
                let _ = unlink_at(self.parent, &self.name);
            }
            TemporaryFileState::Exchanged { target_name } => {
                let _ = exchange_directory_entries(self.parent, &self.name, target_name);
                let _ = unlink_at(self.parent, &self.name);
            }
            TemporaryFileState::Disarmed => {}
        }
    }
}

#[cfg(unix)]
fn open_file_at(
    parent: &std::fs::File,
    name: &std::ffi::CStr,
    flags: libc::c_int,
) -> Result<std::fs::File> {
    use std::os::fd::{AsRawFd, FromRawFd};

    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn exchange_directory_entries(
    parent: &std::fs::File,
    left: &std::ffi::CStr,
    right: &std::ffi::CStr,
) -> Result<()> {
    use std::os::fd::AsRawFd;

    let result = unsafe {
        libc::renameat2(
            parent.as_raw_fd(),
            left.as_ptr(),
            parent.as_raw_fd(),
            right.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn exchange_directory_entries(
    parent: &std::fs::File,
    left: &std::ffi::CStr,
    right: &std::ffi::CStr,
) -> Result<()> {
    use std::os::fd::AsRawFd;

    let result = unsafe {
        libc::renameatx_np(
            parent.as_raw_fd(),
            left.as_ptr(),
            parent.as_raw_fd(),
            right.as_ptr(),
            libc::RENAME_SWAP,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(all(
    unix,
    not(any(target_os = "linux", target_os = "android", target_os = "macos"))
))]
fn exchange_directory_entries(
    _parent: &std::fs::File,
    _left: &std::ffi::CStr,
    _right: &std::ffi::CStr,
) -> Result<()> {
    Err(Error::Message(
        "identity-bound atomic replacement is unsupported on this Unix platform".to_string(),
    ))
}

#[cfg(unix)]
fn link_at(parent: &std::fs::File, source: &std::ffi::CStr, target: &std::ffi::CStr) -> Result<()> {
    use std::os::fd::AsRawFd;

    let result = unsafe {
        libc::linkat(
            parent.as_raw_fd(),
            source.as_ptr(),
            parent.as_raw_fd(),
            target.as_ptr(),
            0,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(unix)]
fn unlink_at(parent: &std::fs::File, name: &std::ffi::CStr) -> Result<()> {
    use std::os::fd::AsRawFd;

    let result = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) };
    if result != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(unix)]
fn open_directory_identity_handle(path: &Path) -> Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt as _;

    Ok(std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?)
}

#[cfg(windows)]
fn open_directory_identity_handle(path: &Path) -> Result<std::fs::File> {
    use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };

    let mut options = std::fs::OpenOptions::new();
    options
        .access_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    let handle = options.open(path)?;
    if handle.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(Error::Message(format!(
            "captured Workspace root is a reparse point: {}",
            path.display()
        )));
    }
    Ok(handle)
}

#[cfg(unix)]
fn directory_object_identity(file: &std::fs::File) -> Result<FilesystemObjectIdentity> {
    filesystem_object_identity(&file.metadata()?)
}

#[cfg(unix)]
fn filesystem_object_identity(metadata: &std::fs::Metadata) -> Result<FilesystemObjectIdentity> {
    use std::os::unix::fs::MetadataExt;

    if metadata.ino() == 0 {
        return Err(Error::Message(
            "filesystem object has no stable inode identity".to_string(),
        ));
    }
    Ok(FilesystemObjectIdentity::Unix {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(windows)]
fn directory_object_identity(file: &std::fs::File) -> Result<FilesystemObjectIdentity> {
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };

    let mut info = FILE_ID_INFO::default();
    let succeeded = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle() as _,
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if succeeded == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if info.FileId.Identifier == [0; 16] {
        return Err(Error::Message(
            "filesystem object has no stable Windows file identity".to_string(),
        ));
    }
    Ok(FilesystemObjectIdentity::Windows {
        volume: info.VolumeSerialNumber,
        file_id: info.FileId.Identifier,
    })
}

#[cfg(not(any(unix, windows)))]
fn filesystem_object_identity(_metadata: &std::fs::Metadata) -> Result<FilesystemObjectIdentity> {
    Err(Error::Message(
        "stable filesystem object identity is unsupported on this platform".to_string(),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FilesystemIdentity {
    pub(crate) requested_absolute: PathBuf,
    pub(crate) resolved: PathBuf,
    pub(crate) uri: String,
}

pub(crate) fn resolve(raw: &str, cwd: &Path) -> Result<FilesystemIdentity> {
    let requested_absolute = crate::host_paths::resolve_input_path(raw, cwd)?;
    let resolved = canonicalize_deepest_existing(&requested_absolute)?;
    let uri = crate::host_paths::path_ref_for_native_path(&resolved).uri;
    Ok(FilesystemIdentity {
        requested_absolute,
        resolved,
        uri,
    })
}

pub(crate) fn canonicalize_deepest_existing(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(Error::Message("empty filesystem path".to_string()));
    }

    let mut current = crate::host_paths::normalized_native_path(path);
    let mut tail = PathBuf::new();
    loop {
        match std::fs::symlink_metadata(&current) {
            Ok(_) => {
                let mut resolved =
                    crate::host_paths::normalized_native_path(&current.canonicalize()?);
                if !tail.as_os_str().is_empty() {
                    resolved.push(tail);
                }
                return Ok(resolved);
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }

        let Some(name) = current.file_name().map(|name| name.to_os_string()) else {
            return Err(Error::Message(format!(
                "no existing ancestor for filesystem path {}",
                path.display()
            )));
        };
        let mut next_tail = PathBuf::from(name);
        if !tail.as_os_str().is_empty() {
            next_tail.push(tail);
        }
        tail = next_tail;
        if !current.pop() {
            return Err(Error::Message(format!(
                "no existing ancestor for filesystem path {}",
                path.display()
            )));
        }
    }
}

pub(crate) fn is_within(root: &Path, target: &Path) -> bool {
    target == root || target.starts_with(root)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn resolves_missing_descendant_through_directory_symlink() {
        let work = tempfile::tempdir().expect("work");
        let outside = tempfile::tempdir().expect("outside");
        std::os::unix::fs::symlink(outside.path(), work.path().join("linked")).expect("symlink");

        let identity = resolve("linked/new/file.txt", work.path()).expect("identity");

        assert_eq!(
            identity.requested_absolute,
            work.path().join("linked/new/file.txt")
        );
        assert_eq!(identity.resolved, outside.path().join("new/file.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_dangling_symlink_identity() {
        let work = tempfile::tempdir().expect("work");
        std::os::unix::fs::symlink(work.path().join("missing"), work.path().join("dangling"))
            .expect("symlink");

        let error = resolve("dangling/file.txt", work.path()).expect_err("dangling identity");

        assert!(error.to_string().contains("No such file"));
    }

    #[test]
    fn captured_directory_rejects_a_new_object_at_the_same_path() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("root");
        let original = temp.path().join("original");
        std::fs::create_dir(&root).expect("root");
        let captured = CapturedDirectoryIdentity::capture(&root).expect("capture");
        std::fs::rename(&root, &original).expect("retain original object");
        std::fs::create_dir(&root).expect("replacement");

        let error = captured
            .validate()
            .expect_err("replacement must fail closed");

        assert!(error.to_string().contains("path_identity_changed"));
    }

    #[test]
    fn captured_directory_rejects_a_replacement_when_the_numeric_identity_is_recycled() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("root");
        let original = temp.path().join("original");
        std::fs::create_dir(&root).expect("root");
        let mut captured = CapturedDirectoryIdentity::capture(&root).expect("capture");
        std::fs::rename(&root, &original).expect("retain original object");
        std::fs::write(&root, "replacement file").expect("replacement");
        captured.object =
            filesystem_object_identity(&std::fs::metadata(&root).expect("replacement metadata"))
                .expect("replacement identity");

        let error = captured
            .validate()
            .expect_err("an opened directory capture must reject a recycled numeric identity");

        assert!(error.to_string().contains("path_identity_changed"));
    }

    #[test]
    fn captured_file_target_rejects_replacement_after_authorization() {
        let temp = tempfile::tempdir().expect("temp");
        let target = temp.path().join("target.txt");
        let original = temp.path().join("original.txt");
        std::fs::write(&target, "original").expect("target");
        let captured = CapturedFileTarget::capture(&target, true).expect("capture target");
        std::fs::rename(&target, &original).expect("retain original");
        std::fs::write(&target, "replacement").expect("replacement");

        let error = captured
            .open_after_authorization()
            .expect_err("replacement must fail closed");

        assert!(error.to_string().contains("path_identity_changed"));
        assert_eq!(
            std::fs::read_to_string(&target).expect("replacement text"),
            "replacement"
        );
        assert_eq!(
            std::fs::read_to_string(&original).expect("original text"),
            "original"
        );
    }

    #[test]
    fn absent_target_creation_does_not_truncate_an_object_that_appeared() {
        let temp = tempfile::tempdir().expect("temp");
        let root = CapturedDirectoryIdentity::capture(temp.path()).expect("capture root");
        let target = temp.path().join("target.txt");
        std::fs::write(&target, "created concurrently").expect("concurrent target");

        let error = root
            .open_descendant(&target, true)
            .expect_err("exclusive creation must reject an existing object");

        assert!(error.to_string().contains("exists"));
        assert_eq!(
            std::fs::read_to_string(&target).expect("target text"),
            "created concurrently"
        );
    }

    #[test]
    fn captured_workspace_root_can_be_opened_as_a_terminal_directory() {
        let temp = tempfile::tempdir().expect("temp");
        let capture =
            WorkspaceRootCapture::capture(&[temp.path().to_path_buf()]).expect("capture root");

        let opened = capture
            .open_directory(temp.path())
            .expect("root directory itself is authorized");

        assert!(opened.metadata().expect("metadata").is_dir());
    }

    #[test]
    fn identity_bound_temporary_file_is_unlinked_on_drop() {
        let temp = tempfile::tempdir().expect("temp");
        let parent = std::fs::File::open(temp.path()).expect("parent");
        let name = {
            let mut temporary = TemporaryFileAt::create(&parent).expect("temporary");
            temporary
                .file_mut()
                .write_all(b"secret replacement")
                .expect("write");
            temporary.name().to_owned()
        };

        assert!(!temp.path().join(name.to_string_lossy().as_ref()).exists());
    }
}
