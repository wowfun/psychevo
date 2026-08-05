use std::collections::BTreeMap;
use std::ffi::{OsStr, c_void};
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::ptr;

use windows_sys::Win32::Foundation::{
    HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAce, AllocateAndInitializeSid, CopySid,
    CreateRestrictedToken, CreateWellKnownSid, FreeSid, GetLengthSid, GetTokenInformation,
    InitializeAcl, PSID, SECURITY_ATTRIBUTES, SECURITY_NT_AUTHORITY, SID_AND_ATTRIBUTES,
    TOKEN_ADJUST_DEFAULT, TOKEN_ASSIGN_PRIMARY, TOKEN_DEFAULT_DACL, TOKEN_DUPLICATE, TOKEN_GROUPS,
    TOKEN_QUERY, TOKEN_USER, TokenDefaultDacl, TokenGroups, TokenUser, WinWorldSid,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW,
    DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess,
    GetExitCodeProcess, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
    OpenProcessToken, PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW,
    UpdateProcThreadAttribute, WaitForSingleObject,
};

use super::ExecInvocation;

const DISABLE_MAX_PRIVILEGE: u32 = 0x01;
const LUA_TOKEN: u32 = 0x04;
const WRITE_RESTRICTED: u32 = 0x08;
const GENERIC_ALL: u32 = 0x1000_0000;
const SE_GROUP_LOGON_ID: u32 = 0xC000_0000;

pub(super) struct WindowsRestrictedSpawn {
    pub(super) process: WindowsRestrictedProcess,
    pub(super) stdin: Option<File>,
    pub(super) stdout: File,
    pub(super) stderr: File,
}

pub(crate) struct WindowsRestrictedProcess {
    process: OwnedHandle,
    job: OwnedHandle,
}

impl WindowsRestrictedProcess {
    pub(crate) fn try_wait(&self) -> io::Result<Option<i32>> {
        let status = unsafe { WaitForSingleObject(raw_handle(&self.process), 0) };
        match status {
            WAIT_TIMEOUT => Ok(None),
            WAIT_OBJECT_0 => {
                let mut code = 0_u32;
                if unsafe { GetExitCodeProcess(raw_handle(&self.process), &mut code) } == 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(Some(code as i32))
            }
            _ => Err(io::Error::last_os_error()),
        }
    }

    pub(crate) fn kill(&self) {
        unsafe {
            let _ = TerminateJobObject(raw_handle(&self.job), 1);
        }
    }
}

