use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use uuid::Uuid;

#[derive(Debug)]
pub struct InstanceLease {
    _file: File,
}

impl InstanceLease {
    pub fn acquire(path: &Path) -> io::Result<Self> {
        let file = open_lock_file(path)?;
        file.lock()?;
        Ok(Self { _file: file })
    }

    pub fn try_acquire(path: &Path) -> io::Result<Option<Self>> {
        let file = open_lock_file(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(error)) => Err(error),
        }
    }
}

fn open_lock_file(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
}

pub fn instance_lease_is_held(path: &Path) -> io::Result<bool> {
    Ok(InstanceLease::try_acquire(path)?.is_none())
}

pub fn atomic_replace(path: &Path, contents: &[u8]) -> io::Result<()> {
    atomic_replace_impl(path, contents, None)
}

pub fn atomic_replace_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    atomic_replace_impl(path, contents, Some(0o600))
}

fn atomic_replace_impl(path: &Path, contents: &[u8], unix_mode: Option<u32>) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "atomic replace path has no parent",
        )
    })?;
    std::fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state");
    let temp = parent.join(format!(".{file_name}.{}.tmp", Uuid::now_v7()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        let mut file = options.open(&temp)?;
        #[cfg(unix)]
        if let Some(mode) = unix_mode {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(mode))?;
        }
        #[cfg(not(unix))]
        let _ = unix_mode;
        file.write_all(contents)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        replace_file(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    std::fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProcessIdentityError {
    #[error("managed process is not running")]
    Dead,
    #[error("managed process identity mismatch: {0}")]
    Mismatch(String),
    #[error("managed process identity is unavailable: {0}")]
    Unavailable(#[source] io::Error),
}

#[cfg(unix)]
#[derive(Debug)]
pub struct ManagedProcess {
    pid: u32,
}

#[cfg(unix)]
impl ManagedProcess {
    pub fn inspect(pid: u32, _instance_id: &str) -> Result<Self, ProcessIdentityError> {
        if pid == 0 {
            return Err(ProcessIdentityError::Mismatch("pid is zero".to_string()));
        }
        check_unix_process_alive(pid)?;
        let process_group = unsafe { libc::getpgid(pid as libc::pid_t) };
        if process_group < 0 {
            return Err(map_unix_inspect_error(io::Error::last_os_error()));
        }
        if process_group != pid as libc::pid_t {
            return Err(ProcessIdentityError::Mismatch(format!(
                "pid {pid} belongs to process group {process_group}"
            )));
        }
        Ok(Self { pid })
    }

    pub fn is_alive(&self) -> io::Result<bool> {
        unix_process_alive(self.pid)
    }

    pub fn request_graceful_termination(&self) -> io::Result<()> {
        signal_unix_process(self.pid, libc::SIGTERM)
    }

    pub fn terminate_tree(&self, _exit_code: u32) -> io::Result<()> {
        let result = unsafe { libc::kill(-(self.pid as libc::pid_t), libc::SIGKILL) };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(error)
        }
    }

    pub fn wait_for_exit(&self, timeout: Duration) -> io::Result<bool> {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if !self.is_alive()? {
                return Ok(true);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        Ok(!self.is_alive()?)
    }

    pub fn wait_for_tree_exit(&self, timeout: Duration) -> io::Result<bool> {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if !unix_process_group_alive(self.pid)? {
                return Ok(true);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        Ok(!unix_process_group_alive(self.pid)?)
    }
}

#[cfg(unix)]
fn check_unix_process_alive(pid: u32) -> Result<(), ProcessIdentityError> {
    match unix_process_alive(pid) {
        Ok(true) => Ok(()),
        Ok(false) => Err(ProcessIdentityError::Dead),
        Err(error) => Err(ProcessIdentityError::Unavailable(error)),
    }
}

#[cfg(unix)]
fn unix_process_alive(pid: u32) -> io::Result<bool> {
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if result == 0 {
        return Ok(!unix_process_is_zombie(pid));
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Err(error),
        _ => Err(error),
    }
}

#[cfg(target_os = "linux")]
fn unix_process_is_zombie(pid: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    stat.rsplit_once(')')
        .and_then(|(_, fields)| fields.split_whitespace().next())
        == Some("Z")
}

#[cfg(all(unix, not(target_os = "linux")))]
fn unix_process_is_zombie(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
fn map_unix_inspect_error(error: io::Error) -> ProcessIdentityError {
    if error.raw_os_error() == Some(libc::ESRCH) {
        ProcessIdentityError::Dead
    } else {
        ProcessIdentityError::Unavailable(error)
    }
}

#[cfg(unix)]
fn signal_unix_process(pid: u32, signal: i32) -> io::Result<()> {
    let result = unsafe { libc::kill(pid as libc::pid_t, signal) };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

#[cfg(target_os = "linux")]
fn unix_process_group_alive(process_group: u32) -> io::Result<bool> {
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        let Some((_, fields)) = stat.rsplit_once(')') else {
            continue;
        };
        let mut fields = fields.split_whitespace();
        let state = fields.next();
        let _parent_pid = fields.next();
        let group = fields.next().and_then(|value| value.parse::<u32>().ok());
        if group == Some(process_group) && state != Some("Z") {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(all(unix, not(target_os = "linux")))]
fn unix_process_group_alive(process_group: u32) -> io::Result<bool> {
    let result = unsafe { libc::kill(-(process_group as libc::pid_t), 0) };
    if result == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(false)
    } else {
        Err(error)
    }
}

#[cfg(windows)]
#[derive(Debug)]
pub struct ManagedProcess {
    process: OwnedHandle,
    job: OwnedHandle,
}

#[cfg(windows)]
impl ManagedProcess {
    pub fn inspect(pid: u32, instance_id: &str) -> Result<Self, ProcessIdentityError> {
        use windows_sys::Win32::Foundation::{
            ERROR_INVALID_PARAMETER, WAIT_OBJECT_0, WAIT_TIMEOUT,
        };
        use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
        use windows_sys::Win32::System::JobObjects::{IsProcessInJob, OpenJobObjectW};
        use windows_sys::Win32::System::SystemServices::{JOB_OBJECT_QUERY, JOB_OBJECT_TERMINATE};
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
            WaitForSingleObject,
        };

        let process = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE,
                0,
                pid,
            )
        };
        let process = match OwnedHandle::new(process) {
            Ok(process) => process,
            Err(error) if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) => {
                return Err(ProcessIdentityError::Dead);
            }
            Err(error) => return Err(ProcessIdentityError::Unavailable(error)),
        };
        match unsafe { WaitForSingleObject(process.0, 0) } {
            WAIT_OBJECT_0 => return Err(ProcessIdentityError::Dead),
            WAIT_TIMEOUT => {}
            _ => return Err(ProcessIdentityError::Unavailable(io::Error::last_os_error())),
        }
        let name = wide_job_name(instance_id);
        let job = unsafe {
            OpenJobObjectW(
                JOB_OBJECT_QUERY | JOB_OBJECT_TERMINATE | SYNCHRONIZE,
                0,
                name.as_ptr(),
            )
        };
        let job = OwnedHandle::new(job).map_err(ProcessIdentityError::Unavailable)?;
        let mut in_job = 0;
        if unsafe { IsProcessInJob(process.0, job.0, &mut in_job) } == 0 {
            return Err(ProcessIdentityError::Unavailable(io::Error::last_os_error()));
        }
        if in_job == 0 {
            return Err(ProcessIdentityError::Mismatch(format!(
                "pid {pid} is not a member of the managed Job Object"
            )));
        }
        Ok(Self { process, job })
    }

    pub fn is_alive(&self) -> io::Result<bool> {
        use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows_sys::Win32::System::Threading::WaitForSingleObject;
        match unsafe { WaitForSingleObject(self.process.0, 0) } {
            WAIT_OBJECT_0 => Ok(false),
            WAIT_TIMEOUT => Ok(true),
            _ => Err(io::Error::last_os_error()),
        }
    }

    pub fn request_graceful_termination(&self) -> io::Result<()> {
        Ok(())
    }

    pub fn terminate_tree(&self, exit_code: u32) -> io::Result<()> {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        if unsafe { TerminateJobObject(self.job.0, exit_code) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub fn wait_for_exit(&self, timeout: Duration) -> io::Result<bool> {
        use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows_sys::Win32::System::Threading::WaitForSingleObject;
        let millis = timeout.as_millis().min(u32::MAX as u128) as u32;
        match unsafe { WaitForSingleObject(self.process.0, millis) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }

    pub fn wait_for_tree_exit(&self, timeout: Duration) -> io::Result<bool> {
        use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows_sys::Win32::System::Threading::WaitForSingleObject;
        let millis = timeout.as_millis().min(u32::MAX as u128) as u32;
        match unsafe { WaitForSingleObject(self.job.0, millis) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

#[cfg(windows)]
#[derive(Debug)]
struct OwnedHandle(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl OwnedHandle {
    fn new(handle: windows_sys::Win32::Foundation::HANDLE) -> io::Result<Self> {
        if handle.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }
}

#[cfg(windows)]
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[derive(Debug)]
pub struct ManagedProcessTreeGuard {
    #[cfg(windows)]
    _job: OwnedHandle,
}

pub fn enter_managed_process_tree(instance_id: &str) -> io::Result<ManagedProcessTreeGuard> {
    enter_managed_process_tree_impl(instance_id)
}

#[cfg(unix)]
fn enter_managed_process_tree_impl(_instance_id: &str) -> io::Result<ManagedProcessTreeGuard> {
    Ok(ManagedProcessTreeGuard {})
}

#[cfg(windows)]
fn enter_managed_process_tree_impl(instance_id: &str) -> io::Result<ManagedProcessTreeGuard> {
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let name = wide_job_name(instance_id);
    let job = unsafe { CreateJobObjectW(std::ptr::null(), name.as_ptr()) };
    let create_error = unsafe { GetLastError() };
    let job = OwnedHandle::new(job)?;
    if create_error == ERROR_ALREADY_EXISTS {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "managed Job Object already exists",
        ));
    }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            std::ptr::addr_of!(limits).cast(),
            std::mem::size_of_val(&limits) as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if unsafe { AssignProcessToJobObject(job.0, GetCurrentProcess()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(ManagedProcessTreeGuard { _job: job })
}

#[cfg(not(any(unix, windows)))]
compile_error!("managed host process lifecycle is unsupported on this platform");

#[cfg(windows)]
fn wide_job_name(instance_id: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(&format!("Local\\PsychevoGateway-{instance_id}"))
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
