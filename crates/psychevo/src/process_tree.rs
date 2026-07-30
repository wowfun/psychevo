use std::io;
use std::process::Command;

/// Owns the platform resources needed to terminate one spawned process tree.
///
/// Callers configure the command before spawn, attach immediately after spawn,
/// and retain the guard until all child output has been drained.
pub(crate) struct ProcessTreeGuard {
    #[cfg(unix)]
    process_group: libc::pid_t,
    #[cfg(windows)]
    job: std::os::windows::io::OwnedHandle,
    terminated: bool,
}

impl ProcessTreeGuard {
    pub(crate) fn configure_std(command: &mut Command) {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(not(unix))]
        {
            let _ = command;
        }
    }

    pub(crate) fn configure_tokio(command: &mut tokio::process::Command) {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.as_std_mut().process_group(0);
        }
        #[cfg(not(unix))]
        {
            let _ = command;
        }
    }

    pub(crate) fn attach_std(child: &std::process::Child) -> io::Result<Self> {
        #[cfg(unix)]
        {
            Ok(Self {
                process_group: child.id() as libc::pid_t,
                terminated: false,
            })
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            create_job(child.as_raw_handle())
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = child;
            Ok(Self { terminated: false })
        }
    }

    pub(crate) fn attach_tokio(child: &tokio::process::Child) -> io::Result<Self> {
        #[cfg(unix)]
        {
            let pid = child.id().ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "child exited before tree attach")
            })?;
            Ok(Self {
                process_group: pid as libc::pid_t,
                terminated: false,
            })
        }
        #[cfg(windows)]
        {
            let handle = child.raw_handle().ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "child exited before tree attach")
            })?;
            create_job(handle)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = child;
            Ok(Self { terminated: false })
        }
    }

    pub(crate) fn terminate(&mut self) {
        if self.terminated {
            return;
        }
        self.terminated = true;
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

impl Drop for ProcessTreeGuard {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(windows)]
fn create_job(
    child_handle: std::os::windows::io::RawHandle,
) -> io::Result<ProcessTreeGuard> {
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
    if unsafe { AssignProcessToJobObject(job.as_raw_handle() as _, child_handle as _) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(ProcessTreeGuard {
        job,
        terminated: false,
    })
}