pub(super) fn spawn_read_only(
    invocation: &ExecInvocation,
    stdin_allowed: bool,
) -> io::Result<WindowsRestrictedSpawn> {
    let args = super::shell_args(&invocation.shell, invocation.login, &invocation.cmd)
        .map_err(io::Error::other)?;
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push(invocation.shell.clone());
    argv.extend(args);
    let application = wide_null(&invocation.shell);
    let mut command_line = wide_null(argv_to_command_line(&argv));
    let cwd = wide_null(invocation.cwd.as_os_str());
    let mut desktop = wide_null("winsta0\\default");

    let mut env = crate::process_env::effective_process_env(
        &invocation.env,
        crate::process_env::ProcessEnvOptions::new(&invocation.path_prefixes),
    )
    .map_err(io::Error::other)?;
    for (key, value) in invocation.sandbox_policy.env_markers() {
        env.insert(key.to_string(), value);
    }
    let environment = unicode_environment_block(&env);

    let token =
        restricted_read_only_token().map_err(|error| with_io_context("create token", error))?;
    let job = kill_on_close_job().map_err(|error| with_io_context("create job", error))?;
    let stdin_pipe =
        inherited_pipe(false).map_err(|error| with_io_context("create stdin pipe", error))?;
    let stdout_pipe =
        inherited_pipe(true).map_err(|error| with_io_context("create stdout pipe", error))?;
    let stderr_pipe =
        inherited_pipe(true).map_err(|error| with_io_context("create stderr pipe", error))?;
    clear_inherit(&stdin_pipe.parent)
        .map_err(|error| with_io_context("protect parent stdin handle", error))?;
    clear_inherit(&stdout_pipe.parent)
        .map_err(|error| with_io_context("protect parent stdout handle", error))?;
    clear_inherit(&stderr_pipe.parent)
        .map_err(|error| with_io_context("protect parent stderr handle", error))?;

    let inherited_handles = vec![
        raw_handle(&stdin_pipe.child),
        raw_handle(&stdout_pipe.child),
        raw_handle(&stderr_pipe.child),
    ];
    let mut attributes = ProcThreadAttributeList::new(1)
        .and_then(|mut attributes| {
            attributes.set_handle_list(inherited_handles)?;
            Ok(attributes)
        })
        .map_err(|error| with_io_context("restrict inherited handles", error))?;
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.lpDesktop = desktop.as_mut_ptr();
    startup.StartupInfo.hStdInput = raw_handle(&stdin_pipe.child);
    startup.StartupInfo.hStdOutput = raw_handle(&stdout_pipe.child);
    startup.StartupInfo.hStdError = raw_handle(&stderr_pipe.child);
    startup.lpAttributeList = attributes.as_mut_ptr();
    let mut process_info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let created = unsafe {
        CreateProcessAsUserW(
            raw_handle(&token),
            application.as_ptr(),
            command_line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            1,
            CREATE_UNICODE_ENVIRONMENT
                | CREATE_NO_WINDOW
                | CREATE_SUSPENDED
                | EXTENDED_STARTUPINFO_PRESENT,
            environment.as_ptr().cast::<c_void>(),
            cwd.as_ptr(),
            &startup.StartupInfo,
            &mut process_info,
        )
    };
    if created == 0 {
        return Err(with_io_context(
            &format!(
                "spawn restricted process application={} cwd={}",
                invocation.shell,
                invocation.cwd.display()
            ),
            io::Error::last_os_error(),
        ));
    }
    let process = unsafe { OwnedHandle::from_raw_handle(process_info.hProcess as _) };
    let thread = unsafe { OwnedHandle::from_raw_handle(process_info.hThread as _) };
    if unsafe { AssignProcessToJobObject(raw_handle(&job), raw_handle(&process)) } == 0 {
        unsafe {
            let _ =
                windows_sys::Win32::System::Threading::TerminateProcess(raw_handle(&process), 1);
        }
        return Err(io::Error::last_os_error());
    }
    if unsafe { ResumeThread(raw_handle(&thread)) } == u32::MAX {
        unsafe {
            let _ = TerminateJobObject(raw_handle(&job), 1);
        }
        return Err(io::Error::last_os_error());
    }
    drop(thread);
    drop(stdin_pipe.child);
    drop(stdout_pipe.child);
    drop(stderr_pipe.child);

    let stdin = if stdin_allowed {
        Some(File::from(stdin_pipe.parent))
    } else {
        drop(stdin_pipe.parent);
        None
    };
    Ok(WindowsRestrictedSpawn {
        process: WindowsRestrictedProcess { process, job },
        stdin,
        stdout: File::from(stdout_pipe.parent),
        stderr: File::from(stderr_pipe.parent),
    })
}

const PROC_THREAD_ATTRIBUTE_HANDLE_LIST: usize = 0x0002_0002;

struct ProcThreadAttributeList {
    buffer: Vec<u8>,
    handles: Vec<HANDLE>,
}

impl ProcThreadAttributeList {
    fn new(attribute_count: u32) -> io::Result<Self> {
        let mut size = 0_usize;
        unsafe {
            InitializeProcThreadAttributeList(ptr::null_mut(), attribute_count, 0, &mut size);
        }
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0_u8; size];
        if unsafe {
            InitializeProcThreadAttributeList(
                buffer.as_mut_ptr().cast(),
                attribute_count,
                0,
                &mut size,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            buffer,
            handles: Vec::new(),
        })
    }

    fn as_mut_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.buffer.as_mut_ptr().cast()
    }

    fn set_handle_list(&mut self, handles: Vec<HANDLE>) -> io::Result<()> {
        self.handles = handles;
        let value = self.handles.as_mut_ptr().cast();
        let size = std::mem::size_of_val(self.handles.as_slice());
        let list = self.as_mut_ptr();
        if unsafe {
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                value,
                size,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for ProcThreadAttributeList {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.as_mut_ptr());
        }
    }
}

fn with_io_context(operation: &str, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{operation}: {error}"))
}

struct Pipe {
    parent: OwnedHandle,
    child: OwnedHandle,
}

fn inherited_pipe(parent_reads: bool) -> io::Result<Pipe> {
    let mut read: HANDLE = ptr::null_mut();
    let mut write: HANDLE = ptr::null_mut();
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: 1,
    };
    if unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let read = unsafe { OwnedHandle::from_raw_handle(read as _) };
    let write = unsafe { OwnedHandle::from_raw_handle(write as _) };
    Ok(if parent_reads {
        Pipe {
            parent: read,
            child: write,
        }
    } else {
        Pipe {
            parent: write,
            child: read,
        }
    })
}

fn clear_inherit(handle: &OwnedHandle) -> io::Result<()> {
    if unsafe { SetHandleInformation(raw_handle(handle), HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn restricted_read_only_token() -> io::Result<OwnedHandle> {
    let mut base: HANDLE = ptr::null_mut();
    if unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ASSIGN_PRIMARY | TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ADJUST_DEFAULT,
            &mut base,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let base = unsafe { OwnedHandle::from_raw_handle(base as _) };
    let identity_sid = RestrictingSid::new()?;
    let user_sid = token_user_sid(raw_handle(&base))?;
    let logon_sid = token_logon_sid(raw_handle(&base))?;
    let world_sid = world_sid()?;
    let mut restrictions = [
        SID_AND_ATTRIBUTES {
            Sid: identity_sid.0,
            Attributes: 0,
        },
        SID_AND_ATTRIBUTES {
            Sid: sid_ptr(&user_sid),
            Attributes: 0,
        },
        SID_AND_ATTRIBUTES {
            Sid: sid_ptr(&logon_sid),
            Attributes: 0,
        },
        SID_AND_ATTRIBUTES {
            Sid: sid_ptr(&world_sid),
            Attributes: 0,
        },
    ];
    let mut restricted: HANDLE = ptr::null_mut();
    if unsafe {
        CreateRestrictedToken(
            raw_handle(&base),
            DISABLE_MAX_PRIVILEGE | LUA_TOKEN | WRITE_RESTRICTED,
            0,
            ptr::null(),
            0,
            ptr::null(),
            restrictions.len() as u32,
            restrictions.as_mut_ptr(),
            &mut restricted,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let restricted = unsafe { OwnedHandle::from_raw_handle(restricted as _) };
    set_compatibility_default_dacl(
        raw_handle(&restricted),
        &[sid_ptr(&user_sid), sid_ptr(&logon_sid), sid_ptr(&world_sid)],
    )?;
    Ok(restricted)
}

fn token_user_sid(token: HANDLE) -> io::Result<Vec<u8>> {
    let mut required = 0_u32;
    unsafe {
        let _ = GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut required);
    }
    if required == 0 {
        return Err(with_io_context(
            "size token user SID buffer",
            io::Error::last_os_error(),
        ));
    }
    let mut storage = aligned_buffer(required as usize);
    let user = storage.as_mut_ptr().cast::<TOKEN_USER>();
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            user.cast::<c_void>(),
            required,
            &mut required,
        )
    } == 0
    {
        return Err(with_io_context(
            "read token user",
            io::Error::last_os_error(),
        ));
    }
    copy_sid(unsafe { (*user).User.Sid })
        .map_err(|error| with_io_context("copy token user SID", error))
}

fn token_logon_sid(token: HANDLE) -> io::Result<Vec<u8>> {
    let mut required = 0_u32;
    unsafe {
        let _ = GetTokenInformation(token, TokenGroups, ptr::null_mut(), 0, &mut required);
    }
    if required == 0 {
        return Err(with_io_context(
            "size token logon SID buffer",
            io::Error::last_os_error(),
        ));
    }
    let mut storage = aligned_buffer(required as usize);
    let groups = storage.as_mut_ptr().cast::<TOKEN_GROUPS>();
    if unsafe {
        GetTokenInformation(
            token,
            TokenGroups,
            groups.cast::<c_void>(),
            required,
            &mut required,
        )
    } == 0
    {
        return Err(with_io_context(
            "read token groups",
            io::Error::last_os_error(),
        ));
    }
    let group_count = unsafe { (*groups).GroupCount as usize };
    let group_entries =
        unsafe { std::slice::from_raw_parts((*groups).Groups.as_ptr(), group_count) };
    let logon = group_entries
        .iter()
        .find(|group| group.Attributes & SE_GROUP_LOGON_ID == SE_GROUP_LOGON_ID)
        .ok_or_else(|| io::Error::other("current token has no logon SID"))?;
    copy_sid(logon.Sid).map_err(|error| with_io_context("copy token logon SID", error))
}

fn world_sid() -> io::Result<Vec<u8>> {
    let mut required = 0_u32;
    unsafe {
        let _ = CreateWellKnownSid(WinWorldSid, ptr::null_mut(), ptr::null_mut(), &mut required);
    }
    if required == 0 {
        return Err(with_io_context(
            "size World SID buffer",
            io::Error::last_os_error(),
        ));
    }
    let mut sid = vec![0_u8; required as usize];
    if unsafe {
        CreateWellKnownSid(
            WinWorldSid,
            ptr::null_mut(),
            sid.as_mut_ptr().cast::<c_void>(),
            &mut required,
        )
    } == 0
    {
        return Err(with_io_context(
            "create World SID",
            io::Error::last_os_error(),
        ));
    }
    Ok(sid)
}

fn copy_sid(source: PSID) -> io::Result<Vec<u8>> {
    let length = unsafe { GetLengthSid(source) };
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut sid = vec![0_u8; length as usize];
    if unsafe { CopySid(length, sid.as_mut_ptr().cast::<c_void>(), source) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(sid)
}

fn set_compatibility_default_dacl(token: HANDLE, sids: &[PSID]) -> io::Result<()> {
    let acl_bytes = std::mem::size_of::<ACL>()
        + sids
            .iter()
            .map(|sid| {
                std::mem::size_of::<ACCESS_ALLOWED_ACE>() - std::mem::size_of::<u32>()
                    + unsafe { GetLengthSid(*sid) as usize }
            })
            .sum::<usize>();
    let mut storage = aligned_buffer(acl_bytes);
    let acl = storage.as_mut_ptr().cast::<ACL>();
    if unsafe { InitializeAcl(acl, acl_bytes as u32, ACL_REVISION) } == 0 {
        return Err(with_io_context(
            "initialize token default DACL",
            io::Error::last_os_error(),
        ));
    }
    for sid in sids {
        if unsafe { AddAccessAllowedAce(acl, ACL_REVISION, GENERIC_ALL, *sid) } == 0 {
            return Err(with_io_context(
                "add token default DACL compatibility SID",
                io::Error::last_os_error(),
            ));
        }
    }
    let info = TOKEN_DEFAULT_DACL { DefaultDacl: acl };
    if unsafe {
        windows_sys::Win32::Security::SetTokenInformation(
            token,
            TokenDefaultDacl,
            (&info as *const TOKEN_DEFAULT_DACL).cast::<c_void>(),
            std::mem::size_of::<TOKEN_DEFAULT_DACL>() as u32,
        )
    } == 0
    {
        return Err(with_io_context(
            "set token default DACL",
            io::Error::last_os_error(),
        ));
    }
    Ok(())
}

fn aligned_buffer(byte_len: usize) -> Vec<usize> {
    vec![0_usize; byte_len.div_ceil(std::mem::size_of::<usize>())]
}

fn sid_ptr(sid: &[u8]) -> PSID {
    sid.as_ptr().cast_mut().cast::<c_void>()
}

struct RestrictingSid(PSID);

impl RestrictingSid {
    fn new() -> io::Result<Self> {
        let mut sid = ptr::null_mut();
        let process_id = std::process::id();
        if unsafe {
            AllocateAndInitializeSid(
                &SECURITY_NT_AUTHORITY,
                4,
                0x5053_5943,
                0x4845_564F,
                process_id,
                1,
                0,
                0,
                0,
                0,
                &mut sid,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(sid))
    }
}

impl Drop for RestrictingSid {
    fn drop(&mut self) {
        unsafe {
            let _ = FreeSid(self.0);
        }
    }
}

fn kill_on_close_job() -> io::Result<OwnedHandle> {
    let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(handle as _) };
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            raw_handle(&handle),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast::<c_void>(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(handle)
}

fn raw_handle(handle: &OwnedHandle) -> HANDLE {
    use std::os::windows::io::AsRawHandle;
    handle.as_raw_handle() as HANDLE
}

fn wide_null(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

fn unicode_environment_block(env: &BTreeMap<String, String>) -> Vec<u16> {
    let mut values = env.iter().collect::<Vec<_>>();
    values.sort_by(|(left, _), (right, _)| {
        left.to_uppercase()
            .cmp(&right.to_uppercase())
            .then(left.cmp(right))
    });
    let mut block = Vec::new();
    for (key, value) in values {
        block.extend(OsStr::new(&format!("{key}={value}")).encode_wide());
        block.push(0);
    }
    block.push(0);
    block
}

fn argv_to_command_line(argv: &[String]) -> String {
    argv.iter()
        .map(|argument| quote_windows_argument(argument))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_windows_argument(argument: &str) -> String {
    let needs_quotes = argument.is_empty()
        || argument
            .chars()
            .any(|character| matches!(character, ' ' | '\t' | '\n' | '\r' | '"'));
    if !needs_quotes {
        return argument.to_string();
    }
    let mut quoted = String::with_capacity(argument.len() + 2);
    quoted.push('"');
    let mut backslashes = 0;
    for character in argument.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                quoted.push(character);
            }
        }
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::Read;
    use std::time::{Duration, Instant};

    use crate::types::RunMode;

    use super::{ExecInvocation, WindowsRestrictedSpawn, argv_to_command_line, spawn_read_only};

    #[test]
    fn quotes_windows_arguments_without_reinterpreting_backslashes() {
        assert_eq!(
            argv_to_command_line(&[
                r"C:\Program Files\Git\bin\bash.exe".to_string(),
                "-c".to_string(),
                r#"printf "%s" "a b""#.to_string(),
            ]),
            r#""C:\Program Files\Git\bin\bash.exe" -c "printf \"%s\" \"a b\"""#
        );
    }

    #[test]
    fn advisory_plan_process_preserves_git_bash_io_and_truthful_markers() {
        let temp = tempfile::tempdir().expect("sandbox fixture");
        let nested = temp.path().join("nested");
        std::fs::create_dir_all(&nested).expect("nested fixture");
        let readable = temp.path().join("readable.txt");
        std::fs::write(&readable, "read-sentinel").expect("read fixture");
        let env = std::env::vars().collect::<BTreeMap<_, _>>();
        let runtime = crate::host_paths::GitBashRuntime::discover(&env).expect("Git Bash runtime");
        let path_for_bash = |path: &std::path::Path| path.to_string_lossy().replace('\\', "/");
        let mut invocation_env = env;
        invocation_env.insert("PSYCHEVO_TEST_READ".to_string(), path_for_bash(&readable));
        invocation_env.insert(
            "PSYCHEVO_TEST_SHELL".to_string(),
            path_for_bash(&runtime.bash),
        );
        let invocation = ExecInvocation {
            cmd: r#"
cat "$PSYCHEVO_TEST_READ"
printf "\nmarkers:%s:%s:%s:%s\n" \
  "$PSYCHEVO_SANDBOX" "$PSYCHEVO_SANDBOX_MODE" \
  "$PSYCHEVO_SANDBOX_BACKEND" "$PSYCHEVO_SANDBOX_HELPERS"
"$PSYCHEVO_TEST_SHELL" -c 'printf child-ok'
"#
            .to_string(),
            cwd: nested,
            shell: runtime.bash.display().to_string(),
            login: false,
            tty: false,
            env: invocation_env,
            path_prefixes: Vec::new(),
            sandbox_policy: crate::sandbox::SandboxPolicy::disabled()
                .narrowed_for_run_mode(RunMode::Plan),
        };

        let WindowsRestrictedSpawn {
            process,
            stdin,
            mut stdout,
            mut stderr,
        } = spawn_read_only(&invocation, false).expect("restricted Plan spawn");
        assert!(stdin.is_none());
        let mut output = String::new();
        stdout.read_to_string(&mut output).expect("stdout");
        let mut error_output = String::new();
        stderr.read_to_string(&mut error_output).expect("stderr");

        let started = Instant::now();
        let exit_code = loop {
            if let Some(exit_code) = process.try_wait().expect("process status") {
                break exit_code;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "restricted process did not exit; stdout={output:?}; stderr={error_output:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(
            exit_code, 0,
            "restricted command failed; stdout={output:?}; stderr={error_output:?}"
        );
        assert!(output.contains("read-sentinel"), "{output:?}");
        assert!(
            output.contains("markers:1:read-only:windows-restricted-token-advisory:not-confined"),
            "{output:?}"
        );
        assert!(output.contains("child-ok"), "{output:?}");
    }
}
